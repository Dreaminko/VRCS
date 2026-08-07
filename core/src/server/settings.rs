use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::config::{save_config, AppConfig};
use crate::models::SettingsUpdate;
use crate::{asr, audio};

use super::{api_error, ApiResult, AppState};

pub(super) async fn get_settings(State(state): State<Arc<AppState>>) -> Json<Value> {
    let config = state.config.read().expect("config lock");
    Json(json!(*config))
}

pub(super) async fn update_settings(
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
    let mut candidate = AppConfig {
        schema_version: update.schema_version,
        server: update.server,
        storage: update.storage,
        audio: update.audio,
        vad: update.vad,
        asr: update.asr,
        dictionary: update.dictionary,
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
    asr::validate_config(&mut candidate.asr).map_err(unprocessable)?;
    // WASAPI 枚举可能阻塞，不能占用 Tokio worker。
    let audio_config = candidate.audio.clone();
    tokio::task::spawn_blocking(move || {
        if audio_config.output.mode == "system" {
            if let Some(device_id) = audio_config.output.device_id {
                audio::validate_device_id(device_id, audio::CaptureSource::Speaker)?;
            }
        }
        if audio_config.microphone.mode == "device" {
            if let Some(device_id) = audio_config.microphone.device_id {
                audio::validate_device_id(device_id, audio::CaptureSource::Microphone)?;
            }
        }
        Ok::<_, audio::AudioError>(())
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "audio.device_validation_task_failed",
            format!("音频设备校验任务失败：{error}"),
        )
    })?
    .map_err(|error| unprocessable(error.to_string()))?;
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

pub(super) fn parse_settings_update(body: &[u8]) -> Result<SettingsUpdate, String> {
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
