//! Transporte de áudio em rede com o Sona Bridge (fase 3). Ver ADR 0003.

use crate::api::Shared;

pub async fn spawn_udp(_app: Shared, _port: u16) -> Result<(), String> {
    Ok(())
}
