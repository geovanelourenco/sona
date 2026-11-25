//! Sona Bridge (app desktop). Abrir o app conecta ao servidor e liga os
//! aparelhos escolhidos; fechar a janela esconde na bandeja e o áudio continua.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use sona_bridge::Bridge;
use std::sync::{Arc, Mutex};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const MINIMIZED: &str = "--minimizado";

struct State {
    url: Mutex<Option<String>>,
    bridge: Mutex<Option<Arc<Bridge>>>,
}

fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("bridge") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let Some(url) = app.state::<State>().url.lock().unwrap().clone() else { return };
    let _ = WebviewWindowBuilder::new(app, "bridge", WebviewUrl::External(Url::parse(&url).expect("url local")))
        .title("Sona Bridge")
        .inner_size(980.0, 640.0)
        .min_inner_size(640.0, 480.0)
        .build();
}

fn main() {
    let minimized = std::env::args().any(|a| a == MINIMIZED);
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![MINIMIZED])))
        .manage(State { url: Mutex::new(None), bridge: Mutex::new(None) })
        .setup(move |app| {
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let log = std::fs::OpenOptions::new().create(true).append(true).open(data.join("sona-bridge.log"))?;
            let _ = tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
                .with_ansi(false)
                .with_writer(Mutex::new(log))
                .try_init();
            let web = app.path().resource_dir()?.join("web");
            let bridge = Bridge::new(data.join("bridge.json"));
            let b2 = bridge.clone();
            std::thread::Builder::new().name("bridge".into()).spawn(move || b2.run())?;
            // Tela local: porta 7350, ou qualquer livre se ela estiver ocupada.
            let listener = tauri::async_runtime::block_on(async {
                match tokio::net::TcpListener::bind(("127.0.0.1", 7350)).await {
                    Ok(l) => Ok(l),
                    Err(_) => tokio::net::TcpListener::bind(("127.0.0.1", 0)).await,
                }
            })?;
            let port = listener.local_addr()?.port();
            let router = sona_bridge::ui::router(bridge.clone(), Some(web));
            tauri::async_runtime::spawn(async move {
                let _ = axum::serve(listener, router).await;
            });
            let st = app.state::<State>();
            *st.url.lock().unwrap() = Some(format!("http://127.0.0.1:{port}"));
            *st.bridge.lock().unwrap() = Some(bridge);
            if !minimized {
                show(app.handle());
            }

            let autostart = app.autolaunch().is_enabled().unwrap_or(false);
            let open = MenuItem::with_id(app, "abrir", "Abrir o Sona Bridge", true, None::<&str>)?;
            let auto = CheckMenuItem::with_id(app, "auto", "Iniciar com o Windows", true, autostart, None::<&str>)?;
            let exit = MenuItem::with_id(app, "sair", "Sair (para o áudio)", true, None::<&str>)?;
            let menu =
                Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &auto, &PredefinedMenuItem::separator(app)?, &exit])?;
            let auto2 = auto.clone();
            TrayIconBuilder::with_id("sona-bridge")
                .icon(app.default_window_icon().cloned().expect("ícone"))
                .tooltip("Sona Bridge")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, ev| match ev.id().as_ref() {
                    "abrir" => show(app),
                    "auto" => {
                        let al = app.autolaunch();
                        let on = al.is_enabled().unwrap_or(false);
                        let _ = if on { al.disable() } else { al.enable() };
                        let _ = auto2.set_checked(al.is_enabled().unwrap_or(false));
                    }
                    "sair" => {
                        if let Some(b) = app.state::<State>().bridge.lock().unwrap().as_ref() {
                            b.shutdown();
                        }
                        app.exit(0);
                    }
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
            if let WindowEvent::CloseRequested { api, .. } = ev {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("app Tauri")
        .run(|_app, ev| {
            if let RunEvent::ExitRequested { api, code, .. } = ev {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
