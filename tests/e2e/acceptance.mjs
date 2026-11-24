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

function start(args = ['--data', data, '--port', String(PORT)]) {
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

function client(token, host = `127.0.0.1:${PORT}`) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`ws://${host}/api/ws?token=${token}`);
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
const access = async (b = base) => (await fetch(`${b}/api/access`)).json();
const login = (password = '', b = base, name = 'Teste') =>
  fetch(`${b}/api/login`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ password, name }) });
const tokenOf = async (r) => (await r.json()).token;
const update = (token, change, b = base) =>
  fetch(`${b}/api/access`, { method: 'POST', headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, body: JSON.stringify(change) });

const first = await access();
// Endereço deste computador na rede: pedidos por ele chegam como "outro computador".
const lanBase = first.lan_urls[0];
const lanHost = lanBase?.replace('http://', '');
let admin = await tokenOf(await login());

await check('FR-12', 'neste computador, sem senha do app, entra direto e vê a primeira configuração', async () => {
  assert.equal(first.local, true);
  assert.equal(first.setup_needed, true);
  assert.equal(first.remote_enabled, false);
  assert.ok(admin);
  assert.ok(lanBase, 'há um endereço de rede para os testes remotos');
});

await check('FR-12', 'recusa controle sem sessão válida', async () => {
  assert.equal((await fetch(`${base}/api/state`)).status, 401);
  assert.equal((await fetch(`${base}/api/state`, { headers: { Authorization: 'Bearer 00' } })).status, 401);
  await assert.rejects(client('deadbeef'));
});

await check('FR-12', 'acesso remoto desligado: outro computador não entra nem com sessão local', async () => {
  const acc = await access(lanBase);
  assert.equal(acc.local, false);
  assert.equal(acc.setup_needed, false, 'primeira configuração só aparece no servidor');
  assert.deepEqual(acc.lan_urls, [], 'não revela endereços com remoto desligado');
  assert.equal((await login('', lanBase)).status, 403);
  assert.equal((await fetch(`${lanBase}/api/state`, { headers: { Authorization: `Bearer ${admin}` } })).status, 401, 'token local não vale pela rede');
  await assert.rejects(client(admin, lanHost));
});

let remoteToken;
await check('FR-12', 'primeira configuração liga o acesso remoto com senha', async () => {
  assert.equal((await update(admin, { remote_password: '123' })).status, 400, 'senha curta recusada');
  const r = await update(admin, { remote_password: 'culto-domingo', finish_setup: true });
  assert.equal(r.status, 200);
  const acc = await access();
  assert.equal(acc.setup_needed, false);
  assert.equal(acc.remote_enabled, true);
  assert.equal((await login('errada', lanBase)).status, 401);
  const ok = await login('culto-domingo', lanBase, 'Ana');
  assert.equal(ok.status, 200);
  remoteToken = await tokenOf(ok);
  const c = await client(remoteToken, lanHost);
  assert.equal(c.hello.name, 'Ana');
  c.ws.close();
});

await check('FR-12', 'trocar a senha remota encerra as sessões remotas abertas', async () => {
  const c = await client(remoteToken, lanHost);
  const revoked = c.until((m) => m.type === 'revoked', 4000);
  assert.equal((await update(admin, { remote_password: 'culto-noite' })).status, 200);
  await revoked;
  assert.equal((await fetch(`${lanBase}/api/state`, { headers: { Authorization: `Bearer ${remoteToken}` } })).status, 401);
  assert.equal((await login('culto-domingo', lanBase)).status, 401);
  remoteToken = await tokenOf(await login('culto-noite', lanBase));
  assert.ok(remoteToken);
});

await check('FR-12', 'senha do app passa a ser pedida neste computador; quem a criou continua dentro', async () => {
  assert.equal((await update(admin, { app_password: 'so-aqui-123' })).status, 200);
  assert.equal((await fetch(`${base}/api/state`, { headers: { Authorization: `Bearer ${admin}` } })).status, 200, 'sessão de quem trocou continua');
  assert.equal((await login('')).status, 401);
  assert.equal((await login('culto-noite')).status, 401, 'senha remota não abre o app local');
  assert.equal((await login('so-aqui-123')).status, 200);
});

await check('FR-12', 'recuperação pela linha de comando vale sem reiniciar', async () => {
  const r = spawnSync(bin, ['--data', data, 'password', '--app', '--clear'], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  await sleep(50);
  assert.equal((await access()).app_password, false);
  assert.equal((await login('')).status, 200);
});

await check('seg.', 'retorno de PA nasce desligado', async () => {
  const a = await client(admin);
  const pa = a.state.mixer.buses.find((b) => b.kind === 'pa_return');
  assert.equal(pa.enabled, false);
  a.ws.close();
});

await check('FR-06', 'dois navegadores veem o mesmo estado', async () => {
  const a = await client(admin);
  const b = await client(await tokenOf(await login()));
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


await check('FR-12', 'muitas senhas erradas bloqueiam o IP por um tempo', async () => {
  for (let i = 0; i < 5; i++) await login('chute-' + i, lanBase);
  assert.equal((await login('culto-noite', lanBase)).status, 429, 'bloqueado mesmo com a senha certa');
});

await check('FR-12', 'desligar o acesso remoto derruba todos os remotos', async () => {
  const c = await client(remoteToken, lanHost);
  const revoked = c.until((m) => m.type === 'revoked', 4000);
  assert.equal((await update(admin, { remote_password: null })).status, 200);
  await revoked;
  assert.equal((await access()).remote_enabled, false);
  assert.equal((await update(admin, { remote_password: 'culto-noite' })).status, 200, 'religa para os próximos testes');
});

// FR-10: reinício restaura; fonte que sumiu fica visível como desconectada.
srv.p.kill('SIGINT');
await new Promise((r) => srv.p.on('exit', r));
const statePath = join(data, 'state.json');
const saved = JSON.parse(readFileSync(statePath, 'utf8'));
saved.mixer.channels[3].source = { kind: 'mono', input: 200 };
writeFileSync(statePath, JSON.stringify(saved));
srv = start();
await waitUp();

await check('FR-10', 'reinício restaura mix, cenas e mostra a entrada ausente', async () => {
  const a = await client(admin);
  assert.equal(a.state.mixer.channels[2].name, 'Pregador');
  assert.equal(a.state.mixer.channels[2].fader_db, -7.5);
  assert.deepEqual(a.state.scenes, ['Culto domingo']);
  assert.deepEqual(a.state.mixer.channels[3].source, { kind: 'mono', input: 200 }, 'config original preservada');
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


srv.p.kill('SIGINT');
await new Promise((r) => srv.p.on('exit', r));
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} verificações passaram.`);
writeFileSync(join(root, 'docs', 'evidence', 'acceptance-last-run.json'), JSON.stringify({ at: new Date().toISOString(), platform: process.platform, results }, null, 2) + '\n');
process.exit(failed.length ? 1 : 0);
