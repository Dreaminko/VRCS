#[cfg(windows)]
pub mod hotkey;
mod selection;
pub mod image;
#[cfg(target_os = "linux")]
pub mod portal;
#[cfg(target_os = "linux")]
pub mod picker;
#[cfg(target_os = "linux")]
pub mod portal_shortcuts;
#[cfg(windows)]
pub mod selection_window;
pub mod shortcut;

#[derive(Default)]
pub struct ShortcutRecording {
    active: std::sync::atomic::AtomicBool,
    transition: std::sync::Mutex<()>,
}

impl ShortcutRecording {
    pub fn is_active(&self) -> bool {
        self.active.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn set(&self, app: &tauri::AppHandle, recording: bool) -> Result<(), String> {
        let _transition = self.transition.lock().unwrap();
        if self.is_active() == recording {
            return Ok(());
        }
        self.active
            .store(recording, std::sync::atomic::Ordering::Release);
        #[cfg(windows)]
        {
            use tauri::Manager as _;
            // Attempt both workers even if one cannot acknowledge the transition.
            let ocr = app.state::<crate::desktop_ocr::Manager>();
            let vision = app.state::<crate::desktop_vision::Manager>();
            let ocr_result = ocr.set_hotkey_paused(recording);
            let vision_result = vision.set_hotkey_paused(recording);
            if let Err(error) = ocr_result.and(vision_result) {
                self.active
                    .store(false, std::sync::atomic::Ordering::Release);
                let _ = ocr.set_hotkey_paused(false);
                let _ = vision.set_hotkey_paused(false);
                return Err(error);
            }
        }
        #[cfg(not(windows))]
        let _ = app;
        Ok(())
    }
}

#[tauri::command]
pub fn set_shortcut_recording(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, ShortcutRecording>,
    recording: bool,
) -> Result<(), String> {
    use tauri::Manager as _;
    if window.label() != "main" {
        return Err("Unsupported shortcut recording window".into());
    }
    // Reject a late start request after native focus-loss cleanup has run.
    if recording && !window.is_focused().map_err(|error| error.to_string())? {
        return Err("Shortcut recording requires the main window to be focused".into());
    }
    state.set(window.app_handle(), recording)
}

pub fn finish_shortcut_recording(app: &tauri::AppHandle) {
    use tauri::Manager as _;
    if let Some(state) = app.try_state::<ShortcutRecording>() {
        if let Err(error) = state.set(app, false) {
            tracing::warn!(%error, "Desktop shortcut recording cleanup failed");
        }
    }
}

#[cfg(any(windows, target_os = "linux"))]
static PICKER_RESERVED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(any(windows, target_os = "linux"))]
pub struct PickerPermit {
    _private: (),
}

#[cfg(any(windows, target_os = "linux"))]
pub fn reserve_picker() -> Result<PickerPermit, &'static str> {
    use std::sync::atomic::Ordering;
    PICKER_RESERVED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| PickerPermit { _private: () })
        .map_err(|_| "selection_busy")
}

#[cfg(any(windows, target_os = "linux"))]
impl Drop for PickerPermit {
    fn drop(&mut self) {
        PICKER_RESERVED.store(false, std::sync::atomic::Ordering::Release);
    }
}

#[cfg(all(windows, test))]
mod tests {
    #[test]
    fn another_picker_or_focusing_window_cannot_take_the_reservation() {
        let permit = super::reserve_picker().unwrap();
        assert!(super::reserve_picker().is_err());
        drop(permit);
        assert!(super::reserve_picker().is_ok());
    }
}

#[cfg(windows)]
pub fn selection_hint(app: &tauri::AppHandle, window: &str) -> String {
    use tauri_plugin_store::StoreExt;
    let locale = app
        .store("preferences.json")
        .ok()
        .and_then(|store| store.get("uiLanguage"))
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|value| value != "system")
        .unwrap_or_else(|| {
            match unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() } {
                0x0404 | 0x0c04 | 0x1404 => "zh-Hant",
                0x0804 | 0x1004 => "zh-CN",
                0x0411 => "ja-JP",
                _ => "en-US",
            }
            .into()
        });
    let messages = match locale.as_str() {
        "zh-CN" => include_str!("../../../src/i18n/locales/zh-CN.json"),
        "zh-Hant" => include_str!("../../../src/i18n/locales/zh-Hant.json"),
        "ja-JP" => include_str!("../../../src/i18n/locales/ja-JP.json"),
        _ => include_str!("../../../src/i18n/locales/en-US.json"),
    };
    serde_json::from_str::<serde_json::Value>(messages)
        .ok()
        .and_then(|value| {
            value
                .pointer(&format!("/translation/{window}/selectionHint"))
                .and_then(|text| text.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "Drag to select an area. Esc / right-click to cancel.".into())
}
