use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::Json;
use serde_json::{json, Value};

use crate::config::{save_config, AppConfig, ALIBABA_PROVIDER, OPENAI_PROVIDER};
use crate::models::SettingsUpdate;
use crate::{asr, audio};

use super::{api_error, ApiResult, AppState, CONFIG_REVISION_HEADER};

pub(super) async fn get_settings(State(state): State<Arc<AppState>>) -> (HeaderMap, Json<Value>) {
    let _config_control = state.config_control.lock().await;
    let config = state.config.read().expect("config lock").clone();
    let revision = state.config_revision.load(Ordering::SeqCst);
    (revision_headers(&state, revision), Json(json!(config)))
}

pub(super) async fn update_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> ApiResult<(HeaderMap, Json<Value>)> {
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
        translation: update.translation,
        osc: update.osc,
        dictionary: update.dictionary,
        anki: update.anki,
    };

    let expected_revision = headers
        .get(CONFIG_REVISION_HEADER)
        .map(|value| {
            value
                .to_str()
                .map(str::to_owned)
                .map_err(|_| "The configuration revision header is invalid".to_string())
        })
        .transpose()
        .map_err(unprocessable)?;
    let _config_control = state.config_control.lock().await;
    let current_revision = state.config_revision.load(Ordering::SeqCst);
    let current_revision_token = revision_token(&state, current_revision);
    if expected_revision.is_some_and(|revision| revision != current_revision_token) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "settings.stale",
            "Settings changed since they were loaded; reload and try again",
        ));
    }
    let _control = state.capture_control.lock().await;
    let current = state.config.read().expect("config lock").clone();
    // The profile catalog has its own mutation endpoints. A full settings payload can
    // be stale, so it must not recreate profiles changed or deleted elsewhere.
    candidate.asr.api_profiles = current.asr.api_profiles.clone();
    if candidate
        .asr
        .active_api_profiles
        .alibaba_cloud
        .as_deref()
        .is_some_and(|profile_id| {
            !current
                .asr
                .api_profiles
                .iter()
                .any(|profile| profile.id == profile_id && profile.provider == ALIBABA_PROVIDER)
        })
    {
        candidate.asr.active_api_profiles.alibaba_cloud =
            current.asr.active_api_profiles.alibaba_cloud.clone();
    }
    if candidate
        .asr
        .active_api_profiles
        .openai
        .as_deref()
        .is_some_and(|profile_id| {
            !current
                .asr
                .api_profiles
                .iter()
                .any(|profile| profile.id == profile_id && profile.provider == OPENAI_PROVIDER)
        })
    {
        candidate.asr.active_api_profiles.openai = current.asr.active_api_profiles.openai.clone();
    }
    let model_directory_changed =
        candidate.storage.model_directory != current.storage.model_directory;
    let capture_running = state.speaker_pipeline.lock().await.running()
        || state.microphone_pipeline.lock().await.running();
    if capture_running
        && (candidate.audio != current.audio
            || candidate.vad != current.vad
            || candidate.asr != current.asr
            || candidate.translation != current.translation
            || model_directory_changed)
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            "settings.capture_must_be_stopped",
            "Stop transcription before changing audio, segmentation, recognition, translation, or model storage settings",
        ));
    }
    if candidate.server != current.server {
        return Err(unprocessable(
            "The Core address is a startup setting and cannot be changed at runtime".into(),
        ));
    }
    if candidate.storage.database_path != current.storage.database_path
        || candidate.storage.subtitle_history_limit != current.storage.subtitle_history_limit
    {
        return Err(unprocessable(
            "The database path and subtitle history limit cannot be changed at runtime".into(),
        ));
    }
    if model_directory_changed && state.asr_model_dir_override.is_some() {
        return Err(unprocessable(
            "VRCS_ASR_MODEL_DIR overrides the model storage path; remove it before changing this setting".into(),
        ));
    }
    if candidate.audio.sample_rate != current.audio.sample_rate {
        return Err(unprocessable(
            "The sample rate cannot be changed at runtime".into(),
        ));
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
            format!("Audio device validation task failed: {error}"),
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
                    format!("Model directory migration task failed: {error}"),
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
                    format!("Settings could not be saved and the previous model directory could not be restored: {error}"),
                ));
            }
        }
        return Err(unprocessable(error));
    }
    *state.config.write().expect("config lock") = candidate.clone();
    let revision = state.config_revision.fetch_add(1, Ordering::SeqCst) + 1;
    state.osc.update_config(candidate.osc.clone());
    let asr = Arc::clone(&state.asr);
    let asr_config = candidate.asr.clone();
    tokio::task::spawn_blocking(move || {
        asr.lock()
            .map_err(|_| "The ASR inference lock is unavailable".to_string())?
            .update(asr_config, candidate_model_dir);
        Ok::<_, String>(())
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "settings.asr_update_task_failed",
            format!("ASR configuration update task failed: {error}"),
        )
    })?
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "settings.asr_update_failed",
            error,
        )
    })?;
    Ok((revision_headers(&state, revision), Json(json!(candidate))))
}

fn revision_token(state: &AppState, revision: u64) -> String {
    format!("{}:{revision}", state.config_epoch)
}

fn revision_headers(state: &AppState, revision: u64) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        CONFIG_REVISION_HEADER,
        HeaderValue::from_str(&revision_token(state, revision)).expect("revision header"),
    );
    headers
}

pub(super) fn parse_settings_update(body: &[u8]) -> Result<SettingsUpdate, String> {
    let mut ignored = Vec::new();
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let update = serde_ignored::deserialize(&mut deserializer, |path| {
        ignored.push(path.to_string());
    })
    .map_err(|error| format!("Invalid settings payload: {error}"))?;
    if let Some(path) = ignored.first() {
        return Err(format!(
            "Settings payload contains an unknown field: {path}"
        ));
    }
    Ok(update)
}
