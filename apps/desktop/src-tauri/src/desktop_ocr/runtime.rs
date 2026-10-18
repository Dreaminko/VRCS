use super::status::DesktopOcrStatus;
use std::sync::Mutex;
#[cfg(windows)]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Emitter, Manager as _, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::watch;
use vrcs_core::{
    ocr::{BlockUpdate, Phase, ScanOutcome, TranslatedBlock, VrOcrService},
    VrOcrConfig, VrOverlayConfig,
};

const STATUS_EVENT: &str = "desktop-ocr-status-changed";

#[derive(Default)]
struct State {
    service: Option<VrOcrService>,
    config: Option<VrOcrConfig>,
    active_config: Option<VrOcrConfig>,
    status: DesktopOcrStatus,
    task: Option<tauri::async_runtime::JoinHandle<()>>,

    #[cfg(windows)]
    selection_cancelled: Arc<AtomicBool>,
}

impl State {
    fn cancel(&mut self) {
        #[cfg(windows)]
        self.selection_cancelled.store(true, Ordering::Release);
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.status.scan_id += 1;
        self.status.revision += 1;
        self.active_config = None;
    }

    fn configure(&mut self, config: &VrOcrConfig) -> bool {
        if self.config.as_ref() == Some(config) {
            return false;
        }
        if self.active_config.as_ref() != Some(config) {
            self.cancel();
            self.status.state = if config.desktop_enabled && cfg!(windows) {
                "idle"
            } else {
                "disabled"
            };
            self.status.blocks.clear();
            self.status.error = None;
            self.status.timed_out = false;
        }
        self.config = Some(config.clone());
        true
    }
}

pub struct Manager {
    app: AppHandle,
    state: Mutex<State>,
    window_creation: Mutex<()>,
    bridge: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    #[cfg(windows)]
    hotkey: Mutex<Option<super::hotkey::Hotkey>>,
}

impl Manager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            state: Mutex::new(State::default()),
            window_creation: Mutex::new(()),
            bridge: Mutex::new(None),
            #[cfg(windows)]
            hotkey: Mutex::new(None),
        }
    }

    pub fn status(&self) -> DesktopOcrStatus {
        self.state.lock().unwrap().status.clone()
    }

    pub fn start(
        &self,
        service: Option<VrOcrService>,
        mut config: watch::Receiver<(VrOverlayConfig, VrOcrConfig)>,
    ) {
        self.stop();
        {
            let mut state = self.state.lock().unwrap();
            state.service = service;
            // A replacement worker needs a binding even when the config is unchanged.
            state.config = None;
        }
        #[cfg(windows)]
        {
            // Serialize installation with recording transitions, including worker startup.
            let mut worker = self.hotkey.lock().unwrap();
            match super::hotkey::Hotkey::new(self.app.clone(), super::hotkey::Action::Ocr) {
                Ok(hotkey) => *worker = Some(hotkey),
                Err(error) => {
                    tracing::warn!(%error, "Desktop OCR shortcut thread failed");
                    self.shortcut_error(Some("desktop_ocr.unavailable".into()));
                }
            }
        }
        self.configure(config.borrow_and_update().1.clone());
        let app = self.app.clone();
        *self.bridge.lock().unwrap() = Some(tauri::async_runtime::spawn(async move {
            while config.changed().await.is_ok() {
                app.state::<Self>()
                    .configure(config.borrow_and_update().1.clone());
            }
        }));
    }

    fn configure(&self, config: VrOcrConfig) {
        {
            let mut state = self.state.lock().unwrap();
            if !state.configure(&config) {
                return;
            }
            self.publish(&state.status);
        }
        if !config.desktop_enabled {
            self.close();
        }
        #[cfg(windows)]
        if let Some(hotkey) = self.hotkey.lock().unwrap().as_ref() {
            if let Err(error) =
                hotkey.update(config.desktop_enabled.then(|| config.shortcut.clone()))
            {
                tracing::warn!(%error, "Desktop OCR shortcut update failed");
                self.shortcut_error(Some("desktop_ocr.unavailable".into()));
            }
        }
    }

    #[cfg(windows)]
    pub fn set_hotkey_paused(&self, paused: bool) -> Result<(), String> {
        if let Some(hotkey) = self.hotkey.lock().unwrap().as_ref() {
            if let Err(error) = hotkey.set_paused(paused) {
                self.shortcut_error(Some("desktop_ocr.unavailable".into()));
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn shortcut_error(&self, error: Option<String>) {
        let mut state = self.state.lock().unwrap();
        if state.status.shortcut_error != error {
            state.status.shortcut_error = error;
            state.status.revision += 1;
            self.publish(&state.status);
        }
    }

    pub fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.cancel();
        state.status.state = if state
            .config
            .as_ref()
            .is_some_and(|config| config.desktop_enabled)
        {
            "idle"
        } else {
            "disabled"
        };
        self.publish(&state.status);
        let scan_id = state.status.scan_id;
        drop(state);
        let app = self.app.clone();
        let _ = self.app.run_on_main_thread(move || {
            if app.state::<Self>().status().scan_id != scan_id {
                return;
            }
            if let Some(window) = app.get_webview_window("ocr") {
                #[cfg(windows)]
                if let Ok(hwnd) = window.hwnd() {
                    unsafe {
                        windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                            hwnd.0 as _,
                            windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE,
                        );
                    }
                }
                #[cfg(not(windows))]
                let _ = window.hide();
            }
        });
    }

    pub fn stop(&self) {
        if let Some(bridge) = self.bridge.lock().unwrap().take() {
            bridge.abort();
        }
        self.close();
        self.state.lock().unwrap().service = None;
        #[cfg(windows)]
        self.hotkey.lock().unwrap().take();
    }

    fn publish(&self, status: &DesktopOcrStatus) {
        let _ = self.app.emit_to("main", STATUS_EVENT, status);
        let _ = self.app.emit_to("ocr", STATUS_EVENT, status);
    }

    fn update(&self, scan_id: u64, update: impl FnOnce(&mut DesktopOcrStatus)) -> bool {
        let mut state = self.state.lock().unwrap();
        if !state.status.update(scan_id, update) {
            return false;
        }
        self.publish(&state.status);
        true
    }

    fn show_result(&self, scan_id: u64) {
        let app = self.app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let manager = app.state::<Self>();
            let _creation = manager.window_creation.lock().unwrap();
            if manager.status().scan_id != scan_id {
                return;
            }
            let window = match app.get_webview_window("ocr") {
                Some(window) => Ok(window),
                None => WebviewWindowBuilder::new(
                    &app,
                    "ocr",
                    WebviewUrl::App("index.html?window=ocr".into()),
                )
                .title("OCR")
                .decorations(false)
                .inner_size(560.0, 640.0)
                .min_inner_size(360.0, 300.0)
                .always_on_top(true)
                .skip_taskbar(true)
                .visible(false)
                .focused(false)
                .build(),
            };
            let app = app.clone();
            let _ = app.clone().run_on_main_thread(move || {
                if app.state::<Self>().status().scan_id != scan_id {
                    return;
                }
                match window {
                    Ok(window) => {
                        #[cfg(windows)]
                        if let Ok(hwnd) = window.hwnd() {
                            unsafe {
                                windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                                    hwnd.0 as _,
                                    windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE,
                                );
                            }
                        }
                        #[cfg(not(windows))]
                        let _ = window.show();
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Desktop OCR result window failed");
                        app.state::<Self>().update(scan_id, |status| {
                            status.state = "error";
                            status.error = Some("desktop_ocr.unavailable".into());
                        });
                    }
                }
            });
        });
    }

    #[cfg(not(windows))]
    pub fn scan(&self) -> Result<(), String> {
        Err("desktop_ocr.unavailable".into())
    }

    #[cfg(windows)]
    pub fn scan(&self) -> Result<(), String> {
        use vrcs_core::ocr::desktop_capture::capture_desktop;
        if self.status().state == "selecting" {
            self.close();
            return Ok(());
        }
        // Native getters can wait for the UI thread. Do not hold scan state here.
        let result_windows: Vec<isize> = ["ocr", "vision"]
            .into_iter()
            .filter_map(|label| self.app.get_webview_window(label))
            .filter_map(|window| window.hwnd().ok())
            .map(|hwnd| hwnd.0 as isize)
            .collect();
        let hint = crate::desktop_capture::selection_hint(&self.app, "ocrWindow");
        let mut state = self.state.lock().unwrap();
        let service = state
            .service
            .clone()
            .ok_or("desktop_ocr.core_unavailable")?;
        let config = service.configuration()?;
        if !config.ocr().desktop_enabled {
            return Err("desktop_ocr.disabled".into());
        }
        let picker =
            crate::desktop_capture::reserve_picker().map_err(|_| "desktop_ocr.selection_busy")?;
        state.cancel();
        let scan_id = state.status.scan_id;
        state.active_config = Some(config.ocr().clone());
        let selection_cancelled = Arc::new(AtomicBool::new(false));
        state.selection_cancelled = selection_cancelled.clone();
        state.status.state = "capturing";
        state.status.blocks.clear();
        state.status.error = None;
        state.status.timed_out = false;
        self.publish(&state.status);

        let app = self.app.clone();
        state.task = Some(tauri::async_runtime::spawn(async move {
            let result = async {
                let mut capture =
                    tauri::async_runtime::spawn_blocking(move || capture_desktop(&result_windows))
                        .await
                        .map_err(|_| "desktop_ocr.capture_failed".to_string())??;

                if !app
                    .state::<Self>()
                    .update(scan_id, |status| status.state = "selecting")
                {
                    return Ok(None);
                }
                let selected = tauri::async_runtime::spawn_blocking(move || {
                    let region = super::selection_window::select_region_reserved(
                        &capture,
                        &selection_cancelled,
                        &hint,
                        &picker,
                    )?;
                    let Some(region) = region else {
                        return Ok::<_, String>(None);
                    };
                    if selection_cancelled.load(Ordering::Acquire) {
                        return Ok(None);
                    }
                    capture.pixels = region.crop(capture.width, capture.height, &capture.pixels)?;
                    capture.width = region.width;
                    capture.height = region.height;
                    Ok(Some(capture))
                })
                .await
                .map_err(|_| "desktop_ocr.capture_failed".to_string())??;
                let Some(capture) = selected else {
                    return Ok(None);
                };
                if !app
                    .state::<Self>()
                    .update(scan_id, |status| status.state = "recognizing")
                {
                    return Ok(None);
                }
                app.state::<Self>().show_result(scan_id);
                // Time spent choosing the region does not consume the OCR deadline.
                let deadline = tokio::time::Instant::now()
                    + std::time::Duration::from_secs(config.ocr().timeout_seconds as u64);
                let image = prepare_image(capture);
                service
                    .process_scan(
                        [image],
                        &config,
                        scan_id,
                        deadline,
                        |phase| {
                            app.state::<Self>().update(scan_id, |status| {
                                status.state = if phase == Phase::Translating {
                                    "translating"
                                } else {
                                    "recognizing"
                                }
                            });
                        },
                        |_| async { Ok(()) },
                        |update| {
                            app.state::<Self>()
                                .update(scan_id, |status| upsert_block(&mut status.blocks, update));
                        },
                    )
                    .await
                    .map(Some)
            }
            .await;
            let manager = app.state::<Self>();
            match result {
                Ok(Some(result)) => {
                    manager.update(scan_id, |status| {
                        status.blocks = result.blocks.into_iter().next().unwrap_or_default();
                        sort_blocks(&mut status.blocks);
                        status.timed_out = result.summary.timed_out;
                        status.state = match result.summary.outcome {
                            ScanOutcome::NoText | ScanOutcome::LowConfidence => "no_text",
                            ScanOutcome::PartialFailure
                            | ScanOutcome::TotalFailure
                            | ScanOutcome::TimedOut => "partial_failure",
                            _ => "complete",
                        };
                    });
                }
                Ok(None) => {
                    manager.update(scan_id, |status| status.state = "idle");
                }
                Err(error) => {
                    tracing::warn!(scan_id, %error, "Desktop OCR scan failed");
                    if manager.update(scan_id, |status| {
                        status.state = "error";
                        status.error = Some(if error.starts_with("desktop_ocr.") {
                            error
                        } else {
                            "desktop_ocr.failed".into()
                        });
                    }) {
                        manager.show_result(scan_id);
                    }
                }
            }
        }));
        Ok(())
    }
}

fn upsert_block(blocks: &mut Vec<TranslatedBlock>, update: BlockUpdate) {
    if let Some(block) = blocks
        .iter_mut()
        .find(|block| block.source.id == update.block.source.id)
    {
        *block = update.block;
    } else {
        blocks.push(update.block);
    }
    sort_blocks(blocks);
}

fn sort_blocks(blocks: &mut [TranslatedBlock]) {
    blocks.sort_by_key(|block| block.source.id);
}

#[cfg(windows)]
fn prepare_image(
    capture: vrcs_core::ocr::desktop_capture::DesktopCapture,
) -> vrcs_core::ocr::OcrImage {
    vrcs_core::ocr::OcrImage {
        data: vrcs_core::ocr::OcrImageData::Rgba(capture.pixels),
        width: capture.width,
        height: capture.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn cancelling_a_scan_signals_its_native_picker() {
        let mut state = State::default();
        let picker = state.selection_cancelled.clone();
        assert!(!picker.load(Ordering::Acquire));
        state.cancel();
        assert!(picker.load(Ordering::Acquire));
        state.selection_cancelled = Arc::new(AtomicBool::new(false));
        assert!(picker.load(Ordering::Acquire));
        assert!(!state.selection_cancelled.load(Ordering::Acquire));
    }

    #[cfg(windows)]
    #[test]
    fn local_desktop_ocr_keeps_original_pixels_for_recognition() {
        use vrcs_core::ocr::{desktop_capture::DesktopCapture, OcrImageData};
        let pixels = vec![42; 3840 * 2 * 4];
        let image = prepare_image(DesktopCapture {
            origin: (0, 0),
            width: 3840,
            height: 2,
            pixels: pixels.clone(),
        });
        assert_eq!((image.width, image.height), (3840, 2));
        let OcrImageData::Rgba(actual) = image.data else {
            panic!("Local OCR needs original RGBA pixels")
        };
        assert_eq!(actual, pixels);
    }

    #[test]
    fn delayed_config_notification_keeps_a_scan_using_the_new_config() {
        let previous = VrOcrConfig::default();
        let mut next = previous.clone();
        next.desktop_enabled = true;
        let mut state = State {
            config: Some(previous),
            active_config: Some(next.clone()),
            status: DesktopOcrStatus {
                scan_id: 8,
                state: "recognizing",
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(state.configure(&next));
        assert_eq!(
            (state.status.scan_id, state.status.state),
            (8, "recognizing")
        );
        next.desktop_enabled = false;
        assert!(state.configure(&next));
        assert_eq!((state.status.scan_id, state.status.state), (9, "disabled"));
    }
}
