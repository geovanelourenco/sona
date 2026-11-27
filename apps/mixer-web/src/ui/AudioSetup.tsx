import { ArrowsClockwise, Plus, Trash, WarningCircle } from '@phosphor-icons/react';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { tokenStore, useMixer } from '../lib/store';
import { Btn, Card, cx } from './kit';

// Espelho de crates/sona-server/src/audio/config.rs e host.rs.
interface Secondary {
  device: string;
  direction: 'input' | 'output';
  channels: number;
  label: string;
}
interface AudioConfig {
  sample_rate: number;
  block: number;
  primary: string;
  primary_channels: number;
  secondary: Secondary[];
}
export interface DeviceStatus {
  role: 'principal' | 'entrada' | 'saída';
  name: string;
  label: string;
  connected: boolean;
  error: string | null;
  drift_ppm: number | null;
  latency_ms: number | null;
  underruns: number;
  overruns: number;
}
interface Range {
  channels: number;
  min_rate: number;
  max_rate: number;
}
interface DeviceInfo {
  host: string;
  name: string;
  inputs: Range[];
  outputs: Range[];
}

const auth = () => ({ Authorization: `Bearer ${tokenStore.get()}` });
const maxCh = (r: Range[], sr: number) => Math.max(0, ...r.filter((x) => x.min_rate <= sr && x.max_rate >= sr).map((x) => x.channels));

/** Dispositivos de áudio: principal (relógio), entradas e saídas extras. */
export function AudioSetup() {
  const toast = useMixer((s) => s.toast);
  const engine = useMixer((s) => s.engine);
  const [saved, setSaved] = useState<AudioConfig | null>(null);
  const [cfg, setCfg] = useState<AudioConfig | null>(null);
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const [scanning, setScanning] = useState(false);

  const load = useCallback(async () => {
    const r = await fetch('/api/audio', { headers: auth() });
    if (!r.ok) return;
    const j = await r.json();
    setSaved(j.config);
    setCfg(structuredClone(j.config));
  }, []);
  const scan = useCallback(async () => {
    setScanning(true);
    try {
      const r = await fetch('/api/devices', { headers: auth() });
      if (r.ok) setDevices((await r.json()).devices ?? []);
    } finally {
      setScanning(false);
    }
  }, []);
  useEffect(() => {
    load();
    scan();
  }, [load, scan]);

  const status: DeviceStatus[] = (engine as unknown as { devices?: DeviceStatus[] })?.devices ?? [];
  const dirty = useMemo(() => JSON.stringify(cfg) !== JSON.stringify(saved), [cfg, saved]);
  if (!cfg) return <div className="p-6 text-subtle">Carregando…</div>;
  const sr = cfg.sample_rate;
  const outputsOf = devices.filter((d) => maxCh(d.outputs, sr) > 0);
  const inputsOf = devices.filter((d) => maxCh(d.inputs, sr) > 0);
  const set = (change: Partial<AudioConfig>) => setCfg({ ...cfg, ...change });
  const setSec = (i: number, change: Partial<Secondary>) => set({ secondary: cfg.secondary.map((s, k) => (k === i ? { ...s, ...change } : s)) });
  const primaryStatus = status.find((s) => s.role === 'principal');
  const secStatus = (i: number) => status.filter((s) => s.role !== 'principal')[i];
  const usedCh = cfg.secondary.reduce((a, s) => a + s.channels, 0);

  const apply = async () => {
    if (!confirm('Aplicar a configuração de áudio? O som para por cerca de 1 segundo enquanto os aparelhos são reabertos.')) return;
    setBusy(true);
    try {
      const r = await fetch('/api/audio', { method: 'POST', headers: { ...auth(), 'Content-Type': 'application/json' }, body: JSON.stringify(cfg) });
      const j = await r.json().catch(() => ({}));
      if (!r.ok) return toast('error', j.error ?? 'Não consegui aplicar.');
      toast('info', 'Configuração de áudio aplicada.');
      await load();
    } finally {
      setBusy(false);
    }
  };

  // Valor guardado: "HOST:Nome" (ex.: "ASIO:Soundcraft…"); o nome sozinho
  // casaria com os pares WASAPI da Soundcraft, que contêm o mesmo texto.
  const key = (d: DeviceInfo) => `${d.host}:${d.name}`;
  const deviceSelect = (value: string, list: DeviceInfo[], onChange: (v: string) => void, extra: { value: string; label: string }[] = [], label = 'Aparelho') => {
    const known = extra.some((e) => e.value === value) || list.some((d) => key(d) === value || d.name === value);
    const asio = list.filter((d) => d.host === 'ASIO');
    const rest = list.filter((d) => d.host !== 'ASIO');
    const opt = (d: DeviceInfo) => (
      <option key={key(d)} value={list.some((x) => x.name === value && x === d) && !value.includes(':') ? value : key(d)}>
        {d.name} · {d.host}
        {d.host === 'ASIO' ? ` (${maxCh(d.inputs, sr)} entradas, ${maxCh(d.outputs, sr)} saídas)` : ''}
      </option>
    );
    return (
      <select aria-label={label} value={value} onChange={(e) => onChange(e.target.value)} className="lm-field h-8 min-w-0 flex-1 px-2 text-[12px]">
        {extra.map((e) => (
          <option key={e.value} value={e.value}>{e.label}</option>
        ))}
        {!known && value && <option value={value}>{value.replace(/^[A-Z]+:/, '')} (não encontrado agora)</option>}
        {asio.length > 0 && <optgroup label="ASIO (todos os canais da mesa num dispositivo)">{asio.map(opt)}</optgroup>}
        {rest.length > 0 && <optgroup label="Windows / sistema">{rest.map(opt)}</optgroup>}
      </select>
    );
  };

  return (
    <div className="grid min-h-0 flex-1 grid-cols-1 gap-3 overflow-auto p-4 xl:grid-cols-[minmax(0,1fr)_380px]">
      <div className="flex min-w-0 flex-col gap-3">
        <Card
          title="Dispositivo principal"
          right={<StatusBadge s={primaryStatus} />}
        >
          <p className="text-[12px] text-subtle">Dita o relógio do áudio. Na igreja: a Ui24R pelo <b className="text-text">ASIO</b> (todos os canais USB num dispositivo só; os itens “Line Out xx/yy · WASAPI” são pares de 2 canais). Sem mesa ligada, use o relógio interno.</p>
          <div className="flex flex-wrap items-center gap-2">
            {deviceSelect(cfg.primary, outputsOf, (v) => set({ primary: v }), [
              { value: 'internal', label: 'Relógio interno (sem mesa)' },
              { value: 'sim', label: 'Mesa simulada (demonstração)' },
            ], 'Dispositivo principal')}
            <label className="flex items-center gap-1.5 text-[12px] text-subtle">
              Canais
              <input
                type="number"
                min={0}
                max={64}
                value={cfg.primary_channels}
                onChange={(e) => set({ primary_channels: Math.max(0, Math.min(64, Number(e.target.value) || 0)) })}
                className="lm-field h-8 w-16 px-2 text-[12px]"
                aria-label="Limite de canais do principal (0 = todos)"
                title="0 = todos que o aparelho oferecer"
              />
            </label>
          </div>
          {primaryStatus?.error && <Problem text={primaryStatus.error} />}
        </Card>

        <Card
          title="Entradas e saídas extras"
          right={<span className="font-mono text-[11px] text-muted">{usedCh}/32 canais</span>}
        >
          <p className="text-[12px] text-subtle">
            Aparelhos com relógio próprio, sincronizados automaticamente com o principal. Ex.: saída <b className="text-text">CABLE Input</b> para o OBS neste computador; entrada de um cabo virtual onde o Lumo toca; um fone.
          </p>
          {cfg.secondary.length === 0 && <p className="rounded-lg border border-line bg-bg/60 px-3 py-2 text-[12px] text-muted">Nenhum aparelho extra.</p>}
          {cfg.secondary.map((s, i) => {
            const st = secStatus(i);
            return (
              <div key={i} className="flex flex-col gap-2 rounded-lg border border-line bg-bg/50 p-2.5">
                <div className="flex flex-wrap items-center gap-2">
                  <span className={cx('rounded-full px-2 py-0.5 text-[10.5px] font-bold', s.direction === 'output' ? 'bg-cyan/15 text-cyan' : 'bg-accent/15 text-accent-text')}>{s.direction === 'output' ? 'SAÍDA' : 'ENTRADA'}</span>
                  {deviceSelect(s.device, s.direction === 'output' ? outputsOf : inputsOf, (v) => setSec(i, { device: v, label: s.label || v.replace(/^[A-Z]+:/, '').split(' (')[0] }))}
                  <input value={s.label} onChange={(e) => setSec(i, { label: e.target.value })} placeholder="Rótulo (ex.: OBS)" aria-label="Rótulo" className="lm-field h-8 w-36 px-2 text-[12px]" />
                  <select aria-label="Canais" value={s.channels} onChange={(e) => setSec(i, { channels: Number(e.target.value) })} className="lm-field h-8 px-2 text-[12px]">
                    <option value={1}>Mono</option>
                    <option value={2}>Estéreo</option>
                    {[4, 6, 8].map((n) => (
                      <option key={n} value={n}>{n} canais</option>
                    ))}
                  </select>
                  <button type="button" aria-label="Remover aparelho" onClick={() => set({ secondary: cfg.secondary.filter((_, k) => k !== i) })} className="grid h-8 w-8 place-items-center rounded-lg text-subtle hover:bg-panel-2 hover:text-live-light">
                    <Trash size={15} weight="bold" />
                  </button>
                </div>
                {st && !dirty && (
                  <div className="flex flex-wrap items-center gap-3 font-mono text-[11px] text-muted">
                    <StatusBadge s={st} />
                    {st.connected && st.drift_ppm !== null && <span title="Diferença de relógio corrigida">deriva {st.drift_ppm > 0 ? '+' : ''}{st.drift_ppm} ppm</span>}
                    {st.connected && st.latency_ms !== null && <span>atraso {st.latency_ms} ms</span>}
                    {st.underruns + st.overruns > 0 && <span className="text-warn-light">{st.underruns + st.overruns} falhas</span>}
                  </div>
                )}
                {st?.error && !dirty && <Problem text={st.error} />}
              </div>
            );
          })}
          <div className="flex gap-2">
            <Btn onClick={() => set({ secondary: [...cfg.secondary, { device: outputsOf[0] ? key(outputsOf[0]) : '', direction: 'output', channels: 2, label: '' }] })}>
              <Plus size={14} weight="bold" /> Saída
            </Btn>
            <Btn onClick={() => set({ secondary: [...cfg.secondary, { device: inputsOf[0] ? key(inputsOf[0]) : '', direction: 'input', channels: 2, label: '' }] })}>
              <Plus size={14} weight="bold" /> Entrada
            </Btn>
            <span className="flex-1" />
            <Btn variant="ghost" onClick={scan} disabled={scanning}>
              <ArrowsClockwise size={14} weight="bold" className={cx(scanning && 'animate-spin')} /> Procurar aparelhos
            </Btn>
          </div>
        </Card>
      </div>

      <div className="flex flex-col gap-3">
        <Card title="Formato">
          <div className="grid grid-cols-2 gap-2">
            <label className="flex flex-col gap-1 text-[11.5px] font-semibold text-subtle">
              Taxa
              <select value={cfg.sample_rate} onChange={(e) => set({ sample_rate: Number(e.target.value) })} className="lm-field h-8 px-2 text-[12px]">
                {[44100, 48000, 96000].map((r) => (
                  <option key={r} value={r}>{r / 1000} kHz</option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1 text-[11.5px] font-semibold text-subtle">
              Bloco
              <select value={cfg.block} onChange={(e) => set({ block: Number(e.target.value) })} className="lm-field h-8 px-2 text-[12px]">
                {[64, 128, 256, 512].map((b) => (
                  <option key={b} value={b}>{b} ({((b / cfg.sample_rate) * 1000).toFixed(1)} ms)</option>
                ))}
              </select>
            </label>
          </div>
          <p className="text-[11.5px] text-muted">48 kHz é o padrão da Ui24R e do OBS. Bloco menor = menos atraso e mais CPU.</p>
        </Card>
        <Card title="Aplicar">
          <p className="text-[12px] text-subtle">{dirty ? 'Há mudanças não aplicadas. O som para por cerca de 1 s ao aplicar; o mix e as cenas ficam como estão.' : 'Tudo aplicado.'}</p>
          <div className="flex gap-2">
            <Btn variant="primary" disabled={!dirty || busy} onClick={apply}>
              {busy ? 'Aplicando…' : 'Aplicar'}
            </Btn>
            <Btn variant="ghost" disabled={!dirty || busy} onClick={() => setCfg(structuredClone(saved))}>
              Desfazer
            </Btn>
          </div>
        </Card>
        <Card title="OBS neste computador">
          <ol className="list-decimal space-y-1 pl-4 text-[12px] text-subtle">
            <li>Instale um cabo virtual (ex.: VB-Cable).</li>
            <li>Aqui: adicione uma <b className="text-text">Saída</b> “CABLE Input”, rótulo “OBS”.</li>
            <li>Em Roteamento, ligue a Transmissão à saída “OBS”.</li>
            <li>No OBS: Captura de entrada de áudio → “CABLE Output”.</li>
          </ol>
          <p className="text-[11.5px] text-muted">OBS em outro computador: instale o Sona Bridge nele.</p>
        </Card>
      </div>
    </div>
  );
}

export function StatusBadge({ s }: { s?: DeviceStatus }) {
  if (!s) return null;
  return (
    <span className={cx('inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[10.5px] font-bold', s.connected ? 'bg-ok/15 text-ok-light' : 'bg-warn/15 text-warn-light')}>
      <span className={cx('h-1.5 w-1.5 rounded-full', s.connected ? 'bg-ok' : 'bg-warn')} />
      {s.connected ? 'CONECTADO' : 'FORA DO AR'}
    </span>
  );
}

function Problem({ text }: { text: string }) {
  return (
    <p className="flex items-start gap-2 rounded-lg border border-warn/40 bg-warn/10 px-2.5 py-1.5 text-[12px] text-warn-light">
      <WarningCircle size={15} weight="bold" className="mt-px shrink-0" />
      {text}
    </p>
  );
}
