import { ArrowCounterClockwise, X } from '@phosphor-icons/react';
import { COMP_DEFAULT, EQ_BANDS_DEFAULT, GATE_DEFAULT, INPUT_DEFAULT } from '../lib/defaults';
import { fmtDb, fmtFreq, OFF_DB } from '../lib/level';
import { useMixer } from '../lib/store';
import type { ChannelConfig, EqBandKind } from '../lib/types';
import { EqCurve } from './EqCurve';
import { Card, cx, Param, Switch } from './kit';
import { sourceLabel, withSend } from './Strip';

const KIND_LABEL: Record<EqBandKind, string> = { low_shelf: 'Shelf grave', peaking: 'Sino', high_shelf: 'Shelf agudo' };

/** Processamento do canal selecionado: entrada, gate, EQ, compressor e envios. */
export function Detail({ index, onClose }: { index: number; onClose: () => void }) {
  const st = useMixer((s) => s.state)!;
  const engine = useMixer((s) => s.engine);
  const patch = useMixer((s) => s.patch);
  const ch = st.mixer.channels[index];
  if (!ch) return null;
  const dis = false;
  const p = (v: Partial<ChannelConfig>) => patch(`/channels/${index}`, v);
  const sr = engine?.sample_rate || 48000;
  // Restaurar: volta o módulo ao padrão. Fader, mute, envios, nome e fonte ficam.
  const resetInput = () => p({ trim_db: INPUT_DEFAULT.trim_db, polarity_invert: INPUT_DEFAULT.polarity_invert, eq: { ...ch.eq, high_pass_enabled: INPUT_DEFAULT.high_pass_enabled, high_pass_hz: INPUT_DEFAULT.high_pass_hz } });
  const resetGate = () => p({ gate: { ...GATE_DEFAULT } });
  const resetEq = () => p({ eq: { ...ch.eq, enabled: true, bands: EQ_BANDS_DEFAULT.map((b) => ({ ...b })) } });
  const resetComp = () => p({ comp: { ...COMP_DEFAULT } });
  const resetAll = () => {
    if (!confirm(`Restaurar todo o processamento de "${ch.name}" (ganho, polaridade, passa-altas, gate, EQ e compressor)? Fader, mute, pan, envios, nome e fonte ficam como estão.`)) return;
    p({
      trim_db: INPUT_DEFAULT.trim_db,
      polarity_invert: INPUT_DEFAULT.polarity_invert,
      gate: { ...GATE_DEFAULT },
      eq: { enabled: true, high_pass_enabled: INPUT_DEFAULT.high_pass_enabled, high_pass_hz: INPUT_DEFAULT.high_pass_hz, bands: EQ_BANDS_DEFAULT.map((b) => ({ ...b })) },
      comp: { ...COMP_DEFAULT },
    });
  };
  const reset = (onClick: () => void, what: string) => (
    <button
      type="button"
      disabled={dis}
      onClick={onClick}
      title={`Restaurar ${what}`}
      aria-label={`Restaurar ${what}`}
      className="grid h-6 w-6 place-items-center rounded-md text-muted transition hover:bg-panel hover:text-text disabled:opacity-40"
    >
      <ArrowCounterClockwise size={13} weight="bold" />
    </button>
  );
  const head = (sw: React.ReactNode, onReset: () => void, what: string) => (
    <span className="flex items-center gap-1.5">
      {reset(onReset, what)}
      {sw}
    </span>
  );

  return (
    <div className="flex max-h-[46vh] min-h-0 flex-col border-t border-line bg-panel/95 backdrop-blur" role="region" aria-label={`Processamento de ${ch.name}`}>
      <div className="flex items-center gap-3 px-4 pb-1 pt-3">
        <span className="font-mono text-[11px] text-muted">{index + 1}</span>
        <h2 className="text-[14.5px] font-bold text-hi">{ch.name}</h2>
        <span className="font-mono text-[11px] text-muted">{sourceLabel(ch, st).text}</span>
        <span className="flex-1" />
        <button
          type="button"
          disabled={dis}
          onClick={resetAll}
          className="inline-flex h-8 items-center gap-1.5 rounded-lg border border-line bg-panel-2 px-3 text-[12px] font-semibold text-subtle hover:border-line-hi hover:text-text"
        >
          <ArrowCounterClockwise size={14} weight="bold" /> Restaurar canal
        </button>
        <button type="button" aria-label="Fechar processamento" onClick={onClose} className="grid h-8 w-8 place-items-center rounded-lg text-subtle hover:bg-panel-2 hover:text-text">
          <X size={16} weight="bold" />
        </button>
      </div>
      <div className="grid min-h-0 grid-cols-1 gap-3 overflow-y-auto px-4 pb-4 md:grid-cols-[200px_200px_minmax(320px,1fr)_220px] xl:grid-cols-[200px_200px_minmax(380px,1fr)_220px_240px]">
        <Card title="Entrada" right={reset(resetInput, 'a entrada')}>
          <Param label="Ganho (trim)" value={ch.trim_db} min={-24} max={24} unit="dB" disabled={dis} onChange={(v) => p({ trim_db: v })} format={fmtDb} />
          <div className="flex items-center justify-between">
            <span className="text-[11.5px] font-semibold text-subtle">Inverter polaridade</span>
            <Switch label="Inverter polaridade" on={ch.polarity_invert} disabled={dis} onChange={(v) => p({ polarity_invert: v })} />
          </div>
          <div className="flex items-center justify-between">
            <span className="text-[11.5px] font-semibold text-subtle">Passa-altas</span>
            <Switch label="Passa-altas" on={ch.eq.high_pass_enabled} disabled={dis} onChange={(v) => p({ eq: { ...ch.eq, high_pass_enabled: v } })} />
          </div>
          <Param label="Corte" value={ch.eq.high_pass_hz} min={20} max={400} unit="Hz" log disabled={dis || !ch.eq.high_pass_enabled} onChange={(v) => p({ eq: { ...ch.eq, high_pass_hz: v } })} />
        </Card>

        <Card title="Gate" right={head(<Switch label="Gate ligado" on={ch.gate.enabled} disabled={dis} onChange={(v) => p({ gate: { ...ch.gate, enabled: v } })} />, resetGate, 'o gate')}>
          <Param label="Limiar" value={ch.gate.threshold_db} min={-80} max={0} step={0.5} unit="dB" disabled={dis || !ch.gate.enabled} onChange={(v) => p({ gate: { ...ch.gate, threshold_db: v } })} />
          <Param label="Atenuação" value={ch.gate.range_db} min={0} max={80} step={1} unit="dB" disabled={dis || !ch.gate.enabled} onChange={(v) => p({ gate: { ...ch.gate, range_db: v } })} />
          <Param label="Hold" value={ch.gate.hold_ms} min={0} max={500} step={1} unit="ms" disabled={dis || !ch.gate.enabled} onChange={(v) => p({ gate: { ...ch.gate, hold_ms: v } })} />
          <Param label="Release" value={ch.gate.release_ms} min={5} max={2000} unit="ms" log disabled={dis || !ch.gate.enabled} onChange={(v) => p({ gate: { ...ch.gate, release_ms: v } })} />
        </Card>

        <Card title="Equalizador" right={head(<Switch label="EQ ligado" on={ch.eq.enabled} disabled={dis} onChange={(v) => p({ eq: { ...ch.eq, enabled: v } })} />, resetEq, 'o equalizador')}>
          <EqCurve
            eq={ch.eq}
            sampleRate={sr}
            disabled={dis}
            onBand={(i, change) => p({ eq: { ...ch.eq, bands: ch.eq.bands.map((b, k) => (k === i ? { ...b, ...change } : b)) } })}
          />
          <div className="grid grid-cols-4 gap-2">
            {ch.eq.bands.map((b, i) => (
              <div key={i} className="flex flex-col gap-1 rounded-lg bg-bg/60 p-2">
                <span className="flex items-center justify-between text-[10.5px] font-bold text-muted">
                  {i + 1} · {KIND_LABEL[b.kind]}
                  <input
                    type="checkbox"
                    aria-label={`Banda ${i + 1} ligada`}
                    checked={b.enabled}
                    disabled={dis}
                    onChange={(e) => p({ eq: { ...ch.eq, bands: ch.eq.bands.map((x, k) => (k === i ? { ...x, enabled: e.target.checked } : x)) } })}
                    className="accent-accent"
                  />
                </span>
                <span className="font-mono text-[11px] text-hi">{fmtFreq(b.freq_hz)} Hz</span>
                <span className={cx('font-mono text-[11px]', b.gain_db === 0 ? 'text-muted' : 'text-accent-text')}>{fmtDb(b.gain_db)} dB</span>
                <span className="font-mono text-[10px] text-muted">Q {b.q.toFixed(2)}</span>
              </div>
            ))}
          </div>
        </Card>

        <Card title="Compressor" right={head(<Switch label="Compressor ligado" on={ch.comp.enabled} disabled={dis} onChange={(v) => p({ comp: { ...ch.comp, enabled: v } })} />, resetComp, 'o compressor')}>
          <Param label="Limiar" value={ch.comp.threshold_db} min={-60} max={0} step={0.5} unit="dB" disabled={dis || !ch.comp.enabled} onChange={(v) => p({ comp: { ...ch.comp, threshold_db: v } })} />
          <Param label="Razão" value={ch.comp.ratio} min={1} max={20} step={0.1} unit=":1" disabled={dis || !ch.comp.enabled} onChange={(v) => p({ comp: { ...ch.comp, ratio: v } })} />
          <Param label="Ataque" value={ch.comp.attack_ms} min={0.1} max={200} step={0.1} unit="ms" disabled={dis || !ch.comp.enabled} onChange={(v) => p({ comp: { ...ch.comp, attack_ms: v } })} />
          <Param label="Release" value={ch.comp.release_ms} min={10} max={2000} unit="ms" log disabled={dis || !ch.comp.enabled} onChange={(v) => p({ comp: { ...ch.comp, release_ms: v } })} />
          <Param label="Ganho de compensação" value={ch.comp.makeup_db} min={0} max={24} step={0.5} unit="dB" disabled={dis || !ch.comp.enabled} onChange={(v) => p({ comp: { ...ch.comp, makeup_db: v } })} />
        </Card>

        <Card title="Envios" className="md:col-span-4 xl:col-span-1">
          {st.mixer.buses.map((bus, b) => {
            const s = ch.sends.find((x) => x.bus === b);
            return (
              <div key={b} className="grid grid-cols-[1fr_auto_auto] items-center gap-2">
                <Param
                  label={bus.name}
                  value={s?.level_db ?? OFF_DB}
                  min={-60}
                  max={10}
                  step={0.5}
                  unit="dB"
                  format={(v) => (s?.level_db ?? OFF_DB) <= OFF_DB ? '−∞' : fmtDb(v)}
                  disabled={dis || !s?.enabled}
                  onChange={(v) => p({ sends: withSend(ch, b, { level_db: v <= -60 ? OFF_DB : v }) })}
                />
                <button
                  type="button"
                  disabled={dis}
                  onClick={() => p({ sends: withSend(ch, b, { pre_fader: !s?.pre_fader }) })}
                  className={cx('h-6 rounded-md border px-1.5 font-mono text-[10px]', s?.pre_fader ? 'border-accent text-accent-text' : 'border-line text-muted')}
                >
                  {s?.pre_fader ? 'PRÉ' : 'PÓS'}
                </button>
                <Switch label={`Envio para ${bus.name}`} on={!!s?.enabled} disabled={dis} onChange={(v) => p({ sends: withSend(ch, b, { enabled: v }) })} />
              </div>
            );
          })}
        </Card>
      </div>
    </div>
  );
}
