import { useRef } from 'react';
import { responseDb } from '../lib/eq';
import { fmtFreq } from '../lib/level';
import type { EqConfig } from '../lib/types';

const W = 520;
const H = 180;
const FMIN = 20;
const FMAX = 20000;
const DB = 18;
const x = (f: number) => (Math.log(f / FMIN) / Math.log(FMAX / FMIN)) * W;
const fx = (px: number) => FMIN * Math.pow(FMAX / FMIN, Math.min(1, Math.max(0, px / W)));
const y = (db: number) => H / 2 - (db / DB) * (H / 2 - 8);
const dy = (py: number) => ((H / 2 - py) / (H / 2 - 8)) * DB;

/** Curva do EQ com pontos arrastáveis (x = frequência, y = ganho, roda = Q). */
export function EqCurve({ eq, sampleRate, disabled, onBand }: { eq: EqConfig; sampleRate: number; disabled?: boolean; onBand: (i: number, change: Partial<EqConfig['bands'][0]>) => void }) {
  const svg = useRef<SVGSVGElement>(null);
  const dragging = useRef<number | null>(null);
  const pts: string[] = [];
  for (let i = 0; i <= 200; i++) {
    const f = FMIN * Math.pow(FMAX / FMIN, i / 200);
    pts.push(`${x(f).toFixed(1)},${y(Math.max(-DB - 6, Math.min(DB + 6, responseDb(eq, f, sampleRate)))).toFixed(1)}`);
  }
  const toLocal = (e: React.PointerEvent) => {
    const r = svg.current!.getBoundingClientRect();
    return [((e.clientX - r.left) / r.width) * W, ((e.clientY - r.top) / r.height) * H];
  };
  const colors = ['#72d6ff', '#a78bfa', '#c4b5fd', '#72d6ff'];
  return (
    <svg
      ref={svg}
      viewBox={`0 0 ${W} ${H}`}
      className="h-auto w-full touch-none rounded-[10px] bg-deep"
      role="img"
      aria-label="Curva de resposta do equalizador"
      onPointerMove={(e) => {
        if (dragging.current === null) return;
        const [px, py] = toLocal(e);
        onBand(dragging.current, { freq_hz: Math.round(fx(px)), gain_db: Math.round(Math.max(-DB, Math.min(DB, dy(py))) * 10) / 10 });
      }}
      onPointerUp={() => (dragging.current = null)}
    >
      {[100, 1000, 10000].map((f) => (
        <g key={f}>
          <line x1={x(f)} x2={x(f)} y1={0} y2={H} stroke="#232836" />
          <text x={x(f) + 3} y={H - 4} fill="#868e9c" fontSize="9" fontFamily="JetBrains Mono">
            {fmtFreq(f)}
          </text>
        </g>
      ))}
      {[-12, -6, 6, 12].map((d) => (
        <line key={d} x1={0} x2={W} y1={y(d)} y2={y(d)} stroke="#1a1d26" />
      ))}
      <line x1={0} x2={W} y1={y(0)} y2={y(0)} stroke="#2f3647" />
      <polyline points={pts.join(' ')} fill="none" stroke="url(#sn-eq)" strokeWidth="2.2" opacity={eq.enabled ? 1 : 0.35} />
      <defs>
        <linearGradient id="sn-eq" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2={W} y2="0">
          <stop offset="0" stopColor="#7c5cff" />
          <stop offset="1" stopColor="#72d6ff" />
        </linearGradient>
      </defs>
      {eq.bands.map((b, i) => (
        <g
          key={i}
          style={{ cursor: disabled ? 'default' : 'grab' }}
          opacity={b.enabled ? 1 : 0.35}
          onPointerDown={(e) => {
            if (disabled) return;
            (e.currentTarget.ownerSVGElement as SVGSVGElement).setPointerCapture(e.pointerId);
            dragging.current = i;
          }}
          onWheel={(e) => !disabled && onBand(i, { q: Math.round(Math.max(0.1, Math.min(10, b.q * (e.deltaY < 0 ? 1.1 : 0.9))) * 100) / 100 })}
        >
          <circle cx={x(b.freq_hz)} cy={y(b.gain_db)} r={11} fill="transparent" />
          <circle cx={x(b.freq_hz)} cy={y(b.gain_db)} r={6.5} fill="#0b0d12" stroke={colors[i]} strokeWidth="2" />
          <text x={x(b.freq_hz)} y={y(b.gain_db) + 3} textAnchor="middle" fill={colors[i]} fontSize="8" fontFamily="JetBrains Mono" pointerEvents="none">
            {i + 1}
          </text>
        </g>
      ))}
    </svg>
  );
}
