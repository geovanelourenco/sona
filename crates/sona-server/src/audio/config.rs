//! Configuração de áudio do servidor (`data/audio.json`), editável pelo app.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// "sim": dispositivo simulado com sinais de teste (demonstração).
/// "internal": relógio interno sem entradas (servidor sem mesa: só bridges e
/// dispositivos secundários). Qualquer outro texto: nome do aparelho.
pub const PRIMARY_SIM: &str = "sim";
pub const PRIMARY_INTERNAL: &str = "internal";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Input,
    Output,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Secondary {
    /// Nome do aparelho como o sistema mostra (ex.: "CABLE Input (VB-Audio Virtual Cable)").
    pub device: String,
    pub direction: Direction,
    #[serde(default = "two")]
    pub channels: u16,
    /// Rótulo nos canais do console (ex.: "OBS", "Lumo", "Fone").
    #[serde(default)]
    pub label: String,
}

fn two() -> u16 {
    2
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub sample_rate: u32,
    pub block: usize,
    pub primary: String,
    /// Limite de canais usados do principal (0 = todos que o aparelho oferecer).
    pub primary_channels: u16,
    pub secondary: Vec<Secondary>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        AudioConfig { sample_rate: 48_000, block: 256, primary: PRIMARY_SIM.into(), primary_channels: 0, secondary: Vec::new() }
    }
}

impl AudioConfig {
    pub fn load(path: &Path) -> Option<AudioConfig> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::state::write_atomic(path, &serde_json::to_vec_pretty(self).expect("serializa"))
    }

    pub fn validate(&self) -> Result<(), String> {
        if ![44_100, 48_000, 96_000].contains(&self.sample_rate) {
            return Err("Taxa de amostragem aceita: 44100, 48000 ou 96000 Hz.".into());
        }
        if !(32..=1024).contains(&self.block) {
            return Err("Bloco entre 32 e 1024 amostras.".into());
        }
        let used: usize = self.secondary.iter().map(|s| s.channels as usize).sum();
        if used > super::SECONDARY_MAX {
            return Err(format!("Dispositivos secundários somam {used} canais; o limite é {}.", super::SECONDARY_MAX));
        }
        if self.secondary.iter().any(|s| s.channels == 0 || s.device.trim().is_empty()) {
            return Err("Cada dispositivo secundário precisa de nome e pelo menos 1 canal.".into());
        }
        Ok(())
    }
}
