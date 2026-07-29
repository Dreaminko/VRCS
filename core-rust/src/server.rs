//! HTTP/WebSocket 服务层，端点与 JSON 形状对齐 Python 版 `app/main.py`。
//! 数据面、音频识别管线与管理端点。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{DefaultBodyLimit, Query, State, WebSocketUpgrade};
use axum::http::{header, Method, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use tower_http::cors::CorsLayer;

use crate::config::{save_config, AppConfig};
use crate::db::Database;
use crate::models::{CardRequest, SettingsUpdate, Subtitle};
use crate::pipeline::TranscriptionPipeline;
use crate::{anki, asr, audio, vad, yomitan};

pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct AppState {
    pub config_path: PathBuf,
    pub asr_model_dir_override: Option<PathBuf>,
    pub config: RwLock<AppConfig>,
    pub db: Arc<Mutex<Database>>,
    pub subtitles_tx: broadcast::Sender<Subtitle>,
    pub http: reqwest::Client,
    pub session_token: Option<String>,
    pub vad_runtime: vad::VadRuntimeState,
    pub asr: Arc<Mutex<asr::AsrService>>,
    pub asr_runtime: asr::AsrRuntimeState,
    pub model_manager: Arc<asr::ModelManager>,
    pub capture_control: AsyncMutex<()>,
    pub speaker_pipeline: AsyncMutex<TranscriptionPipeline>,
    pub microphone_pipeline: AsyncMutex<TranscriptionPipeline>,
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
    if let Some(token) = &state.session_token {
        // 浏览器 WebSocket 不能自定义 Authorization 头；/ws 在处理器中校验 query token。
        if request.method() != Method::OPTIONS && request.uri().path() != "/ws" {
            let supplied = request
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("");
            let expected = format!("Bearer {token}");
            if !token_eq(supplied, &expected) {
                return api_error(
                    StatusCode::UNAUTHORIZED,
                    "auth.unauthorized",
                    "Unauthorized",
                )
                .into_response();
            }
        }
    }
    next.run(request).await
}

pub fn router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin([
            "http://tauri.localhost".parse().unwrap(),
            "https://tauri.localhost".parse().unwrap(),
            "tauri://localhost".parse().unwrap(),
            "http://localhost:1420".parse().unwrap(),
        ])
        .allow_methods(tower_http::cors::Any)
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    Router::new()
        .route("/health", get(health))
        .route("/api/audio/devices", get(audio_devices))
        .route("/api/capture/start", post(capture_start))
        .route("/api/capture/stop", post(capture_stop))
        .route("/api/subtitles", get(subtitle_history))
        .route("/api/settings", get(get_settings).put(update_settings))
        .route("/api/asr/capabilities", get(asr_capabilities))
        .route("/api/asr/models", get(asr_models))
        .route("/api/asr/models/{model}/download", post(asr_model_download))
        .route("/api/asr/models/{model}", delete(asr_model_delete))
        .route("/api/dictionary", get(dictionary_lookup))
        .route("/api/dictionaries", get(dictionary_list))
        .route(
            "/api/dictionaries/import",
            post(dictionary_import).layer(DefaultBodyLimit::max(yomitan::MAX_ARCHIVE_BYTES)),
        )
        .route("/api/dictionaries/{source_id}", delete(dictionary_delete))
        .route("/api/anki/status", get(anki_status))
        .route("/api/anki/cards", post(anki_add_card))
        .route("/ws", get(ws_handler))
        .layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .layer(cors)
        .with_state(state)
}

async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    let config_schema = state.config.read().expect("config lock").schema_version;
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
    let last_error = speaker_error.or(microphone_error).or(asr_error);
    Json(json!({
        "status": "ok",
        "service": "vrcs-core",
        "version": CORE_VERSION,
        "config_schema": config_schema,
        "capture_running": speaker_running || microphone_running,
        "audio_device": audio_device,
        "microphone_device": microphone_device,
        "asr_status": asr_status,
        "vad_backend": vad_backend,
        "vad_model_version": vad_model_version,
        "last_error": last_error,
    }))
}

async fn audio_devices() -> ApiResult<Json<Value>> {
    let devices = audio::list_devices().map_err(|error| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            error.code(),
            error.to_string(),
        )
    })?;
    Ok(Json(json!(devices)))
}

async fn capture_start(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let _control = state.capture_control.lock().await;
    let config = state.config.read().expect("config lock").clone();
    if config.audio.sample_rate != 16_000 {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "capture.invalid_sample_rate",
            "Rust ASR 管线要求 16000 Hz 采样率",
        ));
    }
    if !state
        .model_manager
        .is_downloaded(&config.asr.model)
        .map_err(|error| {
            api_error_with_params(
                StatusCode::UNPROCESSABLE_ENTITY,
                "asr.model.inspect_failed",
                json!({ "model": config.asr.model }),
                error,
            )
        })?
    {
        return Err(api_error_with_params(
            StatusCode::CONFLICT,
            "asr.model.not_downloaded",
            json!({ "model": config.asr.model }),
            format!("识别模型 {} 尚未下载", config.asr.model),
        ));
    }
    if state.speaker_pipeline.lock().await.running()
        || state.microphone_pipeline.lock().await.running()
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            "capture.already_running",
            "Transcription is already running",
        ));
    }

    let output = &config.audio.output;
    let output_device_id = (output.mode == "system")
        .then_some(output.device_id)
        .flatten();
    let process_name = (output.mode == "vrchat").then_some("VRChat.exe");
    let device = state
        .speaker_pipeline
        .lock()
        .await
        .start(
            config.audio.sample_rate,
            output_device_id,
            process_name,
            &config.vad,
            Arc::clone(&state.asr),
            Arc::clone(&state.db),
            state.subtitles_tx.clone(),
            config.storage.subtitle_history_limit,
        )
        .map_err(|error| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                error.code(),
                error.to_string(),
            )
        })?;

    let microphone = if config.audio.microphone.mode == "disabled" {
        None
    } else {
        let microphone_id = (config.audio.microphone.mode == "device")
            .then_some(config.audio.microphone.device_id)
            .flatten();
        match state.microphone_pipeline.lock().await.start(
            config.audio.sample_rate,
            microphone_id,
            None,
            &config.vad,
            Arc::clone(&state.asr),
            Arc::clone(&state.db),
            state.subtitles_tx.clone(),
            config.storage.subtitle_history_limit,
        ) {
            Ok(device) => Some(device),
            Err(error) => {
                state.speaker_pipeline.lock().await.stop().await;
                return Err(api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    error.code(),
                    error.to_string(),
                ));
            }
        }
    };
    Ok(Json(json!({
        "running": true,
        "device": device,
        "microphone_device": microphone,
    })))
}

async fn capture_stop(State(state): State<Arc<AppState>>) -> Json<Value> {
    let _control = state.capture_control.lock().await;
    state.speaker_pipeline.lock().await.stop().await;
    state.microphone_pipeline.lock().await.stop().await;
    Json(json!({ "running": false }))
}

async fn asr_capabilities(State(state): State<Arc<AppState>>) -> Json<Value> {
    let cuda = asr::cuda_capability();
    let active_model = state.config.read().expect("config lock").asr.model.clone();
    let (runtime_status, _) = state.asr_runtime.snapshot();
    let models = state
        .model_manager
        .list(&active_model, runtime_status)
        .into_iter()
        .map(|model| {
            let status = match model.status.as_str() {
                "downloaded" | "loading" | "ready" | "error" => model.status,
                _ => "not_downloaded".into(),
            };
            json!({
                "id": model.id,
                "repository": model.repository,
                "status": status,
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "runtime_available": true,
        "cuda": cuda,
        "compute_types": {
            "auto": ["int8"],
            "cpu": ["int8"],
            "cuda": if cuda.available { vec!["int8"] } else { vec![] },
        },
        "models": models,
    }))
}

async fn asr_models(State(state): State<Arc<AppState>>) -> Json<Value> {
    let active_model = state.config.read().expect("config lock").asr.model.clone();
    let (runtime_status, _) = state.asr_runtime.snapshot();
    Json(json!(state
        .model_manager
        .list(&active_model, runtime_status)))
}

async fn asr_model_download(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(model): axum::extract::Path<String>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if !asr::is_supported_model(&model) {
        return Err(api_error_with_params(
            StatusCode::NOT_FOUND,
            "asr.model.unsupported",
            json!({ "model": model }),
            format!("不支持的识别模型：{model}"),
        ));
    }
    state
        .model_manager
        .start_download(&model)
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.model.download_start_failed",
                json!({ "model": model }),
                error,
            )
        })?;
    let active_model = state.config.read().expect("config lock").asr.model.clone();
    let (runtime_status, _) = state.asr_runtime.snapshot();
    let record = state
        .model_manager
        .describe(&model, &active_model, runtime_status)
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.model.describe_failed",
                json!({ "model": model }),
                error,
            )
        })?;
    Ok((StatusCode::ACCEPTED, Json(json!(record))))
}

async fn asr_model_delete(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(model): axum::extract::Path<String>,
) -> ApiResult<Json<Value>> {
    let active_model = state.config.read().expect("config lock").asr.model.clone();
    if !asr::is_supported_model(&model) {
        return Err(api_error_with_params(
            StatusCode::NOT_FOUND,
            "asr.model.unsupported",
            json!({ "model": model }),
            format!("不支持的识别模型：{model}"),
        ));
    }
    if model == active_model {
        return Err(api_error_with_params(
            StatusCode::CONFLICT,
            "asr.model.in_use",
            json!({ "model": model }),
            "当前正在使用该模型，请先选择其他模型",
        ));
    }
    state
        .model_manager
        .delete(&model, &active_model)
        .await
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.model.delete_failed",
                json!({ "model": model }),
                error,
            )
        })?;
    Ok(Json(json!({ "deleted": true })))
}

async fn subtitle_history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let limit = match params.get("limit") {
        None => 500,
        Some(raw) => raw
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=500).contains(value))
            .ok_or_else(|| {
                api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "subtitles.invalid_limit",
                    "limit 必须在 1 到 500 之间",
                )
            })?,
    };
    let history_limit = state
        .config
        .read()
        .expect("config lock")
        .storage
        .subtitle_history_limit;
    let db = state.db.lock().expect("db lock");
    let subtitles = db
        .subtitle_history(limit.min(history_limit))
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "subtitles.history_failed",
                error,
            )
        })?;
    Ok(Json(json!(subtitles)))
}

async fn get_settings(State(state): State<Arc<AppState>>) -> Json<Value> {
    let config = state.config.read().expect("config lock");
    Json(json!(*config))
}

async fn update_settings(
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    let unprocessable =
        |detail: String| api_error(StatusCode::UNPROCESSABLE_ENTITY, "settings.invalid", detail);
    let update = parse_settings_update(&body).map_err(unprocessable)?;
    if update.schema_version != crate::config::SCHEMA_VERSION {
        return Err(unprocessable(format!(
            "Expected configuration schema v{}",
            crate::config::SCHEMA_VERSION
        )));
    }
    let candidate = AppConfig {
        schema_version: update.schema_version,
        server: update.server,
        storage: update.storage,
        audio: update.audio,
        vad: update.vad,
        asr: update.asr,
        anki: update.anki,
    };

    let _control = state.capture_control.lock().await;
    let current = state.config.read().expect("config lock").clone();
    let model_directory_changed =
        candidate.storage.model_directory != current.storage.model_directory;
    let capture_running = state.speaker_pipeline.lock().await.running()
        || state.microphone_pipeline.lock().await.running();
    if capture_running
        && (candidate.audio != current.audio
            || candidate.vad != current.vad
            || candidate.asr != current.asr
            || model_directory_changed)
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            "settings.capture_must_be_stopped",
            "请先停止转写，再修改音频、断句、识别或模型保存位置",
        ));
    }
    if candidate.server != current.server {
        return Err(unprocessable(
            "Core 地址属于启动配置，不能在运行中修改".into(),
        ));
    }
    if candidate.storage.database_path != current.storage.database_path
        || candidate.storage.subtitle_history_limit != current.storage.subtitle_history_limit
    {
        return Err(unprocessable(
            "数据库路径和字幕保留上限不能在运行中修改".into(),
        ));
    }
    if model_directory_changed && state.asr_model_dir_override.is_some() {
        return Err(unprocessable(
            "VRCS_ASR_MODEL_DIR 正在覆盖模型保存位置，请先移除该环境变量".into(),
        ));
    }
    if candidate.audio.sample_rate != current.audio.sample_rate {
        return Err(unprocessable("采样率不能在运行中修改".into()));
    }
    candidate.validate_settings().map_err(unprocessable)?;
    asr::validate_config(&candidate.asr).map_err(unprocessable)?;
    // 设备有效性校验（对应 Python 版 validate_device_id 调用）
    if candidate.audio.output.mode == "system" {
        if let Some(device_id) = candidate.audio.output.device_id {
            audio::validate_device_id(device_id, audio::CaptureSource::Speaker)
                .map_err(|e| unprocessable(e.to_string()))?;
        }
    }
    if candidate.audio.microphone.mode == "device" {
        if let Some(device_id) = candidate.audio.microphone.device_id {
            audio::validate_device_id(device_id, audio::CaptureSource::Microphone)
                .map_err(|e| unprocessable(e.to_string()))?;
        }
    }
    let candidate_model_dir = state.asr_model_dir_override.clone().unwrap_or_else(|| {
        crate::resolve_config_path(&state.config_path, &candidate.storage.model_directory)
    });
    let previous_model_dir = state.model_manager.model_dir();
    if model_directory_changed {
        let manager = Arc::clone(&state.model_manager);
        let model_dir = candidate_model_dir.clone();
        tokio::task::spawn_blocking(move || manager.move_model_dir(model_dir))
            .await
            .map_err(|error| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "settings.model_directory_migration_failed",
                    format!("模型目录迁移任务失败：{error}"),
                )
            })?
            .map_err(unprocessable)?;
    }
    if let Err(error) = save_config(&state.config_path, &candidate) {
        if model_directory_changed {
            let manager = Arc::clone(&state.model_manager);
            let rollback =
                tokio::task::spawn_blocking(move || manager.move_model_dir(previous_model_dir))
                    .await;
            if !matches!(rollback, Ok(Ok(()))) {
                return Err(api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "settings.rollback_failed",
                    format!("设置保存失败且无法恢复原模型目录：{error}"),
                ));
            }
        }
        return Err(unprocessable(error));
    }
    *state.config.write().expect("config lock") = candidate.clone();
    let asr = Arc::clone(&state.asr);
    let asr_config = candidate.asr.clone();
    tokio::task::spawn_blocking(move || {
        asr.lock()
            .map_err(|_| "ASR 推理锁不可用".to_string())?
            .update(asr_config, candidate_model_dir);
        Ok::<_, String>(())
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "settings.asr_update_task_failed",
            format!("ASR 配置更新任务失败：{error}"),
        )
    })?
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "settings.asr_update_failed",
            error,
        )
    })?;
    Ok(Json(json!(candidate)))
}

async fn dictionary_lookup(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let query = params
        .get("q")
        .filter(|value| !value.is_empty() && value.chars().count() <= 100)
        .ok_or_else(|| {
            api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "dictionary.invalid_query",
                "q 必须在 1 到 100 字符之间",
            )
        })?;
    let db = state.db.lock().expect("db lock");
    let entries = db.lookup(query, 10).map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "dictionary.lookup_failed",
            error,
        )
    })?;
    Ok(Json(json!(entries)))
}

async fn dictionary_list(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let db = state.db.lock().expect("db lock");
    let sources = db.dictionary_sources().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "dictionary.list_failed",
            error,
        )
    })?;
    Ok(Json(json!(sources)))
}

async fn dictionary_import(
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    let db = Arc::clone(&state.db);
    let imported = tokio::task::spawn_blocking(move || {
        db.lock()
            .map_err(|_| "数据库锁不可用".to_string())?
            .import_yomitan(&body)
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "dictionary.import_task_failed",
            format!("词典导入任务失败：{error}"),
        )
    })?
    .map_err(|error| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "dictionary.import_invalid",
            error,
        )
    })?;
    Ok(Json(json!(imported)))
}

async fn dictionary_delete(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(source_id): axum::extract::Path<i64>,
) -> ApiResult<Json<Value>> {
    let db = state.db.lock().expect("db lock");
    let deleted = db.delete_dictionary_source(source_id).map_err(|error| {
        api_error_with_params(
            StatusCode::INTERNAL_SERVER_ERROR,
            "dictionary.delete_failed",
            json!({ "source_id": source_id }),
            error,
        )
    })?;
    if !deleted {
        return Err(api_error_with_params(
            StatusCode::NOT_FOUND,
            "dictionary.not_found",
            json!({ "source_id": source_id }),
            "词典不存在",
        ));
    }
    Ok(Json(json!({ "deleted": true })))
}

async fn anki_status(State(state): State<Arc<AppState>>) -> Json<Value> {
    let config = state.config.read().expect("config lock").anki.clone();
    Json(anki::status(&state.http, &config).await)
}

async fn anki_add_card(
    State(state): State<Arc<AppState>>,
    Json(card): Json<CardRequest>,
) -> ApiResult<Json<Value>> {
    card.validate()
        .map_err(|error| api_error(StatusCode::UNPROCESSABLE_ENTITY, "anki.card_invalid", error))?;
    let config = state.config.read().expect("config lock").anki.clone();
    let note_id = anki::create_card(&state.http, &card, &config)
        .await
        .map_err(|e| {
            api_error_with_params(
                StatusCode::from_u16(e.status_code).unwrap_or(StatusCode::BAD_GATEWAY),
                format!("anki.{}", e.code),
                e.params,
                e.message,
            )
        })?;
    Ok(Json(json!({ "note_id": note_id })))
}

fn parse_settings_update(body: &[u8]) -> Result<SettingsUpdate, String> {
    let mut ignored = Vec::new();
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let update = serde_ignored::deserialize(&mut deserializer, |path| {
        ignored.push(path.to_string());
    })
    .map_err(|error| format!("设置格式无效：{error}"))?;
    if let Some(path) = ignored.first() {
        return Err(format!("设置包含未知字段：{path}"));
    }
    Ok(update)
}

async fn ws_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    if let Some(token) = &state.session_token {
        let supplied = params.get("token").map(String::as_str).unwrap_or("");
        if !token_eq(supplied, token) {
            return api_error(
                StatusCode::UNAUTHORIZED,
                "auth.unauthorized",
                "Unauthorized",
            )
            .into_response();
        }
    }
    ws.on_upgrade(move |socket| handle_socket(state, socket))
}

async fn handle_socket(state: Arc<AppState>, mut socket: WebSocket) {
    let mut receiver = state.subtitles_tx.subscribe();
    if socket
        .send(Message::Text(r#"{"type":"connected"}"#.into()))
        .await
        .is_err()
    {
        return;
    }
    loop {
        match receiver.recv().await {
            Ok(subtitle) => {
                let payload = json!({ "type": "subtitle", "subtitle": subtitle }).to_string();
                if socket.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use serde_json::json;

    use super::{api_error_with_params, parse_settings_update};

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
            "识别模型 small 尚未下载",
        );

        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["code"], "asr.model.not_downloaded");
        assert_eq!(body["params"], json!({ "model": "small" }));
        assert_eq!(body["detail"], "识别模型 small 尚未下载");
    }
}
