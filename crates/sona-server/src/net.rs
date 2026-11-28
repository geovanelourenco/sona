//! Áudio em rede com o Sona Bridge (ADR 0003).
//!
//! - Controle: WebSocket `/api/bridge?token=…` (token de sessão do login). O
//!   bridge manda `bridge.hello` com o nome; o servidor reserva uma vaga de rede
//!   (a mesma para o mesmo nome, guardada em `data/bridges.json`), cria uma
//!   chave de sessão e responde `bridge.ok`.
//! - Áudio: UDP na mesma porta do HTTP. Uma thread recebe todos os pacotes e
//!   despacha pela sessão; cada bridge tem uma thread de envio. O endereço do
//!   bridge é aprendido do primeiro pacote válido (ele manda sinal de vida).

use crate::api::Shared;
use crate::audio::netslot::NetEnds;
use crate::audio::NET_SLOTS;
use serde::Deserialize;
use serde_json::json;
use sona_net::{Header, Key, FRAMES, KIND_ALIVE, KIND_AUDIO, MAX_PACKET};
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct LinkStats {
    pub packets_in: AtomicU64,
    pub packets_out: AtomicU64,
    pub lost: AtomicU64,
    pub late: AtomicU64,
    pub forged: AtomicU64,
}

enum RxCmd {
    Add { session: u32, key: Key, writer: sona_core::drift::Writer, addr: Arc<Mutex<Option<SocketAddr>>>, stats: Arc<LinkStats> },
    Remove { session: u32, back: Sender<sona_core::drift::Writer> },
}

/// Bridge ligado: nome, estatísticas e instante da conexão.
pub type Link = (String, Arc<LinkStats>, Instant);

pub struct Hub {
    socket: UdpSocket,
    port: u16,
    cmd: Mutex<Sender<RxCmd>>,
    sticky: Mutex<HashMap<String, usize>>,
    sticky_path: PathBuf,
    /// Bridges ligados agora: vaga → (nome, estatísticas, instante da conexão).
    pub links: Mutex<HashMap<usize, Link>>,
}

pub async fn spawn_udp(app: Shared, port: u16) -> Result<(), String> {
    let socket = UdpSocket::bind(("0.0.0.0", port)).map_err(|e| format!("Não consegui abrir a porta UDP {port}: {e}"))?;
    socket.set_read_timeout(Some(Duration::from_millis(200))).ok();
    let port = socket.local_addr().map(|a| a.port()).unwrap_or(port);
    let (tx, rx) = channel();
    let rx_sock = socket.try_clone().map_err(|e| e.to_string())?;
    std::thread::Builder::new().name("sona-rede-rx".into()).spawn(move || receive_loop(rx_sock, rx)).map_err(|e| e.to_string())?;
    let sticky_path = app.data_dir().join("bridges.json");
    let sticky = std::fs::read(&sticky_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let hub =
        Arc::new(Hub { socket, port, cmd: Mutex::new(tx), sticky: Mutex::new(sticky), sticky_path, links: Mutex::new(HashMap::new()) });
    app.net.set(hub).map_err(|_| "rede já iniciada".to_string())?;
    Ok(())
}

struct RxSession {
    key: Key,
    writer: sona_core::drift::Writer,
    addr: Arc<Mutex<Option<SocketAddr>>>,
    stats: Arc<LinkStats>,
    expected: Option<u32>,
}

fn receive_loop(socket: UdpSocket, cmds: Receiver<RxCmd>) {
    let mut sessions: HashMap<u32, RxSession> = HashMap::new();
    let mut buf = [0u8; MAX_PACKET + 64];
    let mut samples = vec![0.0f32; FRAMES * 4 * sona_net::MAX_CHANNELS];
    let mut stereo = vec![0.0f32; FRAMES * 4 * 2];
    let silence = vec![0.0f32; FRAMES * 2];
    loop {
        while let Ok(c) = cmds.try_recv() {
            match c {
                RxCmd::Add { session, key, writer, addr, stats } => {
                    sessions.insert(session, RxSession { key, writer, addr, stats, expected: None });
                }
                RxCmd::Remove { session, back } => {
                    if let Some(s) = sessions.remove(&session) {
                        let _ = back.send(s.writer);
                    }
                }
            }
        }
        let (n, from) = match socket.recv_from(&mut buf) {
            Ok(x) => x,
            Err(_) => continue,
        };
        let Some(h) = sona_net::peek(&buf[..n]) else { continue };
        let Some(s) = sessions.get_mut(&h.session) else { continue };
        let Some(h) = sona_net::decode(&buf[..n], &s.key, &mut samples) else {
            s.stats.forged.fetch_add(1, Relaxed);
            continue;
        };
        {
            let mut a = s.addr.lock().unwrap();
            if *a != Some(from) {
                *a = Some(from);
            }
        }
        if h.kind != KIND_AUDIO {
            continue;
        }
        s.stats.packets_in.fetch_add(1, Relaxed);
        if let Some(exp) = s.expected {
            let ahead = h.seq.wrapping_sub(exp);
            if ahead > u32::MAX / 2 {
                // Chegou depois do seguinte: tarde demais para tocar.
                s.stats.late.fetch_add(1, Relaxed);
                continue;
            }
            if ahead > 0 {
                // Perdidos: silêncio no lugar mantém o tempo (até 50 pacotes).
                s.stats.lost.fetch_add(ahead as u64, Relaxed);
                for _ in 0..ahead.min(50) {
                    s.writer.push_interleaved(&silence[..FRAMES * 2]);
                }
            }
        }
        s.expected = Some(h.seq.wrapping_add(1));
        let frames = h.frames as usize;
        let ch = h.channels as usize;
        for i in 0..frames {
            let l = samples[i * ch];
            let r = if ch > 1 { samples[i * ch + 1] } else { l };
            stereo[2 * i] = l;
            stereo[2 * i + 1] = r;
        }
        s.writer.push_interleaved(&stereo[..frames * 2]);
    }
}

fn send_loop(
    socket: UdpSocket,
    mut cons: rtrb::Consumer<f32>,
    key: Key,
    session: u32,
    addr: Arc<Mutex<Option<SocketAddr>>>,
    stats: Arc<LinkStats>,
    stop: Arc<AtomicBool>,
) -> rtrb::Consumer<f32> {
    let mut pkt = [0u8; MAX_PACKET];
    let mut chunk = vec![0.0f32; FRAMES * 2];
    let (mut seq, mut ts) = (0u32, 0u64);
    let mut last_alive = Instant::now();
    while !stop.load(Relaxed) {
        if cons.slots() >= FRAMES * 2 {
            if let Ok(c) = cons.read_chunk(FRAMES * 2) {
                let (a, b) = c.as_slices();
                chunk[..a.len()].copy_from_slice(a);
                chunk[a.len()..a.len() + b.len()].copy_from_slice(b);
                c.commit_all();
            }
            let to = *addr.lock().unwrap();
            if let Some(to) = to {
                let h = Header { kind: KIND_AUDIO, channels: 2, session, seq, timestamp: ts, frames: FRAMES as u16 };
                let len = sona_net::encode(&mut pkt, &key, &h, &chunk);
                if socket.send_to(&pkt[..len], to).is_ok() {
                    stats.packets_out.fetch_add(1, Relaxed);
                }
                last_alive = Instant::now();
            }
            seq = seq.wrapping_add(1);
            ts += FRAMES as u64;
        } else {
            if last_alive.elapsed() > Duration::from_secs(1) {
                if let Some(to) = *addr.lock().unwrap() {
                    let h = Header { kind: KIND_ALIVE, channels: 0, session, seq, timestamp: ts, frames: 0 };
                    let len = sona_net::encode(&mut pkt, &key, &h, &[]);
                    let _ = socket.send_to(&pkt[..len], to);
                }
                last_alive = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    cons
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum BridgeMsg {
    #[serde(rename = "bridge.hello")]
    Hello { name: String },
    #[serde(rename = "ping")]
    Ping,
}

impl Hub {
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Vaga para o nome: a de sempre se estiver livre; senão a primeira livre
    /// que não seja de outro bridge conhecido; senão qualquer livre.
    fn pick_slot(&self, name: &str, free: &[bool]) -> Option<usize> {
        let mut sticky = self.sticky.lock().unwrap();
        let chosen = match sticky.get(name) {
            Some(&s) if free.get(s).copied().unwrap_or(false) => Some(s),
            _ => {
                let owned: Vec<usize> = sticky.values().copied().collect();
                (0..free.len()).find(|&i| free[i] && !owned.contains(&i)).or_else(|| (0..free.len()).find(|&i| free[i]))
            }
        }?;
        if sticky.get(name) != Some(&chosen) {
            sticky.retain(|_, v| *v != chosen);
            sticky.insert(name.to_string(), chosen);
            if let Ok(b) = serde_json::to_vec_pretty(&*sticky) {
                let _ = crate::state::write_atomic(&self.sticky_path, &b);
            }
        }
        Some(chosen)
    }
}

/// Sessão de controle de um bridge (já autenticado).
pub async fn bridge_session(app: Shared, socket: axum::extract::ws::WebSocket, peer: SocketAddr, token: String) {
    use axum::extract::ws::Message;
    use futures_util::{SinkExt, StreamExt};
    let Some(hub) = app.net.get().cloned() else { return };
    let (mut sink, mut stream) = socket.split();
    let send = |v: serde_json::Value| Message::Text(v.to_string().into());

    // 1. Apresentação.
    let name = loop {
        match tokio::time::timeout(Duration::from_secs(10), stream.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => match serde_json::from_str::<BridgeMsg>(&t) {
                Ok(BridgeMsg::Hello { name }) => break name.trim().chars().take(32).collect::<String>(),
                _ => continue,
            },
            _ => return,
        }
    };
    let name = if name.is_empty() { format!("Bridge {}", peer.ip()) } else { name };

    // 2. Vaga e lado de rede. Se a vaga de sempre ainda está com este mesmo
    // bridge (ele reconectou antes de a sessão antiga terminar de fechar),
    // espera ela ser devolvida em vez de cair em outra vaga.
    let generation = app.generation.load(Relaxed);
    let slots = app.net_slots();
    let own = hub.sticky.lock().unwrap().get(&name).copied();
    if let Some(s) = own.and_then(|i| slots.get(i)) {
        for _ in 0..30 {
            let same = s.busy.load(Relaxed) && s.name.lock().unwrap().as_deref() == Some(name.as_str());
            if !same {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    let free: Vec<bool> = slots.iter().map(|s| !s.busy.load(Relaxed)).collect();
    let Some(idx) = hub.pick_slot(&name, &free) else {
        let _ = sink
            .send(send(json!({"type": "bridge.error", "message": format!("Todas as {NET_SLOTS} vagas de rede estão ocupadas.")})))
            .await;
        return;
    };
    let slot = slots[idx].clone();
    let Some(NetEnds { incoming, outgoing }) = slot.take(&name) else {
        let _ = sink.send(send(json!({"type": "bridge.error", "message": "Vaga ocupada. Tente de novo."}))).await;
        return;
    };

    // 3. Sessão UDP.
    let mut rnd = [0u8; 20];
    getrandom::fill(&mut rnd).expect("aleatório");
    let session = u32::from_le_bytes(rnd[..4].try_into().unwrap()) | 1;
    let key: Key = rnd[4..20].try_into().unwrap();
    let addr = Arc::new(Mutex::new(None));
    let stats = Arc::new(LinkStats::default());
    let _ = hub.cmd.lock().unwrap().send(RxCmd::Add { session, key, writer: incoming, addr: addr.clone(), stats: stats.clone() });
    let stop = Arc::new(AtomicBool::new(false));
    let sender = {
        let (sock, addr, stats, stop) = (hub.socket.try_clone().expect("socket"), addr.clone(), stats.clone(), stop.clone());
        std::thread::Builder::new()
            .name("sona-rede-tx".into())
            .spawn(move || send_loop(sock, outgoing, key, session, addr, stats, stop))
            .expect("thread de envio")
    };
    hub.links.lock().unwrap().insert(idx, (name.clone(), stats.clone(), Instant::now()));
    app.store.lock().unwrap().set_net_port(idx, Some(&name));
    app.broadcast_state(0);
    tracing::info!("bridge \"{name}\" conectou de {} (vaga {})", peer.ip(), idx + 1);

    let sample_rate = app.audio.lock().unwrap().sample_rate;
    let ok = json!({
        "type": "bridge.ok",
        "slot": idx,
        "session": session,
        "key": key.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "udp_port": hub.port,
        "sample_rate": sample_rate,
        "frames": FRAMES,
    });
    let mut alive = sink.send(send(ok)).await.is_ok();

    // 4. Enquanto ligado: estado a cada segundo; fim se o motor for reconstruído.
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut reason = "saiu";
    while alive {
        tokio::select! {
            m = stream.next() => match m {
                Some(Ok(Message::Text(t))) => {
                    if let Ok(BridgeMsg::Ping) = serde_json::from_str::<BridgeMsg>(&t) {
                        alive = sink.send(send(json!({"type": "pong"}))).await.is_ok();
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
            _ = tick.tick() => {
                // Senha remota trocada ou acesso remoto desligado: encerra.
                if app.auth.lock().unwrap().verify(&token).is_none() {
                    reason = "teve o acesso encerrado";
                    let _ = sink.send(send(json!({"type": "bridge.revoked", "message": "Acesso encerrado no servidor. Entre de novo com a senha."}))).await;
                    break;
                }
                if app.generation.load(Relaxed) != generation {
                    reason = "motor reconstruído";
                    let _ = sink.send(send(json!({"type": "bridge.restart", "message": "A configuração de áudio do servidor mudou. Reconectando."}))).await;
                    break;
                }
                let status = json!({
                    "type": "bridge.status",
                    "packets_in": stats.packets_in.load(Relaxed),
                    "packets_out": stats.packets_out.load(Relaxed),
                    "lost": stats.lost.load(Relaxed),
                    "late": stats.late.load(Relaxed),
                    "drift_ppm": (slot.stats.correction_ppm() * 10.0).round() / 10.0,
                    "latency_ms": slot.stats.fill_frames.load(Relaxed) as f64 / sample_rate as f64 * 1000.0,
                    "underruns": slot.stats.underruns.load(Relaxed),
                });
                alive = sink.send(send(status)).await.is_ok();
            }
        }
    }

    // 5. Encerramento: devolve os dois lados à vaga.
    stop.store(true, Relaxed);
    let outgoing = tokio::task::spawn_blocking(move || sender.join().ok()).await.ok().flatten();
    let (btx, brx) = channel();
    let _ = hub.cmd.lock().unwrap().send(RxCmd::Remove { session, back: btx });
    let incoming = tokio::task::spawn_blocking(move || brx.recv_timeout(Duration::from_secs(2)).ok()).await.ok().flatten();
    hub.links.lock().unwrap().remove(&idx);
    match (incoming, outgoing) {
        (Some(incoming), Some(outgoing)) => slot.give_back(NetEnds { incoming, outgoing }),
        (i, o) => tracing::warn!("vaga {} não devolvida (entrada {}, saída {})", idx + 1, i.is_some(), o.is_some()),
    }
    if app.generation.load(Relaxed) == generation {
        app.store.lock().unwrap().set_net_port(idx, None);
        app.broadcast_state(0);
    }
    tracing::info!("bridge \"{name}\" {reason} (vaga {})", idx + 1);
}
