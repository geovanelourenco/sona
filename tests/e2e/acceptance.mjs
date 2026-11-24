// Teste de aceitação do servidor real (binário release, dispositivo simulado).
// Cobre FR-06, FR-07, FR-10, FR-12 e a regra "PA return nasce desligado".
// Uso: cargo build --release -p sona-server && node tests/e2e/acceptance.mjs
import { spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const bin = join(root, 'target', 'release', process.platform === 'win32' ? 'sona-server.exe' : 'sona-server');
const data = mkdtempSync(join(tmpdir(), 'sona-e2e-'));
const PORT = 7399;
const base = `http://127.0.0.1:${PORT}`;
const results = [];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function start(args = ['--data', data, '--bind', `127.0.0.1:${PORT}`]) {
  const p = spawn(bin, args, { env: { ...process.env, RUST_LOG: 'warn,sona_server=info', NO_COLOR: '1' } });
  let log = '';
  p.stdout.on('data', (d) => (log += d));
  p.stderr.on('data', (d) => (log += d));
  return { p, log: () => log };
}

async function waitUp() {
  for (let i = 0; i < 50; i++) {
    try {
      const r = await fetch(`${base}/api/health`);
      if (r.ok) return r.json();
    } catch {}
    await sleep(100);
  }
  throw new Error('servidor não subiu');
}

function client(token) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`ws://127.0.0.1:${PORT}/api/ws?token=${token}`);
    const c = { ws, state: null, hello: null, errors: [], meters: null, waiters: [] };
    ws.onmessage = (e) => {
      const m = JSON.parse(e.data);
      if (m.type === 'hello') {
        c.hello = m;
        c.state = m.state;
        resolve(c);
      } else if (m.type === 'state') c.state = m.state;
      else if (m.type === 'error') c.errors.push(m.message);
      else if (m.type === 'meters') c.meters = m;
      c.waiters = c.waiters.filter((w) => !w(m));
    };
    ws.onerror = () => reject(new Error('ws recusado'));
    c.send = (msg) => ws.send(JSON.stringify(msg));
    c.until = (pred, ms = 2000) =>
      new Promise((res, rej) => {
        const t = setTimeout(() => rej(new Error('timeout')), ms);
        c.waiters.push((m) => (pred(m, c) ? (clearTimeout(t), res(m), true) : false));
      });
  });
}

async function check(id, name, fn) {
  try {
    await fn();
    results.push({ id, name, ok: true });
    console.log(`ok   ${id} ${name}`);
  } catch (e) {
    results.push({ id, name, ok: false, error: e.message });
    console.log(`FAIL ${id} ${name}: ${e.message}`);
  }
}

let srv = start();
await waitUp();
const admin = srv.log().match(/token=([0-9a-f]+)/)?.[1];
assert.ok(admin, 'token de admin no log');

await check('FR-12', 'recusa controle e áudio sem token válido', async () => {
  assert.equal((await fetch(`${base}/api/state`)).status, 401);
  assert.equal((await fetch(`${base}/api/state`, { headers: { Authorization: 'Bearer 00' } })).status, 401);
  await assert.rejects(client('deadbeef'));
});

const mkToken = async (name, role) => (await (await fetch(`${base}/api/tokens`, { method: 'POST', headers: { Authorization: `Bearer ${admin}`, 'Content-Type': 'application/json' }, body: JSON.stringify({ name, role }) })).json()).token;
const opToken = await mkToken('Operador A', 'operator');
const monToken = await mkToken('Fone B', 'monitor');

await check('FR-12', 'perfis: operador não muda roteamento; monitor só mexe no próprio fone', async () => {
  const op = await client(opToken);
  op.send({ type: 'patch', path: '/channels/0', value: { source: { kind: 'none' } } });
  op.send({ type: 'patch', path: '/buses/1', value: { enabled: true } });
  await sleep(200);
  assert.equal(op.errors.length, 2, JSON.stringify(op.errors));
  const mon = await client(monToken);
  mon.send({ type: 'patch', path: '/channels/0', value: { fader_db: 0 } });
  mon.send({ type: 'patch', path: '/buses/4/monitor', value: { solo: [1] } });
  await sleep(200);
  assert.equal(mon.errors.length, 1, JSON.stringify(mon.errors));
  assert.deepEqual(mon.state.mixer.buses[4].monitor.solo, [1]);
  mon.send({ type: 'patch', path: '/buses/4/monitor', value: { solo: [] } });
  await sleep(100);
  op.ws.close();
  mon.ws.close();
});

await check('seg.', 'retorno de PA nasce desligado', async () => {
  const a = await client(admin);
  const pa = a.state.mixer.buses.find((b) => b.kind === 'pa_return');
  assert.equal(pa.enabled, false);
  a.ws.close();
});

await check('FR-06', 'dois navegadores veem o mesmo estado', async () => {
  const a = await client(admin);
  const b = await client(opToken);
  b.send({ type: 'patch', path: '/channels/2', value: { fader_db: -7.5, name: 'Pregador' } });
  const seen = (m) => m.type === 'state' && m.state.mixer.channels[2].fader_db === -7.5;
  await Promise.all([a.until(seen), b.until(seen)]);
  assert.equal(a.state.mixer.channels[2].name, 'Pregador');
  assert.equal(a.state.rev, b.state.rev);
  a.ws.close();
  b.ws.close();
});

await check('FR-06', 'sem nenhum navegador, o áudio continua', async () => {
  let h1;
  for (let i = 0; i < 20; i++) {
    h1 = await (await fetch(`${base}/api/health`)).json();
    if (h1.engine.clients === 0) break;
    await sleep(50);
  }
  assert.equal(h1.engine.clients, 0);
  await sleep(1000);
  const h2 = await (await fetch(`${base}/api/health`)).json();
  const blocks = h2.engine.blocks - h1.engine.blocks;
  // 1 s a 48 kHz / 128 = 375 blocos. Aceita ±10 % de folga do sleep.
  assert.ok(blocks > 330 && blocks < 420, `blocos em 1 s: ${blocks}`);
  assert.equal(h2.engine.running, true);
});

await check('FR-07', 'PFL do operador A só muda o fone dele', async () => {
  const a = await client(admin);
  a.send({ type: 'patch', path: '/channels/0', value: { fader_db: 0 } });
  a.send({ type: 'patch', path: '/channels/1', value: { fader_db: 0 } });
  await sleep(300);
  const program = JSON.stringify({ ch: a.state.mixer.channels, pgm: a.state.mixer.buses[0], f2: a.state.mixer.buses[4] });
  // Ch6 está com o fader fechado: só pode chegar ao fone 1 pelo PFL.
  a.send({ type: 'patch', path: '/buses/3/monitor', value: { solo: [5] } });
  await a.until((m) => m.type === 'state' && m.state.mixer.buses[3].monitor.solo[0] === 5);
  assert.equal(JSON.stringify({ ch: a.state.mixer.channels, pgm: a.state.mixer.buses[0], f2: a.state.mixer.buses[4] }), program, 'programa e fone 2 não mudam');
  await sleep(300);
  const rows = [];
  for (let i = 0; i < 8; i++) rows.push((await a.until((m) => m.type === 'meters')).bus);
  const peak = (b) => Math.max(...rows.map((r) => r[b][0]));
  assert.ok(peak(0) > 0.01, 'transmissão continua tocando');
  assert.ok(peak(4) > 0.01, 'fone 2 segue a transmissão');
  assert.ok(peak(3) > 0.001, `fone 1 toca o canal em PFL com o fader fechado (${peak(3)})`);
  a.send({ type: 'patch', path: '/buses/3/monitor', value: { solo: [] } });
  await sleep(100);
  a.ws.close();
});

await check('FR-10', 'cena salva', async () => {
  const a = await client(admin);
  a.send({ type: 'scene.save', name: 'Culto domingo' });
  await a.until((m) => m.type === 'state' && m.state.scenes.includes('Culto domingo'));
  await sleep(1200); // grava com debounce de 0,5 s
  a.ws.close();
});

await check('seg.', 'ciclo entre buses é recusado e o estado não muda', async () => {
  const a = await client(admin);
  const rev = a.state.rev;
  a.send({ type: 'patch', path: '/buses/0', value: { bus_sends: [{ bus: 2, level_db: 0 }] } });
  a.send({ type: 'patch', path: '/buses/2', value: { bus_sends: [{ bus: 0, level_db: 0 }] } });
  await sleep(300);
  assert.ok(a.errors.some((e) => /ciclo|realimentação/.test(e)), JSON.stringify(a.errors));
  a.send({ type: 'patch', path: '/buses/0', value: { bus_sends: [] } });
  await sleep(300);
  assert.ok(a.state.rev > rev);
  a.ws.close();
});


// ── Acesso pela rede com senha ──
const login = (password, name = 'Teste') =>
  fetch(`${base}/api/login`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ password, name }) });
const setPw = (role, password) =>
  fetch(`${base}/api/password`, { method: 'POST', headers: { Authorization: `Bearer ${admin}`, 'Content-Type': 'application/json' }, body: JSON.stringify({ role, password }) });

await check('FR-12', 'senha: sem senha definida, ninguém entra por senha', async () => {
  assert.equal((await (await fetch(`${base}/api/health`)).json()).password_login, false);
  assert.equal((await login('qualquer')).status, 401);
});

await check('FR-12', 'senha definida pelo admin dá sessão com o perfil certo', async () => {
  assert.equal((await setPw('operator', '123')).status, 400, 'senha curta recusada');
  assert.equal((await setPw('operator', 'culto-domingo')).status, 200);
  assert.equal((await (await fetch(`${base}/api/health`)).json()).password_login, true);
  assert.equal((await login('errada')).status, 401);
  const r = await login('culto-domingo', 'Ana');
  assert.equal(r.status, 200);
  const { token, role } = await r.json();
  assert.equal(role, 'operator');
  const c = await client(token);
  assert.equal(c.hello.role, 'operator');
  assert.equal(c.hello.name, 'Ana');
  c.ws.close();
});

await check('FR-12', 'trocar a senha encerra sessões abertas com a antiga', async () => {
  const { token } = await (await login('culto-domingo', 'Bia')).json();
  const c = await client(token);
  const revoked = c.until((m) => m.type === 'revoked', 4000);
  assert.equal((await setPw('operator', 'culto-noite')).status, 200);
  await revoked;
  assert.equal((await fetch(`${base}/api/state`, { headers: { Authorization: `Bearer ${token}` } })).status, 401);
  assert.equal((await login('culto-domingo')).status, 401);
  assert.equal((await login('culto-noite')).status, 200);
});

await check('FR-12', 'senha definida na linha de comando vale sem reiniciar', async () => {
  const r = spawnSync(bin, ['--data', data, 'password', '--role', 'monitor', '--value', 'so-o-fone'], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  await sleep(50);
  const j = await (await login('so-o-fone')).json();
  assert.equal(j.role, 'monitor');
});

await check('FR-12', 'muitas senhas erradas bloqueiam o IP por um tempo', async () => {
  for (let i = 0; i < 5; i++) await login('chute-' + i);
  const r = await login('culto-noite');
  assert.equal(r.status, 429, 'bloqueado mesmo com a senha certa');
});

// FR-10: reinício restaura; fonte que sumiu fica visível como desconectada.
srv.p.kill('SIGINT');
await new Promise((r) => srv.p.on('exit', r));
const statePath = join(data, 'state.json');
const saved = JSON.parse(readFileSync(statePath, 'utf8'));
saved.mixer.channels[3].source = { kind: 'mono', input: 99 };
writeFileSync(statePath, JSON.stringify(saved));
srv = start();
await waitUp();

await check('FR-10', 'reinício restaura mix, cenas e mostra a entrada ausente', async () => {
  const a = await client(admin);
  assert.equal(a.state.mixer.channels[2].name, 'Pregador');
  assert.equal(a.state.mixer.channels[2].fader_db, -7.5);
  assert.deepEqual(a.state.scenes, ['Culto domingo']);
  assert.deepEqual(a.state.mixer.channels[3].source, { kind: 'mono', input: 99 }, 'config original preservada');
  assert.ok(a.state.validation.warnings.some((w) => w.code === 'input_disconnected' && w.path === '/channels/3/source'));
  const h = await (await fetch(`${base}/api/health`)).json();
  assert.equal(h.engine.running, true, 'motor roda mesmo com entrada ausente');
  a.send({ type: 'patch', path: '/channels/2', value: { fader_db: 3 } });
  a.send({ type: 'scene.recall', name: 'Culto domingo' });
  await a.until((m) => m.type === 'state' && m.state.mixer.channels[2].fader_db === -7.5);
  a.ws.close();
});

await check('FR-10', 'estado corrompido não derruba o servidor', async () => {
  srv.p.kill('SIGINT');
  await new Promise((r) => srv.p.on('exit', r));
  writeFileSync(statePath, '{ isto não é json');
  srv = start();
  const h = await waitUp();
  assert.equal(h.engine.running, true);
});


await check('FR-12', 'abrir na rede sem senha é recusado; com senha, abre e mostra o endereço', async () => {
  const fresh = mkdtempSync(join(tmpdir(), 'sona-e2e-net-'));
  const refused = spawnSync(bin, ['--data', fresh, '--bind', '0.0.0.0:7398'], { encoding: 'utf8', timeout: 5000 });
  assert.equal(refused.status, 2, 'recusou abrir na rede sem senha');
  assert.match(refused.stdout + refused.stderr, /defina antes uma senha/);
  spawnSync(bin, ['--data', fresh, 'password', '--value', 'abre-na-rede']);
  const net = start(['--data', fresh, '--port', '7398']);
  let h;
  for (let i = 0; i < 50 && !h; i++) {
    try {
      h = await (await fetch('http://127.0.0.1:7398/api/health')).json();
    } catch {
      await sleep(100);
    }
  }
  net.p.kill('SIGINT');
  await new Promise((res) => net.p.on('exit', res));
  assert.equal(h.password_login, true);
  assert.ok(h.lan_urls.length > 0, 'mostra pelo menos um endereço da rede');
  assert.ok(h.lan_urls.every((u) => u.endsWith(':7398') && !u.includes('127.0.0.1')), JSON.stringify(h.lan_urls));
});

srv.p.kill('SIGINT');
await new Promise((r) => srv.p.on('exit', r));
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} verificações passaram.`);
writeFileSync(join(root, 'docs', 'evidence', 'acceptance-last-run.json'), JSON.stringify({ at: new Date().toISOString(), platform: process.platform, results }, null, 2) + '\n');
process.exit(failed.length ? 1 : 0);
