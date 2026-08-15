//! HTTP/WebSocket 服务层，端点与 JSON 形状对齐 Python 版 `app/main.py`。
//! 数据面、音频识别管线与管理端点。

mod anki;
pub(crate) mod capture;
mod chatbox;
mod cloud;
mod dictionary;
mod external;
mod models;
mod osc;
mod provider_diagnostics;
mod settings;
mod translation;
mod ws;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, RwLock};

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, Method, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::sync::{broadcast, watch, Mutex as AsyncMutex};
use tower_http::cors::CorsLayer;

use crate::config::AppConfig;
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::microphone_monitor::MicrophoneMonitor;
use crate::models::LiveTranscription;
use crate::osc::OscChatboxDispatcher;
use crate::pipeline::TranscriptionPipeline;
use crate::subtitle_output::SubtitleLifecyclePublisher;
use crate::translation::{TranslationDispatcher, TranslationService};
use crate::{asr, vad, yomitan};

pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
const CONFIG_REVISION_HEADER: &str = "x-vrcs-config-revision";
const ALLOWED_ORIGINS: [&str; 4] = [
    "http://tauri.localhost",
    "https://tauri.localhost",
    "tauri://localhost",
    "http://localhost:1420",
];

pub struct AppState {
    pub config_path: PathBuf,
    pub asr_model_dir_override: Option<PathBuf>,
    pub config: RwLock<AppConfig>,
    pub db: Arc<Mutex<Database>>,
    pub live_tx: broadcast::Sender<LiveTranscription>,
    pub subtitle_output: SubtitleLifecyclePublisher,
    pub translation_service: Arc<TranslationService>,
    pub translation_dispatcher: TranslationDispatcher,
    pub glossary_subscription: Arc<crate::translation::GlossarySubscriptionStore>,
    pub osc: OscChatboxDispatcher,
    pub http: reqwest::Client,
    pub session_token: String,
    pub external_api_status: crate::external_api::ExternalApiRuntimeStatus,
    pub shutdown: watch::Receiver<bool>,
    pub vad_runtime: vad::VadRuntimeState,
    pub asr: Arc<Mutex<asr::AsrService>>,
    pub asr_runtime: asr::AsrRuntimeState,
    pub model_manager: Arc<asr::ModelManager>,
    pub config_epoch: String,
    pub config_revision: AtomicU64,
    pub config_control: AsyncMutex<()>,
    pub capture_control: AsyncMutex<()>,
    pub capture_requested: AtomicBool,
    pub speaker_pipeline: AsyncMutex<TranscriptionPipeline>,
    pub microphone_pipeline: AsyncMutex<TranscriptionPipeline>,
    pub microphone_monitor: AsyncMutex<MicrophoneMonitor>,
    pub vrchat_mute_sync: crate::vrchat_mute_sync::VrchatMuteSync,
}

type ApiResult<T> = Result<T, (StatusCode, Json<Value>)>;

fn api_error(
    status: StatusCode,
    code: impl Into<String>,
    detail: impl Into<String>,
) -> (StatusCode, Json<Value>) {
    api_error_with_params(status, code, json!({}), detail)
}

fn api_error_with_params(
    status: StatusCode,
    code: impl Into<String>,
    params: Value,
    detail: impl Into<String>,
) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({
            "code": code.into(),
            "params": params,
            "detail": detail.into(),
        })),
    )
}

fn api_domain_error(error: AppError, code: &'static str) -> (StatusCode, Json<Value>) {
    api_domain_error_with_params(error, code, json!({}))
}

fn api_domain_error_with_params(
    error: AppError,
    code: &'static str,
    params: Value,
) -> (StatusCode, Json<Value>) {
    let status = match &error {
        AppError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
        AppError::Conflict(_) => StatusCode::CONFLICT,
        AppError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        AppError::Storage(_) | AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    api_error_with_params(status, code, params, error.to_string())
}

fn dictionary_import_error(error: AppError) -> (StatusCode, Json<Value>) {
    let code = match &error {
        AppError::Validation(_) => "dictionary.import_invalid",
        AppError::Conflict(_) => "dictionary.import_conflict",
        AppError::Unavailable(_) => "dictionary.import_unavailable",
        AppError::Storage(_) => "dictionary.import_storage_failed",
        AppError::Internal(_) => "dictionary.import_failed",
    };
    api_domain_error(error, code)
}

async fn db_call<T, F>(db: Arc<Mutex<Database>>, operation: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Database) -> AppResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let mut database = db
            .lock()
            .map_err(|_| AppError::internal("Database lock is unavailable"))?;
        operation(&mut database)
    })
    .await
    .map_err(|error| AppError::internal(format!("Database task exited unexpectedly: {error}")))?
}

/// 简单的常时间比较，避免 token 比较的时序侧信道
fn token_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

async fn authenticate(
    State(state): State<Arc<AppState>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // 浏览器 WebSocket 不能自定义 Authorization 头；/ws 在处理器中校验 query token。
    if request.method() != Method::OPTIONS && request.uri().path() != "/ws" {
        let supplied = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let expected = format!("Bearer {}", state.session_token);
        if !token_eq(supplied, &expected) {
            return api_error(
                StatusCode::UNAUTHORIZED,
                "auth.unauthorized",
                "Unauthorized",
            )
            .into_response();
        }
    }
    next.run(request).await
}

pub fn router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(ALLOWED_ORIGINS.map(|origin| origin.parse().unwrap()))
        .allow_methods(tower_http::cors::Any)
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::HeaderName::from_static("x-vrcs-import-id"),
            header::HeaderName::from_static(CONFIG_REVISION_HEADER),
        ])
        .expose_headers([header::HeaderName::from_static(CONFIG_REVISION_HEADER)]);

    Router::new()
        .route("/health", get(health))
        .route("/api/audio/devices", get(capture::audio_devices))
        .route("/api/capture/start", post(capture::capture_start))
        .route("/api/capture/stop", post(capture::capture_stop))
        .route(
            "/api/audio/microphone-test/start",
            post(capture::microphone_test_start),
        )
        .route(
            "/api/audio/microphone-test/stop",
            post(capture::microphone_test_stop),
        )
        .route("/api/osc/test", post(osc::test_message))
        .route("/api/chatbox/preview", post(chatbox::preview))
        .route("/api/chatbox/messages", post(chatbox::send))
        .route("/api/subtitles", get(dictionary::subtitle_history))
        .route(
            "/api/translations/preview",
            post(translation::translation_preview),
        )
        .route(
            "/api/translations/prompt-preview",
            post(translation::prompt_preview),
        )
        .route(
            "/api/translations/glossaries/status",
            get(translation::glossary_statuses),
        )
        .route(
            "/api/translations/glossaries/{id}/refresh",
            post(translation::glossary_refresh),
        )
        .route(
            "/api/translations/glossary-subscription/status",
            get(translation::glossary_subscription_status),
        )
        .route(
            "/api/translations/glossary-subscription/refresh",
            post(translation::glossary_subscription_refresh),
        )
        .route(
            "/api/subtitles/{subtitle_id}/translation",
            post(translation::subtitle_translate),
        )
        .route(
            "/api/settings",
            get(settings::get_settings).put(settings::update_settings),
        )
        .route(
            "/api/external-api/token",
            get(external::token_status)
                .put(external::token_write)
                .delete(external::token_delete),
        )
        .route("/api/external-api/status", get(external::runtime_status))
        .route("/api/asr/capabilities", get(models::asr_capabilities))
        .route("/api/providers", get(cloud::provider_list))
        .route(
            "/api/asr/profiles",
            get(cloud::profile_list).post(cloud::profile_create),
        )
        .route(
            "/api/asr/profiles/{profile_id}",
            axum::routing::put(cloud::profile_update).delete(cloud::profile_delete),
        )
        .route(
            "/api/asr/profiles/{profile_id}/credential",
            axum::routing::put(cloud::credential_write).delete(cloud::credential_delete),
        )
        .route(
            "/api/asr/profiles/active/{provider}",
            axum::routing::put(cloud::profile_activate),
        )
        .route(
            "/api/asr/profiles/{profile_id}/test",
            post(provider_diagnostics::credential_test),
        )
        .route(
            "/api/asr/profiles/{profile_id}/models",
            get(cloud::profile_models),
        )
        .route("/api/asr/models", get(models::asr_models))
        .route(
            "/api/asr/models/{model}/download",
            post(models::asr_model_download),
        )
        .route("/api/asr/models/{model}", delete(models::asr_model_delete))
        .route("/api/dictionary", get(dictionary::dictionary_lookup))
        .route("/api/dictionaries", get(dictionary::dictionary_list))
        .route(
            "/api/dictionaries/import",
            post(dictionary::dictionary_import)
                .layer(DefaultBodyLimit::max(yomitan::MAX_ARCHIVE_BYTES)),
        )
        .route(
            "/api/dictionaries/import/{import_id}",
            get(dictionary::dictionary_import_progress),
        )
        .route(
            "/api/dictionaries/{source_id}",
            delete(dictionary::dictionary_delete),
        )
        .route("/api/anki/status", get(anki::anki_status))
        .route("/api/anki/cards", post(anki::anki_add_card))
        .route("/ws", get(ws::ws_handler))
        .layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .layer(cors)
        .with_state(state)
}

async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    let (config_schema, microphone_enabled) = {
        let config = state.config.read().expect("config lock");
        (
            config.schema_version,
            config.audio.microphone.mode != "disabled",
        )
    };
    let vad_backend = state.vad_runtime.backend();
    let vad_model_version = state.vad_runtime.model_version();
    let (asr_status, asr_error) = state.asr_runtime.snapshot();
    let (speaker_running, audio_device, speaker_error) = {
        let pipeline = state.speaker_pipeline.lock().await;
        (
            pipeline.running(),
            pipeline.device().cloned(),
            pipeline.last_error(),
        )
    };
    let (microphone_running, microphone_device, microphone_error) = {
        let pipeline = state.microphone_pipeline.lock().await;
        (
            pipeline.running(),
            pipeline.device().cloned(),
            pipeline.last_error(),
        )
    };
    let (microphone_test_running, microphone_test_device) = {
        let monitor = state.microphone_monitor.lock().await;
        (monitor.running(), monitor.device().cloned())
    };
    let last_error = speaker_error.or(microphone_error).or(asr_error);
    let osc = state.osc.status();
    let capture_requested = state
        .capture_requested
        .load(std::sync::atomic::Ordering::SeqCst);
    let vrchat_mute_sync = state.vrchat_mute_sync.status();
    Json(json!({
        "status": "ok",
        "service": "vrcs-core",
        "version": CORE_VERSION,
        "config_schema": config_schema,
        "capture_running": speaker_running || microphone_running,
        "capture_requested": capture_requested,
        "microphone_capture_state": if capture_requested && microphone_enabled
            && vrchat_mute_sync.muted == Some(true) {
            "paused_vrchat_muted"
        } else if microphone_running {
            "running"
        } else {
            "stopped"
        },
        "audio_device": audio_device,
        "microphone_device": microphone_device,
        "microphone_test_running": microphone_test_running,
        "microphone_test_device": microphone_test_device,
        "asr_status": asr_status,
        "vad_backend": vad_backend,
        "vad_model_version": vad_model_version,
        "last_error": last_error,
        "osc": osc,
        "vrchat_mute_sync": vrchat_mute_sync,
    }))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use serde_json::json;

    use super::{api_error_with_params, dictionary_import_error, settings::parse_settings_update};
    use crate::error::AppError;

    #[test]
    fn settings_reject_unknown_nested_fields() {
        let mut settings = serde_json::to_value(crate::config::AppConfig::default()).unwrap();
        settings["audio"]["unknown"] = serde_json::json!(true);
        let body = serde_json::to_vec(&settings).unwrap();

        assert!(parse_settings_update(&body)
            .err()
            .unwrap()
            .contains("audio.unknown"));
    }

    #[test]
    fn api_errors_include_stable_code_params_and_diagnostic_detail() {
        let (status, body) = api_error_with_params(
            StatusCode::CONFLICT,
            "asr.model.not_downloaded",
            json!({ "model": "small" }),
            "Recognition model small has not been downloaded",
        );

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["code"], "asr.model.not_downloaded");
        assert_eq!(body["params"], json!({ "model": "small" }));
        assert_eq!(
            body["detail"],
            "Recognition model small has not been downloaded"
        );
    }

    #[test]
    fn dictionary_storage_errors_are_not_reported_as_validation_errors() {
        let (status, body) = dictionary_import_error(AppError::Storage("disk full".into()));

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["code"], "dictionary.import_storage_failed");
    }
}
