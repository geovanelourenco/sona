#![allow(dead_code)]
use sona_core::*;

pub const SR: f32 = 48_000.0;
pub const BLOCK: usize = 128;

pub fn sine(freq: f32, amp: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR).sin()).collect()
}

/// Amplitude de pico de uma frequência (Goertzel sobre a janela inteira).
pub fn tone_amp(x: &[f32], freq: f32) -> f32 {
    let w = 2.0 * std::f64::consts::PI * freq as f64 / SR as f64;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s = v as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    (2.0 * power.sqrt() / x.len() as f64) as f32
}

pub fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

pub fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

pub fn layout(inputs: usize, outputs: usize) -> Layout {
    Layout::new(SR, BLOCK, inputs, outputs)
}

pub fn mono(name: &str, input: usize) -> ChannelConfig {
    ChannelConfig { name: name.into(), source: SourceMap::Mono { input }, fader_db: 0.0, ..Default::default() }
}

pub fn send(bus: usize, level_db: f32) -> SendConfig {
    SendConfig { bus, level_db, ..Default::default() }
}

pub fn out_bus(name: &str, kind: BusKind, left: usize) -> BusConfig {
    BusConfig { outputs: vec![OutputPair { left, right: left + 1 }], enabled: true, ..BusConfig::new(name, kind) }
}

/// Roda o motor sobre as entradas e devolve as saídas inteiras.
pub fn run(engine: &mut Engine, inputs: &[Vec<f32>], outputs: usize, frames: usize) -> Vec<Vec<f32>> {
    let mut out = vec![vec![0.0f32; frames]; outputs];
    let mut off = 0;
    while off < frames {
        let n = BLOCK.min(frames - off);
        let ins: Vec<&[f32]> = inputs.iter().map(|v| &v[off..off + n]).collect();
        let mut chunk: Vec<Vec<f32>> = vec![vec![0.0; n]; outputs];
        {
            let mut outs: Vec<&mut [f32]> = chunk.iter_mut().map(|v| v.as_mut_slice()).collect();
            engine.process(&ins, &mut outs, n);
        }
        for (o, c) in out.iter_mut().zip(chunk) {
            o[off..off + n].copy_from_slice(&c);
        }
        off += n;
    }
    out
}

pub fn engine(cfg: &MixerConfig, l: Layout) -> (Engine, EngineHandle) {
    let (plan, _) = compile(cfg, &l).expect("config válida");
    Engine::new(l, plan)
}

/// Última fração do sinal (depois de rampas e envelopes assentarem).
pub fn tail(x: &[f32], frac: f32) -> &[f32] {
    &x[(x.len() as f32 * (1.0 - frac)) as usize..]
}
