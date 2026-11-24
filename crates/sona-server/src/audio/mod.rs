//! Hospedagem do motor: dispositivo simulado e dispositivos reais.

#[cfg(feature = "hw")]
pub mod hw;
pub mod sim;

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};

/// Um canal na lista plana de entradas ou saídas do motor.
#[derive(Clone, Debug, Serialize)]
pub struct Port {
    pub name: String,
    pub device: String,
    /// Domínio de relógio / dispositivo. Mesmo número = mesmo aparelho.
    pub domain: u32,
    pub connected: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct IoLayout {
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
}

impl IoLayout {
    pub fn to_layout(&self, sample_rate: u32, block: usize) -> sona_core::Layout {
        let mut l = sona_core::Layout::new(sample_rate as f32, block.max(64), self.inputs.len(), self.outputs.len());
        l.input_domains = self.inputs.iter().map(|p| p.domain).collect();
        l.output_domains = self.outputs.iter().map(|p| p.domain).collect();
        l
    }
}

/// Estatísticas da thread de áudio, sem trava.
#[derive(Default)]
pub struct EngineStats {
    pub running: AtomicBool,
    pub sample_rate: AtomicU32,
    pub block: AtomicU32,
    /// Carga do último bloco, em milésimos do orçamento de tempo real.
    pub load_permille: AtomicU32,
    /// Pior carga desde a última leitura.
    pub load_peak_permille: AtomicU32,
    /// Blocos que estouraram o orçamento (equivale a underrun no hardware).
    pub late_blocks: AtomicU64,
    /// Entrada sem dados quando a saída pediu (duplex em hardware).
    pub input_starved: AtomicU64,
    pub blocks: AtomicU64,
}

impl EngineStats {
    pub fn record(&self, elapsed_s: f64, budget_s: f64) {
        let permille = ((elapsed_s / budget_s) * 1000.0).min(u32::MAX as f64) as u32;
        self.load_permille.store(permille, Relaxed);
        self.load_peak_permille.fetch_max(permille, Relaxed);
        self.blocks.fetch_add(1, Relaxed);
        if elapsed_s > budget_s {
            self.late_blocks.fetch_add(1, Relaxed);
        }
    }

    pub fn snapshot(&self, device: &str) -> serde_json::Value {
        serde_json::json!({
            "device": device,
            "running": self.running.load(Relaxed),
            "sample_rate": self.sample_rate.load(Relaxed),
            "block": self.block.load(Relaxed),
            "load": self.load_permille.load(Relaxed) as f32 / 1000.0,
            "load_peak": self.load_peak_permille.swap(0, Relaxed) as f32 / 1000.0,
            "late_blocks": self.late_blocks.load(Relaxed),
            "input_starved": self.input_starved.load(Relaxed),
            "blocks": self.blocks.load(Relaxed),
        })
    }
}
