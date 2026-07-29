use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, RunEvent, State, WindowEvent};
use tauri_plugin_store::StoreExt;
use uuid::Uuid;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoreConnection {
    http_url: String,
    ws_url: String,
    token: String,
}

struct CoreRuntime(Mutex<Option<vrcs_core::CoreHandle>>);

struct NativeUiState {
    show_item: Mutex<Option<MenuItem<tauri::Wry>>>,
    quit_item: Mutex<Option<MenuItem<tauri::Wry>>>,
}

const DEFAULT_CORE_PORT: u16 = 8766;

fn available_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .expect("failed to allocate a local port for VRCS Core")
}

fn core_connection_config() -> CoreConnection {
    if cfg!(debug_assertions) {
        return CoreConnection {
            http_url: format!("http://127.0.0.1:{DEFAULT_CORE_PORT}"),
            ws_url: format!("ws://127.0.0.1:{DEFAULT_CORE_PORT}/ws"),
            token: String::new(),
        };
    }

    let port = available_port();
    CoreConnection {
        http_url: format!("http://127.0.0.1:{port}"),
        ws_url: format!("ws://127.0.0.1:{port}/ws"),
        token: Uuid::new_v4().simple().to_string(),
    }
}

#[tauri::command]
fn core_connection(connection: State<'_, CoreConnection>) -> CoreConnection {
    connection.inner().clone()
}

#[tauri::command]
fn update_native_labels(
    show: String,
    quit: String,
    native_ui: State<'_, NativeUiState>,
) -> Result<(), String> {
    if let Some(item) = native_ui
        .show_item
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        item.set_text(show).map_err(|error| error.to_string())?;
    }
    if let Some(item) = native_ui
        .quit_item
        .lock()
        .map_err(|error| error.to_string())?
        .as_ref()
    {
        item.set_text(quit).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn minimize_to_tray_enabled(app: &tauri::AppHandle) -> bool {
    app.store("preferences.json")
        .ok()
        .and_then(|store| store.get("minimizeToTray"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn stop_core(app: &tauri::AppHandle) {
    let runtime = app.state::<CoreRuntime>();
    let core = runtime.0.lock().expect("core runtime lock poisoned").take();
    if let Some(mut core) = core {
        core.stop();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = core.wait().await {
                eprintln!("VRCS Core shutdown failed: {error}");
            }
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let log_dir = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok()
        .map(|path| path.join(".vrcs").join("logs"));
    let _logging_guard =
        vrcs_core::init_tracing(log_dir.as_deref()).unwrap_or_else(|error| panic!("{error}"));
    let connection = core_connection_config();
    let setup_connection = connection.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(connection)
        .manage(CoreRuntime(Mutex::new(None)))
        .manage(NativeUiState {
            show_item: Mutex::new(None),
            quit_item: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            core_connection,
            update_native_labels
        ])
        .setup(move |app| {
            app.store("preferences.json")?;

            let show_item = MenuItem::with_id(app, "show", "Show VRCS", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit VRCS", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;
            let native_ui = app.state::<NativeUiState>();
            *native_ui
                .show_item
                .lock()
                .expect("native UI state lock poisoned") = Some(show_item.clone());
            *native_ui
                .quit_item
                .lock()
                .expect("native UI state lock poisoned") = Some(quit_item.clone());
            let mut tray = TrayIconBuilder::new()
                .tooltip("VRCS")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            let data_dir = app.path().local_data_dir()?.join(".vrcs");
            std::fs::create_dir_all(data_dir.join("models"))?;

            let port = setup_connection
                .http_url
                .rsplit(':')
                .next()
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| std::io::Error::other("core URL is missing a valid port"))?;
            let session_token =
                (!setup_connection.token.is_empty()).then(|| setup_connection.token.clone());
            let core = tauri::async_runtime::block_on(vrcs_core::start(vrcs_core::CoreOptions {
                config_path: data_dir.join("config.json"),
                host: Some("127.0.0.1".into()),
                port: Some(port),
                session_token,
                vad_model_path: None,
                asr_model_dir: None,
            }))
            .map_err(std::io::Error::other)?;
            *app.state::<CoreRuntime>()
                .0
                .lock()
                .expect("core runtime lock poisoned") = Some(core);
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    if minimize_to_tray_enabled(window.app_handle()) {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building VRCS");

    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
            stop_core(app_handle);
        }
    });
}

pub fn release_self_test() -> Result<(), String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "vrcs-release-self-test-{}-{nonce}",
        std::process::id()
    ));
    let result = tauri::async_runtime::block_on(async {
        let handle = vrcs_core::start(vrcs_core::CoreOptions {
            config_path: directory.join("config.json"),
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: None,
            vad_model_path: None,
            asr_model_dir: None,
        })
        .await?;
        let vad_error = (handle.vad_backend() != "silero-onnx"
            || handle.vad_model_version() != Some("v6.2.1"))
        .then(|| "Silero v6.2.1 未通过首次启动下载与加载自检".to_string());
        let shutdown_result = handle.shutdown().await;
        if let Some(error) = vad_error {
            return Err(error);
        }
        shutdown_result
    });
    let _ = std::fs::remove_dir_all(&directory);
    result
}

#[cfg(test)]
mod tests {
    use super::DEFAULT_CORE_PORT;

    const CAPABILITIES: &str = include_str!("../capabilities/default.json");

    #[test]
    fn custom_window_actions_are_allowed() {
        for permission in [
            "core:window:allow-start-dragging",
            "core:window:allow-minimize",
            "core:window:allow-toggle-maximize",
            "core:window:allow-close",
            "core:window:allow-set-resizable",
            "core:window:allow-set-always-on-top",
            "core:window:allow-is-always-on-top",
            "autostart:allow-enable",
            "autostart:allow-disable",
            "autostart:allow-is-enabled",
            "dialog:allow-open",
            "store:default",
        ] {
            assert!(CAPABILITIES.contains(permission), "missing {permission}");
        }
    }

    #[test]
    fn development_core_port_avoids_anki_and_vrchat_osc_defaults() {
        assert_eq!(DEFAULT_CORE_PORT, 8766);
        assert!(![8765, 9000, 9001].contains(&DEFAULT_CORE_PORT));
    }
}
