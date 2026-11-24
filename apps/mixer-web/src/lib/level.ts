// Conversões e curva do fader. Testado em level.test.ts.

export const OFF_DB = -90;
export const FADER_MAX_DB = 10;

/**
 * Curva do fader (posição 0..1 → dB), no jeito das mesas: 0 dB a 3/4 do curso,
 * resolução fina perto do 0 e grossa embaixo.
 */
const POINTS: [number, number][] = [
  [0, OFF_DB],
  [0.05, -60],
  [0.2, -40],
  [0.4, -20],
  [0.55, -10],
  [0.65, -5],
  [0.75, 0],
  [0.875, 5],
  [1, FADER_MAX_DB],
];

export function posToDb(pos: number): number {
  const p = Math.min(1, Math.max(0, pos));
  for (let i = 1; i < POINTS.length; i++) {
    const [p0, d0] = POINTS[i - 1];
    const [p1, d1] = POINTS[i];
    if (p <= p1) return d0 + ((p - p0) / (p1 - p0)) * (d1 - d0);
  }
  return FADER_MAX_DB;
}

export function dbToPos(db: number): number {
  const d = Math.min(FADER_MAX_DB, Math.max(OFF_DB, db));
  for (let i = 1; i < POINTS.length; i++) {
    const [p0, d0] = POINTS[i - 1];
    const [p1, d1] = POINTS[i];
    if (d <= d1) return p0 + ((d - d0) / (d1 - d0)) * (p1 - p0);
  }
  return 1;
}

export function fmtDb(db: number): string {
  if (db <= OFF_DB) return '−∞';
  const r = Math.round(db * 10) / 10;
  return (r > 0 ? '+' : r < 0 ? '−' : '') + Math.abs(r).toFixed(1);
}

export function gainToDb(g: number): number {
  return g <= 0 ? -Infinity : 20 * Math.log10(g);
}

/** Posição do medidor (0..1) para um pico linear: −60 dBFS a 0 dBFS. */
export function meterPos(peak: number): number {
  const db = gainToDb(peak);
  if (!isFinite(db) || db < -60) return 0;
  return Math.min(1, (db + 60) / 60);
}

export function fmtFreq(hz: number): string {
  return hz >= 1000 ? `${(hz / 1000).toFixed(hz >= 10000 ? 0 : 1)}k` : `${Math.round(hz)}`;
}
