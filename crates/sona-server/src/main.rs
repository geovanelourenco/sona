//! Sona: servidor de áudio.

#![allow(clippy::needless_range_loop)]

use clap::{Parser, Subcommand};
use sona_server::{auth, bench};
use std::path::PathBuf;

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
    /// Troca o dispositivo principal salvo: "sim" (simulado), "internal"
    /// (relógio interno, sem mesa) ou o nome do aparelho. O normal é escolher
    /// no app, em Configurações → Áudio.
    #[arg(long)]
    device: Option<String>,
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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let cli = Cli::parse();
    match cli.cmd.unwrap_or(Cmd::Serve) {
        Cmd::Serve => serve(cli.serve).await,
        Cmd::Devices => devices(),
        Cmd::Bench { seconds } => bench::run(seconds, 48_000),
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
        let list = sona_server::audio::hw::list_devices();
        if list.is_empty() {
            println!("Nenhum dispositivo de áudio encontrado.");
        }
        println!("{}", serde_json::to_string_pretty(&list).unwrap());
    }
    #[cfg(not(feature = "hw"))]
    println!("Compilado sem a feature \"hw\": só o dispositivo simulado existe.");
}

async fn serve(args: ServeArgs) {
    let opts = sona_server::Options { data: args.data, bind: args.bind, port: args.port, web: Some(args.web), primary: args.device };
    let running = match sona_server::start(opts).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("{e}");
            std::process::exit(2);
        }
    };
    tracing::info!("Sona no ar. Neste computador: {}", running.local_url);
    let remote_on = running.app.auth.lock().unwrap().status().remote_enabled;
    for u in &running.lan_urls {
        if remote_on {
            tracing::info!("Na rede (com a senha de acesso remoto): {u}");
        } else {
            tracing::info!("Na rede: {u} (acesso remoto desligado; ative no app, em Configurações)");
        }
    }
    if args.open {
        open_browser(&running.local_url);
    }
    running
        .serve(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
}
