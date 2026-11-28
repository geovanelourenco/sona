//! Hospedagem do motor: um dispositivo principal (dita o relógio), dispositivos
//! secundários (cada um com o seu relógio, via ponte de deriva) e vagas de rede
//! para os bridges.
//!
//! Lista plana de canais do motor, com faixas fixas para que trocar um aparelho
//! não embaralhe o roteamento salvo:
//!
//! | faixa | uso |
//! |---|---|
//! | 0..64 | dispositivo principal (Ui24R, simulado) |
//! | 64..96 | dispositivos secundários, na ordem da configuração |
//! | 96..112 | vagas de rede: vaga `i` usa 96 + 2i (L) e 97 + 2i (R) |

pub mod composite;
pub mod config;
pub mod host;
#[cfg(feature = "hw")]
pub mod hw;
pub mod netslot;
pub mod sim;
pub mod ui24r;

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};

pub const PRIMARY_BASE: usize = 0;
pub const PRIMARY_MAX: usize = 64;
pub const SECONDARY_BASE: usize = 64;
pub const SECONDARY_MAX: usize = 32;
pub const NET_BASE: usize = 96;
pub const NET_SLOTS: usize = 8;
pub const PORTS: usize = NET_BASE + 2 * NET_SLOTS;

/// Um canal na lista plana de entradas ou saídas do motor.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Port {
    pub index: usize,
    pub name: String,
    pub device: String,
    /// Domínio de relógio / dispositivo. Mesmo número = mesmo aparelho.
    pub domain: u32,
    /// Existe na configuração atual (faixas vazias ficam com `present: false`).
    pub present: bool,
    /// O aparelho ou bridge está ligado agora.
    pub connected: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct IoLayout {
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
}

impl Default for IoLayout {
    fn default() -> Self {
        let blank = |i| Port { index: i, ..Port::default() };
        IoLayout { inputs: (0..PORTS).map(blank).collect(), outputs: (0..PORTS).map(blank).collect() }
    }
}

impl IoLayout {
    pub fn to_layout(&self, sample_rate: u32, block: usize) -> sona_core::Layout {
        let mut l = sona_core::Layout::new(sample_rate as f32, block.max(64), PORTS, PORTS);
        l.input_domains = self.inputs.iter().map(|p| p.domain).collect();
        l.output_domains = self.outputs.iter().map(|p| p.domain).collect();
        l
    }

    pub fn set(&mut self, output: bool, index: usize, name: String, device: &str, domain: u32) {
        let list = if output { &mut self.outputs } else { &mut self.inputs };
        if let Some(p) = list.get_mut(index) {
            *p = Port { index, name, device: device.into(), domain, present: true, connected: true };
        }
    }
}

/// Domínios de relógio (para o aviso de laço pela mesa).
pub const DOMAIN_PRIMARY: u32 = 1;
pub const DOMAIN_NET: u32 = 1000;

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
    /// Entrada do principal sem dados quando a saída pediu.
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
