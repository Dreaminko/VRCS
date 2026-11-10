use axum::http::StatusCode;

use crate::config::AppConfig;
use crate::{audio, credentials};

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
    candidate.resolve_model_assignments();
    candidate.clear_changed_inherited_reasoning(current);
    candidate.validate_settings().map_err(invalid)?;
    if candidate.features.external_api
        && candidate.external_api.enabled
        && candidate.external_api.require_token
    {
        let token = if candidate.external_api != current.external_api
            || candidate.features.external_api != current.features.external_api
        {
            credentials::read_external_api_token_for_use()
        } else {
            credentials::read_external_api_token()
        }
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                credentials::error_reason_code(&error).unwrap_or("external_api.token_status_failed"),
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
    if candidate.vrchat_chatbox.enabled && candidate.vrchat_chatbox != current.vrchat_chatbox {
        crate::platform_capabilities::require_network_capture()
            .map_err(super::super::super::capture::platform_error)?;
    }
    if candidate.audio != current.audio {
        validate_audio_devices(current.audio.clone(), candidate.audio.clone()).await?;
    }
    Ok(())
}

async fn validate_audio_devices(
    current: crate::config::AudioConfig,
    config: crate::config::AudioConfig,
) -> ApiResult<()> {
    if config.output.mode == "vrchat_players" && config.output.mode != current.output.mode {
        crate::platform_capabilities::require_network_capture()
            .map_err(super::super::super::capture::platform_error)?;
    }
    if config.output.player_audio != current.output.player_audio {
        crate::platform_capabilities::require_network_capture()
            .map_err(super::super::super::capture::platform_error)?;
    }
    if config.output.mode == "vrchat" && config.output.mode != current.output.mode && !cfg!(windows) {
        return Err(super::super::super::capture::platform_error("platform.application_audio_unavailable"));
    }
    // WASAPI enumeration can block, so it must not occupy a Tokio worker.
    tokio::task::spawn_blocking(move || {
        for (changed, enabled, key, source) in [
            (config.output.mode != current.output.mode || config.output.device_id != current.output.device_id || config.output.device_key != current.output.device_key,
             config.output.mode != "disabled", config.output.device_key.as_deref(), audio::CaptureSource::Speaker),
            (config.microphone.mode != current.microphone.mode || config.microphone.device_id != current.microphone.device_id || config.microphone.device_key != current.microphone.device_key,
             config.microphone.mode != "disabled", config.microphone.device_key.as_deref(), audio::CaptureSource::Microphone),
        ] {
            if changed && enabled {
                audio::validate_device_key(key, source)?;
            }
        }
        // A disconnected, unchanged route must not block repairing the other route.
        if config.output.mode == "system"
            && (config.output.mode != current.output.mode
                || config.output.device_id != current.output.device_id)
        {
            if let Some(device_id) = config.output.device_id.filter(|_| config.output.device_key.is_none()) {
                audio::validate_device_id(device_id, audio::CaptureSource::Speaker)?;
            }
        }
        if config.microphone.mode == "device"
            && (config.microphone.mode != current.microphone.mode
                || config.microphone.device_id != current.microphone.device_id)
        {
            if let Some(device_id) = config.microphone.device_id.filter(|_| config.microphone.device_key.is_none()) {
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
            error.code(),
            error.to_string(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AudioConfig;

    fn stale_routes() -> AudioConfig {
        let mut config = AudioConfig::default();
        config.output.device_id = Some(-1);
        config.microphone.mode = "device".into();
        config.microphone.device_id = Some(-2);
        config
    }

    #[tokio::test]
    async fn threshold_changes_do_not_revalidate_unchanged_devices() {
        let current = stale_routes();
        let mut candidate = current.clone();
        candidate.output.trigger_threshold_dbfs = -50.0;
        candidate.microphone.trigger_threshold_dbfs = -50.0;

        validate_audio_devices(current, candidate).await.unwrap();
    }

    #[tokio::test]
    async fn new_invalid_selections_keep_the_audio_error() {
        for output in [true, false] {
            let current = AudioConfig::default();
            let mut candidate = current.clone();
            if output {
                candidate.output.device_id = Some(-1);
            } else {
                candidate.microphone.mode = "device".into();
                candidate.microphone.device_id = Some(-2);
            }

            let (status, axum::Json(error)) = validate_audio_devices(current, candidate)
                .await
                .unwrap_err();
            if cfg!(windows) {
                assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
                assert!(
                    error["code"].as_str().unwrap().starts_with("audio."),
                    "{error}"
                );
            } else if cfg!(target_os = "linux") {
                assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
                assert_eq!(
                    error["code"],
                    crate::platform_capabilities::DEVICE_KEY_UNSUPPORTED
                );
            } else {
                assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
                assert_eq!(
                    error["code"],
                    crate::platform_capabilities::AUDIO_UNAVAILABLE
                );
            }
            assert!(!error["detail"].as_str().unwrap().is_empty());
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "requires live Windows audio endpoints"]
    async fn listed_devices_can_replace_one_stale_route() {
        let devices = tokio::task::spawn_blocking(audio::list_devices)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !devices.is_empty(),
            "No live endpoints are available for this test"
        );
        for device in devices {
            let current = stale_routes();
            let mut candidate = current.clone();
            if device.is_loopback {
                candidate.output.device_id = Some(device.id);
            } else {
                candidate.microphone.device_id = Some(device.id);
            }
            validate_audio_devices(current, candidate).await.unwrap();
        }
    }
}
