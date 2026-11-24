import { useEffect, useRef } from 'react';
import { meterData, onFrame } from '../lib/meters';
import { meterPos } from '../lib/level';
import { cx } from './kit';

type Pick = () => number;

/** Barra de nível com pico segurado e LED de clip. Atualiza fora do React. */
export function MeterBar({ pick, className }: { pick: Pick; className?: string }) {
  const bar = useRef<HTMLDivElement>(null);
  const hold = useRef<HTMLDivElement>(null);
  const clip = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let shown = 0;
    let held = 0;
    let heldAt = 0;
    let clipAt = -1e9;
    let last = performance.now();
    return onFrame((now) => {
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      const raw = pick() || 0;
      const target = meterPos(raw);
      // Sobe na hora, desce a ~20 dB/s (escala de 60 dB).
      shown = target > shown ? target : Math.max(target, shown - dt * (20 / 60));
      if (shown >= held || now - heldAt > 1500) {
        held = shown;
        heldAt = now;
      }
      if (raw >= 0.999) clipAt = now;
      if (bar.current) bar.current.style.clipPath = `inset(${(1 - shown) * 100}% 0 0 0)`;
      if (hold.current) hold.current.style.bottom = `${held * 100}%`;
      if (clip.current) clip.current.style.opacity = now - clipAt < 2000 ? '1' : '0.12';
    });
  }, [pick]);
  return (
    <div className={cx('relative flex w-1.5 flex-col', className)}>
      <div ref={clip} className="mb-0.5 h-1 w-full rounded-sm bg-live opacity-10 transition-opacity" aria-hidden />
      <div className="relative flex-1 overflow-hidden rounded-sm bg-deep shadow-[inset_0_0_0_1px_#1a1d26]">
        <div ref={bar} className="sn-meter absolute inset-0" style={{ clipPath: 'inset(100% 0 0 0)' }} />
        <div ref={hold} className="absolute left-0 right-0 h-px bg-hi/80" style={{ bottom: 0 }} />
      </div>
    </div>
  );
}

export const pickChannel = (i: number) => () => meterData.ch[i] ?? 0;
export const pickBus = (b: number, side: 0 | 1) => () => meterData.bus[b]?.[side] ?? 0;
