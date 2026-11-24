//! Gate, compressor e limitador. Parâmetros prontos (coeficientes) vêm do
//! plano; o estado mora no motor. Detecção ligada em estéreo (máximo de L/R).

use crate::level::{db_to_gain, flush, gain_to_db, time_coef};

// ── Gate ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateParams {
    pub enabled: bool,
    /// Limiar linear de abertura.
    pub open: f32,
    /// Limiar linear de fechamento (histerese de 3 dB abaixo da abertura).
    pub close: f32,
    /// Ganho com o gate fechado (atenuação de `range_db`).
    pub floor: f32,
    pub attack: f32,
    pub release: f32,
    pub hold_samples: u32,
    /// Coeficiente do detector de pico (decaimento).
    pub detect: f32,
}

impl GateParams {
    pub const BYPASS: GateParams =
        GateParams { enabled: false, open: 0.0, close: 0.0, floor: 1.0, attack: 0.0, release: 0.0, hold_samples: 0, detect: 0.0 };

    pub fn new(threshold_db: f32, range_db: f32, attack_ms: f32, hold_ms: f32, release_ms: f32, sr: f32) -> Self {
        GateParams {
            enabled: true,
            open: db_to_gain(threshold_db),
            close: db_to_gain(threshold_db - 3.0),
            floor: db_to_gain(-range_db.abs()),
            attack: time_coef(attack_ms.max(0.05), sr),
            release: time_coef(release_ms.max(1.0), sr),
            hold_samples: (hold_ms.max(0.0) * 0.001 * sr) as u32,
            detect: time_coef(5.0, sr),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GateState {
    env: f32,
    gain: f32,
    open: bool,
    hold: u32,
}

impl Default for GateState {
    fn default() -> Self {
        GateState { env: 0.0, gain: 1.0, open: true, hold: 0 }
    }
}

impl GateState {
    /// Devolve o ganho a aplicar nesta amostra.
    #[inline]
    pub fn tick(&mut self, p: &GateParams, level: f32) -> f32 {
        if !p.enabled {
            self.gain = 1.0;
            return 1.0;
        }
        self.env = if level > self.env { level } else { flush(p.detect * self.env + (1.0 - p.detect) * level) };
        if self.env >= p.open {
            self.open = true;
            self.hold = p.hold_samples;
        } else if self.env < p.close {
            if self.hold > 0 {
                self.hold -= 1;
            } else {
                self.open = false;
            }
        }
        let (target, coef) = if self.open { (1.0, p.attack) } else { (p.floor, p.release) };
        self.gain = target + coef * (self.gain - target);
        self.gain
    }

    pub fn gain(&self) -> f32 {
        self.gain
    }
}

// ── Compressor ───────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompParams {
    pub enabled: bool,
    pub threshold_db: f32,
    pub ratio: f32,
    pub knee_db: f32,
    pub makeup: f32,
    pub attack: f32,
    pub release: f32,
}

impl CompParams {
    pub const BYPASS: CompParams =
        CompParams { enabled: false, threshold_db: 0.0, ratio: 1.0, knee_db: 0.0, makeup: 1.0, attack: 0.0, release: 0.0 };

    pub fn new(threshold_db: f32, ratio: f32, knee_db: f32, attack_ms: f32, release_ms: f32, makeup_db: f32, sr: f32) -> Self {
        CompParams {
            enabled: true,
            threshold_db,
            ratio: ratio.max(1.0),
            knee_db: knee_db.max(0.0),
            makeup: db_to_gain(makeup_db),
            attack: time_coef(attack_ms.max(0.05), sr),
            release: time_coef(release_ms.max(1.0), sr),
        }
    }

    /// Curva estática: redução de ganho (dB, ≤ 0) para um nível de entrada em dB.
    pub fn gain_reduction_db(&self, level_db: f32) -> f32 {
        let over = level_db - self.threshold_db;
        let slope = 1.0 / self.ratio - 1.0;
        let k = self.knee_db;
        if k > 0.0 && over.abs() <= k / 2.0 {
            slope * (over + k / 2.0).powi(2) / (2.0 * k)
        } else if over > 0.0 {
            slope * over
        } else {
            0.0
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CompState {
    env: f32,
    gr_db: f32,
}

impl CompState {
    #[inline]
    pub fn tick(&mut self, p: &CompParams, level: f32) -> f32 {
        if !p.enabled {
            self.gr_db = 0.0;
            return 1.0;
        }
        let c = if level > self.env { p.attack } else { p.release };
        self.env = flush(c * self.env + (1.0 - c) * level);
        self.gr_db = p.gain_reduction_db(gain_to_db(self.env).max(-120.0));
        db_to_gain(self.gr_db) * p.makeup
    }

    /// Redução de ganho atual em dB (≤ 0). Para medidores.
    pub fn gr_db(&self) -> f32 {
        self.gr_db
    }
}

// ── Limitador de bus ─────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LimiterParams {
    pub enabled: bool,
    pub ceiling: f32,
    pub release: f32,
}

impl LimiterParams {
    pub const BYPASS: LimiterParams = LimiterParams { enabled: false, ceiling: 1.0, release: 0.0 };

    pub fn new(ceiling_db: f32, release_ms: f32, sr: f32) -> Self {
        LimiterParams { enabled: true, ceiling: db_to_gain(ceiling_db.min(0.0)), release: time_coef(release_ms.max(1.0), sr) }
    }
}

/// Limitador de pico sem lookahead: o ganho cai na hora em que o pico passaria
/// do teto, e volta com o tempo de release. A saída nunca passa do teto.
#[derive(Clone, Copy, Debug)]
pub struct LimiterState {
    gain: f32,
}

impl Default for LimiterState {
    fn default() -> Self {
        LimiterState { gain: 1.0 }
    }
}

impl LimiterState {
    #[inline]
    pub fn tick(&mut self, p: &LimiterParams, peak: f32) -> f32 {
        if !p.enabled {
            self.gain = 1.0;
            return 1.0;
        }
        self.gain = 1.0 + p.release * (self.gain - 1.0);
        if peak * self.gain > p.ceiling {
            self.gain = p.ceiling / peak;
        }
        self.gain
    }

    pub fn gain(&self) -> f32 {
        self.gain
    }
}
