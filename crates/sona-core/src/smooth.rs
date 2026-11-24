//! Suavização de ganho em rampa linear. Evita estalos quando um fader muda.

#[derive(Clone, Copy, Debug)]
pub struct Smoother {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
    ramp: u32,
}

impl Smoother {
    pub fn new(value: f32, ramp_samples: u32) -> Self {
        Self { current: value, target: value, step: 0.0, remaining: 0, ramp: ramp_samples.max(1) }
    }

    /// Define o novo alvo. A rampa leva `ramp_samples` amostras.
    #[inline]
    pub fn set_target(&mut self, target: f32) {
        if target == self.target {
            return;
        }
        self.target = target;
        self.remaining = self.ramp;
        self.step = (target - self.current) / self.ramp as f32;
    }

    /// Pula direto para o valor, sem rampa.
    #[inline]
    pub fn reset(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.remaining = 0;
    }

    #[inline]
    pub fn next_value(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            self.current = if self.remaining == 0 { self.target } else { self.current + self.step };
        }
        self.current
    }

    #[inline]
    pub fn is_settled(&self) -> bool {
        self.remaining == 0
    }

    #[inline]
    pub fn current(&self) -> f32 {
        self.current
    }

    #[inline]
    pub fn target(&self) -> f32 {
        self.target
    }
}
