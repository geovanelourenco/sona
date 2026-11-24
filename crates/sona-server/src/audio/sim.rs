//! Dispositivo simulado: gera sinais de teste nas entradas e descarta as
//! saídas, no ritmo de um relógio de áudio. Serve para desenvolver e testar
//! sem hardware. O ritmo usa `sleep` na thread do dispositivo, nunca dentro de
//! `Engine::process`.

use super::{EngineStats, IoLayout, Port};
use sona_core::Engine;
use std::f32::consts::TAU;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const CONSOLE_DOMAIN: u32 = 1;
pub const NETWORK_DOMAIN: u32 = 2;

/// Layout de demonstração: 24 canais de "mesa" e 2 fontes estéreo de rede.
pub fn demo_layout() -> IoLayout {
    let port = |name: String, device: &str, domain| Port { name, device: device.into(), domain, connected: true };
    let mut inputs: Vec<Port> = (1..=24).map(|i| port(format!("Mesa {i}"), "Mesa simulada", CONSOLE_DOMAIN)).collect();
    for (src, dev) in [("Lumo", "Bridge Mídia (sim)"), ("Navegador", "Bridge Mídia (sim)")] {
        inputs.push(port(format!("{src} L"), dev, NETWORK_DOMAIN));
        inputs.push(port(format!("{src} R"), dev, NETWORK_DOMAIN));
    }
    let mut outputs = vec![
        port("OBS L".into(), "Bridge OBS (sim)", NETWORK_DOMAIN),
        port("OBS R".into(), "Bridge OBS (sim)", NETWORK_DOMAIN),
        port("Mesa USB 1".into(), "Mesa simulada", CONSOLE_DOMAIN),
        port("Mesa USB 2".into(), "Mesa simulada", CONSOLE_DOMAIN),
        port("Gravação L".into(), "Gravador (sim)", 3),
        port("Gravação R".into(), "Gravador (sim)", 3),
    ];
    for f in 1..=2 {
        outputs.push(port(format!("Fone {f} L"), "Fones (sim)", 4));
        outputs.push(port(format!("Fone {f} R"), "Fones (sim)", 4));
    }
    IoLayout { inputs, outputs }
}

struct Voice {
    phase: f32,
    inc: f32,
    env_phase: f32,
    env_inc: f32,
    amp: f32,
}

pub struct SimHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for SimHandle {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub fn start(mut engine: Engine, io: &IoLayout, sample_rate: u32, block: usize, stats: Arc<EngineStats>) -> SimHandle {
    let n_in = io.inputs.len();
    let n_out = io.outputs.len();
    let sr = sample_rate as f32;
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    stats.sample_rate.store(sample_rate, Relaxed);
    stats.block.store(block as u32, Relaxed);
    let thread = std::thread::Builder::new()
        .name("sona-audio-sim".into())
        .spawn(move || {
            let mut voices: Vec<Voice> = (0..n_in)
                .map(|i| {
                    let f = 110.0 * 2f32.powf((i % 24) as f32 / 6.0) * if i >= 24 { 1.5 } else { 1.0 };
                    Voice {
                        phase: 0.0,
                        inc: TAU * f / sr,
                        env_phase: i as f32,
                        env_inc: TAU * (0.15 + 0.05 * (i % 7) as f32) / sr,
                        amp: if i >= 24 { 0.3 } else { 0.18 },
                    }
                })
                .collect();
            let mut ins = vec![vec![0.0f32; block]; n_in];
            let mut outs = vec![vec![0.0f32; block]; n_out];
            let budget = Duration::from_secs_f64(block as f64 / sample_rate as f64);
            let mut deadline = Instant::now();
            stats.running.store(true, Relaxed);
            while !stop2.load(Relaxed) {
                for (v, buf) in voices.iter_mut().zip(ins.iter_mut()) {
                    for s in buf.iter_mut() {
                        let env = 0.55 + 0.45 * v.env_phase.sin();
                        *s = v.amp * env * v.phase.sin();
                        v.phase = (v.phase + v.inc) % TAU;
                        v.env_phase = (v.env_phase + v.env_inc) % TAU;
                    }
                }
                let t0 = Instant::now();
                {
                    let in_refs: [&[f32]; 64] = std::array::from_fn(|i| ins.get(i).map(|v| v.as_slice()).unwrap_or(&[]));
                    let mut out_iter = outs.iter_mut();
                    let mut out_refs: [&mut [f32]; 64] =
                        std::array::from_fn(|_| out_iter.next().map(|v| v.as_mut_slice()).unwrap_or(&mut []));
                    engine.process(&in_refs[..n_in.min(64)], &mut out_refs[..n_out.min(64)], block);
                }
                stats.record(t0.elapsed().as_secs_f64(), budget.as_secs_f64());
                deadline += budget;
                let now = Instant::now();
                if now > deadline + budget {
                    // Atrasou mais de um bloco: conta e realinha o relógio.
                    stats.late_blocks.fetch_add(1, Relaxed);
                    deadline = now;
                } else if deadline > now {
                    std::thread::sleep(deadline - now);
                }
            }
            stats.running.store(false, Relaxed);
        })
        .expect("thread de áudio");
    SimHandle { stop, thread: Some(thread) }
}
