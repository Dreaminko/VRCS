use std::backtrace::Backtrace;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

const MAX_ERROR_TEXT: usize = 4_000;
const MAX_STACK_TEXT: usize = 16_000;
const MAX_EXPORTED_LOG_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct DiagnosticState {
    log_dir: PathBuf,
    session_id: String,
    latest_report_id: Arc<Mutex<Option<String>>>,
}

impl DiagnosticState {
    pub(crate) fn new(log_dir: PathBuf) -> Self {
        Self {
            log_dir,
            session_id: Uuid::new_v4().simple().to_string(),
            latest_report_id: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiagnosticStatus {
    log_directory: String,
    session_id: String,
    latest_report_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FrontendErrorReport {
    kind: String,
    operation: String,
    message: String,
    stack: Option<String>,
    component_stack: Option<String>,
}

pub(crate) fn desktop_log_dir() -> PathBuf {
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("VRCS"))
        .join(".vrcs")
        .join("logs")
}

fn report_id() -> String {
    Uuid::new_v4().simple().to_string()[..12].to_string()
}

fn clean_identifier(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "unknown".into()
    } else {
        cleaned
    }
}

fn clean_log_text(value: &str, max_chars: usize) -> String {
    let lower = value.to_ascii_lowercase();
    if [
        "authorization",
        "bearer ",
        "api_key",
        "api key",
        "x-api-key",
        "x-goog-api-key",
        "token=",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return "[redacted potentially sensitive text]".into();
    }

    let mut cleaned = value
        .replace('\r', "")
        .replace('\n', " | ")
        .replace('\t', " ");
    for (variable, replacement) in [
        ("USERPROFILE", "%USERPROFILE%"),
        ("LOCALAPPDATA", "%LOCALAPPDATA%"),
    ] {
        if let Ok(prefix) = std::env::var(variable) {
            if !prefix.is_empty() {
                cleaned = cleaned.replace(&prefix, replacement);
            }
        }
    }

    let mut chars = cleaned.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn remember_report(state: &DiagnosticState, report_id: &str) {
    if let Ok(mut latest) = state.latest_report_id.lock() {
        *latest = Some(report_id.to_string());
    }
}

pub(crate) fn record_error(
    state: &DiagnosticState,
    component: &str,
    operation: &str,
    code: &str,
    error: &str,
    stack: Option<&str>,
) -> String {
    let report_id = report_id();
    remember_report(state, &report_id);
    let operation = clean_identifier(operation);
    let code = clean_identifier(code);
    let error = clean_log_text(error, MAX_ERROR_TEXT);
    let stack = stack.map(|value| clean_log_text(value, MAX_STACK_TEXT));
    tracing::error!(
        session_id = %state.session_id,
        report_id = %report_id,
        component,
        operation = %operation,
        code = %code,
        error = %error,
        stack = stack.as_deref().unwrap_or(""),
        "application error reported"
    );
    report_id
}

pub(crate) fn install_panic_hook(state: DiagnosticState) {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-string panic payload");
        let location = info
            .location()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_else(|| "unknown".into());
        let backtrace = Backtrace::force_capture().to_string();
        record_error(
            &state,
            "desktop",
            "panic",
            "desktop.panic",
            &format!("{message} at {location}"),
            Some(&backtrace),
        );
        original_hook(info);
    }));
}

#[tauri::command]
pub(crate) fn diagnostic_status(
    state: State<'_, DiagnosticState>,
) -> Result<DiagnosticStatus, String> {
    let latest_report_id = state
        .latest_report_id
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    Ok(DiagnosticStatus {
        log_directory: state.log_dir.display().to_string(),
        session_id: state.session_id.clone(),
        latest_report_id,
    })
}

#[tauri::command]
pub(crate) fn report_frontend_error(
    report: FrontendErrorReport,
    state: State<'_, DiagnosticState>,
) -> String {
    let kind = clean_identifier(&report.kind);
    let stack = [report.stack.as_deref(), report.component_stack.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" | component stack: ");
    record_error(
        state.inner(),
        "frontend",
        &report.operation,
        &format!("frontend.{kind}"),
        &report.message,
        (!stack.is_empty()).then_some(stack.as_str()),
    )
}

fn open_directory(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|error| error.to_string())?;

    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer.exe");
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");

    command
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Failed to open log directory: {error}"))
}

#[tauri::command]
pub(crate) fn open_log_directory(state: State<'_, DiagnosticState>) -> Result<(), String> {
    open_directory(&state.log_dir)
}

fn recent_log_files(log_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = std::fs::read_dir(log_dir)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    (name.starts_with("errorlog.") || name.starts_with("vrcs-core."))
                        && name.ends_with(".log")
                })
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.split_once('.'))
            .map(|(_, suffix)| suffix.to_string())
            .unwrap_or_default()
    });
    if files.len() > 3 {
        files.drain(..files.len() - 3);
    }
    files.reverse();
    Ok(files)
}

#[tauri::command]
pub(crate) fn export_error_report(
    path: PathBuf,
    state: State<'_, DiagnosticState>,
) -> Result<(), String> {
    let files = recent_log_files(&state.log_dir)?;
    let generated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs();
    let latest_report_id = state
        .latest_report_id
        .lock()
        .map_err(|error| error.to_string())?
        .clone()
        .unwrap_or_else(|| "none".into());
    let mut output = format!(
        "VRCS error report\nGenerated: {generated_at}\nVersion: {}\nSession: {}\nLatest report: {latest_report_id}\n\nThis report was generated locally and is not uploaded automatically.\n",
        env!("CARGO_PKG_VERSION"),
        state.session_id,
    );
    let mut remaining = MAX_EXPORTED_LOG_BYTES;
    for log_path in files {
        if remaining == 0 {
            break;
        }
        let bytes = std::fs::read(&log_path).map_err(|error| error.to_string())?;
        let start = bytes.len().saturating_sub(remaining);
        let text = String::from_utf8_lossy(&bytes[start..]);
        output.push_str("\n\n===== ");
        output.push_str(
            log_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("errorlog"),
        );
        output.push_str(" =====\n");
        for line in text.lines() {
            output.push_str(&clean_log_text(line, MAX_STACK_TEXT));
            output.push('\n');
        }
        remaining = remaining.saturating_sub(bytes.len() - start);
    }
    std::fs::write(path, output).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{clean_identifier, clean_log_text, recent_log_files};

    #[test]
    fn diagnostic_text_is_bounded_and_sensitive_values_are_rejected() {
        assert_eq!(clean_identifier("react render/error"), "reactrendererror");
        assert_eq!(clean_log_text("abcdef", 3), "abc…");
        assert_eq!(
            clean_log_text("Authorization: Bearer secret", 100),
            "[redacted potentially sensitive text]"
        );
    }

    #[test]
    fn diagnostic_export_selects_the_three_newest_logs() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "vrcs-diagnostic-test-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        for name in [
            "errorlog.2026-01-01.log",
            "errorlog.2026-01-02.log",
            "vrcs-core.2026-01-03.log",
            "errorlog.2026-01-04.log",
            "unrelated.log",
        ] {
            std::fs::write(directory.join(name), name).unwrap();
        }

        let names = recent_log_files(&directory)
            .unwrap()
            .into_iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "errorlog.2026-01-04.log",
                "vrcs-core.2026-01-03.log",
                "errorlog.2026-01-02.log",
            ]
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
