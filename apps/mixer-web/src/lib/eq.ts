// Curva de resposta do EQ para desenhar. Mesmas fórmulas (RBJ) de
// sona-core/src/biquad.rs, em f64.
import type { EqBand, EqConfig } from './types';

type C = [number, number, number, number, number];

function norm(b0: number, b1: number, b2: number, a0: number, a1: number, a2: number): C {
  return [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0];
}

function w0(f: number, sr: number) {
  const w = (2 * Math.PI * Math.min(Math.max(f, 10), sr * 0.49)) / sr;
  return [Math.cos(w), Math.sin(w)];
}

function bandCoefs(b: EqBand, sr: number): C {
  const a = Math.pow(10, b.gain_db / 40);
  const [c, s] = w0(b.freq_hz, sr);
  const alpha = s / (2 * Math.max(b.q, 0.05));
  const sa = 2 * Math.sqrt(a) * alpha;
  switch (b.kind) {
    case 'peaking':
      return norm(1 + alpha * a, -2 * c, 1 - alpha * a, 1 + alpha / a, -2 * c, 1 - alpha / a);
    case 'low_shelf':
      return norm(a * (a + 1 - (a - 1) * c + sa), 2 * a * (a - 1 - (a + 1) * c), a * (a + 1 - (a - 1) * c - sa), a + 1 + (a - 1) * c + sa, -2 * (a - 1 + (a + 1) * c), a + 1 + (a - 1) * c - sa);
    case 'high_shelf':
      return norm(a * (a + 1 + (a - 1) * c + sa), -2 * a * (a - 1 + (a + 1) * c), a * (a + 1 + (a - 1) * c - sa), a + 1 - (a - 1) * c + sa, 2 * (a - 1 - (a + 1) * c), a + 1 - (a - 1) * c - sa);
  }
}

function hpCoefs(f: number, sr: number): C {
  const [c, s] = w0(f, sr);
  const alpha = s / (2 * Math.SQRT1_2);
  return norm((1 + c) / 2, -(1 + c), (1 + c) / 2, 1 + alpha, -2 * c, 1 - alpha);
}

function magDb([b0, b1, b2, a1, a2]: C, f: number, sr: number): number {
  const w = (2 * Math.PI * f) / sr;
  const [c1, s1, c2, s2] = [Math.cos(w), -Math.sin(w), Math.cos(2 * w), -Math.sin(2 * w)];
  const nr = b0 + b1 * c1 + b2 * c2;
  const ni = b1 * s1 + b2 * s2;
  const dr = 1 + a1 * c1 + a2 * c2;
  const di = a1 * s1 + a2 * s2;
  return 10 * Math.log10((nr * nr + ni * ni) / (dr * dr + di * di));
}

export function responseDb(eq: EqConfig, f: number, sr: number): number {
  if (!eq.enabled) return 0;
  let db = 0;
  if (eq.high_pass_enabled) db += magDb(hpCoefs(eq.high_pass_hz, sr), f, sr);
  for (const b of eq.bands) if (b.enabled && b.gain_db !== 0) db += magDb(bandCoefs(b, sr), f, sr);
  return db;
}
