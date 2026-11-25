//! API local do bridge (só deste computador) para a tela do app.

use crate::client::Bridge;
use crate::config::{BridgeConfig, Endpoint};
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
        .route("/api/devices", get(devices));
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
    s.server_view["type"] = serde_json::Value::Null;
    Json(s).into_response()
}

async fn config_get(State(b): S) -> Response {
    let c = b.config.lock().unwrap().clone();
    Json(json!({ "server": c.server, "name": c.name, "logged_in": !c.token.is_empty(), "send": c.send, "receive": c.receive }))
        .into_response()
}

#[derive(Deserialize)]
struct ConfigReq {
    name: Option<String>,
    send: Option<Option<Endpoint>>,
    receive: Option<Option<Endpoint>>,
}

async fn config_set(State(b): S, Json(req): Json<ConfigReq>) -> Response {
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
}

async fn login(State(b): S, Json(req): Json<LoginReq>) -> Response {
    let b2 = b.clone();
    match tokio::task::spawn_blocking(move || b2.login(&req.server, &req.password, &req.name)).await {
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
