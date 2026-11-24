//! Plano de controle: HTTP + WebSocket. O áudio não depende de nenhum cliente
//! conectado: fechar todas as abas não para o motor (FR-06).

use crate::audio::EngineStats;
use crate::auth::{Auth, Role};
use crate::state::Store;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};

pub struct App {
    pub store: Mutex<Store>,
    pub auth: Mutex<Auth>,
    pub stats: Arc<EngineStats>,
    pub device: String,
    pub tx: broadcast::Sender<Arc<str>>,
    pub sessions: AtomicU64,
    pub clients: AtomicU64,
}

pub type Shared = Arc<App>;

impl App {
    pub fn new(store: Store, auth: Auth, stats: Arc<EngineStats>, device: String) -> Shared {
        let (tx, _) = broadcast::channel(256);
        Arc::new(App {
            store: Mutex::new(store),
            auth: Mutex::new(auth),
            stats,
            device,
            tx,
            sessions: AtomicU64::new(0),
            clients: AtomicU64::new(0),
        })
    }

    fn broadcast_state(&self, by: u64) {
        let snap = self.store.lock().unwrap().snapshot();
        let msg = json!({ "type": "state", "by": by, "state": snap }).to_string();
        let _ = self.tx.send(msg.into());
    }

    pub fn engine_status(&self) -> Value {
        let mut v = self.stats.snapshot(&self.device);
        v["clients"] = self.clients.load(Relaxed).into();
        v
    }
}

pub fn router(app: Shared, web_dir: Option<std::path::PathBuf>) -> Router {
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/state", get(state))
        .route("/api/tokens", post(issue_token))
        .route("/api/devices", get(devices))
        .route("/api/ws", get(ws));
    let api = match web_dir {
        Some(dir) if dir.join("index.html").exists() => {
            let index = dir.join("index.html");
            api.fallback_service(tower_http::services::ServeDir::new(dir).fallback(tower_http::services::ServeFile::new(index)))
        }
        _ => api,
    };
    api.with_state(app)
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get("authorization")?.to_str().ok()?.strip_prefix("Bearer ")
}

#[allow(clippy::result_large_err)]
fn authed(app: &App, token: Option<&str>) -> Result<(String, Role), Response> {
    token
        .and_then(|t| app.auth.lock().unwrap().verify(t))
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(json!({"error": "Token ausente ou inválido."}))).into_response())
}

async fn health(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "name": "sona", "version": env!("CARGO_PKG_VERSION"), "engine": app.engine_status() }))
}

async fn state(State(app): State<Shared>, headers: HeaderMap) -> Response {
    match authed(&app, bearer(&headers)) {
        Ok(_) => Json(app.store.lock().unwrap().snapshot()).into_response(),
        Err(r) => r,
    }
}

#[derive(Deserialize)]
struct TokenReq {
    name: String,
    role: Role,
}

async fn issue_token(State(app): State<Shared>, headers: HeaderMap, Json(req): Json<TokenReq>) -> Response {
    match authed(&app, bearer(&headers)) {
        Ok((_, Role::Admin)) => {
            let token = app.auth.lock().unwrap().issue(&req.name, req.role);
            Json(json!({ "token": token, "role": req.role })).into_response()
        }
        Ok(_) => (StatusCode::FORBIDDEN, Json(json!({"error": "Só o admin cria acessos."}))).into_response(),
        Err(r) => r,
    }
}

async fn devices(State(app): State<Shared>, headers: HeaderMap) -> Response {
    match authed(&app, bearer(&headers)) {
        Ok((_, Role::Admin)) => {
            #[cfg(feature = "hw")]
            let list = tokio::task::spawn_blocking(crate::audio::hw::list_devices).await.unwrap_or_default();
            #[cfg(not(feature = "hw"))]
            let list: Vec<Value> = Vec::new();
            Json(json!({ "devices": list })).into_response()
        }
        Ok(_) => (StatusCode::FORBIDDEN, Json(json!({"error": "Só o admin lista dispositivos."}))).into_response(),
        Err(r) => r,
    }
}

#[derive(Deserialize)]
struct WsQuery {
    token: Option<String>,
}

async fn ws(State(app): State<Shared>, Query(q): Query<WsQuery>, upgrade: WebSocketUpgrade) -> Response {
    match authed(&app, q.token.as_deref()) {
        Ok((name, role)) => upgrade.on_upgrade(move |socket| session(app, socket, name, role)),
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

async fn session(app: Shared, socket: WebSocket, name: String, role: Role) {
    let id = app.sessions.fetch_add(1, Relaxed) + 1;
    app.clients.fetch_add(1, Relaxed);
    tracing::info!("cliente {id} conectou ({name}, {role:?})");
    let (mut sink, mut stream) = socket.split();
    let (direct_tx, mut direct_rx) = mpsc::channel::<String>(32);
    let mut bcast = app.tx.subscribe();

    let hello = json!({
        "type": "hello",
        "session": id,
        "name": name,
        "role": role,
        "state": app.store.lock().unwrap().snapshot(),
        "engine": app.engine_status(),
    });
    if sink.send(Message::Text(hello.to_string().into())).await.is_err() {
        app.clients.fetch_sub(1, Relaxed);
        return;
    }

    let writer = tokio::spawn(async move {
        loop {
            let text: String = tokio::select! {
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
                ClientMsg::Patch { path, value } => store.patch(role, &path, value),
                ClientMsg::SceneSave { name } if role != Role::Monitor => {
                    store.save_scene(&name);
                    Ok(())
                }
                ClientMsg::SceneRecall { name } if role != Role::Monitor => store.recall_scene(&name),
                ClientMsg::SceneDelete { name } if role == Role::Admin => {
                    store.delete_scene(&name);
                    Ok(())
                }
                ClientMsg::Ping => {
                    reply(json!({"type": "pong"}));
                    continue;
                }
                _ => Err(crate::state::PatchError::Forbidden("Seu perfil não pode fazer isso.".into())),
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
        let meters = app.store.lock().unwrap().meters().clone();
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        let mut n = 0u64;
        let r = |v: f32| ((v as f64) * 10_000.0).round() / 10_000.0;
        loop {
            tick.tick().await;
            n += 1;
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
