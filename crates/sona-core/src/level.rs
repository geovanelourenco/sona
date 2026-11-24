//! Conversões de nível.

/// Valor de fader que significa "desligado" (ganho zero).
pub const OFF_DB: f32 = -90.0;

/// Converte dB em ganho linear. Valores em ou abaixo de [`OFF_DB`] dão zero.
#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    if db <= OFF_DB || !db.is_finite() {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

/// Converte ganho linear em dB. Zero dá `f32::NEG_INFINITY`.
#[inline]
pub fn gain_to_db(g: f32) -> f32 {
    if g <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * g.log10()
    }
}

/// Coeficiente de um filtro de um polo para uma constante de tempo em ms.
#[inline]
pub fn time_coef(ms: f32, sample_rate: f32) -> f32 {
    if ms <= 0.0 {
        0.0
    } else {
        (-1.0 / (ms * 0.001 * sample_rate)).exp()
    }
}

/// Zera valores subnormais (evita custo de CPU com denormais).
#[inline]
pub fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 {
        0.0
    } else {
        x
    }
}

/// Ganhos de pan com potência constante para fonte mono (-3 dB no centro).
#[inline]
pub fn pan_mono(pan: f32) -> (f32, f32) {
    let p = pan.clamp(-1.0, 1.0);
    let theta = (p + 1.0) * std::f32::consts::FRAC_PI_4;
    (theta.cos(), theta.sin())
}

/// Ganhos de balanço para fonte estéreo (0 dB no centro).
#[inline]
pub fn pan_stereo(pan: f32) -> (f32, f32) {
    let p = pan.clamp(-1.0, 1.0);
    let l = if p > 0.0 { 1.0 - p } else { 1.0 };
    let r = if p < 0.0 { 1.0 + p } else { 1.0 };
    (l, r)
}
