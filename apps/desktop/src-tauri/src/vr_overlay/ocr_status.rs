use serde::Serialize;

#[cfg(any(windows, linux_vr))]
pub fn failure_code(error: &str) -> &'static str {
    if error.contains("timed out") {
        "timeout"
    } else if error.contains("models are missing") {
        "models_missing"
    } else if error.contains("configuration changed") {
        "configuration_changed"
    } else if error.contains("tracking") || error.contains("pose") {
        "tracking_lost"
    } else if error.contains("position")
        || error.contains("scene")
        || error.contains("view changed")
    {
        "view_changed"
    } else if error.contains("network") || error.contains("download") {
        "network"
    } else if error.contains("model")
        || error.contains("recognition failed")
        || error.contains("detection failed")
    {
        "local_failed"
    } else {
        "unavailable"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrState {
    Disabled,
    #[cfg(any(windows, linux_vr))]
    WaitingVr,
    #[cfg(any(windows, linux_vr))]
    Ready,
    #[cfg(any(windows, linux_vr))]
    Unbound,
    #[cfg(any(windows, linux_vr))]
    Selecting,
    #[cfg(any(windows, linux_vr))]
    Capturing,
    #[cfg(any(windows, linux_vr))]
    LoadingModel,
    #[cfg(any(windows, linux_vr))]
    Recognizing,
    #[cfg(any(windows, linux_vr))]
    Translating,
    #[cfg(any(windows, linux_vr))]
    Recognized,
    #[cfg(any(windows, linux_vr, test))]
    NoText,
    #[cfg(any(windows, linux_vr, test))]
    LowConfidence,
    #[cfg(any(windows, linux_vr, test))]
    SourceVisible,
    #[cfg(any(windows, linux_vr, test))]
    PartialVisible,
    #[cfg(any(windows, linux_vr, test))]
    TranslationFailed,
    #[cfg(any(windows, linux_vr, test))]
    TimedOut,
    #[cfg(any(windows, linux_vr, test))]
    Visible,
    #[cfg(any(windows, linux_vr, test))]
    Invalid,
    #[cfg(any(windows, linux_vr, test))]
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OcrStatus {
    pub state: OcrState,
    pub scan_id: u64,
    pub block_count: usize,
    pub controller_bound: bool,
    pub gesture_available: bool,
    pub layout_limited: bool,
    pub completed_translations: usize,
    pub failed_translations: usize,
    pub timed_out: bool,
    pub last_error_code: Option<String>,
    pub last_error: Option<String>,
    pub wrist_state: OcrWristState,
    pub wrist_error: Option<String>,
    pub progress: Option<super::ocr_progress::ProgressView>,
    pub progress_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrWristState {
    #[default]
    Hidden,
    #[cfg(any(windows, linux_vr))]
    Visible,
    #[cfg(any(windows, linux_vr))]
    DeviceUnavailable,
    #[cfg(any(windows, linux_vr))]
    Error,
}

impl Default for OcrStatus {
    fn default() -> Self {
        Self {
            state: OcrState::Disabled,
            scan_id: 0,
            block_count: 0,
            controller_bound: false,
            gesture_available: false,
            layout_limited: false,
            completed_translations: 0,
            failed_translations: 0,
            timed_out: false,
            last_error_code: None,
            last_error: None,
            wrist_state: OcrWristState::Hidden,
            wrist_error: None,
            progress: None,
            progress_error: None,
        }
    }
}
