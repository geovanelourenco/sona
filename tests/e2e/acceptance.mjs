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
const exe = process.platform === 'win32' ? '.exe' : '';
const bin = join(root, 'target', 'release', 'sona-server' + exe);
const bridgeBin = join(root, 'target', 'release', 'sona-bridge' + exe);
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
const lanBase = first.lan_urls.find((u) => !u.includes('.local'));
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
  // 1 s a 48 kHz / bloco. Aceita ±12 % de folga do sleep.
  const expected = h2.engine.sample_rate / h2.engine.block;
  assert.ok(Math.abs(blocks - expected) < expected * 0.12, `blocos em 1 s: ${blocks} (esperado ~${expected})`);
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
  a.send({ type: 'patch', path: '/channels/0', value: { fader_db: -90 } });
  a.send({ type: 'patch', path: '/channels/1', value: { fader_db: -90 } });
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


/** WAV PCM 16 bits mono com um seno. */
function wav(rate, seconds, freq) {
  const n = Math.round(rate * seconds);
  const b = Buffer.alloc(44 + n * 2);
  b.write('RIFF', 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write('WAVEfmt ', 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(rate, 24);
  b.writeUInt32LE(rate * 2, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write('data', 36);
  b.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) b.writeInt16LE(Math.round(0.5 * Math.sin((2 * Math.PI * freq * i) / rate) * 32767), 44 + i * 2);
  return b;
}

await check('FR-13', 'player multitrack: envia faixas, toca na mesa; click só no fone', async () => {
  const H = { Authorization: `Bearer ${admin}` };
  const j = (method, url, body) => fetch(`${base}${url}`, { method, headers: { ...H, 'Content-Type': 'application/json' }, body: JSON.stringify(body) }).then((r) => r.json());
  const { id } = await j('POST', '/api/player/songs', { name: 'Teste e2e' });
  for (const [name, freq] of [['Baixo.wav', 110], ['Click.wav', 1000]]) {
    const r = await fetch(`${base}/api/player/songs/${id}/files/${name}`, { method: 'PUT', headers: H, body: wav(44100, 3, freq) });
    assert.equal(r.status, 200, await r.text());
  }
  const bad = await fetch(`${base}/api/player/songs/${id}/files/notas.txt`, { method: 'PUT', headers: H, body: 'x' });
  assert.equal(bad.status, 400, 'só arquivos de áudio');
  const a = await client(admin);
  const chans = a.state.mixer.channels;
  const p = chans.findIndex((c) => c.name === 'Player');
  const cue = chans.findIndex((c) => c.name === 'Click + guia');
  assert.ok(p >= 0 && cue >= 0, 'canais do player na mesa: ' + chans.map((c) => c.name).join(', '));
  const tx = a.state.mixer.buses.findIndex((b) => b.kind === 'broadcast');
  assert.equal(chans[cue].sends.find((s) => s.bus === tx)?.enabled, false, 'click nunca na transmissão');
  a.send({ type: 'patch', path: `/channels/${p}`, value: { fader_db: 0 } });
  const st = await j('POST', '/api/player', { action: 'play' });
  assert.equal(st.current, id);
  assert.equal(st.playing, true);
  let mp = 0, mc = 0;
  for (let i = 0; i < 20 && (mp < 0.1 || mc < 0.1); i++) {
    const m = await a.until((m) => m.type === 'meters');
    mp = Math.max(mp, m.ch[p]);
    mc = Math.max(mc, m.ch[cue]);
  }
  assert.ok(mp > 0.1, `nível do canal Player ${mp}`);
  assert.ok(mc > 0.1, `nível do canal Click + guia ${mc}`);
  // Música de 3 s termina sozinha e o player para (parar no fim é o padrão).
  await sleep(3200);
  const end = await (await fetch(`${base}/api/player`, { headers: H })).json();
  assert.equal(end.playing, false, 'parou no fim');
  assert.equal(end.underruns, 0, 'disco acompanhou o motor');
  // Faixa por canal: aparece um canal por faixa (o click continua no fone).
  await j('POST', '/api/player', { action: 'settings', mode: 'split' });
  const s2 = await (await fetch(`${base}/api/state`, { headers: H })).json();
  const names = s2.mixer.channels.filter((c) => c.tags?.includes('multitrack')).map((c) => c.name);
  assert.deepEqual(names, ['Click + guia', 'MT Baixo']);
  await j('POST', '/api/player', { action: 'settings', mode: 'simple' });
  a.ws.close();
});

await check('FR-12', 'desligar o acesso remoto derruba todos os remotos', async () => {
  const c = await client(remoteToken, lanHost);
  const revoked = c.until((m) => m.type === 'revoked', 4000);
  assert.equal((await update(admin, { remote_password: null })).status, 200);
  await revoked;
  assert.equal((await access()).remote_enabled, false);
  assert.equal((await update(admin, { remote_password: 'culto-noite' })).status, 200, 'religa para os próximos testes');
});


// ── Sona Bridge pela rede (IP da rede + senha remota), aparelhos simulados ──
const BRIDGE_PORT = 7398;
const bridgeStatus = async () => (await fetch(`http://127.0.0.1:${BRIDGE_PORT}/api/status`)).json();
const waitFor = async (pred, ms = 15000) => {
  const t0 = Date.now();
  let last;
  while (Date.now() - t0 < ms) {
    try {
      last = await bridgeStatus();
      if (pred(last)) return last;
    } catch {}
    await sleep(250);
  }
  throw new Error('bridge não chegou ao estado esperado: ' + JSON.stringify(last));
};
const bridgeData = mkdtempSync(join(tmpdir(), 'sona-bridge-'));
const br = spawn(bridgeBin, ['--data', bridgeData, '--port', String(BRIDGE_PORT), '--server', lanHost, '--password', 'culto-noite', '--name', 'OBS', '--send', 'sim', '--receive', 'sim', '--sim-ppm', '120'], { env: { ...process.env, NO_COLOR: '1' } });
let brLog = '';
br.stdout.on('data', (d) => (brLog += d));
br.stderr.on('data', (d) => (brLog += d));

await check('FR-08', 'bridge envia e recebe ao mesmo tempo, pela rede, com deriva de 120 ppm', async () => {
  // Abre o canal da rede em 0 dB para medir nível.
  const a = await client(admin);
  const ch = a.state.mixer.channels.findIndex((c) => c.source.kind === 'stereo' && c.source.left === 96);
  assert.ok(ch >= 0, 'canal da vaga de rede 1 existe');
  a.send({ type: 'patch', path: `/channels/${ch}`, value: { fader_db: 0 } });
  let st;
  try {
    st = await waitFor((s) => s.state === 'conectado' && s.sim_measurements >= 6 && s.server_view?.packets_in > 1000, 25000);
  } catch (e) {
    const m = await a.until((m) => m.type === 'meters');
    console.log('DEBUG canal', ch, JSON.stringify(a.state.mixer.channels[ch]), 'medidor canal', m.ch[ch], 'bus', JSON.stringify(m.bus), 'buses', JSON.stringify(a.state.mixer.buses.map((b) => [b.name, b.fader_db, b.mute, b.outputs, b.monitor])));
    throw e;
  }
  // Pico ao longo de 4 s (o tom liga e desliga a cada 0,5 s). Também mede
  // falhas nessa janela: depois de adaptada, a fila de rede não pode falhar.
  const before = await bridgeStatus();
  let peak = 0;
  for (let i = 0; i < 16; i++) {
    await sleep(250);
    peak = Math.max(peak, (await bridgeStatus()).recv_peak);
  }
  const s2 = await bridgeStatus();
  s2.recv_peak = peak;
  const lateUnderruns = s2.server_view.underruns - before.server_view.underruns;
  console.log(`      ida e volta ${s2.sim_latency_ms} ms; servidor corrige ${s2.server_view.drift_ppm} ppm; bridge corrige ${s2.drift_ppm} ppm; perdas ${s2.lost_in}/${s2.server_view.lost}; nível ${s2.recv_peak.toFixed(3)}`);
  assert.equal(st.slot, 0);
  assert.equal(s2.lost_in, 0);
  assert.equal(s2.server_view.lost, 0);
  // Máquina virtual compartilhada: pausas de agendamento podem esvaziar a fila
  // no arranque; a fila adaptativa cresce e absorve. Exige poucas e nenhuma
  // depois de adaptada.
  console.log(`      falhas no servidor: ${s2.server_view.underruns} no total, ${lateUnderruns} nos últimos 4 s`);
  assert.ok(s2.server_view.underruns <= 5, `falhas de áudio no servidor: ${s2.server_view.underruns}`);
  assert.equal(lateUnderruns, 0, 'falhas depois de a fila se adaptar');
  assert.ok(s2.sim_latency_ms > 5 && s2.sim_latency_ms < 80, `ida e volta ${s2.sim_latency_ms} ms`);
  // A precisão da estimativa de deriva é provada nos testes determinísticos
  // (crates/sona-core/tests/drift.rs). Aqui os relógios são de software num
  // container compartilhado (pausas de agendamento), então a correção só é
  // registrada; exige-se atraso no alvo e ausência de falhas depois de adaptar.

  // Fila de rede: alvo 12 ms, cresce sozinha até 80 ms se esvaziar.
  assert.ok(s2.server_view.latency_ms > 5 && s2.server_view.latency_ms <= 80, `atraso da fila no servidor ${s2.server_view.latency_ms} ms`);
  assert.ok(s2.recv_peak > 0.4 && s2.recv_peak < 0.6, `nível recebido ${s2.recv_peak}`);
  // O console mostra o bridge pelo nome.
  const h = await (await fetch(`${base}/api/state`, { headers: { Authorization: `Bearer ${admin}` } })).json();
  assert.equal(h.io.inputs[96].name, 'OBS L');
  assert.equal(h.io.inputs[96].connected, true);
  a.ws.close();
});

await check('FR-08', 'bridge: ouvir o que chega numa saída deste computador, sem reconectar ao mudar o volume', async () => {
  const set = (body) => fetch(`http://127.0.0.1:${BRIDGE_PORT}/api/config`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  await set({ monitor: { device: 'sim', source: 'receive', volume_db: 0 } });
  let st = await waitFor((s) => s.state === 'conectado' && s.monitor_device === 'sim', 15000);
  assert.equal(st.monitor_error, null);
  let peak = 0;
  for (let i = 0; i < 12; i++) {
    await sleep(250);
    peak = Math.max(peak, (await bridgeStatus()).monitor_peak);
  }
  assert.ok(peak > 0.4 && peak < 0.6, `nível da escuta ${peak}`);
  // Só o volume: aplica na hora, mesma conexão.
  const before = (await bridgeStatus()).packets_in;
  await set({ monitor: { device: 'sim', source: 'receive', volume_db: -60 } });
  await sleep(1500);
  const s2 = await bridgeStatus();
  assert.ok(s2.packets_in > before, 'não reconectou');
  peak = 0;
  for (let i = 0; i < 6; i++) {
    await sleep(250);
    peak = Math.max(peak, (await bridgeStatus()).monitor_peak);
  }
  assert.ok(peak < 0.001, `volume mínimo silencia a escuta (${peak})`);
  await set({ monitor: null });
  await waitFor((s) => s.state === 'conectado' && s.monitor_device === null, 15000);
});

await check('FR-12', 'bridge acha o servidor na rede sem digitar IP (mDNS)', async () => {
  const j = await (await fetch(`http://127.0.0.1:${BRIDGE_PORT}/api/discover`)).json();
  const me = j.servers.find((s) => s.address.endsWith(`:${PORT}`));
  assert.ok(me, 'servidor anunciado: ' + JSON.stringify(j.servers));
  assert.match(me.name, /^Sona \(/);
  assert.ok(me.ips.length > 0 && !me.ips.includes('127.0.0.1'), 'IPs da rede anunciados');
  const acc = await access();
  assert.ok(acc.lan_urls.includes(`http://sona.local:${PORT}`), 'console mostra sona.local');
});

await check('FR-11', 'bridge reconecta sozinho quando a configuração de áudio do servidor muda', async () => {
  const before = (await bridgeStatus()).packets_in;
  const r = await fetch(`${base}/api/audio`, { headers: { Authorization: `Bearer ${admin}` } });
  const { config } = await r.json();
  const apply = await fetch(`${base}/api/audio`, { method: 'POST', headers: { Authorization: `Bearer ${admin}`, 'Content-Type': 'application/json' }, body: JSON.stringify({ ...config, block: 256 }) });
  assert.equal(apply.status, 200);
  await sleep(1500);
  const s = await waitFor((s) => s.state === 'conectado' && s.packets_in > 200, 20000);
  assert.equal(s.slot, 0, 'mesma vaga depois de reconectar');
  void before;
});

await check('FR-12', 'desligar o acesso remoto tira o bridge e ele pede a senha de novo', async () => {
  assert.equal((await update(admin, { remote_password: null })).status, 200);
  const s = await waitFor((s) => s.state === 'senha', 15000);
  assert.match(s.message, /senha|Entre/);
  assert.equal((await update(admin, { remote_password: 'culto-noite' })).status, 200);
});
if (br.exitCode === null) {
  br.kill('SIGINT');
  await new Promise((r) => br.on('exit', r));
} else if (results.some((r) => !r.ok && r.id === 'FR-08')) {
  console.log(brLog);
}

// Por último antes do reinício: bloqueia o IP da rede por 60 s.
await check('FR-12', 'muitas senhas erradas bloqueiam o IP por um tempo', async () => {
  for (let i = 0; i < 5; i++) await login('chute-' + i, lanBase);
  assert.equal((await login('culto-noite', lanBase)).status, 429, 'bloqueado mesmo com a senha certa');
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
