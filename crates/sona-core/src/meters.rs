//! Medidores compartilhados entre a thread de áudio e o controle.
//!
//! Sem trava: cada valor é um `AtomicU32` com os bits de um `f32` positivo.
//! Para `f32` ≥ 0 a ordem dos bits é a mesma ordem dos números, então
//! `fetch_max` guarda o pico. Quem lê usa `swap(0)` e recebe o pico desde a
//! última leitura.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};

pub struct Meters {
    pub channel_peak: Vec<AtomicU32>,
    pub channel_gr_db: Vec<AtomicU32>,
    pub channel_gate: Vec<AtomicU32>,
    pub bus_peak: Vec<[AtomicU32; 2]>,
    pub bus_limit_db: Vec<AtomicU32>,
    pub blocks: AtomicU64,
}

fn zeros(n: usize) -> Vec<AtomicU32> {
    (0..n).map(|_| AtomicU32::new(0)).collect()
}

impl Meters {
    pub fn new(channels: usize, buses: usize) -> Self {
        Meters {
            channel_peak: zeros(channels),
            channel_gr_db: zeros(channels),
            channel_gate: zeros(channels),
            bus_peak: (0..buses).map(|_| [AtomicU32::new(0), AtomicU32::new(0)]).collect(),
            bus_limit_db: zeros(buses),
            blocks: AtomicU64::new(0),
        }
    }

    #[inline]
    pub(crate) fn put(slot: &AtomicU32, v: f32) {
        let v = v.abs();
        if v.is_finite() {
            slot.fetch_max(v.to_bits(), Relaxed);
        }
    }

    /// Lê e zera.
    pub fn take(slot: &AtomicU32) -> f32 {
        f32::from_bits(slot.swap(0, Relaxed))
    }
}
