//! Vagas de rede: cada vaga é um par estéreo de entrada (bridge → servidor) e
//! um de saída (servidor → bridge). O motor guarda o lado de tempo real; a
//! sessão de rede pega o outro lado enquanto o bridge está ligado.

use sona_core::drift::{DriftStats, Reader, Writer};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

/// Atraso alvo da fila de entrada de rede (absorve jitter da LAN). Cresce
/// sozinho até `NET_TARGET_MAX_MS` se a fila esvaziar (Wi-Fi ruim).
pub const NET_TARGET_MS: f64 = 12.0;
pub const NET_TARGET_MAX_MS: f64 = 80.0;

/// Lado de rede de uma vaga, emprestado à sessão do bridge.
pub struct NetEnds {
    /// Áudio que chega do bridge (a sessão escreve, o motor lê com deriva).
    pub incoming: Writer,
    /// Áudio que vai para o bridge (o motor escreve, a sessão lê e envia).
    pub outgoing: rtrb::Consumer<f32>,
}

pub struct NetSlot {
    pub index: usize,
    pub name: Mutex<Option<String>>,
    pub ends: Mutex<Option<NetEnds>>,
    pub busy: AtomicBool,
    pub stats: Arc<DriftStats>,
}

/// Lado de tempo real (fica no motor).
pub struct NetRt {
    pub incoming: Reader,
    pub outgoing: rtrb::Producer<f32>,
}

pub fn make(index: usize, sample_rate: u32) -> (Arc<NetSlot>, NetRt) {
    let (writer, mut reader, stats) = sona_core::drift::bridge(2, sample_rate as f64, NET_TARGET_MS);
    reader.set_adaptive(NET_TARGET_MAX_MS);
    // 0,5 s de saída: se ninguém estiver lendo, a fila enche e o motor descarta.
    let (prod, cons) = rtrb::RingBuffer::new(sample_rate as usize);
    let slot = Arc::new(NetSlot {
        index,
        name: Mutex::new(None),
        ends: Mutex::new(Some(NetEnds { incoming: writer, outgoing: cons })),
        busy: AtomicBool::new(false),
        stats,
    });
    (slot, NetRt { incoming: reader, outgoing: prod })
}

impl NetSlot {
    /// Empresta o lado de rede a uma sessão. `None` se a vaga está ocupada.
    pub fn take(&self, name: &str) -> Option<NetEnds> {
        if self.busy.swap(true, Relaxed) {
            return None;
        }
        let mut ends = self.ends.lock().unwrap().take()?;
        // Descarta o que acumulou enquanto ninguém lia (evita atraso inicial).
        let n = ends.outgoing.slots();
        if let Ok(c) = ends.outgoing.read_chunk(n) {
            c.commit_all();
        }
        *self.name.lock().unwrap() = Some(name.to_string());
        Some(ends)
    }

    pub fn give_back(&self, ends: NetEnds) {
        *self.ends.lock().unwrap() = Some(ends);
        *self.name.lock().unwrap() = None;
        self.busy.store(false, Relaxed);
    }
}
