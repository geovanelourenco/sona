//! Configuração do bridge (`bridge.json`). Guarda o token de sessão, nunca a senha.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Endpoint {
    /// Nome do aparelho. "sim" = aparelho simulado (testes).
    pub device: String,
    pub channels: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BridgeConfig {
    /// Endereço do servidor, ex.: "192.168.0.10:7340".
    pub server: String,
    /// Nome deste bridge no console (ex.: "OBS", "Mídia").
    pub name: String,
    /// Token de sessão recebido no login (não é a senha).
    pub token: String,
    /// Aparelho cujo áudio vai para o servidor (ex.: cabo onde o Lumo toca).
    pub send: Option<Endpoint>,
    /// Aparelho que toca o que vem do servidor (ex.: "CABLE Input" para o OBS).
    pub receive: Option<Endpoint>,
    /// Desvio do relógio dos aparelhos simulados, em ppm (testes de deriva).
    pub sim_ppm: f64,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        BridgeConfig { server: String::new(), name: "Bridge".into(), token: String::new(), send: None, receive: None, sim_ppm: 0.0 }
    }
}

impl BridgeConfig {
    pub fn load(path: &Path) -> BridgeConfig {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).expect("serializa"))?;
        std::fs::rename(tmp, path)
    }

    /// "host:porta" sem esquema; porta 7340 se faltar.
    pub fn server_addr(&self) -> String {
        let s = self.server.trim().trim_start_matches("http://").trim_end_matches('/');
        if s.contains(':') {
            s.to_string()
        } else {
            format!("{s}:7340")
        }
    }
}
