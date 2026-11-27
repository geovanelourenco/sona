//! Gerencia os dispositivos: monta a lista de canais, abre o principal (ou o
//! relógio interno se ele faltar), abre os secundários e reconecta quem cair.
//! O PA não depende de nada disto: o Sona só lê o USB e devolve retornos.

use super::composite::{Composite, SecIn, SecOut};
use super::config::{AudioConfig, Direction, PRIMARY_INTERNAL, PRIMARY_SIM};
use super::netslot::{self, NetSlot};
use super::*;
use serde::Serialize;
use sona_core::drift::DriftStats;
use sona_core::Engine;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Atraso alvo da ponte de um secundário: 3 blocos, no mínimo 10 ms.
fn sec_target_ms(cfg: &AudioConfig) -> f64 {
    (3.0 * cfg.block as f64 / cfg.sample_rate as f64 * 1000.0).max(10.0)
}

#[derive(Clone, Debug, Serialize)]
pub struct DeviceStatus {
    pub role: &'static str,
    pub name: String,
    pub label: String,
    pub connected: bool,
    pub error: Option<String>,
    pub drift_ppm: Option<f64>,
    pub latency_ms: Option<f64>,
    pub underruns: u64,
    pub overruns: u64,
}

/// "ASIO:Soundcraft…" → "Soundcraft… (ASIO)", para mostrar.
pub fn display_name(spec: &str) -> &str {
    match spec.split_once(':') {
        Some((h, rest)) if !h.is_empty() && h.len() <= 12 && h.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => spec,
    }
}

/// Monta a lista de canais para a configuração. Também diz quantos canais o
/// principal usa (lidos do aparelho; se ele faltar, a faixa fica vazia).
pub fn plan_io(cfg: &AudioConfig) -> (IoLayout, u16, u16, Option<String>) {
    let mut io = IoLayout::default();
    let mut err = None;
    let (pin, pout) = match cfg.primary.as_str() {
        PRIMARY_SIM => (sim::SIM_INPUTS as u16, sim::SIM_OUTPUTS as u16),
        PRIMARY_INTERNAL => (0, 0),
        #[cfg(feature = "hw")]
        name => match hw::channels(name, cfg.sample_rate) {
            Ok(c) => c,
            Err(e) => {
                err = Some(e);
                (0, 0)
            }
        },
        #[cfg(not(feature = "hw"))]
        _ => {
            err = Some("Compilado sem suporte a dispositivos reais.".into());
            (0, 0)
        }
    };
    let cap = |n: u16| {
        let n = n.min(PRIMARY_MAX as u16);
        if cfg.primary_channels > 0 {
            n.min(cfg.primary_channels)
        } else {
            n
        }
    };
    let (pin, pout) = (cap(pin), cap(pout));
    let dev = match cfg.primary.as_str() {
        PRIMARY_SIM => "Mesa simulada",
        PRIMARY_INTERNAL => "Relógio interno",
        n => display_name(n),
    };
    let sim_out = ["Saída 1", "Saída 2", "Saída 3", "Saída 4", "Saída 5", "Saída 6", "Saída 7", "Saída 8", "Saída 9", "Saída 10"];
    for i in 0..pin as usize {
        let name = if cfg.primary == PRIMARY_SIM { format!("Mesa {}", i + 1) } else { format!("USB {}", i + 1) };
        io.set(false, PRIMARY_BASE + i, name, dev, DOMAIN_PRIMARY);
    }
    for i in 0..pout as usize {
        let name = if cfg.primary == PRIMARY_SIM { sim_out[i % 10].to_string() } else { format!("USB {}", i + 1) };
        io.set(true, PRIMARY_BASE + i, name, dev, DOMAIN_PRIMARY);
    }
    let (mut bi, mut bo) = (SECONDARY_BASE, SECONDARY_BASE);
    for (k, s) in cfg.secondary.iter().enumerate() {
        let label = if s.label.trim().is_empty() { s.device.clone() } else { s.label.clone() };
        let domain = 10 + k as u32;
        let output = s.direction == Direction::Output;
        let base = if output { &mut bo } else { &mut bi };
        for c in 0..s.channels as usize {
            let suffix = match (s.channels, c) {
                (2, 0) => " L".to_string(),
                (2, 1) => " R".to_string(),
                (1, _) => String::new(),
                _ => format!(" {}", c + 1),
            };
            io.set(output, *base + c, format!("{label}{suffix}"), &s.device, domain);
        }
        *base += s.channels as usize;
    }
    for i in 0..NET_SLOTS {
        for (c, side) in ["L", "R"].iter().enumerate() {
            let idx = NET_BASE + 2 * i + c;
            io.set(false, idx, format!("Rede {} {side}", i + 1), "Sona Bridge", DOMAIN_NET + i as u32);
            io.set(true, idx, format!("Rede {} {side}", i + 1), "Sona Bridge", DOMAIN_NET + i as u32);
            io.inputs[idx].connected = false;
            io.outputs[idx].connected = false;
        }
    }
    if err.is_some() {
        for p in io.inputs.iter_mut().chain(io.outputs.iter_mut()).take(PRIMARY_MAX) {
            p.connected = false;
        }
    }
    (io, pin, pout, err)
}

enum Driver {
    Clock(sim::ClockHandle),
    #[cfg(feature = "hw")]
    Hw(hw::PrimaryHandle),
}

#[cfg(feature = "hw")]
struct SecState {
    cfg: super::config::Secondary,
    handle: Option<hw::SecondaryHandle>,
    parked: Option<hw::SecEnd>,
    stats: Arc<DriftStats>,
    error: Option<String>,
}

struct Inner {
    cfg: AudioConfig,
    driver: Option<Driver>,
    primary_error: Option<String>,
    primary_ch: (u16, u16),
    last_period: u64,
    /// O principal apareceu depois da partida: a lista de canais precisa ser
    /// remontada (o servidor reconstrói o motor).
    rebuild: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(feature = "hw")]
    secondaries: Vec<SecState>,
    #[cfg(not(feature = "hw"))]
    secondary_errors: Vec<String>,
}

pub struct AudioHost {
    inner: Arc<Mutex<Inner>>,
    pub slots: Vec<Arc<NetSlot>>,
    pub rebuild: Arc<std::sync::atomic::AtomicBool>,
    stats: Arc<EngineStats>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    supervisor: Option<std::thread::JoinHandle<()>>,
}

impl AudioHost {
    /// Liga tudo com o motor dado (criado pelo `Store` para o `io` de `plan_io`).
    pub fn start(
        cfg: &AudioConfig,
        engine: Engine,
        primary_ch: (u16, u16),
        primary_error: Option<String>,
        stats: Arc<EngineStats>,
    ) -> AudioHost {
        let target = sec_target_ms(cfg);
        let mut sec_in = Vec::new();
        let mut sec_out = Vec::new();
        #[cfg(feature = "hw")]
        let mut secondaries = Vec::new();
        let (mut bi, mut bo) = (SECONDARY_BASE, SECONDARY_BASE);
        for s in &cfg.secondary {
            let ch = s.channels as usize;
            let (writer, reader, dstats) = sona_core::drift::bridge(ch, cfg.sample_rate as f64, target);
            #[cfg(not(feature = "hw"))]
            let _ = (&writer, &reader, &dstats);
            match s.direction {
                Direction::Input => {
                    sec_in.push(SecIn { base: bi, channels: ch, reader });
                    bi += ch;
                    #[cfg(feature = "hw")]
                    secondaries.push(SecState {
                        cfg: s.clone(),
                        handle: None,
                        parked: Some(hw::SecEnd::In(writer)),
                        stats: dstats,
                        error: None,
                    });
                }
                Direction::Output => {
                    sec_out.push(SecOut { base: bo, channels: ch, writer });
                    bo += ch;
                    #[cfg(feature = "hw")]
                    secondaries.push(SecState {
                        cfg: s.clone(),
                        handle: None,
                        parked: Some(hw::SecEnd::Out(reader)),
                        stats: dstats,
                        error: None,
                    });
                }
            }
        }
        let mut slots = Vec::new();
        let mut net = Vec::new();
        for i in 0..NET_SLOTS {
            let (slot, rt) = netslot::make(i, cfg.sample_rate);
            slots.push(slot);
            net.push(rt);
        }
        let comp = Composite::new(engine, cfg.block.max(64), sec_in, sec_out, net);
        let rebuild = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut inner = Inner {
            cfg: cfg.clone(),
            driver: None,
            primary_error,
            primary_ch,
            last_period: u64::MAX,
            rebuild: rebuild.clone(),
            #[cfg(feature = "hw")]
            secondaries,
            #[cfg(not(feature = "hw"))]
            secondary_errors: cfg.secondary.iter().map(|_| "Compilado sem suporte a dispositivos reais.".to_string()).collect(),
        };
        inner.open_primary(comp, &stats);
        #[cfg(feature = "hw")]
        inner.open_secondaries();
        let inner = Arc::new(Mutex::new(inner));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (i2, s2, st2) = (inner.clone(), stop.clone(), stats.clone());
        let supervisor = std::thread::Builder::new()
            .name("sona-dispositivos".into())
            .spawn(move || {
                while !s2.load(Relaxed) {
                    std::thread::sleep(Duration::from_millis(500));
                    i2.lock().unwrap().supervise(&st2);
                }
            })
            .expect("thread de supervisão");
        AudioHost { inner, slots, rebuild, stats, stop, supervisor: Some(supervisor) }
    }

    pub fn status(&self) -> Vec<DeviceStatus> {
        self.inner.lock().unwrap().status()
    }

    pub fn primary_name(&self) -> String {
        let i = self.inner.lock().unwrap();
        match i.cfg.primary.as_str() {
            PRIMARY_SIM => "Simulado".into(),
            PRIMARY_INTERNAL => "Relógio interno".into(),
            n => display_name(n).into(),
        }
    }

    /// Para tudo (antes de trocar a configuração).
    pub fn shutdown(mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.supervisor.take() {
            let _ = t.join();
        }
        let mut i = self.inner.lock().unwrap();
        #[cfg(feature = "hw")]
        for s in i.secondaries.iter_mut() {
            if let Some(h) = s.handle.take() {
                h.stop();
            }
        }
        match i.driver.take() {
            Some(Driver::Clock(c)) => drop(c.stop()),
            #[cfg(feature = "hw")]
            Some(Driver::Hw(h)) => drop(h.stop()),
            None => {}
        }
        self.stats.running.store(false, Relaxed);
    }
}

impl Inner {
    fn open_primary(&mut self, comp: Composite, stats: &Arc<EngineStats>) {
        let cfg = &self.cfg;
        match cfg.primary.as_str() {
            PRIMARY_SIM => self.driver = Some(Driver::Clock(sim::start(comp, sim::SIM_INPUTS, cfg.sample_rate, cfg.block, stats.clone()))),
            PRIMARY_INTERNAL => self.driver = Some(Driver::Clock(sim::start(comp, 0, cfg.sample_rate, cfg.block, stats.clone()))),
            #[cfg(feature = "hw")]
            name => {
                let (ci, co) = self.primary_ch;
                match hw::start_primary(name, comp, ci, co, cfg.sample_rate, cfg.block, stats.clone()) {
                    Ok(h) => {
                        self.primary_error = None;
                        self.driver = Some(Driver::Hw(h));
                    }
                    Err((e, comp)) => {
                        // Sem o principal, o relógio interno mantém rede e secundários vivos.
                        tracing::warn!("Principal indisponível ({e}). Usando o relógio interno.");
                        self.primary_error = Some(e);
                        self.driver = Some(Driver::Clock(sim::start(comp, 0, cfg.sample_rate, cfg.block, stats.clone())));
                    }
                }
            }
            #[cfg(not(feature = "hw"))]
            _ => self.driver = Some(Driver::Clock(sim::start(comp, 0, cfg.sample_rate, cfg.block, stats.clone()))),
        }
    }

    #[cfg(feature = "hw")]
    fn open_secondaries(&mut self) {
        let (sr, block) = (self.cfg.sample_rate, self.cfg.block);
        for s in self.secondaries.iter_mut() {
            if s.handle.is_some() {
                continue;
            }
            let Some(end) = s.parked.take() else { continue };
            match hw::start_secondary(&s.cfg.device, end, s.cfg.channels, sr, block) {
                Ok(h) => {
                    if s.error.is_some() {
                        tracing::info!("{} reconectado.", s.cfg.device);
                    }
                    s.error = None;
                    s.handle = Some(h);
                }
                Err((e, end)) => {
                    if s.error.as_deref() != Some(e.as_str()) {
                        tracing::warn!("{e}");
                    }
                    s.error = Some(e);
                    s.parked = Some(end);
                }
            }
        }
    }

    /// A cada 0,5 s: derruba o que falhou e tenta reabrir (a cada ~3 s).
    fn supervise(&mut self, stats: &Arc<EngineStats>) {
        #[cfg(feature = "hw")]
        {
            for s in self.secondaries.iter_mut() {
                if s.handle.as_ref().is_some_and(|h| h.failed.load(Relaxed)) {
                    let h = s.handle.take().unwrap();
                    s.parked = h.stop();
                    s.error = Some("Aparelho caiu. Tentando reconectar…".into());
                }
            }
            let period = stats.blocks.load(Relaxed) / (self.cfg.sample_rate as u64 * 3 / self.cfg.block as u64).max(1);
            let new_period = std::mem::replace(&mut self.last_period, period) != period;
            if new_period && self.secondaries.iter().any(|s| s.handle.is_none()) {
                self.open_secondaries();
            }
            let real_primary = !matches!(self.cfg.primary.as_str(), PRIMARY_SIM | PRIMARY_INTERNAL);
            // Principal ausente desde a partida (sem canais na lista): quando
            // aparecer, pede a reconstrução completa.
            if new_period && real_primary && self.primary_ch == (0, 0) {
                if let Ok((ci, co)) = hw::channels(&self.cfg.primary, self.cfg.sample_rate) {
                    if co > 0 {
                        tracing::info!("{} apareceu. Remontando os canais.", self.cfg.primary);
                        self.rebuild.store(true, Relaxed);
                    }
                    let _ = ci;
                }
                return;
            }
            // Principal real que caiu: volta ao relógio interno e tenta de novo depois.
            let primary_failed = matches!(&self.driver, Some(Driver::Hw(h)) if h.failed.load(Relaxed));
            let retry_primary = new_period && self.primary_error.is_some() && real_primary;
            if primary_failed || retry_primary {
                let comp = match self.driver.take() {
                    Some(Driver::Hw(h)) => h.stop(),
                    Some(Driver::Clock(c)) => Some(c.stop()),
                    None => None,
                };
                if let Some(comp) = comp {
                    if primary_failed {
                        self.primary_error = Some("Aparelho caiu. Tentando reconectar…".into());
                    }
                    self.open_primary(comp, stats);
                }
            }
        }
        #[cfg(not(feature = "hw"))]
        let _ = stats;
    }

    fn status(&self) -> Vec<DeviceStatus> {
        let mut v = vec![DeviceStatus {
            role: "principal",
            name: self.cfg.primary.clone(),
            label: match self.cfg.primary.as_str() {
                PRIMARY_SIM => "Mesa simulada".into(),
                PRIMARY_INTERNAL => "Relógio interno".into(),
                n => n.into(),
            },
            connected: self.primary_error.is_none(),
            error: self.primary_error.clone(),
            drift_ppm: None,
            latency_ms: None,
            underruns: 0,
            overruns: 0,
        }];
        #[cfg(feature = "hw")]
        for s in &self.secondaries {
            v.push(DeviceStatus {
                role: if s.cfg.direction == Direction::Input { "entrada" } else { "saída" },
                name: s.cfg.device.clone(),
                label: s.cfg.label.clone(),
                connected: s.handle.is_some(),
                error: s.error.clone(),
                drift_ppm: s.handle.is_some().then(|| (s.stats.correction_ppm() * 10.0).round() / 10.0),
                latency_ms: s
                    .handle
                    .is_some()
                    .then(|| (s.stats.fill_frames.load(Relaxed) as f64 / self.cfg.sample_rate as f64 * 10_000.0).round() / 10.0),
                underruns: s.stats.underruns.load(Relaxed),
                overruns: s.stats.overruns.load(Relaxed),
            });
        }
        #[cfg(not(feature = "hw"))]
        for (s, e) in self.cfg.secondary.iter().zip(&self.secondary_errors) {
            v.push(DeviceStatus {
                role: if s.direction == Direction::Input { "entrada" } else { "saída" },
                name: s.device.clone(),
                label: s.label.clone(),
                connected: false,
                error: Some(e.clone()),
                drift_ppm: None,
                latency_ms: None,
                underruns: 0,
                overruns: 0,
            });
        }
        v
    }
}
