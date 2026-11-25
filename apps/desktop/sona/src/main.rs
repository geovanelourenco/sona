//! Sona (app desktop). Abrir o app liga o servidor de áudio; a janela mostra o
//! console. Fechar a janela esconde na bandeja: o áudio continua até "Sair".
//! Com "Iniciar com o Windows", o app abre minimizado no login e o broadcast já
//! funciona sem ninguém abrir nada.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const MINIMIZED: &str = "--minimizado";

struct Server {
    url: Mutex<Option<String>>,
    stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    done: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
}

fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("console") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let url = app.state::<Server>().url.lock().unwrap().clone();
    let target = match url {
        Some(u) => WebviewUrl::External(Url::parse(&u).expect("url local")),
        None => WebviewUrl::App("index.html".into()),
    };
    let _ = WebviewWindowBuilder::new(app, "console", target).title("Sona").inner_size(1440.0, 900.0).min_inner_size(900.0, 600.0).build();
}

fn error_page(app: &AppHandle, msg: &str) {
    let html = format!(
        "<html><body style='background:#0b0d12;color:#e5e7eb;font-family:Segoe UI,sans-serif;display:grid;place-items:center;height:100vh;margin:0'><div style='max-width:520px'><h2 style='font-weight:300'>O Sona não conseguiu ligar</h2><p style='color:#fcd34d'>{}</p><p style='color:#868e9c'>Se outro Sona já está aberto, use o ícone na bandeja. Registro em: {}</p></div></body></html>",
        msg.replace('<', "&lt;"),
        app.path().app_data_dir().map(|d| d.join("sona.log").display().to_string()).unwrap_or_default()
    );
    let url = format!("data:text/html;charset=utf-8,{}", html.replace('%', "%25").replace('#', "%23"));
    let _ = WebviewWindowBuilder::new(app, "erro", WebviewUrl::External(Url::parse(&url).expect("data url")))
        .title("Sona")
        .inner_size(640.0, 400.0)
        .build();
}

fn quit(app: &AppHandle) {
    let st = app.state::<Server>();
    if let Some(stop) = st.stop.lock().unwrap().take() {
        let _ = stop.send(());
        if let Some(done) = st.done.lock().unwrap().take() {
            // Espera o servidor salvar o estado e fechar os aparelhos.
            let _ = tauri::async_runtime::block_on(async { tokio::time::timeout(std::time::Duration::from_secs(5), done).await });
        }
    }
    app.exit(0);
}

fn main() {
    let minimized = std::env::args().any(|a| a == MINIMIZED);
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![MINIMIZED])))
        .manage(Server { url: Mutex::new(None), stop: Mutex::new(None), done: Mutex::new(None) })
        .setup(move |app| {
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let log = std::fs::OpenOptions::new().create(true).append(true).open(data.join("sona.log"))?;
            let _ = tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
                .with_ansi(false)
                .with_writer(Mutex::new(log))
                .try_init();
            let web = app.path().resource_dir()?.join("web");
            // Instalação nova: relógio interno até escolher a mesa na primeira configuração.
            let opts = sona_server::Options {
                data,
                bind: None,
                port: 7340,
                web: Some(web),
                primary: None,
                default_primary: Some("internal".into()),
            };
            let handle = app.handle().clone();
            match tauri::async_runtime::block_on(sona_server::start(opts)) {
                Ok(running) => {
                    tracing::info!("Sona (app) no ar: {}", running.local_url);
                    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
                    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
                    let st = handle.state::<Server>();
                    *st.url.lock().unwrap() = Some(running.local_url.clone());
                    *st.stop.lock().unwrap() = Some(stop_tx);
                    *st.done.lock().unwrap() = Some(done_rx);
                    tauri::async_runtime::spawn(async move {
                        running
                            .serve(async {
                                let _ = stop_rx.await;
                            })
                            .await;
                        let _ = done_tx.send(());
                    });
                    if !minimized {
                        show(&handle);
                    }
                }
                Err(e) => {
                    tracing::error!("{e}");
                    error_page(&handle, &e);
                }
            }

            // Bandeja: abrir, abrir no navegador, iniciar com o Windows, sair.
            let autostart = app.autolaunch().is_enabled().unwrap_or(false);
            let open = MenuItem::with_id(app, "abrir", "Abrir o console", true, None::<&str>)?;
            let browser = MenuItem::with_id(app, "navegador", "Abrir no navegador", true, None::<&str>)?;
            let auto = CheckMenuItem::with_id(app, "auto", "Iniciar com o Windows", true, autostart, None::<&str>)?;
            let exit = MenuItem::with_id(app, "sair", "Sair (para o áudio)", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&open, &browser, &PredefinedMenuItem::separator(app)?, &auto, &PredefinedMenuItem::separator(app)?, &exit],
            )?;
            let auto2 = auto.clone();
            TrayIconBuilder::with_id("sona")
                .icon(app.default_window_icon().cloned().expect("ícone"))
                .tooltip("Sona — mesa de som")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, ev| match ev.id().as_ref() {
                    "abrir" => show(app),
                    "navegador" => {
                        if let Some(u) = app.state::<Server>().url.lock().unwrap().clone() {
                            open_browser(&u);
                        }
                    }
                    "auto" => {
                        let al = app.autolaunch();
                        let on = al.is_enabled().unwrap_or(false);
                        let _ = if on { al.disable() } else { al.enable() };
                        let _ = auto2.set_checked(al.is_enabled().unwrap_or(false));
                    }
                    "sair" => quit(app),
                    _ => {}
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        show(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|w, ev| {
            // Fechar a janela do console só esconde: o áudio continua.
            if let WindowEvent::CloseRequested { api, .. } = ev {
                if w.label() == "console" {
                    api.prevent_close();
                    let _ = w.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("app Tauri")
        .run(|app, ev| {
            // Sem janelas visíveis o app continua (bandeja); só "Sair" encerra.
            if let RunEvent::ExitRequested { api, code, .. } = ev {
                if code.is_none() {
                    api.prevent_exit();
                }
                let _ = app;
            }
        });
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
