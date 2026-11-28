import { ArrowDown, ArrowUp, ArrowsClockwise, Broadcast, Headphones, SignOut } from '@phosphor-icons/react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { meterPos } from '../lib/level';
import { SonaMark } from '../ui/Brand';
import { Btn, Card, cx } from '../ui/kit';

// Espelho de crates/sona-bridge/src/client.rs (Status) e ui.rs.
interface Status {
  state: 'parado' | 'conectando' | 'conectado' | 'senha' | 'erro';
  message: string;
  server: string;
  name: string;
  slot: number | null;
  send_error: string | null;
  receive_error: string | null;
  packets_in: number;
  packets_out: number;
  lost_in: number;
  server_view: { lost?: number; latency_ms?: number; drift_ppm?: number; underruns?: number; packets_in?: number };
  drift_ppm: number;
  latency_ms: number;
  underruns: number;
  send_peak: number;
  recv_peak: number;
  obs_plugin: boolean | null;
  monitor_device: string | null;
  monitor_error: string | null;
  monitor_peak: number;
}
interface Monitor {
  device: string;
  source: 'receive' | 'send';
  volume_db: number;
}
interface Endpoint {
  device: string;
  channels: number;
}
interface Config {
  server: string;
  server_name: string;
  name: string;
  logged_in: boolean;
  send: Endpoint | null;
  receive: Endpoint | null;
  monitor: Monitor | null;
}
interface Dev {
  name: string;
  host: string;
  inputs: number;
  outputs: number;
}

const STATE: Record<Status['state'], [string, string]> = {
  parado: ['PARADO', 'bg-panel text-muted'],
  conectando: ['CONECTANDO', 'bg-cyan/15 text-cyan'],
  conectado: ['CONECTADO', 'bg-ok/15 text-ok-light'],
  senha: ['SENHA NECESSÁRIA', 'bg-warn/15 text-warn-light'],
  erro: ['SEM CONEXÃO', 'bg-warn/15 text-warn-light'],
};

const post = (path: string, body: unknown) => fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });

/** Barra de nível horizontal com decaimento. */
function Level({ peak }: { peak: number }) {
  const [shown, setShown] = useState(0);
  const last = useRef(0);
  useEffect(() => {
    const p = meterPos(peak);
    last.current = p > last.current ? p : Math.max(p, last.current - 0.08);
    setShown(last.current);
  }, [peak]);
  return (
    <div className="h-2 w-full overflow-hidden rounded-full bg-deep shadow-[inset_0_0_0_1px_#1a1d26]" aria-hidden>
      <div className="h-full rounded-full transition-[width] duration-150" style={{ width: `${shown * 100}%`, background: 'linear-gradient(90deg,#7c5cff,#72d6ff 75%,#fcd34d 92%,#ef4444)' }} />
    </div>
  );
}

export function BridgeApp() {
  const [status, setStatus] = useState<Status | null>(null);
  const [cfg, setCfg] = useState<Config | null>(null);
  const [devices, setDevices] = useState<Dev[]>([]);
  const [offline, setOffline] = useState(false);

  const loadCfg = useCallback(() => fetch('/api/config').then((r) => r.json()).then(setCfg).catch(() => {}), []);
  useEffect(() => {
    loadCfg();
    fetch('/api/devices').then((r) => r.json()).then((j) => setDevices(j.devices ?? [])).catch(() => {});
    const t = setInterval(async () => {
      try {
        const s = await (await fetch('/api/status')).json();
        setStatus(s);
        setOffline(false);
      } catch {
        setOffline(true);
      }
    }, 250);
    return () => clearInterval(t);
  }, [loadCfg]);

  // Volta para a tela de entrada quando o servidor pede a senha de novo.
  useEffect(() => {
    if (status?.state === 'senha' && cfg?.logged_in) loadCfg();
  }, [status?.state, cfg?.logged_in, loadCfg]);

  if (!cfg) return <main className="grid h-full place-items-center text-subtle">{offline ? 'O bridge não está rodando.' : 'Carregando…'}</main>;
  const needsLogin = !cfg.logged_in || status?.state === 'senha';

  return (
    <main className="flex h-full flex-col overflow-auto">
      <header className="flex h-14 shrink-0 items-center gap-3 border-b border-line/70 bg-panel/60 px-5">
        <SonaMark size={26} />
        <span className="text-[21px] font-light tracking-[0.02em] text-hi/90">
          sona <span className="text-accent-text">bridge</span>
        </span>
        <span className="flex-1" />
        {status && <span className={cx('rounded-full px-2.5 py-1 text-[11px] font-bold', STATE[status.state][1])}>{STATE[status.state][0]}</span>}
        {cfg.logged_in && (
          <button
            type="button"
            aria-label="Sair do servidor"
            title="Sair do servidor"
            onClick={async () => {
              await post('/api/logout', {});
              loadCfg();
            }}
            className="grid h-8 w-8 place-items-center rounded-lg text-subtle hover:bg-panel-2 hover:text-text"
          >
            <SignOut size={16} weight="bold" />
          </button>
        )}
      </header>
      {needsLogin ? <Login cfg={cfg} message={status?.state === 'senha' && cfg.logged_in ? status.message : undefined} onDone={loadCfg} /> : <Connected cfg={cfg} status={status} devices={devices} onSaved={loadCfg} />}
    </main>
  );
}

interface Found {
  name: string;
  address: string;
  ips: string[];
}

function Login({ cfg, message, onDone }: { cfg: Config; message?: string; onDone: () => void }) {
  const [server, setServer] = useState(cfg.server);
  const [serverName, setServerName] = useState(cfg.server_name ?? '');
  const [found, setFound] = useState<Found[] | null>(null);
  const [searching, setSearching] = useState(false);
  const search = useCallback(async () => {
    setSearching(true);
    try {
      const j = await (await fetch('/api/discover')).json();
      const list: Found[] = j.servers ?? [];
      setFound(list);
      // Um só na rede e nada digitado: já escolhe.
      if (list.length === 1) {
        setServer((s) => s || list[0].address);
        setServerName((n) => n || list[0].name);
      }
    } catch {
      setFound([]);
    } finally {
      setSearching(false);
    }
  }, []);
  useEffect(() => {
    search();
  }, [search]);
  const [name, setName] = useState(cfg.name === 'Bridge' ? '' : cfg.name);
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(message ?? null);
  return (
    <div className="grid flex-1 place-items-center p-4">
      <form
        className="flex w-full max-w-md flex-col gap-4 rounded-[18px] border border-line bg-panel/80 p-7 shadow-[var(--shadow-modal)]"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          setError(null);
          const r = await post('/api/login', { server, password, name, server_name: serverName });
          const j = await r.json().catch(() => ({}));
          setBusy(false);
          if (!r.ok) return setError(j.error ?? 'Não consegui entrar.');
          onDone();
        }}
      >
        <div>
          <h1 className="text-[20px] font-light text-hi">Conectar ao servidor Sona</h1>
          <p className="text-[12.5px] text-subtle">Escolha o servidor achado na rede e use a senha de acesso remoto (no servidor, em Configurações).</p>
        </div>
        <div className="flex flex-col gap-1.5">
          <span className="flex items-center justify-between text-[12px] font-semibold text-subtle">
            Servidores na rede
            <button type="button" onClick={search} disabled={searching} className="inline-flex items-center gap-1 text-[11.5px] font-semibold text-accent-text hover:underline disabled:opacity-50">
              <ArrowsClockwise size={12} weight="bold" className={cx(searching && 'animate-spin')} /> Procurar
            </button>
          </span>
          {found === null || (searching && !found.length) ? (
            <p className="rounded-lg border border-line bg-bg/60 px-3 py-2 text-[12px] text-muted">Procurando servidores Sona na rede…</p>
          ) : found.length === 0 ? (
            <p className="rounded-lg border border-line bg-bg/60 px-3 py-2 text-[12px] text-muted">Nenhum servidor achado. Confira se o Sona está aberto e na mesma rede, ou digite o endereço abaixo.</p>
          ) : (
            found.map((f) => (
              <button
                key={f.name}
                type="button"
                onClick={() => {
                  setServer(f.address);
                  setServerName(f.name);
                }}
                className={cx('flex items-center gap-2.5 rounded-lg border px-3 py-2 text-left transition', server === f.address ? 'lm-selected border-transparent' : 'border-line bg-bg/60 hover:border-line-hi')}
              >
                <Broadcast size={16} weight="bold" className="shrink-0 text-cyan" />
                <span className="flex-1 text-[13px] font-semibold text-hi">{f.name}</span>
                <span className="font-mono text-[11px] text-muted">{f.address}</span>
              </button>
            ))
          )}
        </div>
        <label className="flex flex-col gap-1.5">
          <span className="text-[12px] font-semibold text-subtle">Endereço do servidor</span>
          <input
            value={server}
            onChange={(e) => {
              setServer(e.target.value);
              setServerName('');
            }}
            placeholder="ex.: sona.local:7340 ou 192.168.0.10:7340"
            className="lm-field h-10 px-3 font-mono text-[13px]"
          />
        </label>
        <label className="flex flex-col gap-1.5">
          <span className="text-[12px] font-semibold text-subtle">Nome deste computador no console</span>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="ex.: OBS, Mídia" className="lm-field h-10 px-3 text-[13px]" />
        </label>
        <label className="flex flex-col gap-1.5">
          <span className="text-[12px] font-semibold text-subtle">Senha de acesso remoto</span>
          <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} className="lm-field h-10 px-3 text-[13px]" />
        </label>
        {error && <p role="alert" className="rounded-lg border border-warn/40 bg-warn/10 px-3 py-2 text-[12px] text-warn-light">{error}</p>}
        <Btn variant="primary" type="submit" className="h-10 text-[13.5px]" disabled={busy || !server.trim()}>
          {busy ? 'Conectando…' : 'Conectar'}
        </Btn>
      </form>
    </div>
  );
}

function Connected({ cfg, status, devices, onSaved }: { cfg: Config; status: Status | null; devices: Dev[]; onSaved: () => void }) {
  const save = async (change: Partial<Pick<Config, 'send' | 'receive' | 'name' | 'monitor'>>) => {
    await post('/api/config', change);
    onSaved();
  };
  const select = (value: string | undefined, list: Dev[], onChange: (v: string | null) => void, label: string, loopback: Dev[] = [], obs = false) => {
    const known = list.some((d) => d.name === value) || loopback.some((d) => `loopback:${d.name}` === value) || (obs && value === 'obs');
    return (
      <select aria-label={label} value={value ?? ''} onChange={(e) => onChange(e.target.value || null)} className="lm-field h-9 w-full px-2.5 text-[12.5px]">
        <option value="">— nenhum —</option>
        {obs && (
          <optgroup label="Sem cabo virtual">
            <option value="obs">OBS neste computador (fonte “Sona”)</option>
          </optgroup>
        )}
        {value && value !== 'sim' && !known && <option value={value}>{value.replace(/^loopback:/, 'Áudio do sistema — ')} (não encontrado agora)</option>}
        {loopback.length > 0 && (
          <optgroup label="Áudio do sistema (o que toca nos alto-falantes)">
            {loopback.map((d) => (
              <option key={`lb:${d.host}:${d.name}`} value={`loopback:${d.name}`}>
                Áudio do sistema — {d.name}
              </option>
            ))}
          </optgroup>
        )}
        <optgroup label={obs ? 'Saídas' : 'Entradas'}>
          {list.map((d) => (
            <option key={`${d.host}:${d.name}`} value={d.name}>
              {d.name} · {d.host}
            </option>
          ))}
        </optgroup>
      </select>
    );
  };
  // Loopback é do WASAPI (Windows): oferece as saídas desse host.
  const loopbackDevs = devices.filter((d) => d.outputs > 0 && d.host === 'WASAPI');
  const sv = status?.server_view ?? {};
  return (
    <div className="mx-auto grid w-full max-w-4xl grid-cols-1 gap-3 p-5 md:grid-cols-2">
      <Card title="Conexão" className="md:col-span-2">
        <p className="text-[13px] text-text">{status?.message}</p>
        {status?.state === 'conectado' && (
          <div className="flex flex-wrap gap-x-5 gap-y-1 font-mono text-[11.5px] text-muted">
            <span>vaga {status.slot! + 1}</span>
            <span>pacotes ↑ {status.packets_out} ↓ {status.packets_in}</span>
            <span className={cx((status.lost_in > 0 || (sv.lost ?? 0) > 0) && 'text-warn-light')}>perdidos ↑ {sv.lost ?? 0} ↓ {status.lost_in}</span>
            <span>atraso ↓ {status.latency_ms} ms</span>
            <span title="Diferença de relógio corrigida">deriva {status.drift_ppm} ppm</span>
            {(status.underruns > 0 || (sv.underruns ?? 0) > 0) && <span className="text-warn-light">falhas ↑ {sv.underruns} ↓ {status.underruns}</span>}
          </div>
        )}
      </Card>

      <Card title={<span className="inline-flex items-center gap-1.5"><ArrowUp size={13} weight="bold" className="text-accent-text" /> Enviar para o servidor</span>}>
        <p className="text-[12px] text-subtle">Aparece no console como o canal “{cfg.name}”. Para mandar o que toca neste computador (Lumo, navegador, vídeos), escolha <b className="text-text">Áudio do sistema</b>: não precisa de cabo virtual.</p>
        {select(cfg.send?.device, devices.filter((d) => d.inputs > 0), (v) => save({ send: v ? { device: v, channels: 2 } : null }), 'Aparelho de envio', loopbackDevs)}
        <Level peak={status?.send_peak ?? 0} />
        {status?.send_error && <p className="text-[12px] text-warn-light">{status.send_error}</p>}
        {cfg.send?.device?.startsWith('loopback:') && cfg.send.device === `loopback:${cfg.receive?.device}` && (
          <p className="rounded-lg border border-warn/40 bg-warn/10 px-2.5 py-1.5 text-[12px] text-warn-light">
            O áudio recebido toca na mesma saída que está sendo capturada: ele volta para o servidor (laço). Receba em outro aparelho ou no OBS.
          </p>
        )}
      </Card>

      <Card title={<span className="inline-flex items-center gap-1.5"><ArrowDown size={13} weight="bold" className="text-cyan" /> Receber do servidor</span>}>
        <p className="text-[12px] text-subtle">
          O que o servidor manda para “{cfg.name}” (ex.: a Transmissão). Para o OBS, escolha <b className="text-text">OBS neste computador</b> e, no OBS, adicione a fonte de áudio <b className="text-text">“Sona (transmissão)”</b>. Não precisa de cabo virtual.
        </p>
        {select(cfg.receive?.device, devices.filter((d) => d.outputs > 0), (v) => save({ receive: v ? { device: v, channels: 2 } : null }), 'Aparelho de recepção', [], true)}
        <Level peak={status?.recv_peak ?? 0} />
        {status?.receive_error && <p className="text-[12px] text-warn-light">{status.receive_error}</p>}
        {cfg.receive?.device === 'obs' && status?.obs_plugin === false && (
          <p className="rounded-lg border border-warn/40 bg-warn/10 px-2.5 py-1.5 text-[12px] text-warn-light">
            O plugin “Sona” não está na pasta do OBS. Reinstale o Sona Bridge (ele instala o plugin) e reabra o OBS.
          </p>
        )}
        {cfg.receive?.device === 'obs' && status?.obs_plugin && (
          <p className="text-[11.5px] text-muted">Plugin do OBS instalado. Se o OBS já estava aberto na instalação, reabra-o uma vez.</p>
        )}
      </Card>

      <MonitorCard cfg={cfg} status={status} outputs={devices.filter((d) => d.outputs > 0)} save={save} />

      <p className="text-[11.5px] text-muted md:col-span-2">
        No servidor, o que vai para este computador é escolhido em Roteamento: ligue a Transmissão à saída “{cfg.name}”. O bridge reconecta sozinho se a rede cair.
      </p>
    </div>
  );
}

const fmtVol = (db: number) => (db <= -60 ? '−∞' : `${db > 0 ? '+' : db < 0 ? '−' : ''}${Math.abs(db).toFixed(0)} dB`);

/** Escuta local: toca uma cópia do que chega (ou do que sai) num fone deste computador. */
function MonitorCard({ cfg, status, outputs, save }: { cfg: Config; status: Status | null; outputs: Dev[]; save: (c: Partial<Config>) => Promise<void> }) {
  const mon = cfg.monitor;
  const on = !!mon?.device;
  const [vol, setVol] = useState(mon?.volume_db ?? -12);
  const [dev, setDev] = useState(mon?.device || '');
  useEffect(() => {
    if (mon) setVol(mon.volume_db);
    if (mon?.device) setDev(mon.device);
  }, [mon]);
  const source = mon?.source ?? 'receive';
  const pick = (device: string) => {
    setDev(device);
    if (on && device) save({ monitor: { device, source, volume_db: vol } });
  };
  const toggle = () => {
    if (on) return save({ monitor: null });
    const d = dev || outputs[0]?.name || '';
    if (!d) return;
    setDev(d);
    save({ monitor: { device: d, source, volume_db: vol } });
  };
  const seg = (sel: boolean) => cx('h-9 flex-1 rounded-lg px-2 text-[12.5px] font-bold transition', sel ? 'bg-accent text-white' : 'text-subtle hover:text-text');
  return (
    <Card
      className={cx('md:col-span-2', on && 'border-accent/70')}
      title={<span className="inline-flex items-center gap-1.5"><Headphones size={14} weight="bold" className="text-cyan" /> Ouvir o que chega</span>}
      right={
        <button type="button" role="switch" aria-checked={on} aria-label="Ouvir o que chega" onClick={toggle} disabled={!on && !outputs.length} className={cx('relative h-6 w-11 rounded-full border transition disabled:opacity-40', on ? 'border-accent bg-accent' : 'border-line-hi bg-bg')}>
          <span className={cx('absolute top-0.5 h-[18px] w-[18px] rounded-full bg-white transition-all', on ? 'left-[22px]' : 'left-0.5')} />
        </button>
      }
    >
      <p className="text-[12px] text-subtle">Para conferir num fone deste computador. Não muda nada no que vai para o OBS nem no que volta para o servidor.</p>
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
        <div className="flex flex-col gap-1.5">
          <span className="text-[11.5px] font-bold text-muted">O que ouvir</span>
          <div role="group" aria-label="O que ouvir" className="flex rounded-[10px] border border-line bg-deep p-0.5">
            <button type="button" aria-pressed={source === 'receive'} className={seg(source === 'receive')} onClick={() => on && save({ monitor: { device: dev, source: 'receive', volume_db: vol } })} disabled={!on}>O que chega do servidor</button>
            <button type="button" aria-pressed={source === 'send'} className={seg(source === 'send')} onClick={() => on && save({ monitor: { device: dev, source: 'send', volume_db: vol } })} disabled={!on}>O que este PC envia</button>
          </div>
        </div>
        <label className="flex flex-col gap-1.5">
          <span className="text-[11.5px] font-bold text-muted">Tocar em</span>
          <select aria-label="Saída da escuta" value={dev} onChange={(e) => pick(e.target.value)} className="lm-field h-9 w-full px-2.5 text-[12.5px]">
            {!dev && <option value="">— escolha um fone —</option>}
            {dev && !outputs.some((d) => d.name === dev) && <option value={dev}>{dev} (não encontrado agora)</option>}
            {outputs.map((d) => (
              <option key={`${d.host}:${d.name}`} value={d.name}>{d.name} · {d.host}</option>
            ))}
          </select>
        </label>
      </div>
      <div className="flex items-center gap-3">
        <span className="w-16 text-[11.5px] font-bold text-muted">Volume</span>
        <input
          type="range"
          aria-label="Volume da escuta"
          min={-60}
          max={0}
          step={1}
          value={vol}
          onChange={(e) => setVol(Number(e.target.value))}
          onPointerUp={() => on && save({ monitor: { device: dev, source, volume_db: vol } })}
          onKeyUp={() => on && save({ monitor: { device: dev, source, volume_db: vol } })}
          className="h-1.5 flex-1 cursor-pointer accent-accent"
        />
        <span className="w-14 text-right font-mono text-[11.5px] text-subtle">{fmtVol(vol)}</span>
      </div>
      {on && <Level peak={status?.monitor_peak ?? 0} />}
      {status?.monitor_error && <p className="text-[12px] text-warn-light">{status.monitor_error}</p>}
      {on && source === 'receive' && cfg.receive?.device && cfg.receive.device === dev && (
        <p className="text-[12px] text-warn-light">A escuta está na mesma saída da recepção: o som vai tocar duas vezes. Escolha outro fone.</p>
      )}
    </Card>
  );
}
