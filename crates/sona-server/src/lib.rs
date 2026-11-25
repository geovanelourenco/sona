//! Sona: servidor de áudio como biblioteca. O binário `sona-server` e o app
//! desktop usam [`start`].

// Erros que devolvem o motor ou a ponte (para reabrir o aparelho) são grandes
// de propósito: o caminho de erro é raro e não está na thread de áudio.
#![allow(clippy::needless_range_loop, clippy::result_large_err)]

pub mod api;
pub mod audio;
pub mod auth;
pub mod bench;
pub mod net;
pub mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Options {
    /// Pasta de dados (estado, cenas, acessos, áudio).
    pub data: PathBuf,
    /// Endereço de escuta. `None`: 0.0.0.0 na porta dada (ver ADR 0005).
    pub bind: Option<String>,
    pub port: u16,
    /// Pasta com o build da interface.
    pub web: Option<PathBuf>,
    /// Troca o dispositivo principal salvo (ex.: "sim", "internal", "Ui24R").
    pub primary: Option<String>,
    /// Principal da primeira instalação (sem `audio.json`). Padrão: "sim".
    pub default_primary: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            data: PathBuf::from("data"),
            bind: None,
            port: 7340,
            web: Some(PathBuf::from("apps/mixer-web/dist")),
            primary: None,
            default_primary: None,
        }
    }
}

/// Servidor ligado: endereço local e a aplicação.
pub struct Running {
    pub local_url: String,
    pub lan_urls: Vec<String>,
    pub app: api::Shared,
    pub listener: tokio::net::TcpListener,
    pub web: Option<PathBuf>,
}

/// Endereços IPv4 desta máquina nas redes locais (sem loopback).
pub fn lan_urls(port: u16) -> Vec<String> {
    let mut v: Vec<String> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(ip) if !ip.is_link_local() => Some(format!("http://{ip}:{port}")),
            _ => None,
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Liga o áudio e abre a porta de controle. Chame [`Running::serve`] depois.
pub async fn start(opts: Options) -> Result<Running, String> {
    let bind_text = opts.bind.clone().unwrap_or_else(|| format!("0.0.0.0:{}", opts.port));
    let bind: SocketAddr = bind_text.parse().map_err(|_| format!("Endereço inválido: {bind_text} (use ip:porta, ex. 0.0.0.0:7340)"))?;
    let auth = auth::Auth::load(&opts.data.join("auth.json"));

    let audio_path = opts.data.join("audio.json");
    let mut audio_cfg = audio::config::AudioConfig::load(&audio_path).unwrap_or_else(|| audio::config::AudioConfig {
        primary: opts.default_primary.clone().unwrap_or_else(|| audio::config::PRIMARY_SIM.into()),
        ..Default::default()
    });
    if let Some(p) = &opts.primary {
        audio_cfg.primary = p.clone();
    }
    audio_cfg.validate()?;
    audio_cfg.save(&audio_path).map_err(|e| format!("Não consegui salvar {}: {e}", audio_path.display()))?;

    let stats = Arc::new(audio::EngineStats::default());
    let (io, pin, pout, perr) = audio::host::plan_io(&audio_cfg);
    let layout = io.to_layout(audio_cfg.sample_rate, audio_cfg.block);
    let (store, engine) = state::Store::load(opts.data.join("state.json"), io, layout);
    for w in &store.validation.warnings {
        tracing::warn!("{}: {}", w.code, w.message);
    }
    let host = audio::host::AudioHost::start(&audio_cfg, engine, (pin, pout), perr, stats.clone());

    let lan = if bind.ip().is_loopback() { Vec::new() } else { lan_urls(bind.port()) };
    let app = api::App::new(store, auth, stats, host, audio_cfg, audio_path, lan.clone());
    api::spawn_meter_task(app.clone());
    api::spawn_save_task(app.clone());
    api::spawn_rebuild_watch(app.clone());
    net::spawn_udp(app.clone(), bind.port()).await?;

    let listener = tokio::net::TcpListener::bind(bind).await.map_err(|e| format!("Não consegui abrir {bind}: {e}"))?;
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(bind.port());
    Ok(Running { local_url: format!("http://127.0.0.1:{port}"), lan_urls: lan, app, listener, web: opts.web })
}

impl Running {
    /// Atende até `shutdown` terminar; salva o estado e para o áudio no fim.
    pub async fn serve(self, shutdown: impl std::future::Future<Output = ()> + Send + 'static) {
        let app = self.app.clone();
        let router = api::router(self.app, self.web);
        let _ =
            axum::serve(self.listener, router.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(shutdown).await;
        app.store.lock().unwrap().save();
        if let Some(h) = app.host.lock().unwrap().take() {
            h.shutdown();
        }
        tracing::info!("Sona parou. Estado salvo.");
    }
}
