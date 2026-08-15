use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoreStartup {
    state: CoreStartupState,
    error: Option<String>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum CoreStartupState {
    Starting,
    Ready,
    Failed,
}

#[derive(Clone)]
struct CoreLaunchOptions {
    config_path: PathBuf,
    port: u16,
    token: String,
}

struct CoreRuntime {
    handle: Mutex<Option<vrcs_core::CoreHandle>>,
    launch_task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    startup: Mutex<CoreStartup>,
    options: CoreLaunchOptions,
    stop_requested: AtomicBool,
}

struct NativeUiState {
    show_item: Mutex<Option<MenuItem<tauri::Wry>>>,
    quit_item: Mutex<Option<MenuItem<tauri::Wry>>>,
    compact_topmost: AtomicBool,
}

const DEFAULT_CORE_PORT: u16 = 8766;

fn available_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .expect("failed to allocate a local port for VRCS Core")
}

fn core_connection_config() -> CoreConnection {
    let token = Uuid::new_v4().simple().to_string();
    if cfg!(debug_assertions) {
        return CoreConnection {
            http_url: format!("http://127.0.0.1:{DEFAULT_CORE_PORT}"),
            ws_url: format!("ws://127.0.0.1:{DEFAULT_CORE_PORT}/ws"),
            token,
        };
    }

    let port = available_port();
    CoreConnection {
        http_url: format!("http://127.0.0.1:{port}"),
        ws_url: format!("ws://127.0.0.1:{port}/ws"),
        token,
    }
}

#[tauri::command]
fn core_connection(connection: State<'_, CoreConnection>) -> CoreConnection {
    connection.inner().clone()
}

#[tauri::command]
fn core_startup(runtime: State<'_, CoreRuntime>) -> Result<CoreStartup, String> {
    runtime
        .startup
        .lock()
        .map(|startup| startup.clone())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn retry_core(app: tauri::AppHandle) -> Result<(), String> {
    launch_core(&app)
}

#[tauri::command]
fn write_glossary_file(path: PathBuf, contents: String) -> Result<(), String> {
    std::fs::write(path, contents.as_bytes()).map_err(|error| error.to_string())
}

fn launch_core(app: &tauri::AppHandle) -> Result<(), String> {
    let runtime = app.state::<CoreRuntime>();
    {
        let mut startup = runtime.startup.lock().map_err(|error| error.to_string())?;
        if matches!(
            startup.state,
            CoreStartupState::Starting | CoreStartupState::Ready
        ) {
            return Ok(());
        }
        *startup = CoreStartup {
            state: CoreStartupState::Starting,
            error: None,
        };
    }
    runtime.stop_requested.store(false, Ordering::Release);

    let options = runtime.options.clone();
    let app = app.clone();
    let launch_task = tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let result = vrcs_core::start_with_deferred_vad(vrcs_core::CoreOptions {
            config_path: options.config_path,
            host: Some("127.0.0.1".into()),
            port: Some(options.port),
            session_token: Some(options.token),
            vad_model_path: None,
            asr_model_dir: None,
        })
        .await;
        let runtime = app.state::<CoreRuntime>();
        match result {
            Ok(core) if runtime.stop_requested.load(Ordering::Acquire) => {
                if let Err(error) = core.shutdown().await {
                    tracing::warn!(%error, "Core shutdown after cancelled startup failed");
                }
            }
            Ok(core) => {
                *runtime.handle.lock().expect("core runtime lock poisoned") = Some(core);
                *runtime.startup.lock().expect("core startup lock poisoned") = CoreStartup {
                    state: CoreStartupState::Ready,
                    error: None,
                };
                tracing::info!(
                    elapsed_ms = started.elapsed().as_millis(),
                    "desktop Core startup ready"
                );
            }
            Err(error) => {
                tracing::error!(
                    %error,
                    elapsed_ms = started.elapsed().as_millis(),
                    "desktop Core startup failed"
                );
                *runtime.startup.lock().expect("core startup lock poisoned") = CoreStartup {
                    state: CoreStartupState::Failed,
                    error: Some(error),
                };
            }
        }
    });
    *runtime
        .launch_task
        .lock()
        .map_err(|error| error.to_string())? = Some(launch_task);
    Ok(())
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

#[cfg(windows)]
fn apply_native_topmost(window: &tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, WS_EX_TOPMOST,
    };

    let hwnd = window.hwnd().map_err(|error| error.to_string())?.0 as _;
    let insert_after = if enabled {
        HWND_TOPMOST
    } else {
        HWND_NOTOPMOST
    };
    let result = unsafe {
        SetWindowPos(
            hwnd,
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOOWNERZORDER | SWP_NOSIZE,
        )
    };
    if result == 0 {
        return Err(format!(
            "Windows failed to update the compact subtitle window Z-order: {}",
            std::io::Error::last_os_error()
        ));
    }

    let extended_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    let actual = extended_style & WS_EX_TOPMOST != 0;
    if actual != enabled {
        return Err("Windows did not retain the requested compact subtitle Z-order".into());
    }
    Ok(())
}

fn apply_compact_topmost(
    window: &tauri::WebviewWindow,
    enabled: bool,
    reason: &'static str,
) -> Result<(), String> {
    window
        .set_always_on_top(enabled)
        .map_err(|error| error.to_string())?;

    #[cfg(windows)]
    apply_native_topmost(window, enabled)?;

    #[cfg(not(windows))]
    if window
        .is_always_on_top()
        .map_err(|error| error.to_string())?
        != enabled
    {
        return Err("The compact subtitle window did not retain its requested Z-order".into());
    }

    tracing::debug!(enabled, reason, "compact window topmost state applied");
    Ok(())
}

#[tauri::command]
fn set_compact_window_topmost(
    window: tauri::WebviewWindow,
    enabled: bool,
    native_ui: State<'_, NativeUiState>,
) -> Result<(), String> {
    apply_compact_topmost(&window, enabled, "mode_change")?;
    native_ui.compact_topmost.store(enabled, Ordering::Release);
    Ok(())
}

fn reassert_compact_topmost(window: &tauri::WebviewWindow, reason: &'static str) {
    if !window
        .app_handle()
        .state::<NativeUiState>()
        .compact_topmost
        .load(Ordering::Acquire)
    {
        return;
    }
    if let Err(error) = apply_compact_topmost(window, true, reason) {
        tracing::warn!(%error, reason, "compact window topmost reassertion failed");
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        reassert_compact_topmost(&window, "window_shown");
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
    runtime.stop_requested.store(true, Ordering::Release);
    let launch_task = runtime
        .launch_task
        .lock()
        .expect("core launch task lock poisoned")
        .take();
    let app = app.clone();
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
    tauri::async_runtime::spawn(async move {
        if let Some(task) = launch_task {
            let _ = task.await;
        }
        let core = app
            .state::<CoreRuntime>()
            .handle
            .lock()
            .expect("core runtime lock poisoned")
            .take();
        let result = match core {
            Some(core) => core.shutdown().await,
            None => Ok(()),
        };
        let _ = done_tx.send(result);
    });

    match done_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("VRCS Core shutdown failed: {error}"),
        Err(_) => eprintln!("VRCS Core shutdown timed out"),
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
        .manage(NativeUiState {
            show_item: Mutex::new(None),
            quit_item: Mutex::new(None),
            compact_topmost: AtomicBool::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            core_connection,
            core_startup,
            retry_core,
            write_glossary_file,
            update_native_labels,
            set_compact_window_topmost
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
            app.manage(CoreRuntime {
                handle: Mutex::new(None),
                launch_task: Mutex::new(None),
                startup: Mutex::new(CoreStartup {
                    state: CoreStartupState::Failed,
                    error: None,
                }),
                options: CoreLaunchOptions {
                    config_path: data_dir.join("config.json"),
                    port,
                    token: setup_connection.token.clone(),
                },
                stop_requested: AtomicBool::new(false),
            });
            launch_core(app.handle()).map_err(std::io::Error::other)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if matches!(event, WindowEvent::Focused(false)) {
                    if let Some(window) = window.app_handle().get_webview_window("main") {
                        reassert_compact_topmost(&window, "focus_lost");
                    }
                }
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
        .then(|| "Silero v6.2.1 failed the first-start download and load self-test".to_string());
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
    use super::{core_connection_config, DEFAULT_CORE_PORT};

    const CAPABILITIES: &str = include_str!("../capabilities/default.json");

    #[test]
    fn custom_window_actions_are_allowed() {
        for permission in [
            "core:window:allow-start-dragging",
            "core:window:allow-minimize",
            "core:window:allow-toggle-maximize",
            "core:window:allow-close",
            "core:window:allow-set-resizable",
            "autostart:allow-enable",
            "autostart:allow-disable",
            "autostart:allow-is-enabled",
            "dialog:allow-open",
            "dialog:allow-save",
            "store:default",
        ] {
            assert!(CAPABILITIES.contains(permission), "missing {permission}");
        }
    }

    #[test]
    fn development_core_port_avoids_anki_and_vrchat_osc_defaults() {
        assert_eq!(DEFAULT_CORE_PORT, 8766);
        assert!(![8765, 9000, 9001].contains(&DEFAULT_CORE_PORT));
        assert!(!core_connection_config().token.is_empty());
    }
}
