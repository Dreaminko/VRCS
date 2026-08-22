use axum::http::StatusCode;

use crate::config::AppConfig;
use crate::{asr, audio, credentials};

use super::super::super::{api_error, ApiResult, SettingsContext};

pub(super) async fn validate_candidate(
    state: &SettingsContext,
    candidate: &mut AppConfig,
    current: &AppConfig,
) -> ApiResult<()> {
    let invalid = |detail| api_error(StatusCode::UNPROCESSABLE_ENTITY, "settings.invalid", detail);
    if candidate.server != current.server {
        return Err(invalid(
            "The Core address is a startup setting and cannot be changed at runtime".into(),
        ));
    }
    if candidate.storage.database_path != current.storage.database_path {
        return Err(invalid(
            "The database path cannot be changed at runtime".into(),
        ));
    }
    let model_directory_changed =
        candidate.storage.model_directory != current.storage.model_directory;
    if model_directory_changed && state.config.asr_model_dir_override.is_some() {
        return Err(invalid(
            "VRCS_ASR_MODEL_DIR overrides the model storage path; remove it before changing this setting"
                .into(),
        ));
    }
    if candidate.audio.sample_rate != current.audio.sample_rate {
        return Err(invalid(
            "The sample rate cannot be changed at runtime".into(),
        ));
    }
    candidate.validate_settings().map_err(invalid)?;
    if candidate.external_api.enabled && candidate.external_api.require_token {
        let token = credentials::read_external_api_token().map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "external_api.token_status_failed",
                error,
            )
        })?;
        if token.is_none() {
            return Err(api_error(
                StatusCode::CONFLICT,
                "external_api.token_required",
                "Save an External API token before enabling token authentication",
            ));
        }
    }
    asr::validate_config(&mut candidate.asr).map_err(invalid)?;
    if candidate.audio != current.audio {
        validate_audio_devices(candidate.audio.clone()).await?;
    }
    Ok(())
}

async fn validate_audio_devices(config: crate::config::AudioConfig) -> ApiResult<()> {
    // WASAPI enumeration can block, so it must not occupy a Tokio worker.
    tokio::task::spawn_blocking(move || {
        if config.output.mode == "system" {
            if let Some(device_id) = config.output.device_id {
                audio::validate_device_id(device_id, audio::CaptureSource::Speaker)?;
            }
        }
        if config.microphone.mode == "device" {
            if let Some(device_id) = config.microphone.device_id {
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
    .map_err(|error| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "settings.invalid",
            error.to_string(),
        )
    })
}
