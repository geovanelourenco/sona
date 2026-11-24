import { describe, expect, it } from 'vitest';
import { dbToPos, fmtDb, meterPos, posToDb } from './level';
import { responseDb } from './eq';

describe('curva do fader', () => {
  it('0 dB fica a 3/4 do curso', () => {
    expect(posToDb(0.75)).toBeCloseTo(0);
    expect(dbToPos(0)).toBeCloseTo(0.75);
  });
  it('ida e volta', () => {
    for (const db of [-90, -60, -33, -10, -2.5, 0, 4, 10]) expect(posToDb(dbToPos(db))).toBeCloseTo(db, 5);
  });
  it('formata', () => {
    expect(fmtDb(-90)).toBe('−∞');
    expect(fmtDb(0)).toBe('0.0');
    expect(fmtDb(3.04)).toBe('+3.0');
  });
  it('medidor de −60 a 0 dBFS', () => {
    expect(meterPos(1)).toBe(1);
    expect(meterPos(0.001)).toBeCloseTo(0);
    expect(meterPos(0.1)).toBeCloseTo(2 / 3);
  });
});

describe('curva do EQ (mesmas fórmulas do motor)', () => {
  it('peaking +6 dB em 1 kHz', () => {
    const bands = [{ enabled: true, kind: 'peaking' as const, freq_hz: 1000, gain_db: 6, q: 1 }];
    expect(responseDb({ enabled: true, high_pass_enabled: false, high_pass_hz: 80, bands }, 1000, 48000)).toBeCloseTo(6, 1);
  });
  it('passa-altas em 100 Hz derruba 30 Hz', () => {
    expect(responseDb({ enabled: true, high_pass_enabled: true, high_pass_hz: 100, bands: [] }, 30, 48000)).toBeLessThan(-18);
  });
});
