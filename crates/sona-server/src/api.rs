//! Plano de controle: HTTP + WebSocket. O áudio não depende de nenhum cliente
//! conectado: fechar todas as abas não para o motor (FR-06).
//!
//! Acesso (ver `auth.rs`): pedido deste computador usa a senha do app (ou entra
//! direto sem ela); pedido de outro computador só passa com acesso remoto ligado
//! e token de sessão remota.

use crate::audio::config::AudioConfig;
use crate::audio::host::AudioHost;
use crate::audio::EngineStats;
use crate::auth::{Auth, Scope};
use crate::state::Store;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

pub struct App {
    pub store: Mutex<Store>,
    pub auth: Mutex<Auth>,
    pub stats: Arc<EngineStats>,
    pub host: Mutex<Option<AudioHost>>,
    pub audio: Mutex<AudioConfig>,
    audio_path: std::path::PathBuf,
    /// Aumenta a cada reconstrução do motor (sessões de rede pegam vagas novas).
    pub generation: AtomicU64,
    pub net: std::sync::OnceLock<Arc<crate::net::Hub>>,
    pub tx: broadcast::Sender<Arc<str>>,
    pub sessions: AtomicU64,
    pub clients: AtomicU64,
    /// Endereços deste computador na rede (ex.: http://192.168.0.10:7340).
    pub lan_urls: Vec<String>,
    login_failures: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

/// Tentativas erradas seguidas antes de bloquear um IP.
const MAX_FAILURES: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(60);

pub type Shared = Arc<App>;

impl App {
    pub fn new(
        store: Store,
        auth: Auth,
        stats: Arc<EngineStats>,
        host: AudioHost,
        audio: AudioConfig,
        audio_path: std::path::PathBuf,
        lan_urls: Vec<String>,
    ) -> Shared {
        let (tx, _) = broadcast::channel(256);
        Arc::new(App {
            store: Mutex::new(store),
            auth: Mutex::new(auth),
            stats,
            host: Mutex::new(Some(host)),
            audio: Mutex::new(audio),
            audio_path,
            generation: AtomicU64::new(0),
            net: std::sync::OnceLock::new(),
            tx,
            sessions: AtomicU64::new(0),
            clients: AtomicU64::new(0),
            lan_urls,
            login_failures: Mutex::new(HashMap::new()),
        })
    }

    pub fn broadcast_state(&self, by: u64) {
        let snap = self.store.lock().unwrap().snapshot();
        let msg = json!({ "type": "state", "by": by, "state": snap }).to_string();
        let _ = self.tx.send(msg.into());
    }

    pub fn engine_status(&self) -> Value {
        let (name, devices) = match self.host.lock().unwrap().as_ref() {
            Some(h) => (h.primary_name(), serde_json::to_value(h.status()).unwrap_or_default()),
            None => ("parado".to_string(), Value::Array(vec![])),
        };
        let mut v = self.stats.snapshot(&name);
        v["clients"] = self.clients.load(Relaxed).into();
        v["devices"] = devices;
        v["bridges"] = self.bridges_status();
        v
    }

    /// Bridges ligados: vaga, nome, pacotes, perdas, deriva e atraso.
    pub fn bridges_status(&self) -> Value {
        let Some(hub) = self.net.get() else { return Value::Array(vec![]) };
        let slots = self.net_slots();
        let sr = self.audio.lock().unwrap().sample_rate as f64;
        let mut list: Vec<Value> = hub
            .links
            .lock()
            .unwrap()
            .iter()
            .map(|(&i, (name, st, since))| {
                let ds = slots.get(i).map(|s| s.stats.clone());
                json!({
                    "slot": i,
                    "name": name,
                    "seconds": since.elapsed().as_secs(),
                    "packets_in": st.packets_in.load(Relaxed),
                    "packets_out": st.packets_out.load(Relaxed),
                    "lost": st.lost.load(Relaxed),
                    "late": st.late.load(Relaxed),
                    "drift_ppm": ds.as_ref().map(|d| (d.correction_ppm() * 10.0).round() / 10.0),
                    "latency_ms": ds.as_ref().map(|d| (d.fill_frames.load(Relaxed) as f64 / sr * 10_000.0).round() / 10.0),
                    "underruns": ds.as_ref().map(|d| d.underruns.load(Relaxed)),
                })
            })
            .collect();
        list.sort_by_key(|v| v["slot"].as_u64());
        Value::Array(list)
    }

    pub fn data_dir(&self) -> std::path::PathBuf {
        self.audio_path.parent().map(|p| p.to_path_buf()).unwrap_or_default()
    }

    /// Vagas de rede do motor atual.
    pub fn net_slots(&self) -> Vec<Arc<crate::audio::netslot::NetSlot>> {
        self.host.lock().unwrap().as_ref().map(|h| h.slots.clone()).unwrap_or_default()
    }

    /// Troca a configuração de áudio: para os aparelhos, remonta os canais,
    /// cria um motor novo com o mix atual e liga de novo. Bloqueante (~1 s).
    pub fn apply_audio(&self, cfg: AudioConfig) -> Result<(), String> {
        cfg.validate()?;
        cfg.save(&self.audio_path).map_err(|e| format!("Não consegui salvar: {e}"))?;
        if let Some(old) = self.host.lock().unwrap().take() {
            old.shutdown();
        }
        let (io, pin, pout, perr) = crate::audio::host::plan_io(&cfg);
        let layout = io.to_layout(cfg.sample_rate, cfg.block);
        let engine = self.store.lock().unwrap().rebuild(io, layout);
        let host = AudioHost::start(&cfg, engine, (pin, pout), perr, self.stats.clone());
        *self.host.lock().unwrap() = Some(host);
        *self.audio.lock().unwrap() = cfg;
        self.generation.fetch_add(1, Relaxed);
        self.broadcast_state(0);
        Ok(())
    }
}

pub fn router(app: Shared, web_dir: Option<std::path::PathBuf>) -> Router {
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/access", get(access).post(update_access))
        .route("/api/login", axum::routing::post(login))
        .route("/api/state", get(state))
        .route("/api/devices", get(devices))
        .route("/api/audio", get(audio_get).post(audio_set))
        .route("/api/ws", get(ws))
        .route("/api/bridge", get(bridge_ws));
    let api = match web_dir {
        Some(dir) if dir.join("index.html").exists() => {
            let index = dir.join("index.html");
            api.fallback_service(tower_http::services::ServeDir::new(dir).fallback(tower_http::services::ServeFile::new(index)))
        }
        _ => api,
    };
    api.with_state(app)
}

fn is_local(peer: &SocketAddr) -> bool {
    match peer.ip() {
        IpAddr::V4(ip) => ip.is_loopback(),
        IpAddr::V6(ip) => ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback()),
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get("authorization")?.to_str().ok()?.strip_prefix("Bearer ")
}

fn deny(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({ "error": msg }))).into_response()
}

/// Confere o token e a origem. Token local só vale deste computador; token
/// remoto só vale com o acesso remoto ligado (desligar apaga as sessões remotas).
#[allow(clippy::result_large_err)]
fn authed(app: &App, token: Option<&str>, peer: &SocketAddr) -> Result<(String, Scope), Response> {
    let Some((name, scope)) = token.and_then(|t| app.auth.lock().unwrap().verify(t)) else {
        return Err(deny(StatusCode::UNAUTHORIZED, "Sessão ausente ou encerrada. Entre de novo."));
    };
    if scope == Scope::Local && !is_local(peer) {
        return Err(deny(StatusCode::UNAUTHORIZED, "Esta sessão só vale no computador do servidor."));
    }
    if scope == Scope::Remote && !app.auth.lock().unwrap().status().remote_enabled {
        return Err(deny(StatusCode::UNAUTHORIZED, "Acesso remoto desligado."));
    }
    Ok((name, scope))
}

async fn health(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "name": "sona", "version": env!("CARGO_PKG_VERSION"), "engine": app.engine_status() }))
}

/// Sem token: diz à tela de entrada o que pedir.
async fn access(State(app): State<Shared>, ConnectInfo(peer): ConnectInfo<SocketAddr>) -> Json<Value> {
    let st = app.auth.lock().unwrap().status();
    let local = is_local(&peer);
    Json(json!({
        "local": local,
        // A primeira configuração só aparece neste computador.
        "setup_needed": local && !st.setup_done,
        "app_password": st.app_password,
        "remote_enabled": st.remote_enabled,
        "lan_urls": if local || st.remote_enabled { app.lan_urls.clone() } else { Vec::new() },
    }))
}

#[derive(Deserialize)]
struct LoginReq {
    #[serde(default)]
    password: String,
    #[serde(default)]
    name: String,
}

async fn login(State(app): State<Shared>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Json(req): Json<LoginReq>) -> Response {
    let ip = peer.ip();
    let scope = if is_local(&peer) { Scope::Local } else { Scope::Remote };
    if scope == Scope::Remote && !app.auth.lock().unwrap().status().remote_enabled {
        return deny(StatusCode::FORBIDDEN, "Acesso remoto desligado. Ative no computador do servidor, em Configurações.");
    }
    {
        let mut f = app.login_failures.lock().unwrap();
        if let Some((n, at)) = f.get(&ip).copied() {
            if n >= MAX_FAILURES && at.elapsed() < LOCKOUT {
                let wait = (LOCKOUT - at.elapsed()).as_secs() + 1;
                return deny(StatusCode::TOO_MANY_REQUESTS, &format!("Muitas tentativas. Tente de novo em {wait} s."));
            }
            if at.elapsed() >= LOCKOUT {
                f.remove(&ip);
            }
        }
    }
    // Argon2 custa dezenas de ms: roda fora do executor assíncrono.
    let app2 = app.clone();
    let password = req.password;
    let ok = tokio::task::spawn_blocking(move || app2.auth.lock().unwrap().check(scope, &password)).await.unwrap_or(false);
    if !ok {
        let mut f = app.login_failures.lock().unwrap();
        let e = f.entry(ip).or_insert((0, Instant::now()));
        e.0 += 1;
        e.1 = Instant::now();
        tracing::warn!("senha errada de {ip} ({}ª)", e.0);
        return deny(StatusCode::UNAUTHORIZED, "Senha incorreta.");
    }
    app.login_failures.lock().unwrap().remove(&ip);
    let name = match (scope, req.name.trim()) {
        (Scope::Local, _) => "Este computador".to_string(),
        (_, "") => ip.to_string(),
        (_, n) => n.chars().take(40).collect(),
    };
    let token = app.auth.lock().unwrap().issue(&name, scope);
    if scope == Scope::Remote {
        tracing::info!("entrada remota: {name} de {ip}");
    }
    Json(json!({ "token": token, "name": name })).into_response()
}

/// Muda senhas e conclui a primeira configuração. Campo ausente = sem mudança;
/// `null` = apagar a senha.
#[allow(clippy::result_large_err)]
async fn update_access(
    State(app): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let token = bearer(&headers).map(str::to_string);
    let (name, _) = match authed(&app, token.as_deref(), &peer) {
        Ok(x) => x,
        Err(r) => return r,
    };
    let field = |k: &str| -> Result<Option<Option<String>>, Response> {
        match body.get(k) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(Value::String(s)) => Ok(Some(Some(s.clone()))),
            Some(_) => Err(deny(StatusCode::BAD_REQUEST, "Senha precisa ser texto ou null.")),
        }
    };
    let (app_pw, remote_pw) = match (field("app_password"), field("remote_password")) {
        (Ok(a), Ok(r)) => (a, r),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    let finish = body.get("finish_setup").and_then(Value::as_bool).unwrap_or(false);
    let app2 = app.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut a = app2.auth.lock().unwrap();
        // Valida as duas antes de gravar qualquer uma.
        for p in [&app_pw, &remote_pw].into_iter().flatten().flatten() {
            if p.chars().count() < crate::auth::MIN_PASSWORD {
                return Err(format!("A senha precisa de pelo menos {} caracteres.", crate::auth::MIN_PASSWORD));
            }
        }
        if let Some(p) = app_pw {
            a.set_password(Scope::Local, p.as_deref(), token.as_deref())?;
        }
        if let Some(p) = remote_pw {
            a.set_password(Scope::Remote, p.as_deref(), token.as_deref())?;
        }
        if finish {
            a.finish_setup();
        }
        Ok(a.status())
    })
    .await;
    match result {
        Ok(Ok(st)) => {
            tracing::info!("{name} mudou o acesso: senha do app {}, acesso remoto {}", on(st.app_password), on(st.remote_enabled));
            Json(json!({ "app_password": st.app_password, "remote_enabled": st.remote_enabled, "setup_done": st.setup_done }))
                .into_response()
        }
        Ok(Err(e)) => deny(StatusCode::BAD_REQUEST, &e),
        Err(_) => deny(StatusCode::INTERNAL_SERVER_ERROR, "Falha ao salvar."),
    }
}

fn on(b: bool) -> &'static str {
    if b {
        "ligada"
    } else {
        "desligada"
    }
}

async fn state(State(app): State<Shared>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    match authed(&app, bearer(&headers), &peer) {
        Ok(_) => Json(app.store.lock().unwrap().snapshot()).into_response(),
        Err(r) => r,
    }
}

async fn devices(State(app): State<Shared>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    match authed(&app, bearer(&headers), &peer) {
        Ok(_) => {
            #[cfg(feature = "hw")]
            let list = tokio::task::spawn_blocking(crate::audio::hw::list_devices).await.unwrap_or_default();
            #[cfg(not(feature = "hw"))]
            let list: Vec<Value> = Vec::new();
            Json(json!({ "devices": list })).into_response()
        }
        Err(r) => r,
    }
}

async fn audio_get(State(app): State<Shared>, ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap) -> Response {
    match authed(&app, bearer(&headers), &peer) {
        Ok(_) => {
            let cfg = app.audio.lock().unwrap().clone();
            let status = app.host.lock().unwrap().as_ref().map(|h| h.status()).unwrap_or_default();
            Json(json!({ "config": cfg, "status": status })).into_response()
        }
        Err(r) => r,
    }
}

async fn audio_set(
    State(app): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(cfg): Json<AudioConfig>,
) -> Response {
    let name = match authed(&app, bearer(&headers), &peer) {
        Ok((n, _)) => n,
        Err(r) => return r,
    };
    let app2 = app.clone();
    match tokio::task::spawn_blocking(move || app2.apply_audio(cfg)).await {
        Ok(Ok(())) => {
            tracing::info!("{name} trocou a configuração de áudio.");
            let status = app.host.lock().unwrap().as_ref().map(|h| h.status()).unwrap_or_default();
            Json(json!({ "ok": true, "status": status })).into_response()
        }
        Ok(Err(e)) => deny(StatusCode::BAD_REQUEST, &e),
        Err(_) => deny(StatusCode::INTERNAL_SERVER_ERROR, "Falha ao trocar a configuração de áudio."),
    }
}

#[derive(Deserialize)]
struct WsQuery {
    token: Option<String>,
}

async fn bridge_ws(
    State(app): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(q): Query<WsQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    match authed(&app, q.token.as_deref(), &peer) {
        Ok(_) => {
            let token = q.token.unwrap_or_default();
            upgrade.on_upgrade(move |socket| crate::net::bridge_session(app, socket, peer, token))
        }
        Err(r) => r,
    }
}

async fn ws(
    State(app): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(q): Query<WsQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    match authed(&app, q.token.as_deref(), &peer) {
        Ok((name, _)) => {
            let token = q.token.unwrap_or_default();
            upgrade.on_upgrade(move |socket| session(app, socket, name, token))
        }
        Err(r) => r,
    }
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ClientMsg {
    #[serde(rename = "patch")]
    Patch { path: String, value: Value },
    #[serde(rename = "scene.save")]
    SceneSave { name: String },
    #[serde(rename = "scene.recall")]
    SceneRecall { name: String },
    #[serde(rename = "scene.delete")]
    SceneDelete { name: String },
    #[serde(rename = "ping")]
    Ping,
}

async fn session(app: Shared, socket: WebSocket, name: String, token: String) {
    let id = app.sessions.fetch_add(1, Relaxed) + 1;
    app.clients.fetch_add(1, Relaxed);
    tracing::info!("cliente {id} conectou ({name})");
    let (mut sink, mut stream) = socket.split();
    let (direct_tx, mut direct_rx) = mpsc::channel::<String>(32);
    let mut bcast = app.tx.subscribe();

    let hello = json!({
        "type": "hello",
        "session": id,
        "name": name,
        "state": app.store.lock().unwrap().snapshot(),
        "engine": app.engine_status(),
    });
    if sink.send(Message::Text(hello.to_string().into())).await.is_err() {
        app.clients.fetch_sub(1, Relaxed);
        return;
    }

    let revoked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (app_w, revoked_w) = (app.clone(), revoked.clone());
    let writer = tokio::spawn(async move {
        let mut check = tokio::time::interval(Duration::from_secs(2));
        loop {
            let text: String = tokio::select! {
                _ = check.tick() => {
                    // Senha trocada ou acesso remoto desligado: encerra a sessão aberta.
                    if app_w.auth.lock().unwrap().verify(&token).is_none() {
                        revoked_w.store(true, Relaxed);
                        let msg = json!({"type": "revoked", "message": "Sua sessão foi encerrada porque a senha mudou. Entre de novo."});
                        let _ = sink.send(Message::Text(msg.to_string().into())).await;
                        let _ = sink.send(Message::Close(None)).await;
                        break;
                    }
                    continue;
                },
                m = bcast.recv() => match m {
                    Ok(t) => t.to_string(),
                    // Cliente lento: pula medidores perdidos, segue.
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                },
                m = direct_rx.recv() => match m { Some(t) => t, None => break },
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = stream.next().await {
        if revoked.load(Relaxed) {
            break;
        }
        let Message::Text(text) = msg else { continue };
        let reply = |m: Value| {
            let _ = direct_tx.try_send(m.to_string());
        };
        let parsed: ClientMsg = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                reply(json!({"type": "error", "message": format!("Mensagem inválida: {e}")}));
                continue;
            }
        };
        let result = {
            let mut store = app.store.lock().unwrap();
            match parsed {
                ClientMsg::Patch { path, value } => store.patch(&path, value),
                ClientMsg::SceneSave { name } => {
                    store.save_scene(&name);
                    Ok(())
                }
                ClientMsg::SceneRecall { name } => store.recall_scene(&name),
                ClientMsg::SceneDelete { name } => {
                    store.delete_scene(&name);
                    Ok(())
                }
                ClientMsg::Ping => {
                    reply(json!({"type": "pong"}));
                    continue;
                }
            }
        };
        match result {
            Ok(()) => app.broadcast_state(id),
            Err(e) => {
                let issues = match &e {
                    crate::state::PatchError::Rejected(v) => serde_json::to_value(v).unwrap_or_default(),
                    _ => Value::Null,
                };
                reply(json!({"type": "error", "message": e.to_string(), "issues": issues}));
                // Reenvia o estado para o cliente desfazer o que mostrou otimista.
                let snap = app.store.lock().unwrap().snapshot();
                reply(json!({"type": "state", "by": 0, "state": snap}));
            }
        }
    }
    writer.abort();
    app.clients.fetch_sub(1, Relaxed);
    tracing::info!("cliente {id} saiu");
}

/// Medidores a 20 Hz e estado do motor a 1 Hz para todos os clientes.
pub fn spawn_meter_task(app: Shared) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        let mut n = 0u64;
        let r = |v: f32| ((v as f64) * 10_000.0).round() / 10_000.0;
        loop {
            tick.tick().await;
            n += 1;
            // Pega a cada vez: o motor pode ter sido reconstruído.
            let meters = app.store.lock().unwrap().meters();
            if app.tx.receiver_count() == 0 {
                // Ninguém olhando: zera os picos e segue (o áudio não para).
                for m in &meters.channel_peak {
                    sona_core::Meters::take(m);
                }
                continue;
            }
            let (nc, nb) = {
                let s = app.store.lock().unwrap();
                (s.mixer.channels.len(), s.mixer.buses.len())
            };
            use sona_core::Meters as M;
            let ch: Vec<f64> = meters.channel_peak.iter().take(nc).map(|m| r(M::take(m))).collect();
            let gr: Vec<f64> = meters.channel_gr_db.iter().take(nc).map(|m| ((M::take(m) as f64) * 10.0).round() / 10.0).collect();
            let gate: Vec<f64> = meters.channel_gate.iter().take(nc).map(|m| r(M::take(m))).collect();
            let bus: Vec<[f64; 2]> = meters.bus_peak.iter().take(nb).map(|[a, b]| [r(M::take(a)), r(M::take(b))]).collect();
            let lim: Vec<f64> = meters.bus_limit_db.iter().take(nb).map(|m| ((M::take(m) as f64) * 10.0).round() / 10.0).collect();
            let _ = app.tx.send(json!({"type": "meters", "ch": ch, "gr": gr, "gate": gate, "bus": bus, "lim": lim}).to_string().into());
            if n.is_multiple_of(20) {
                let _ = app.tx.send(json!({"type": "engine", "engine": app.engine_status()}).to_string().into());
            }
        }
    });
}

/// Reconstrói o motor quando o gerenciador pede (principal apareceu depois).
pub fn spawn_rebuild_watch(app: Shared) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            let want = app.host.lock().unwrap().as_ref().is_some_and(|h| h.rebuild.swap(false, Relaxed));
            if want {
                let cfg = app.audio.lock().unwrap().clone();
                let app2 = app.clone();
                let _ = tokio::task::spawn_blocking(move || app2.apply_audio(cfg)).await;
            }
        }
    });
}

/// Salva em disco 0,5 s depois da última mudança e libera planos devolvidos.
pub fn spawn_save_task(app: Shared) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        let mut last_rev = 0;
        loop {
            tick.tick().await;
            let app2 = app.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let mut s = app2.store.lock().unwrap();
                s.collect();
                // Espera um tique sem mudança (debounce) antes de gravar.
                if s.is_dirty() && s.rev == last_rev {
                    s.save();
                }
                s.rev
            })
            .await
            .map(|rev| last_rev = rev);
        }
    });
}
