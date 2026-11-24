import { useMemo } from 'react';
import { meterData } from '../lib/meters';
import { fmtDb } from '../lib/level';
import { useMixer } from '../lib/store';
import { BUS_KIND_LABEL, type BusConfig } from '../lib/types';
import { Fader } from './Fader';
import { MeterBar } from './Meter';
import { cx, Toggle } from './kit';

export function OnAir({ on }: { on: boolean }) {
  return (
    <span className={cx('inline-flex items-center gap-1 rounded-full px-1.5 py-px font-mono text-[9px] font-bold tracking-wider', on ? 'bg-live/15 text-live-light' : 'bg-panel-2 text-muted')}>
      <span className={cx('h-1.5 w-1.5 rounded-full', on ? 'bg-live shadow-[0_0_8px_#ef4444]' : 'bg-line-hi')} />
      {on ? 'NO AR' : 'FORA'}
    </span>
  );
}

export function BusStrip({ index, bus, active, onLayer }: { index: number; bus: BusConfig; active: boolean; onLayer: () => void }) {
  const st = useMixer((s) => s.state)!;
  const patch = useMixer((s) => s.patch);
  const toast = useMixer((s) => s.toast);
  const pickL = useMemo(() => () => meterData.bus[index]?.[0] ?? 0, [index]);
  const pickR = useMemo(() => () => meterData.bus[index]?.[1] ?? 0, [index]);
  const outs = bus.outputs.map((o) => st.io.outputs[o.left]?.name.replace(/ L$/, '') ?? '?').join(', ') || 'sem saída';
  const live = bus.enabled && !bus.mute && bus.outputs.length > 0;
  const isMon = bus.kind === 'monitor';

  return (
    <div className={cx('flex w-[92px] shrink-0 flex-col items-center gap-1.5 rounded-xl border px-1.5 pb-2 pt-1.5', active ? 'lm-selected border-transparent' : 'border-line/60 bg-panel/80')}>
      <button type="button" onClick={onLayer} className="flex h-9 w-full flex-col items-center justify-center" title="Mostrar os envios deste bus nos faders">
        <span className="text-[9.5px] font-bold uppercase tracking-[0.08em] text-muted">{BUS_KIND_LABEL[bus.kind]}</span>
        <span className="w-full truncate text-[12px] font-bold text-hi">{bus.name}</span>
      </button>
      <span className="w-full truncate text-center font-mono text-[9.5px] text-muted" title={outs}>
        → {outs}
      </span>
      {bus.kind === 'broadcast' ? (
        <OnAir on={live} />
      ) : bus.kind === 'pa_return' ? (
        <Toggle
          on={bus.enabled}
          tone="warn"
          className="w-full"
          title="Liga o retorno para a mesa física. Confira se o canal da mesa não volta para o mesmo retorno."
          onClick={() => {
            if (!bus.enabled && !confirm(`Ligar "${bus.name}"? O áudio passa a voltar para a mesa física. Confira se não há realimentação.`)) return;
            patch(`/buses/${index}`, { enabled: !bus.enabled });
            toast('info', bus.enabled ? `${bus.name} desligado.` : `${bus.name} ligado.`);
          }}
        >
          {bus.enabled ? 'LIGADO' : 'DESLIGADO'}
        </Toggle>
      ) : isMon ? (
        <Toggle
          on={!!bus.monitor?.solo.length}
          tone="cyan"
          className="w-full"
          disabled={!bus.monitor?.solo.length}
          onClick={() => patch(`/buses/${index}/monitor`, { solo: [] })}
          title="Limpa o PFL deste fone"
        >
          {bus.monitor?.solo.length ? `PFL ${bus.monitor.solo.length} ✕` : 'SEM PFL'}
        </Toggle>
      ) : (
        <span className="h-7" />
      )}
      <div className="flex min-h-0 w-full flex-1 justify-center gap-1">
        <Fader db={bus.fader_db} label={`Master ${bus.name}`} onChange={(v) => patch(`/buses/${index}`, { fader_db: v })} />
        <MeterBar pick={pickL} />
        <MeterBar pick={pickR} />
      </div>
      <span className="font-mono text-[11px] text-hi">{fmtDb(bus.fader_db)}</span>
      <Toggle on={bus.mute} tone="warn" className="w-full" onClick={() => patch(`/buses/${index}`, { mute: !bus.mute })}>
        MUTE
      </Toggle>
    </div>
  );
}
