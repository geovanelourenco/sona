//! Ciclo de vida do bridge: login, canal de controle, sessão UDP, aparelhos e
//! reconexão automática.

use crate::audio::{self, Levels};
use crate::config::BridgeConfig;
use serde::Serialize;
use serde_json::{json, Value};
use sona_net::{Header, Key, FRAMES, KIND_ALIVE, KIND_AUDIO, MAX_PACKET};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    /// "parado", "conectando", "conectado", "senha", "erro".
    pub state: String,
    pub message: String,
    pub server: String,
    pub name: String,
    pub slot: Option<usize>,
    pub send_device: Option<String>,
    pub receive_device: Option<String>,
    pub send_error: Option<String>,
    pub receive_error: Option<String>,
    pub packets_in: u64,
    pub packets_out: u64,
    pub lost_in: u64,
    /// Estado que o servidor mede do nosso envio.
    pub server_view: Value,
    pub drift_ppm: f64,
    pub latency_ms: f64,
    pub underruns: u64,
    pub send_peak: f32,
    pub recv_peak: f32,
    pub sim_latency_ms: Option<f64>,
    pub sim_measurements: u64,
}

pub struct Bridge {
    pub config: Mutex<BridgeConfig>,
    pub status: Mutex<Status>,
    pub levels: Arc<Levels>,
    path: PathBuf,
    /// Aumenta para pedir reconexão (configuração mudou).
    restart: AtomicU64,
    stop: AtomicBool,
}

impl Bridge {
    pub fn new(path: PathBuf) -> Arc<Bridge> {
        let cfg = BridgeConfig::load(&path);
        Arc::new(Bridge {
            config: Mutex::new(cfg),
            status: Mutex::new(Status { state: "parado".into(), ..Default::default() }),
            levels: Arc::new(Levels::default()),
            path,
            restart: AtomicU64::new(0),
            stop: AtomicBool::new(false),
        })
    }

    pub fn save(&self) {
        if let Err(e) = self.config.lock().unwrap().save(&self.path) {
            tracing::error!("Não consegui salvar a configuração: {e}");
        }
    }

    /// Pede para reconectar com a configuração atual.
    pub fn reload(&self) {
        self.restart.fetch_add(1, Relaxed);
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Relaxed);
    }

    fn set_state(&self, state: &str, message: impl Into<String>) {
        let mut s = self.status.lock().unwrap();
        s.state = state.into();
        s.message = message.into();
    }

    /// Entra no servidor com a senha e guarda o token. `server_name`: nome do
    /// servidor achado na rede (vazio se o endereço foi digitado).
    pub fn login(&self, server: &str, password: &str, name: &str, server_name: &str) -> Result<(), String> {
        let mut cfg = self.config.lock().unwrap().clone();
        cfg.server = server.trim().to_string();
        cfg.server_name = server_name.trim().to_string();
        if !name.trim().is_empty() {
            cfg.name = name.trim().to_string();
        }
        let body = json!({ "password": password, "name": format!("Bridge {}", cfg.name) }).to_string();
        let (code, resp) = crate::http::post_json(&cfg.server_addr(), "/api/login", &body)?;
        let j: Value = serde_json::from_str(&resp).unwrap_or_default();
        if code != 200 {
            return Err(j["error"].as_str().unwrap_or("Não consegui entrar.").to_string());
        }
        cfg.token = j["token"].as_str().unwrap_or_default().to_string();
        *self.config.lock().unwrap() = cfg;
        self.save();
        self.reload();
        Ok(())
    }

    /// Roda para sempre (numa thread própria): conecta, mantém e reconecta.
    pub fn run(self: &Arc<Self>) {
        let mut backoff = Duration::from_millis(500);
        while !self.stop.load(Relaxed) {
            let generation = self.restart.load(Relaxed);
            let cfg = self.config.lock().unwrap().clone();
            {
                let mut s = self.status.lock().unwrap();
                s.server = cfg.server.clone();
                s.name = cfg.name.clone();
                s.slot = None;
            }
            if cfg.server.trim().is_empty() {
                self.set_state("parado", "Informe o endereço do servidor.");
                self.wait_change(generation, Duration::from_secs(3600));
                continue;
            }
            if cfg.token.is_empty() {
                self.set_state("senha", "Entre com a senha de acesso remoto do servidor.");
                self.wait_change(generation, Duration::from_secs(3600));
                continue;
            }
            self.set_state("conectando", format!("Conectando a {}…", cfg.server));
            match self.session(&cfg, generation) {
                Ok(()) => backoff = Duration::from_millis(500),
                Err(SessionEnd::Unauthorized(m)) => {
                    // Token revogado (senha trocada): pede a senha de novo.
                    self.config.lock().unwrap().token.clear();
                    self.save();
                    self.set_state("senha", m);
                    continue;
                }
                Err(SessionEnd::Failed(m)) => {
                    self.set_state("erro", m);
                    // Servidor achado pela rede e fora do ar: talvez mudou de IP.
                    if !cfg.server_name.is_empty() && self.rediscover(&cfg) {
                        continue;
                    }
                    self.wait_change(generation, backoff);
                    backoff = (backoff * 2).min(Duration::from_secs(5));
                }
            }
        }
        self.set_state("parado", "Bridge parado.");
    }

    /// Procura o servidor pelo nome; se achou noutro endereço, troca e salva.
    fn rediscover(&self, cfg: &BridgeConfig) -> bool {
        let found = sona_net::discovery::discover(Duration::from_secs(2));
        let Some(f) = found.into_iter().find(|f| f.name == cfg.server_name) else { return false };
        if f.address == cfg.server_addr() {
            return false;
        }
        tracing::info!("\"{}\" agora está em {}", cfg.server_name, f.address);
        self.config.lock().unwrap().server = f.address;
        self.save();
        true
    }

    fn wait_change(&self, generation: u64, max: Duration) {
        let t0 = Instant::now();
        while t0.elapsed() < max && self.restart.load(Relaxed) == generation && !self.stop.load(Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn session(self: &Arc<Self>, cfg: &BridgeConfig, generation: u64) -> Result<(), SessionEnd> {
        let addr = cfg.server_addr();
        let url = format!("ws://{addr}/api/bridge?token={}", cfg.token);
        let (mut ws, _) = match tungstenite::connect(&url) {
            Ok(x) => x,
            Err(tungstenite::Error::Http(r)) if r.status() == 401 => {
                return Err(SessionEnd::Unauthorized(
                    "Acesso encerrado no servidor (senha trocada ou acesso remoto desligado). Entre de novo.".into(),
                ))
            }
            Err(tungstenite::Error::Http(r)) => return Err(SessionEnd::Failed(format!("Servidor recusou: {}", r.status()))),
            Err(e) => return Err(SessionEnd::Failed(format!("Servidor {addr} fora do ar ({e}). Tentando de novo…"))),
        };
        ws.send(tungstenite::Message::Text(json!({"type": "bridge.hello", "name": cfg.name}).to_string().into()))
            .map_err(|e| SessionEnd::Failed(e.to_string()))?;
        let ok: Value = loop {
            match ws.read() {
                Ok(tungstenite::Message::Text(t)) => {
                    let v: Value = serde_json::from_str(&t).unwrap_or_default();
                    match v["type"].as_str() {
                        Some("bridge.ok") => break v,
                        Some("bridge.error") => return Err(SessionEnd::Failed(v["message"].as_str().unwrap_or("Recusado.").into())),
                        _ => continue,
                    }
                }
                Ok(_) => continue,
                Err(e) => return Err(SessionEnd::Failed(e.to_string())),
            }
        };
        let session = ok["session"].as_u64().unwrap_or(0) as u32;
        let key: Key = hex::decode(ok["key"].as_str().unwrap_or(""))
            .ok()
            .and_then(|v| v.try_into().ok())
            .ok_or(SessionEnd::Failed("Chave inválida.".into()))?;
        let sr = ok["sample_rate"].as_u64().unwrap_or(48_000) as u32;
        let udp_port = ok["udp_port"].as_u64().unwrap_or(7340) as u16;
        let host = addr.rsplit_once(':').map(|x| x.0).unwrap_or(&addr).to_string();
        let server_udp: SocketAddr = (host.as_str(), udp_port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut i| i.next())
            .ok_or(SessionEnd::Failed("Endereço do servidor inválido.".into()))?;
        let slot = ok["slot"].as_u64().map(|s| s as usize);

        // Sessão UDP e aparelhos.
        let sock = UdpSocket::bind("0.0.0.0:0").map_err(|e| SessionEnd::Failed(e.to_string()))?;
        sock.set_read_timeout(Some(Duration::from_millis(200))).ok();
        let stop = Arc::new(AtomicBool::new(false));
        let counters = Arc::new(Counters::default());

        let (tx_prod, tx_cons) = rtrb::RingBuffer::<f32>::new(sr as usize); // 0,5 s
        let (writer, mut reader, dstats) = sona_core::drift::bridge(2, sr as f64, 12.0);
        reader.set_adaptive(80.0);

        let mut send_err = None;
        let mut recv_err = None;
        let send_handle = cfg.send.as_ref().filter(|e| !e.device.is_empty()).and_then(|e| {
            match audio::start_send(&e.device, sr, cfg.sim_ppm, tx_prod, self.levels.clone()) {
                Ok(h) => Some(h),
                Err(m) => {
                    send_err = Some(m);
                    None
                }
            }
        });
        let recv_handle = cfg.receive.as_ref().filter(|e| !e.device.is_empty()).and_then(|e| {
            match audio::start_receive(&e.device, sr, cfg.sim_ppm, reader, self.levels.clone()) {
                Ok(h) => Some(h),
                Err(m) => {
                    recv_err = Some(m);
                    None
                }
            }
        });
        let sending = send_handle.is_some();
        let tx_thread = {
            let (sock, stop, counters) = (sock.try_clone().map_err(|e| SessionEnd::Failed(e.to_string()))?, stop.clone(), counters.clone());
            std::thread::Builder::new()
                .name("bridge-tx".into())
                .spawn(move || send_loop(sock, server_udp, tx_cons, key, session, sending, counters, stop))
                .expect("thread")
        };
        let rx_thread = {
            let (sock, stop, counters) = (sock.try_clone().map_err(|e| SessionEnd::Failed(e.to_string()))?, stop.clone(), counters.clone());
            std::thread::Builder::new()
                .name("bridge-rx".into())
                .spawn(move || receive_loop(sock, writer, key, session, counters, stop))
                .expect("thread")
        };
        {
            let mut s = self.status.lock().unwrap();
            s.state = "conectado".into();
            s.message = format!("Conectado a {} como \"{}\" (vaga {}).", cfg.server, cfg.name, slot.map(|x| x + 1).unwrap_or(0));
            s.slot = slot;
            s.send_device = cfg.send.as_ref().map(|e| e.device.clone());
            s.receive_device = cfg.receive.as_ref().map(|e| e.device.clone());
            s.send_error = send_err;
            s.receive_error = recv_err;
        }
        tracing::info!("conectado a {} (vaga {:?})", cfg.server, slot.map(|x| x + 1));

        // Controle: lê estado do servidor; ping a cada 5 s; sai se a config mudar.
        if let tungstenite::stream::MaybeTlsStream::Plain(s) = ws.get_ref() {
            s.set_read_timeout(Some(Duration::from_millis(250))).ok();
        }
        let mut last_ping = Instant::now();
        let result = loop {
            if self.stop.load(Relaxed) || self.restart.load(Relaxed) != generation {
                let _ = ws.close(None);
                break Ok(());
            }
            match ws.read() {
                Ok(tungstenite::Message::Text(t)) => {
                    let v: Value = serde_json::from_str(&t).unwrap_or_default();
                    match v["type"].as_str() {
                        Some("bridge.status") => self.status.lock().unwrap().server_view = v,
                        Some("bridge.restart") => break Err(SessionEnd::Failed(v["message"].as_str().unwrap_or("Reconectando.").into())),
                        Some("bridge.revoked") => {
                            break Err(SessionEnd::Unauthorized(v["message"].as_str().unwrap_or("Acesso encerrado. Entre de novo.").into()))
                        }
                        _ => {}
                    }
                }
                Ok(tungstenite::Message::Close(_)) => break Err(SessionEnd::Failed("Servidor encerrou a conexão. Reconectando…".into())),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => break Err(SessionEnd::Failed(format!("Conexão perdida ({e}). Reconectando…"))),
            }
            if last_ping.elapsed() > Duration::from_secs(5) {
                let _ = ws.send(tungstenite::Message::Text(json!({"type": "ping"}).to_string().into()));
                last_ping = Instant::now();
            }
            let mut s = self.status.lock().unwrap();
            s.packets_in = counters.packets_in.load(Relaxed);
            s.packets_out = counters.packets_out.load(Relaxed);
            s.lost_in = counters.lost.load(Relaxed);
            s.drift_ppm = (dstats.correction_ppm() * 10.0).round() / 10.0;
            s.latency_ms = (dstats.fill_frames.load(Relaxed) as f64 / sr as f64 * 10_000.0).round() / 10.0;
            s.underruns = dstats.underruns.load(Relaxed);
            s.send_peak = s.send_peak.max(Levels::take(&self.levels.send_peak));
            s.recv_peak = s.recv_peak.max(Levels::take(&self.levels.recv_peak));
            let n = self.levels.sim_measurements.load(Relaxed);
            s.sim_measurements = n;
            s.sim_latency_ms = (n > 0).then(|| self.levels.sim_latency_us.load(Relaxed) as f64 / 1000.0);
        };
        stop.store(true, Relaxed);
        drop(send_handle);
        drop(recv_handle);
        let _ = tx_thread.join();
        let _ = rx_thread.join();
        result
    }
}

enum SessionEnd {
    Unauthorized(String),
    Failed(String),
}

#[derive(Default)]
struct Counters {
    packets_in: AtomicU64,
    packets_out: AtomicU64,
    lost: AtomicU64,
}

#[allow(clippy::too_many_arguments)]
fn send_loop(
    sock: UdpSocket,
    to: SocketAddr,
    mut cons: rtrb::Consumer<f32>,
    key: Key,
    session: u32,
    sending: bool,
    c: Arc<Counters>,
    stop: Arc<AtomicBool>,
) {
    let mut pkt = [0u8; MAX_PACKET];
    let mut chunk = vec![0.0f32; FRAMES * 2];
    let (mut seq, mut ts) = (0u32, 0u64);
    let mut last_alive = Instant::now() - Duration::from_secs(10);
    while !stop.load(Relaxed) {
        if sending && cons.slots() >= FRAMES * 2 {
            if let Ok(ch) = cons.read_chunk(FRAMES * 2) {
                let (a, b) = ch.as_slices();
                chunk[..a.len()].copy_from_slice(a);
                chunk[a.len()..a.len() + b.len()].copy_from_slice(b);
                ch.commit_all();
            }
            let h = Header { kind: KIND_AUDIO, channels: 2, session, seq, timestamp: ts, frames: FRAMES as u16 };
            let len = sona_net::encode(&mut pkt, &key, &h, &chunk);
            if sock.send_to(&pkt[..len], to).is_ok() {
                c.packets_out.fetch_add(1, Relaxed);
            }
            seq = seq.wrapping_add(1);
            ts += FRAMES as u64;
            last_alive = Instant::now();
        } else {
            // Sinal de vida: o servidor aprende o nosso endereço por ele.
            if last_alive.elapsed() > Duration::from_millis(500) {
                let h = Header { kind: KIND_ALIVE, channels: 0, session, seq, timestamp: ts, frames: 0 };
                let len = sona_net::encode(&mut pkt, &key, &h, &[]);
                let _ = sock.send_to(&pkt[..len], to);
                last_alive = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn receive_loop(sock: UdpSocket, mut writer: sona_core::drift::Writer, key: Key, session: u32, c: Arc<Counters>, stop: Arc<AtomicBool>) {
    let mut buf = [0u8; MAX_PACKET + 64];
    let mut samples = vec![0.0f32; FRAMES * 4 * sona_net::MAX_CHANNELS];
    let silence = vec![0.0f32; FRAMES * 2];
    let mut expected: Option<u32> = None;
    while !stop.load(Relaxed) {
        let Ok((n, _)) = sock.recv_from(&mut buf) else { continue };
        let Some(h) = sona_net::decode(&buf[..n], &key, &mut samples) else { continue };
        if h.session != session || h.kind != KIND_AUDIO || h.channels != 2 {
            continue;
        }
        c.packets_in.fetch_add(1, Relaxed);
        if let Some(exp) = expected {
            let ahead = h.seq.wrapping_sub(exp);
            if ahead > u32::MAX / 2 {
                continue;
            }
            if ahead > 0 {
                c.lost.fetch_add(ahead as u64, Relaxed);
                for _ in 0..ahead.min(50) {
                    writer.push_interleaved(&silence);
                }
            }
        }
        expected = Some(h.seq.wrapping_add(1));
        writer.push_interleaved(&samples[..h.frames as usize * 2]);
    }
}
