import { useMemo, useState } from 'react';
import { meterData } from '../lib/meters';
import { fmtDb, OFF_DB } from '../lib/level';
import { useMixer } from '../lib/store';
import type { ChannelConfig, SendConfig, ServerState } from '../lib/types';
import { Fader } from './Fader';
import { MeterBar } from './Meter';
import { cx, Toggle } from './kit';
import { useEffect, useRef } from 'react';
import { onFrame } from '../lib/meters';

export function sourceLabel(ch: ChannelConfig, st: ServerState): { text: string; missing: boolean } {
  const ports = st.io.inputs;
  const s = ch.source;
  if (s.kind === 'none') return { text: 'sem fonte', missing: false };
  const a = s.kind === 'mono' ? s.input : s.left;
  const pa = ports[a];
  if (!pa) return { text: `entrada ${a + 1} ausente`, missing: true };
  if (s.kind === 'mono') return { text: pa.name, missing: !pa.connected };
  return { text: pa.name.replace(/ L$/, '') + ' L/R', missing: !pa.connected };
}

/** Atualiza sends trocando (ou criando) o envio para um bus. */
export function withSend(ch: ChannelConfig, bus: number, change: Partial<SendConfig>): SendConfig[] {
  const has = ch.sends.some((s) => s.bus === bus);
  if (has) return ch.sends.map((s) => (s.bus === bus ? { ...s, ...change } : s));
  return [...ch.sends, { bus, enabled: true, level_db: OFF_DB, pre_fader: false, ...change }];
}

/** Luzes de dinâmica: gate fechando (G) e redução do compressor (C). */
function DynLights({ index, ch }: { index: number; ch: ChannelConfig }) {
  const g = useRef<HTMLSpanElement>(null);
  const c = useRef<HTMLSpanElement>(null);
  useEffect(
    () =>
      onFrame(() => {
        const gate = meterData.gate[index] ?? 1;
        const gr = meterData.gr[index] ?? 0;
        if (g.current) g.current.dataset.on = String(ch.gate.enabled && gate < 0.5);
        if (c.current) c.current.dataset.on = String(ch.comp.enabled && gr < -1);
      }),
    [index, ch.gate.enabled, ch.comp.enabled],
  );
  const base = 'grid h-4 w-4 place-items-center rounded-[4px] font-mono text-[9px] font-bold border';
  return (
    <div className="flex gap-1" aria-hidden>
      <span ref={g} className={cx(base, ch.gate.enabled ? 'border-line-hi text-subtle data-[on=true]:border-warn data-[on=true]:bg-warn/20 data-[on=true]:text-warn-light' : 'border-line text-line')}>G</span>
      <span ref={c} className={cx(base, ch.comp.enabled ? 'border-line-hi text-subtle data-[on=true]:border-cyan data-[on=true]:bg-cyan/20 data-[on=true]:text-cyan' : 'border-line text-line')}>C</span>
      <span className={cx(base, ch.eq.enabled && (ch.eq.high_pass_enabled || ch.eq.bands.some((b) => b.enabled && b.gain_db !== 0)) ? 'border-accent/60 text-accent-text' : 'border-line text-line')}>E</span>
    </div>
  );
}

export function Strip({ index, ch, layer, selected, onSelect }: { index: number; ch: ChannelConfig; layer: number | null; selected: boolean; onSelect: () => void }) {
  const st = useMixer((s) => s.state)!;
  const patch = useMixer((s) => s.patch);
  const myMonitor = useMixer((s) => s.myMonitor);
  const [editing, setEditing] = useState(false);
  const src = sourceLabel(ch, st);
  const pick = useMemo(() => () => meterData.ch[index] ?? 0, [index]);

  const bus = layer === null ? null : st.mixer.buses[layer];
  const send = layer === null ? undefined : ch.sends.find((s) => s.bus === layer);
  const faderDb = layer === null ? ch.fader_db : (send?.level_db ?? OFF_DB);
  const mon = myMonitor !== null ? st.mixer.buses[myMonitor]?.monitor : null;
  const soloed = !!mon?.solo.includes(index);

  return (
    <div
      className={cx('flex w-[78px] shrink-0 flex-col items-center gap-1.5 rounded-xl border px-1.5 pb-2 pt-1.5 transition-colors', selected ? 'lm-selected border-transparent' : 'border-line/60 bg-panel/70 hover:border-line')}
      onClick={(e) => {
        if ((e.target as HTMLElement).closest('button,input,[role=slider]')) return;
        onSelect();
      }}
    >
      <div className="flex h-9 w-full flex-col items-center justify-center" onDoubleClick={() => setEditing(true)}>
        <span className="font-mono text-[10px] text-muted">{index + 1}</span>
        {editing ? (
          <input
            autoFocus
            defaultValue={ch.name}
            className="lm-field h-5 w-full px-1 text-center text-[11.5px]"
            onBlur={(e) => {
              setEditing(false);
              if (e.target.value.trim() && e.target.value !== ch.name) patch(`/channels/${index}`, { name: e.target.value.trim() });
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
              if (e.key === 'Escape') setEditing(false);
            }}
          />
        ) : (
          <button type="button" onClick={onSelect} className="w-full truncate text-[12px] font-bold text-hi" title={`${ch.name} — duplo clique para renomear`}>
            {ch.name}
          </button>
        )}
      </div>
      <span className={cx('w-full truncate text-center font-mono text-[9.5px]', src.missing ? 'text-warn-light' : 'text-muted')} title={src.text}>
        {src.missing ? '⚠ ' : ''}
        {src.text}
      </span>
      <DynLights index={index} ch={ch} />

      <input
        type="range"
        aria-label={`Pan ${ch.name}`}
        min={-100}
        max={100}
        value={Math.round(ch.pan * 100)}
       
        onChange={(e) => {
          const v = Number(e.target.value);
          patch(`/channels/${index}`, { pan: Math.abs(v) < 4 ? 0 : v / 100 });
        }}
        onDoubleClick={() => patch(`/channels/${index}`, { pan: 0 })}
        className="h-1 w-full cursor-pointer accent-cyan"
        title={ch.pan === 0 ? 'Centro' : ch.pan < 0 ? `E ${Math.round(-ch.pan * 100)}` : `D ${Math.round(ch.pan * 100)}`}
      />

      {bus ? (
        <Toggle
          on={!!send?.pre_fader}
          tone="accent"
          className="w-full"
         
          onClick={() => patch(`/channels/${index}`, { sends: withSend(ch, layer!, { pre_fader: !send?.pre_fader }) })}
          title="Pré-fader: o envio não segue o fader do canal"
        >
          {send?.pre_fader ? 'PRÉ' : 'PÓS'}
        </Toggle>
      ) : (
        <Toggle
          on={soloed}
          tone="cyan"
          className="w-full"
          disabled={!mon}
          title={mon ? `Escutar no seu fone (${st.mixer.buses[myMonitor!].name}). Não muda a transmissão nem o PA.` : 'Escolha seu fone no topo para usar PFL'}
          onClick={() => {
            if (!mon || myMonitor === null) return;
            const solo = soloed ? mon.solo.filter((c) => c !== index) : [...mon.solo, index];
            patch(`/buses/${myMonitor}/monitor`, { solo });
          }}
        >
          PFL
        </Toggle>
      )}

      <div className="flex min-h-0 w-full flex-1 justify-center gap-1">
        <Fader
          db={faderDb}
          label={bus ? `Envio de ${ch.name} para ${bus.name}` : `Fader ${ch.name}`}
          tint={bus ? 'cyan' : 'accent'}
         
          scale={false}
          onChange={(v) => {
            if (layer === null) patch(`/channels/${index}`, { fader_db: v });
            else patch(`/channels/${index}`, { sends: withSend(ch, layer, { level_db: v }) });
          }}
        />
        <MeterBar pick={pick} />
      </div>

      <span className={cx('font-mono text-[11px]', faderDb > 0 ? 'text-warn-light' : 'text-hi')}>{fmtDb(faderDb)}</span>

      {bus ? (
        <Toggle on={!!send?.enabled} tone="ok" className="w-full" onClick={() => patch(`/channels/${index}`, { sends: withSend(ch, layer!, { enabled: !send?.enabled }) })}>
          {send?.enabled ? 'ON' : 'OFF'}
        </Toggle>
      ) : (
        <Toggle on={ch.mute} tone="warn" className="w-full" onClick={() => patch(`/channels/${index}`, { mute: !ch.mute })}>
          MUTE
        </Toggle>
      )}
    </div>
  );
}
