import { useState } from 'react';
import { useMixer } from '../lib/store';
import { BUS_KIND_LABEL } from '../lib/types';
import { BusStrip } from './BusStrip';
import { Detail } from './Detail';
import { Strip } from './Strip';
import { cx } from './kit';

/** A mesa: camada de faders, canais e masters. */
export function Console() {
  const st = useMixer((s) => s.state)!;
  const [layer, setLayer] = useState<number | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const layerBus = layer === null ? null : st.mixer.buses[layer];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-2 overflow-x-auto px-4 py-2" role="tablist" aria-label="Camada dos faders">
        <span className="mr-1 text-[11px] font-bold uppercase tracking-[0.08em] text-muted">Faders</span>
        <LayerChip active={layer === null} onClick={() => setLayer(null)} label="Canais" hint="Fader do canal" />
        {st.mixer.buses.map((b, i) => (
          <LayerChip key={i} active={layer === i} onClick={() => setLayer(i)} label={b.name} hint={`Envios → ${BUS_KIND_LABEL[b.kind]}`} dim={!b.enabled} />
        ))}
        {layerBus && <span className="ml-2 rounded-full bg-cyan/10 px-2.5 py-1 text-[11.5px] font-semibold text-cyan">Os faders agora mexem nos envios para {layerBus.name}</span>}
      </div>
      <div className="flex min-h-0 flex-1 gap-3 px-4 pb-3">
        <div className="flex min-h-0 min-w-0 flex-1 gap-1.5 overflow-x-auto overflow-y-hidden pb-1">
          {st.mixer.channels.map((ch, i) => (
            <Strip key={i} index={i} ch={ch} layer={layer} selected={selected === i} onSelect={() => setSelected(selected === i ? null : i)} />
          ))}
        </div>
        <div className="flex min-h-0 shrink-0 gap-1.5 border-l border-line/70 pl-3">
          {st.mixer.buses.map((b, i) => (
            <BusStrip key={i} index={i} bus={b} active={layer === i} onLayer={() => setLayer(layer === i ? null : i)} />
          ))}
        </div>
      </div>
      {selected !== null && <Detail index={selected} onClose={() => setSelected(null)} />}
    </div>
  );
}

function LayerChip({ active, onClick, label, hint, dim }: { active: boolean; onClick: () => void; label: string; hint: string; dim?: boolean }) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onClick}
      title={hint}
      className={cx('h-8 shrink-0 rounded-full border px-3.5 text-[12.5px] font-semibold transition', active ? 'lm-outline' : 'border-line bg-panel-2 text-subtle hover:border-line-hi hover:text-text', dim && !active && 'opacity-60')}
    >
      {label}
    </button>
  );
}
