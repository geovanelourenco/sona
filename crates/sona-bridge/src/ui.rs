//! API local do bridge (só deste computador) para a tela do app.

use crate::client::Bridge;
use crate::config::{BridgeConfig, Endpoint, Monitor};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

type S = State<Arc<Bridge>>;

pub fn router(b: Arc<Bridge>, web: Option<std::path::PathBuf>) -> Router {
    let r = Router::new()
        .route("/api/status", get(status))
        .route("/api/config", get(config_get).post(config_set))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/devices", get(devices))
        .route("/api/discover", get(discover));
    let r = match web {
        Some(dir) if dir.join("bridge.html").exists() => {
            // A pasta é a mesma do console: a raiz tem de abrir bridge.html, não index.html.
            let index = dir.join("bridge.html");
            r.route_service("/", tower_http::services::ServeFile::new(index.clone())).fallback_service(
                tower_http::services::ServeDir::new(dir)
                    .append_index_html_on_directories(false)
                    .fallback(tower_http::services::ServeFile::new(index)),
            )
        }
        _ => r,
    };
    r.with_state(b)
}

async fn status(State(b): S) -> Response {
    let mut s = b.status.lock().unwrap().clone();
    // Pico desde a última leitura da tela.
    let st = &mut b.status.lock().unwrap();
    st.send_peak = 0.0;
    st.recv_peak = 0.0;
    st.monitor_peak = 0.0;
    s.server_view["type"] = serde_json::Value::Null;
    let mut v = serde_json::to_value(&s).unwrap_or_default();
    v["obs_plugin"] = obs_plugin_installed().into();
    Json(v).into_response()
}

/// O plugin "Sona" está na pasta de plugins do OBS? `None` fora do Windows.
fn obs_plugin_installed() -> Option<bool> {
    if !cfg!(windows) {
        return None;
    }
    let base = std::env::var_os("PROGRAMDATA")?;
    Some(std::path::Path::new(&base).join("obs-studio/plugins/sona-obs/bin/64bit/sona-obs.dll").exists())
}

async fn config_get(State(b): S) -> Response {
    let c = b.config.lock().unwrap().clone();
    Json(json!({ "server": c.server, "server_name": c.server_name, "name": c.name, "logged_in": !c.token.is_empty(), "send": c.send, "receive": c.receive, "monitor": c.monitor }))
        .into_response()
}

/// `null` explícito vira `Some(None)` (desligar); campo ausente, `None` (não mexe).
fn double<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
struct ConfigReq {
    name: Option<String>,
    #[serde(default, deserialize_with = "double")]
    send: Option<Option<Endpoint>>,
    #[serde(default, deserialize_with = "double")]
    receive: Option<Option<Endpoint>>,
    #[serde(default, deserialize_with = "double")]
    monitor: Option<Option<Monitor>>,
}

async fn config_set(State(b): S, Json(req): Json<ConfigReq>) -> Response {
    // Só o volume da escuta mudou: aplica na hora, sem reconectar.
    if req.name.is_none() && req.send.is_none() && req.receive.is_none() {
        if let Some(Some(m)) = &req.monitor {
            let mut c = b.config.lock().unwrap();
            let same = c.monitor.as_ref().is_some_and(|o| o.device == m.device && o.source == m.source);
            if same {
                c.monitor = Some(m.clone());
                drop(c);
                b.monitor_gain.store(crate::audio::db_to_gain(m.volume_db).to_bits(), std::sync::atomic::Ordering::Relaxed);
                b.save();
                return Json(json!({"ok": true})).into_response();
            }
        }
    }
    {
        let mut c: std::sync::MutexGuard<BridgeConfig> = b.config.lock().unwrap();
        if let Some(n) = req.name.filter(|n| !n.trim().is_empty()) {
            c.name = n.trim().chars().take(32).collect();
        }
        if let Some(s) = req.send {
            c.send = s;
        }
        if let Some(r) = req.receive {
            c.receive = r;
        }
        if let Some(m) = req.monitor {
            c.monitor = m;
        }
    }
    b.save();
    b.reload();
    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
struct LoginReq {
    server: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    server_name: String,
}

/// Servidores Sona achados na rede (mDNS, ~2 s).
async fn discover() -> Response {
    let list = tokio::task::spawn_blocking(|| sona_net::discovery::discover(std::time::Duration::from_secs(2))).await.unwrap_or_default();
    let v: Vec<_> = list.into_iter().map(|f| json!({"name": f.name, "address": f.address, "ips": f.ips})).collect();
    Json(json!({ "servers": v })).into_response()
}

async fn login(State(b): S, Json(req): Json<LoginReq>) -> Response {
    let b2 = b.clone();
    match tokio::task::spawn_blocking(move || b2.login(&req.server, &req.password, &req.name, &req.server_name)).await {
        Ok(Ok(())) => Json(json!({"ok": true})).into_response(),
        Ok(Err(e)) => (StatusCode::UNAUTHORIZED, Json(json!({"error": e}))).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "Falha interna."}))).into_response(),
    }
}

async fn logout(State(b): S) -> Response {
    b.config.lock().unwrap().token.clear();
    b.save();
    b.reload();
    Json(json!({"ok": true})).into_response()
}

async fn devices() -> Response {
    #[cfg(feature = "hw")]
    let list = tokio::task::spawn_blocking(|| crate::audio::hw::list(48_000)).await.unwrap_or_default();
    #[cfg(not(feature = "hw"))]
    let list: Vec<serde_json::Value> = Vec::new();
    Json(json!({ "devices": list })).into_response()
}
