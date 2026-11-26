//! Relógio de software para o motor: dispositivo simulado (com sinais de teste
//! nas entradas) ou relógio interno (sem entradas, para servidor sem mesa ou
//! quando o principal caiu). O ritmo usa `sleep` nesta thread, nunca dentro de
//! `Engine::process`.

use super::composite::Composite;
use super::EngineStats;
use std::f32::consts::TAU;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const SIM_INPUTS: usize = 24;
pub const SIM_OUTPUTS: usize = 10;
/// Até quanto atraso o relógio recupera processando blocos seguidos.
const CATCH_UP: Duration = Duration::from_millis(200);

struct Voice {
    phase: f32,
    inc: f32,
    env_phase: f32,
    env_inc: f32,
    amp: f32,
}

pub struct ClockHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Composite>>,
}

impl ClockHandle {
    /// Para o relógio e devolve o motor.
    pub fn stop(mut self) -> Composite {
        self.stop.store(true, Relaxed);
        self.thread.take().unwrap().join().expect("thread de áudio")
    }
}

impl Drop for ClockHandle {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// `tones`: quantas entradas do principal recebem sinal de teste (0 = relógio interno).
pub fn start(mut comp: Composite, tones: usize, sample_rate: u32, block: usize, stats: Arc<EngineStats>) -> ClockHandle {
    let sr = sample_rate as f32;
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    stats.sample_rate.store(sample_rate, Relaxed);
    stats.block.store(block as u32, Relaxed);
    let block = block.min(comp.max_block());
    let thread = std::thread::Builder::new()
        .name("sona-relogio".into())
        .spawn(move || {
            let mut voices: Vec<Voice> = (0..tones)
                .map(|i| Voice {
                    phase: 0.0,
                    inc: TAU * 110.0 * 2f32.powf((i % 24) as f32 / 6.0) / sr,
                    env_phase: i as f32,
                    env_inc: TAU * (0.15 + 0.05 * (i % 7) as f32) / sr,
                    amp: 0.18,
                })
                .collect();
            let mut ins = vec![vec![0.0f32; block]; tones];
            let mut outs = vec![vec![0.0f32; block]; SIM_OUTPUTS];
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
                    let in_refs: [&[f32]; SIM_INPUTS] = std::array::from_fn(|i| ins.get(i).map(|v| v.as_slice()).unwrap_or(&[]));
                    let mut it = outs.iter_mut();
                    let mut out_refs: [&mut [f32]; SIM_OUTPUTS] = std::array::from_fn(|_| it.next().unwrap().as_mut_slice());
                    comp.run(&in_refs[..tones.min(SIM_INPUTS)], &mut out_refs[..if tones > 0 { SIM_OUTPUTS } else { 0 }], block);
                }
                stats.record(t0.elapsed().as_secs_f64(), budget.as_secs_f64());
                deadline += budget;
                let now = Instant::now();
                if deadline > now {
                    std::thread::sleep(deadline - now);
                } else if now - deadline > CATCH_UP {
                    // Atraso grande (sistema travou): conta e realinha.
                    stats.late_blocks.fetch_add(1, Relaxed);
                    deadline = now;
                }
                // Atraso pequeno: segue sem dormir até recuperar. Assim o
                // relógio de software anda, em média, no ritmo do tempo real
                // (pular o atraso o deixaria mais lento que os outros relógios).
            }
            stats.running.store(false, Relaxed);
            comp
        })
        .expect("thread de áudio");
    ClockHandle { stop, thread: Some(thread) }
}
