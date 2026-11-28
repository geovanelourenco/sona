// Valores padrão do processamento do canal. Espelho dos `Default` de
// crates/sona-core/src/config.rs; mude os dois juntos.
import type { CompConfig, EqConfig, GateConfig } from './types';

export const GATE_DEFAULT: GateConfig = { enabled: false, threshold_db: -50, range_db: 60, attack_ms: 0.5, hold_ms: 50, release_ms: 150 };

export const EQ_BANDS_DEFAULT: EqConfig['bands'] = [
  { enabled: true, kind: 'low_shelf', freq_hz: 120, gain_db: 0, q: 0.7 },
  { enabled: true, kind: 'peaking', freq_hz: 400, gain_db: 0, q: 1 },
  { enabled: true, kind: 'peaking', freq_hz: 2500, gain_db: 0, q: 1 },
  { enabled: true, kind: 'high_shelf', freq_hz: 8000, gain_db: 0, q: 0.7 },
];

export const COMP_DEFAULT: CompConfig = { enabled: false, threshold_db: -18, ratio: 3, knee_db: 6, attack_ms: 10, release_ms: 120, makeup_db: 0 };

export const INPUT_DEFAULT = { trim_db: 0, polarity_invert: false, high_pass_enabled: false, high_pass_hz: 80 };
