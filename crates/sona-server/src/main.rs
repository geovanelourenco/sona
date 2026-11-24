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
    /// Endereço do controle. Padrão só local; use 0.0.0.0:7340 para a rede da igreja.
    #[arg(long, default_value = "127.0.0.1:7340")]
    bind: String,
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
    /// Cria um acesso (token) sem precisar do servidor ligado.
    Token {
        #[arg(long)]
        name: String,
        #[arg(long, value_parser = ["admin", "operator", "monitor"])]
        role: String,
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
        Cmd::Token { name, role } => {
            let role = match role.as_str() {
                "admin" => auth::Role::Admin,
                "operator" => auth::Role::Operator,
                _ => auth::Role::Monitor,
            };
            let (mut a, first) = auth::Auth::load(&cli.serve.data.join("auth.json"));
            if let Some(t) = first {
                println!("admin (criado agora): {t}");
            }
            println!("{name} ({role:?}): {}", a.issue(&name, role));
        }
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

    let (auth, first) = auth::Auth::load(&args.data.join("auth.json"));
    if let Some(token) = &first {
        tracing::warn!("Primeiro início. Token de admin (guarde, não aparece de novo): {token}");
    }
    let device_name = if args.device == "sim" { "Simulado".to_string() } else { args.device.clone() };
    let app = api::App::new(store, auth, stats, device_name);
    api::spawn_meter_task(app.clone());
    api::spawn_save_task(app.clone());

    let listener = match tokio::net::TcpListener::bind(&args.bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Não consegui abrir {}: {e}", args.bind);
            std::process::exit(2);
        }
    };
    tracing::info!("Sona no ar em http://{} ({} Hz, bloco {})", args.bind, args.rate, args.block);
    if let Some(token) = first {
        tracing::info!("Abra http://{}/#token={token}", args.bind);
    }
    let app_for_exit = app.clone();
    axum::serve(listener, api::router(app, Some(args.web)))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .ok();
    app_for_exit.store.lock().unwrap().save();
    tracing::info!("Sona parou. Estado salvo.");
}
