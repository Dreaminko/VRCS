use super::status::DesktopVisionStatus;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Manager as _, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::{oneshot, watch};
use vrcs_core::{
    vision::{VisionMode, VisionService, VisionSession},
    VisionConfig, VrOcrConfig, VrOverlayConfig,
};

const STATUS_EVENT: &str = "desktop-vision-status-changed";

#[derive(Default)]
struct State {
    service: Option<VisionService>,
    lifecycle: u64,
    status: DesktopVisionStatus,
    image: Option<Arc<Vec<u8>>>,
    recording: Option<VisionSession>,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    selection_cancelled: Arc<AtomicBool>,

    binding: Option<(Option<String>, bool)>,
}

impl State {
    fn cancel(&mut self) {
        self.selection_cancelled.store(true, Ordering::Release);
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.recording = None;
        self.status.session_id += 1;
        self.status.revision += 1;
        self.status.clear_error();
        self.status.state = if !self.status.available {
            "disabled"
        } else if self.image.is_some() {
            "editing"
        } else {
            "idle"
        };
    }

    fn clear(&mut self) {
        self.cancel();
        self.image = None;

        self.status.image_id = None;
        self.status.question.clear();
        self.status.answer.clear();
        self.status.state = if self.status.available {
            "idle"
        } else {
            "disabled"
        };
    }

    fn ready(&self) -> Result<VisionService, String> {
        let service = self
            .service
            .clone()
            .ok_or("desktop_vision.core_unavailable")?;
        if !cfg!(windows) {
            return Err("desktop_vision.unavailable".into());
        }
        if !service.configuration_for(VisionMode::Desktop).enabled {
            return Err("desktop_vision.disabled".into());
        }
        if self.status.busy() {
            return Err("desktop_vision.busy".into());
        }
        Ok(service)
    }

    fn update(&mut self, id: u64, update: impl FnOnce(&mut Self)) -> bool {
        if self.status.session_id != id {
            return false;
        }
        update(self);
        self.status.revision += 1;
        true
    }

    fn selected_image(&self, image_id: u64) -> Result<Arc<Vec<u8>>, String> {
        if self.status.image_id != Some(image_id) {
            return Err("desktop_vision.stale_image".into());
        }
        self.image
            .clone()
            .ok_or_else(|| "desktop_vision.no_image".into())
    }
}

pub struct Manager {
    app: AppHandle,
    state: Mutex<State>,
    window_creation: Mutex<()>,
    bridge: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    #[cfg(windows)]
    hotkey: Mutex<Option<crate::desktop_capture::hotkey::Hotkey>>,
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

    pub fn status(&self) -> DesktopVisionStatus {
        self.state.lock().unwrap().status.clone()
    }

    #[cfg(target_os = "linux")]
    pub fn shortcut_binding(&self) -> Option<String> {
        self.state.lock().unwrap().binding.as_ref().and_then(|binding| binding.0.clone())
    }

    pub fn start(
        &self,
        service: VisionService,
        mut config: watch::Receiver<VisionConfig>,
        mut ocr: watch::Receiver<(VrOverlayConfig, VrOcrConfig)>,
    ) {
        self.stop();
        let lifecycle = {
            let mut state = self.state.lock().unwrap();
            state.service = Some(service);
            state.lifecycle
        };
        #[cfg(windows)]
        {
            // Serialize installation with recording transitions, including worker startup.
            let mut worker = self.hotkey.lock().unwrap();
            match crate::desktop_capture::hotkey::Hotkey::new(
                self.app.clone(),
                crate::desktop_capture::hotkey::Action::Vision(lifecycle),
            ) {
                Ok(hotkey) => *worker = Some(hotkey),
                Err(_) => {
                    self.shortcut_error_for(lifecycle, Some("desktop_vision.unavailable".into()))
                }
            }
        }
        self.configure(
            lifecycle,
            &config.borrow_and_update(),
            &ocr.borrow_and_update().1,
        );
        let app = self.app.clone();
        *self.bridge.lock().unwrap() = Some(tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    result = config.changed() => if result.is_err() { break; },
                    result = ocr.changed() => if result.is_err() { break; },
                }
                app.state::<Self>().configure(
                    lifecycle,
                    &config.borrow_and_update(),
                    &ocr.borrow_and_update().1,
                );
            }
        }));
    }

    fn configure(&self, lifecycle: u64, config: &VisionConfig, ocr: &VrOcrConfig) {
        let available = cfg!(any(windows, target_os = "linux")) && config.enabled && config.desktop_enabled;
        let conflict = available
            && ocr.desktop_enabled
            && shortcuts_match(&config.desktop_shortcut, &ocr.shortcut);
        let binding = (
            available
                .then(|| config.desktop_shortcut.clone())
                .filter(|_| !conflict),
            conflict,
        );
        let (changed_binding, hide, id) = {
            let mut state = self.state.lock().unwrap();
            if state.lifecycle != lifecycle {
                return;
            }
            let changed_binding = state.binding.as_ref() != Some(&binding);
            state.binding = Some(binding.clone());
            let hide = state.status.available && !available;
            state.status.available = available;
            if !available {
                state.clear();
            } else if state.status.state == "disabled" {
                state.status.state = "idle";
            }
            state.status.shortcut = config.desktop_shortcut.clone();
            state.status.model = config.model.clone();
            state.status.profile_name = state
                .service
                .as_ref()
                .map(VisionService::profile_name)
                .unwrap_or_default();
            state.status.revision += 1;
            self.publish(&state.status);
            (changed_binding, hide, state.status.session_id)
        };
        if hide {
            self.hide_window(id);
        }
        #[cfg(windows)]
        if changed_binding {
            if let Some(hotkey) = self.hotkey.lock().unwrap().as_ref() {
                if self.state.lock().unwrap().lifecycle != lifecycle {
                    return;
                }
                let error = conflict.then(|| "desktop_vision.shortcut_conflict".into());
                if hotkey.update_with_error(binding.0, error).is_err() {
                    self.shortcut_error_for(lifecycle, Some("desktop_vision.unavailable".into()));
                }
            }
        }
        #[cfg(not(windows))]
        let _ = changed_binding;
    }

    #[cfg(windows)]
    pub fn set_hotkey_paused(&self, paused: bool) -> Result<(), String> {
        if let Some(hotkey) = self.hotkey.lock().unwrap().as_ref() {
            if let Err(error) = hotkey.set_paused(paused) {
                let lifecycle = self.state.lock().unwrap().lifecycle;
                self.shortcut_error_for(lifecycle, Some("desktop_vision.unavailable".into()));
                return Err(error);
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    pub fn shortcut_error_for(&self, lifecycle: u64, error: Option<String>) {
        let mut state = self.state.lock().unwrap();
        if state.lifecycle == lifecycle && state.status.shortcut_error != error {
            state.status.shortcut_error = error;
            state.status.revision += 1;
            self.publish(&state.status);
        }
    }

    fn publish(&self, status: &DesktopVisionStatus) {
        let _ = self.app.emit_to("main", STATUS_EVENT, status);
        let _ = self.app.emit_to("vision", STATUS_EVENT, status);
    }

    fn update(&self, id: u64, update: impl FnOnce(&mut State)) -> bool {
        let mut state = self.state.lock().unwrap();
        if !state.update(id, update) {
            return false;
        }
        self.publish(&state.status);
        true
    }

    async fn show_window(&self, id: u64) -> Result<(), String> {
        let result: Result<(), String> = async {
            let app = self.app.clone();
            // WebView2 creation must not block the UI thread or hold the picker permit.
            let window = tauri::async_runtime::spawn_blocking(move || {
                let manager = app.state::<Self>();
                let _creation = manager.window_creation.lock().unwrap();
                if manager.status().session_id != id {
                    return Ok(None);
                }
                match app.get_webview_window("vision") {
                    Some(window) => Ok(Some(window)),
                    None => WebviewWindowBuilder::new(
                        &app,
                        "vision",
                        WebviewUrl::App("index.html?window=vision".into()),
                    )
                    .title("Visual question answering")
                    .decorations(false)
                    .inner_size(600.0, 760.0)
                    .min_inner_size(380.0, 400.0)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .visible(false)
                    .focused(false)
                    .build()
                    .map(Some)
                    .map_err(|error| error.to_string()),
                }
            })
            .await
            .map_err(|error| error.to_string())??;
            let Some(window) = window else {
                return Ok(());
            };
            let app = self.app.clone();
            let (sender, receiver) = oneshot::channel();
            self.app
                .run_on_main_thread(move || {
                    let result = (|| {
                        if app.state::<Self>().status().session_id != id {
                            return Ok(());
                        }
                        // Only showing and focusing the window competes with selection.
                        #[cfg(windows)]
                        let _picker = crate::desktop_capture::reserve_picker()
                            .map_err(|_| "desktop_vision.selection_busy".to_string())?;
                        window
                            .show()
                            .and_then(|_| window.set_focus())
                            .map_err(|error| error.to_string())
                    })();
                    let _ = sender.send(result);
                })
                .map_err(|error| error.to_string())?;
            receiver.await.map_err(|error| error.to_string())?
        }
        .await;
        result.map_err(|detail| {
            tracing::warn!(%detail, "Desktop vision window failed to open");
            if detail == "desktop_vision.selection_busy" {
                return detail;
            }
            self.update(id, |state| state.status.fail("openFailed", Some(detail)));
            "desktop_vision.openFailed".into()
        })
    }

    fn hide_window(&self, id: u64) {
        let app = self.app.clone();
        let _ = self.app.run_on_main_thread(move || {
            if app.state::<Self>().status().session_id == id {
                if let Some(window) = app.get_webview_window("vision") {
                    let _ = window.hide();
                }
            }
        });
    }

    pub async fn open(&self) -> Result<(), String> {
        self.show_window(self.status().session_id).await
    }

    pub fn cancel(&self) {
        let mut state = self.state.lock().unwrap();
        state.cancel();
        self.publish(&state.status);
    }

    pub fn close(&self) {
        let id = {
            let mut state = self.state.lock().unwrap();
            state.clear();
            self.publish(&state.status);
            state.status.session_id
        };
        self.hide_window(id);
    }

    pub fn stop(&self) {
        let id = {
            let mut state = self.state.lock().unwrap();
            state.lifecycle += 1;
            state.service = None;
            state.status.available = false;
            state.binding = None;
            state.clear();
            self.publish(&state.status);
            state.status.session_id
        };
        if let Some(bridge) = self.bridge.lock().unwrap().take() {
            bridge.abort();
        }
        #[cfg(windows)]
        {
            self.hotkey.lock().unwrap().take();
        }
        self.hide_window(id);
    }

    pub fn image(&self, image_id: u64) -> Result<Vec<u8>, String> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .selected_image(image_id)?
            .as_ref()
            .clone())
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    pub fn capture(&self) -> Result<(), String> {
        Err("desktop_vision.unavailable".into())
    }

    #[cfg(any(windows, target_os = "linux"))]
    pub fn capture(&self) -> Result<(), String> {
        self.capture_inner(None)
    }

    #[cfg(windows)]
    pub fn capture_for(&self, lifecycle: u64) -> Result<(), String> {
        tracing::info!("Desktop vision screenshot shortcut triggered");
        let result = self.capture_inner(Some(lifecycle));
        if let Err(error) = &result {
            tracing::warn!(%error, "Desktop vision screenshot shortcut failed");
            let show = {
                let mut state = self.state.lock().unwrap();
                if state.lifecycle != lifecycle {
                    return result;
                }
                let show = !state.status.busy();
                state.status.error = Some(error.clone());
                state.status.error_detail = None;
                if show && state.status.available {
                    state.status.state = "error";
                }
                state.status.revision += 1;
                self.publish(&state.status);
                show.then_some(state.status.session_id)
            };
            if let Some(id) = show {
                let app = self.app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = app.state::<Self>().show_window(id).await;
                });
            }
        }
        result
    }

    #[cfg(any(windows, target_os = "linux"))]
    fn capture_inner(&self, lifecycle: Option<u64>) -> Result<(), String> {
        if self.status().state == "selecting" {
            if lifecycle.is_some_and(|value| self.state.lock().unwrap().lifecycle != value) {
                return Ok(());
            }
            self.cancel();
            return Ok(());
        }
        // Native window access can wait on the UI thread; do it before locking state.
        #[cfg(windows)]
        let result_windows: Vec<isize> = ["ocr", "vision"]
            .into_iter()
            .filter_map(|label| self.app.get_webview_window(label))
            .filter_map(|window| window.hwnd().ok())
            .map(|hwnd| hwnd.0 as isize)
            .collect();
        #[cfg(windows)]
        let hint = crate::desktop_capture::selection_hint(&self.app, "visionWindow");
        let mut state = self.state.lock().unwrap();
        if lifecycle.is_some_and(|value| state.lifecycle != value) {
            return Ok(());
        }
        let service = state.ready()?;
        let config = service.configuration_for(VisionMode::Desktop);
        #[cfg(windows)]
        let picker = crate::desktop_capture::reserve_picker()
            .map_err(|_| "desktop_vision.selection_busy".to_string())?;
        state.cancel();
        let id = state.status.session_id;

        let cancelled = Arc::new(AtomicBool::new(false));
        state.selection_cancelled = cancelled.clone();
        state.status.state = "capturing";
        self.publish(&state.status);
        let app = self.app.clone();
        state.task = Some(tauri::async_runtime::spawn(async move {
            let result = async {
                #[cfg(target_os = "linux")]
                {
                    let selected = crate::desktop_capture::picker::capture_region(app.clone(), cancelled.clone(), || {
                        app.state::<Self>().update(id, |state| state.status.state = "selecting");
                    }).await?;
                    let Some(image) = selected else { return Ok(None); };
                    crate::vision_image::prepare_image(image, &config).map(Some)
                }
                #[cfg(windows)]
                {
                let capture = tauri::async_runtime::spawn_blocking(move || {
                    vrcs_core::ocr::desktop_capture::capture_desktop(&result_windows)
                })
                .await
                .map_err(|_| "desktop_vision.capture_failed".to_string())?
                .map_err(capture_error)?;
                if !app
                    .state::<Self>()
                    .update(id, |state| state.status.state = "selecting")
                {
                    return Ok(None);
                }
                tauri::async_runtime::spawn_blocking(move || {
                    if cancelled.load(Ordering::Acquire) {
                        return Ok(None);
                    }
                    let region = crate::desktop_capture::selection_window::select_region_reserved(
                        &capture, &cancelled, &hint, &picker,
                    )
                    .map_err(capture_error)?;
                    let Some(region) = region else {
                        return Ok(None);
                    };
                    if cancelled.load(Ordering::Acquire) {
                        return Ok(None);
                    }
                    let pixels = region
                        .crop(capture.width, capture.height, &capture.pixels)
                        .map_err(capture_error)?;

                    drop(capture);
                    let image = crate::desktop_capture::image::Image {
                        width: region.width,
                        height: region.height,
                        pixels,
                    };
                    let bytes = crate::vision_image::prepare_image(image, &config)
                        .map_err(|_| "desktop_vision.capture_failed".to_string())?;
                    Ok::<_, String>(Some(bytes))
                })
                .await
                .map_err(|_| "desktop_vision.capture_failed".to_string())?
                }
            }
            .await;
            let manager = app.state::<Self>();
            let updated = manager.update(id, |state| {
                state.task = None;
                match result {
                    Ok(Some(image)) => {
                        state.image = Some(Arc::new(image));
                        state.status.image_id = Some(id);
                        state.status.question.clear();
                        state.status.answer.clear();
                        state.status.state = "editing";
                    }
                    Ok(None) => {
                        state.status.state = if state.image.is_some() {
                            "editing"
                        } else {
                            "idle"
                        };
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Desktop vision screenshot failed");
                        state.status.state = "error";
                        state.status.error = Some(error);
                    }
                }
            });
            if updated {
                let _ = manager.show_window(id).await;
            }
        }));
        Ok(())
    }

    pub fn ask(&self, question: String, image_id: u64) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        let service = state.ready()?;
        let image = state.selected_image(image_id)?;
        state.cancel();
        let id = state.status.session_id;
        state.status.question = question.trim().into();
        state.status.answer.clear();
        state.status.state = "requesting";
        self.publish(&state.status);
        let app = self.app.clone();
        state.task = Some(tauri::async_runtime::spawn(async move {
            let result = service
                .answer_text(VisionMode::Desktop, question, image.as_ref().clone())
                .await;
            app.state::<Self>().update(id, |state| {
                state.task = None;
                match result {
                    Ok(answer) => {
                        state.status.question = answer.question;
                        state.status.answer = answer.answer;
                        state.status.state = "complete";
                    }
                    Err(detail) => state.status.fail("request_failed", Some(detail)),
                }
            });
        }));
        Ok(())
    }

    pub fn start_recording(&self) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        let service = state.ready()?;
        if state.image.is_none() {
            return Err("desktop_vision.no_image".into());
        }
        // Failure to acquire a microphone leaves the image and editable draft intact.
        let session = match service.begin_for(VisionMode::Desktop) {
            Ok(session) => session,
            Err(detail) => {
                state.status.fail("recognition_failed", Some(detail));
                state.status.revision += 1;
                self.publish(&state.status);
                return Ok(());
            }
        };
        let max_seconds = service
            .configuration_for(VisionMode::Desktop)
            .max_capture_seconds;
        state.cancel();
        let id = state.status.session_id;
        state.recording = Some(session);
        state.status.question.clear();
        state.status.state = "recording";
        self.publish(&state.status);
        let app = self.app.clone();
        state.task = Some(tauri::async_runtime::spawn(async move {
            let started = std::time::Instant::now();
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
            loop {
                tick.tick().await;
                let manager = app.state::<Self>();
                let mut state = manager.state.lock().unwrap();
                if state.status.session_id != id {
                    return;
                }
                let Some(session) = &state.recording else {
                    return;
                };
                let error = session.error();
                let draft = session.draft();
                let timed_out = started.elapsed().as_secs_f64() >= f64::from(max_seconds);
                if error.is_some() || timed_out {
                    state.recording = None;
                    state.task = None;
                    state.status.fail(
                        if timed_out {
                            "recording_timeout"
                        } else {
                            "recognition_failed"
                        },
                        error,
                    );
                    state.status.revision += 1;
                    manager.publish(&state.status);
                    return;
                }
                if state.status.question != draft {
                    state.status.question = draft;
                    state.status.revision += 1;
                    manager.publish(&state.status);
                }
            }
        }));
        Ok(())
    }

    pub fn finish_recording(&self) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if state.status.state != "recording" {
            return Err("desktop_vision.busy".into());
        }
        let mut session = state
            .recording
            .take()
            .ok_or("desktop_vision.recognition_failed")?;
        session.release();
        if let Some(task) = state.task.take() {
            task.abort();
        }
        let id = state.status.session_id;
        state.status.state = "finalizing_asr";
        state.status.revision += 1;
        self.publish(&state.status);
        let app = self.app.clone();
        state.task = Some(tauri::async_runtime::spawn(async move {
            let result = session.finalize().await;
            app.state::<Self>().update(id, |state| {
                state.task = None;
                match result {
                    Ok(question) => {
                        state.status.question = question;
                        state.status.state = "editing";
                    }
                    Err(detail) => state.status.fail("recognition_failed", Some(detail)),
                }
            });
        }));
        Ok(())
    }
}

fn shortcuts_match(left: &str, right: &str) -> bool {
    crate::desktop_capture::shortcut::parse_shortcut(left)
        .is_ok_and(|left| crate::desktop_capture::shortcut::parse_shortcut(right) == Ok(left))
}

#[cfg(windows)]
fn capture_error(error: String) -> String {
    error.replace("desktop_ocr.", "desktop_vision.")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Native Tauri windows require Common Controls v6; library tests have no app resources.
    #[cfg(all(windows, target_env = "msvc"))]
    #[used]
    #[link_section = ".drectve"]
    static COMMON_CONTROLS: [u8; 168] = *b" /MANIFESTDEPENDENCY:\"type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'\" ";

    fn selected() -> State {
        State {
            image: Some(Arc::new(vec![1, 2, 3])),
            status: DesktopVisionStatus {
                available: true,
                state: "editing",
                session_id: 1,
                image_id: Some(1),
                question: "draft".into(),
                answer: "answer".into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn cancel_invalidates_work_but_preserves_the_selected_image_and_question() {
        let mut state = selected();
        state.status.state = "requesting";
        state.cancel();
        assert_eq!(state.status.session_id, 2);
        assert_eq!(state.status.state, "editing");
        assert_eq!(state.selected_image(1).unwrap().as_ref(), &[1, 2, 3]);
        assert_eq!(state.status.question, "draft");
        assert!(state.selection_cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn cancelled_work_cannot_replace_current_content() {
        let mut state = selected();
        let id = state.status.session_id;
        state.cancel();
        assert!(!state.update(id, |state| {
            state.status.answer = "late answer".into();
            state.status.state = "complete";
        }));
        assert_eq!(state.status.answer, "answer");
        assert_eq!(state.status.state, "editing");
    }

    #[test]
    fn close_clears_sensitive_content_and_rejects_old_image_ids() {
        let mut state = selected();

        state.clear();
        assert!(state.image.is_none());

        assert!(state.status.image_id.is_none());
        assert!(state.status.question.is_empty());
        assert!(state.status.answer.is_empty());
        assert!(state.selected_image(1).is_err());
    }

    #[test]
    fn busy_phases_cannot_be_reentered() {
        for phase in [
            "capturing",
            "selecting",
            "recording",
            "finalizing_asr",
            "requesting",
        ] {
            let status = DesktopVisionStatus {
                state: phase,
                ..Default::default()
            };
            assert!(status.busy());
        }
        assert!(!selected().status.busy());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Requires an interactive Windows desktop and WebView2"]
    fn native_question_window_opens_reopens_and_respects_selection() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut context = tauri::generate_context!();
        context.config_mut().identifier = "app.vrcs.desktop.window-test".into();
        context.config_mut().app.windows.clear();
        context.config_mut().build.dev_url = None;
        let app = tauri::Builder::default()
            .any_thread()
            .plugin(tauri_plugin_store::Builder::default().build())
            .invoke_handler(tauri::generate_handler![
                crate::desktop_vision::desktop_vision_status,
                crate::desktop_vision::desktop_vision_close,
            ])
            .setup(move |app| {
                app.manage(Manager::new(app.handle().clone()));
                let app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let result: Result<(), String> = async {
                        let manager = app.state::<Manager>();
                        let (first, second) = tokio::join!(manager.open(), manager.open());
                        first?;
                        second?;
                        let window = app.get_webview_window("vision").ok_or("Missing window")?;
                        if !window.is_visible().map_err(|error| error.to_string())? {
                            return Err("The first open did not show the window".into());
                        }
                        let stale = manager.status().session_id;
                        manager.close();
                        manager.show_window(stale).await?;
                        if window.is_visible().map_err(|error| error.to_string())? {
                            return Err("A cancelled open showed the closed window".into());
                        }
                        let picker = crate::desktop_capture::reserve_picker()?;
                        if manager.open().await != Err("desktop_vision.selection_busy".into()) {
                            return Err("Opening did not report the active selection".into());
                        }
                        if window.is_visible().map_err(|error| error.to_string())? {
                            return Err("Opening during selection showed the window".into());
                        }
                        drop(picker);
                        manager.open().await?;
                        if !window.is_visible().map_err(|error| error.to_string())? {
                            return Err("Reopening did not show the window".into());
                        }
                        let _picker = crate::desktop_capture::reserve_picker()?;
                        Ok(())
                    }
                    .await;
                    let _ = sender.send(result);
                    app.exit(0);
                });
                Ok(())
            })
            .build(context)
            .unwrap();
        assert_eq!(app.run_return(|_, _| {}), 0);
        receiver.recv().unwrap().unwrap();
    }

    #[test]
    fn shortcut_conflicts_use_key_identity_not_spelling() {
        assert!(shortcuts_match("Control + Alt + v", "Alt+Ctrl+V"));
        assert!(!shortcuts_match("Ctrl+Alt+V", "Ctrl+Alt+O"));
        assert!(!shortcuts_match("invalid", "invalid"));
    }
}
