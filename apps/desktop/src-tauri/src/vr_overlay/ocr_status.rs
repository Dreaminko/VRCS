use serde::Serialize;

pub fn failure_code(error: &str) -> &'static str {
    if error.contains("timed out") {
        "timeout"
    } else if error.contains("authentication") {
        "authentication"
    } else if error.contains("token") {
        "credentials_missing"
    } else if error.contains("rate limit") {
        "rate_limit"
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
    } else if error.contains("OCR result")
        || error.contains("JSONL")
        || error.contains("structured")
    {
        "invalid_result"
    } else {
        "unavailable"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrState {
    Disabled,
    WaitingVr,
    Ready,
    Unbound,
    Selecting,
    Capturing,
    WaitingHands,
    Submitting,
    Pending,
    Running,
    Downloading,
    LoadingModel,
    Recognizing,
    Translating,
    Recognized,
    NoText,
    LowConfidence,
    SourceVisible,
    PartialVisible,
    TranslationFailed,
    TimedOut,
    Visible,
    Invalid,
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
        }
    }
}
