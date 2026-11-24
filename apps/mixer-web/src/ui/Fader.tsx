import { useRef, useState } from 'react';
import { dbToPos, fmtDb, OFF_DB, posToDb } from '../lib/level';
import { cx } from './kit';

const TICKS = [10, 5, 0, -5, -10, -20, -40, -60];

/**
 * Fader vertical. Arrasto relativo (Shift = fino), duplo clique = 0 dB,
 * setas ±0,5 dB, PageUp/PageDown ±5 dB, Home = 0 dB, End = desligado.
 */
export function Fader({ db, onChange, label, tint = 'accent', disabled, scale = true }: {
  db: number;
  onChange: (db: number) => void;
  label: string;
  tint?: 'accent' | 'cyan' | 'warn';
  disabled?: boolean;
  scale?: boolean;
}) {
  const track = useRef<HTMLDivElement>(null);
  const drag = useRef<{ y: number; pos: number } | null>(null);
  const [local, setLocal] = useState<number | null>(null);
  const value = local ?? db;
  const pos = dbToPos(value);

  // Valor local só durante o arrasto (o eco do servidor chega atrasado). Fora
  // do arrasto o fader mostra sempre o valor do servidor.
  const set = (v: number) => {
    const c = Math.max(OFF_DB, Math.min(10, Math.round(v * 10) / 10));
    if (drag.current) setLocal(c);
    onChange(c);
  };

  return (
    <div className="flex h-full min-h-0 items-stretch gap-1">
      {scale && (
        <div className="w-5 select-none py-3 font-mono text-[9px] leading-none text-muted" aria-hidden>
          <div className="relative h-full">
            {TICKS.map((t) => (
              <span key={t} className="absolute right-0 translate-y-1/2" style={{ bottom: `${dbToPos(t) * 100}%` }}>
                {t > 0 ? `+${t}` : t}
              </span>
            ))}
          </div>
        </div>
      )}
      <div
        ref={track}
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        aria-valuemin={OFF_DB}
        aria-valuemax={10}
        aria-valuenow={Math.round(value * 10) / 10}
        aria-valuetext={`${fmtDb(value)} dB`}
        aria-disabled={disabled}
        className={cx('relative w-9 touch-none py-3 outline-offset-2', disabled ? 'cursor-not-allowed opacity-40' : 'cursor-ns-resize')}
        onDoubleClick={() => !disabled && set(0)}
        onKeyDown={(e) => {
          if (disabled) return;
          const step = { ArrowUp: 0.5, ArrowDown: -0.5, PageUp: 5, PageDown: -5 }[e.key];
          if (step) {
            e.preventDefault();
            set((value <= OFF_DB ? -60 : value) + step);
          } else if (e.key === 'Home') set(0);
          else if (e.key === 'End') set(OFF_DB);
        }}
        onPointerDown={(e) => {
          if (disabled) return;
          (e.target as HTMLElement).setPointerCapture(e.pointerId);
          drag.current = { y: e.clientY, pos };
        }}
        onPointerMove={(e) => {
          if (!drag.current || !track.current) return;
          const h = track.current.clientHeight - 24;
          const k = e.shiftKey ? 0.2 : 1;
          const p = drag.current.pos + ((drag.current.y - e.clientY) / h) * k;
          if (e.shiftKey) drag.current = { y: e.clientY, pos: p };
          set(posToDb(p));
        }}
        onPointerUp={() => {
          drag.current = null;
          setLocal(null);
        }}
        onPointerCancel={() => {
          drag.current = null;
          setLocal(null);
        }}
      >
        <div className="sn-slot absolute bottom-3 left-1/2 top-3 w-1.5 -translate-x-1/2 rounded-full" />
        <div
          className={cx('absolute bottom-3 left-1/2 w-1.5 -translate-x-1/2 rounded-full opacity-70', tint === 'cyan' ? 'bg-cyan' : tint === 'warn' ? 'bg-warn' : 'bg-accent')}
          style={{ height: `calc((100% - 24px) * ${pos})` }}
        />
        <div className="sn-cap pointer-events-none absolute left-0 right-0 h-6 rounded-md" style={{ bottom: `calc((100% - 24px) * ${pos})` }} />
      </div>
    </div>
  );
}
