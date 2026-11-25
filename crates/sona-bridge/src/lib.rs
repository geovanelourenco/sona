//! Sona Bridge: envia o áudio de um aparelho deste computador para o servidor
//! Sona e toca num outro aparelho o que vem do servidor, ao mesmo tempo.
//! Uso típico: no computador do OBS, recebe a Transmissão e toca num cabo
//! virtual que o OBS captura; no computador de mídia, envia o que toca.

pub mod audio;
pub mod client;
pub mod config;
pub mod http;
pub mod ui;

pub use client::Bridge;
