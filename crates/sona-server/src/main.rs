//! Sona: servidor de áudio.

#![allow(clippy::needless_range_loop)]

mod api;
mod audio;
mod auth;
mod bench;
mod state;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "sona-server", version, about = "Sona — mesa de som em software (servidor de áudio)")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    #[command(flatten)]
    serve: ServeArgs,
}

#[derive(clap::Args, Clone)]
struct ServeArgs {
    /// Pasta de dados (estado, cenas, tokens).
    #[arg(long, default_value = "data")]
    data: PathBuf,
    /// Endereço de escuta. Padrão: todas as interfaces, para o acesso remoto poder
    /// ser ligado pelo app sem reiniciar. Pedidos de outros computadores são
    /// recusados enquanto não houver senha de acesso remoto.
    #[arg(long)]
    bind: Option<String>,
    #[arg(long, default_value_t = 7340)]
    port: u16,
    /// Abre o console no navegador ao ligar (uso como app).
    #[arg(long)]
    open: bool,
    /// "sim" para o dispositivo simulado, ou o nome do dispositivo de áudio.
    #[arg(long, default_value = "sim")]
    device: String,
    #[arg(long, default_value_t = 48_000)]
    rate: u32,
    #[arg(long, default_value_t = 128)]
    block: usize,
    /// Pasta com o build da interface (apps/mixer-web/dist).
    #[arg(long, default_value = "apps/mixer-web/dist")]
    web: PathBuf,
}

#[derive(Subcommand)]
enum Cmd {
    /// Liga o servidor (padrão).
    Serve,
    /// Lista dispositivos de áudio, canais e taxas (FR-01).
    Devices,
    /// Mede o custo do motor por bloco (64/128/256).
    Bench {
        #[arg(long, default_value_t = 10.0)]
        seconds: f32,
    },
    /// Recuperação: define ou apaga senhas neste computador (vale sem reiniciar).
    /// O uso normal é pelo app, em Configurações.
    Password {
        /// Senha de acesso remoto (outros computadores da rede).
        #[arg(long, conflicts_with = "app")]
        remote: bool,
        /// Senha do app (este computador).
        #[arg(long)]
        app: bool,
        #[arg(long)]
        value: Option<String>,
        /// Apaga a senha (remoto: desliga o acesso remoto).
        #[arg(long)]
        clear: bool,
    },
}

enum Host {
    Sim(#[allow(dead_code)] audio::sim::SimHandle),
    #[cfg(feature = "hw")]
    Hw(#[allow(dead_code)] audio::hw::HwHandle),
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let cli = Cli::parse();
    match cli.cmd.unwrap_or(Cmd::Serve) {
        Cmd::Serve => serve(cli.serve).await,
        Cmd::Devices => devices(),
        Cmd::Bench { seconds } => bench::run(seconds, cli.serve.rate),
        Cmd::Password { remote, app, value, clear } => {
            if !remote && !app {
                eprintln!("Escolha --remote (acesso pela rede) ou --app (este computador).");
                std::process::exit(2);
            }
            let scope = if remote { auth::Scope::Remote } else { auth::Scope::Local };
            let what = if remote { "de acesso remoto" } else { "do app" };
            let mut a = auth::Auth::load(&cli.serve.data.join("auth.json"));
            let pw = if clear {
                None
            } else {
                Some(value.unwrap_or_else(|| {
                    eprint!("Nova senha {what}: ");
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line).ok();
                    line.trim_end_matches(['\r', '\n']).to_string()
                }))
            };
            match a.set_password(scope, pw.as_deref(), None) {
                Ok(()) if clear => println!("Senha {what} apagada."),
                Ok(()) => println!("Senha {what} definida. Sessões abertas com a antiga foram encerradas."),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(2);
                }
            }
        }
    }
}

/// Endereços IPv4 desta máquina nas redes locais (sem loopback).
fn lan_urls(port: u16) -> Vec<String> {
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

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        tracing::warn!("Não consegui abrir o navegador ({e}). Abra {url}.");
    }
}

fn devices() {
    #[cfg(feature = "hw")]
    {
        let list = audio::hw::list_devices();
        if list.is_empty() {
            println!("Nenhum dispositivo de áudio encontrado.");
        }
        println!("{}", serde_json::to_string_pretty(&list).unwrap());
    }
    #[cfg(not(feature = "hw"))]
    println!("Compilado sem a feature \"hw\": só o dispositivo simulado existe.");
}

async fn serve(args: ServeArgs) {
    let auth = auth::Auth::load(&args.data.join("auth.json"));
    let bind_text = args.bind.clone().unwrap_or_else(|| format!("0.0.0.0:{}", args.port));
    let bind: std::net::SocketAddr = match bind_text.parse() {
        Ok(b) => b,
        Err(_) => {
            tracing::error!("Endereço inválido: {bind_text} (use ip:porta, ex. 0.0.0.0:7340)");
            std::process::exit(2);
        }
    };
    let stats = Arc::new(audio::EngineStats::default());
    let io = if args.device == "sim" {
        audio::sim::demo_layout()
    } else {
        #[cfg(feature = "hw")]
        match audio::hw::probe_layout(&args.device, args.rate) {
            Ok((io, _, _)) => io,
            Err(e) => {
                tracing::error!("{e}");
                std::process::exit(2);
            }
        }
        #[cfg(not(feature = "hw"))]
        {
            tracing::error!("Compilado sem a feature \"hw\".");
            std::process::exit(2);
        }
    };
    let layout = io.to_layout(args.rate, args.block);
    let (store, engine) = state::Store::load(args.data.join("state.json"), io.clone(), layout);
    for w in &store.validation.warnings {
        tracing::warn!("{}: {}", w.code, w.message);
    }

    let _host = if args.device == "sim" {
        Host::Sim(audio::sim::start(engine, &io, args.rate, args.block, stats.clone()))
    } else {
        #[cfg(feature = "hw")]
        match audio::hw::start(&args.device, engine, args.rate, args.block, stats.clone()) {
            Ok(h) => Host::Hw(h),
            Err(e) => {
                tracing::error!("{e}");
                std::process::exit(2);
            }
        }
        #[cfg(not(feature = "hw"))]
        unreachable!()
    };

    let device_name = if args.device == "sim" { "Simulado".to_string() } else { args.device.clone() };
    let lan = if bind.ip().is_loopback() { Vec::new() } else { lan_urls(bind.port()) };
    let app = api::App::new(store, auth, stats, device_name, lan.clone());
    api::spawn_meter_task(app.clone());
    api::spawn_save_task(app.clone());

    let listener = match tokio::net::TcpListener::bind(bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Não consegui abrir {bind}: {e}");
            std::process::exit(2);
        }
    };
    let local = format!("http://127.0.0.1:{}", bind.port());
    tracing::info!("Sona no ar ({} Hz, bloco {}). Neste computador: {local}", args.rate, args.block);
    let remote_on = app.auth.lock().unwrap().status().remote_enabled;
    for u in &lan {
        if remote_on {
            tracing::info!("Na rede (com a senha de acesso remoto): {u}");
        } else {
            tracing::info!("Na rede: {u} (acesso remoto desligado; ative no app, em Configurações)");
        }
    }
    if args.open {
        open_browser(&local);
    }
    let app_for_exit = app.clone();
    axum::serve(listener, api::router(app, Some(args.web)).into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .ok();
    app_for_exit.store.lock().unwrap().save();
    tracing::info!("Sona parou. Estado salvo.");
}
