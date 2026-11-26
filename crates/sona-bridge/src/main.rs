//! Sona Bridge (linha de comando). O app desktop usa a mesma biblioteca.

use clap::Parser;
use sona_bridge::config::Endpoint;
use sona_bridge::Bridge;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sona-bridge", version, about = "Sona Bridge — envia e recebe áudio do servidor Sona pela rede")]
struct Cli {
    /// Pasta de dados (bridge.json).
    #[arg(long, default_value = "bridge-data")]
    data: PathBuf,
    /// Porta da tela local (só neste computador).
    #[arg(long, default_value_t = 7350)]
    port: u16,
    /// Pasta com o build da tela (apps/mixer-web/dist).
    #[arg(long, default_value = "apps/mixer-web/dist")]
    web: PathBuf,
    /// Entra no servidor antes de ligar: endereço (ex.: 192.168.0.10:7340).
    #[arg(long)]
    server: Option<String>,
    /// Senha de acesso remoto do servidor (só no primeiro uso; o token fica salvo).
    #[arg(long)]
    password: Option<String>,
    #[arg(long)]
    name: Option<String>,
    /// Aparelho que envia para o servidor ("sim" = gerador de tom).
    #[arg(long)]
    send: Option<String>,
    /// Aparelho que toca o que vem do servidor ("sim" = medidor).
    #[arg(long)]
    receive: Option<String>,
    /// Desvio de relógio dos aparelhos simulados (ppm), para testes.
    #[arg(long)]
    sim_ppm: Option<f64>,
    /// Lista os aparelhos de áudio e sai.
    #[arg(long)]
    devices: bool,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let cli = Cli::parse();
    if cli.devices {
        #[cfg(feature = "hw")]
        println!("{}", serde_json::to_string_pretty(&sona_bridge::audio::hw::list(48_000)).unwrap());
        return;
    }
    let bridge = Bridge::new(cli.data.join("bridge.json"));
    {
        let mut c = bridge.config.lock().unwrap();
        if let Some(n) = &cli.name {
            c.name = n.clone();
        }
        if let Some(d) = &cli.send {
            c.send = Some(Endpoint { device: d.clone(), channels: 2 });
        }
        if let Some(d) = &cli.receive {
            c.receive = Some(Endpoint { device: d.clone(), channels: 2 });
        }
        if let Some(p) = cli.sim_ppm {
            c.sim_ppm = p;
        }
    }
    bridge.save();
    if let Some(server) = &cli.server {
        let name = cli.name.clone().unwrap_or_default();
        if let Err(e) = bridge.login(server, cli.password.as_deref().unwrap_or(""), &name, "") {
            tracing::error!("{e}");
            std::process::exit(2);
        }
    }
    let b2 = bridge.clone();
    std::thread::Builder::new().name("bridge".into()).spawn(move || b2.run()).expect("thread");
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", cli.port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Não consegui abrir a tela local na porta {}: {e}", cli.port);
            std::process::exit(2);
        }
    };
    tracing::info!("Sona Bridge: tela em http://127.0.0.1:{}", cli.port);
    let b3 = bridge.clone();
    let _ = axum::serve(listener, sona_bridge::ui::router(bridge, Some(cli.web)))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    b3.shutdown();
}
