//! Filtros biquad (RBJ Audio EQ Cookbook), forma direta transposta II.

use crate::level::flush;
use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coefs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Coefs {
    pub const IDENTITY: Coefs = Coefs { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn norm(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Coefs {
        Coefs { b0: (b0 / a0) as f32, b1: (b1 / a0) as f32, b2: (b2 / a0) as f32, a1: (a1 / a0) as f32, a2: (a2 / a0) as f32 }
    }

    fn w0(freq: f32, sr: f32) -> (f64, f64) {
        let f = freq.clamp(10.0, sr * 0.49) as f64;
        let w0 = 2.0 * PI as f64 * f / sr as f64;
        (w0.cos(), w0.sin())
    }

    pub fn peaking(freq: f32, q: f32, gain_db: f32, sr: f32) -> Coefs {
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let (c, s) = Self::w0(freq, sr);
        let alpha = s / (2.0 * q.max(0.05) as f64);
        Self::norm(1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a)
    }

    pub fn low_shelf(freq: f32, q: f32, gain_db: f32, sr: f32) -> Coefs {
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let (c, s) = Self::w0(freq, sr);
        let alpha = s / (2.0 * q.max(0.05) as f64);
        let sa = 2.0 * a.sqrt() * alpha;
        Self::norm(
            a * ((a + 1.0) - (a - 1.0) * c + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - sa),
            (a + 1.0) + (a - 1.0) * c + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - sa,
        )
    }

    pub fn high_shelf(freq: f32, q: f32, gain_db: f32, sr: f32) -> Coefs {
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let (c, s) = Self::w0(freq, sr);
        let alpha = s / (2.0 * q.max(0.05) as f64);
        let sa = 2.0 * a.sqrt() * alpha;
        Self::norm(
            a * ((a + 1.0) + (a - 1.0) * c + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - sa),
            (a + 1.0) - (a - 1.0) * c + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - sa,
        )
    }

    pub fn high_pass(freq: f32, q: f32, sr: f32) -> Coefs {
        let (c, s) = Self::w0(freq, sr);
        let alpha = s / (2.0 * q.max(0.05) as f64);
        Self::norm((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
    }

    pub fn low_pass(freq: f32, q: f32, sr: f32) -> Coefs {
        let (c, s) = Self::w0(freq, sr);
        let alpha = s / (2.0 * q.max(0.05) as f64);
        Self::norm((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
    }

    /// Resposta em magnitude (dB) na frequência dada. Usado pela UI e por testes.
    pub fn magnitude_db(&self, freq: f32, sr: f32) -> f32 {
        let w = 2.0 * PI as f64 * freq as f64 / sr as f64;
        let (c1, s1) = (w.cos(), -w.sin());
        let (c2, s2) = ((2.0 * w).cos(), -(2.0 * w).sin());
        let (b0, b1, b2, a1, a2) = (self.b0 as f64, self.b1 as f64, self.b2 as f64, self.a1 as f64, self.a2 as f64);
        let nr = b0 + b1 * c1 + b2 * c2;
        let ni = b1 * s1 + b2 * s2;
        let dr = 1.0 + a1 * c1 + a2 * c2;
        let di = a1 * s1 + a2 * s2;
        let mag2 = (nr * nr + ni * ni) / (dr * dr + di * di);
        (10.0 * mag2.log10()) as f32
    }
}

/// Estado de um biquad (uma amostra de memória por canal).
#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    z1: f32,
    z2: f32,
}

impl State {
    #[inline]
    pub fn process(&mut self, c: &Coefs, x: f32) -> f32 {
        let y = c.b0 * x + self.z1;
        self.z1 = flush(c.b1 * x - c.a1 * y + self.z2);
        self.z2 = flush(c.b2 * x - c.a2 * y);
        y
    }

    pub fn clear(&mut self) {
        *self = State::default();
    }
}
