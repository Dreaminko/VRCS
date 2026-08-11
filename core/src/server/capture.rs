use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::audio;
use crate::error::AppError;
use crate::pipeline::PipelineDependencies;

use super::{api_domain_error, api_error, api_error_with_params, ApiResult, AppState};

pub(super) async fn audio_devices() -> ApiResult<Json<Value>> {
    let devices = tokio::task::spawn_blocking(audio::list_devices)
        .await
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "audio.device_task_failed",
                format!("Audio device enumeration task failed: {error}"),
            )
        })?
        .map_err(|error| {
            let code = error.code();
            api_domain_error(AppError::Unavailable(error.to_string()), code)
        })?;
    Ok(Json(json!(devices)))
}

pub(super) async fn capture_start(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let _control = state.capture_control.lock().await;
    let config = state.config.read().expect("config lock").clone();
    if config.audio.sample_rate != 16_000 {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "capture.invalid_sample_rate",
            "The Rust ASR pipeline requires a 16000 Hz sample rate",
        ));
    }
    if config.asr.backend != "local_whisper" {
        crate::asr::validate_cloud_connection(&config.asr)
            .map_err(|error| api_error(StatusCode::CONFLICT, "asr.cloud_profile_invalid", error))?;
    }
    let manager = Arc::clone(&state.model_manager);
    let model = config.asr.local.model.clone();
    let local_required =
        config.asr.backend == "local_whisper" || config.asr.cloud_failure_policy == "local";
    if local_required
        && !tokio::task::spawn_blocking(move || manager.is_downloaded(&model))
            .await
            .map_err(|error| {
                api_error_with_params(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "asr.model.inspect_task_failed",
                    json!({ "model": config.asr.local.model }),
                    format!("Model validation task failed: {error}"),
                )
            })?
            .map_err(|error| {
                api_error_with_params(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "asr.model.inspect_failed",
                    json!({ "model": config.asr.local.model }),
                    error,
                )
            })?
    {
        return Err(api_error_with_params(
            StatusCode::CONFLICT,
            "asr.model.not_downloaded",
            json!({ "model": config.asr.local.model }),
            format!(
                "Recognition model {} has not been downloaded",
                config.asr.local.model
            ),
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
    let dependencies = PipelineDependencies::new(
        Arc::clone(&state.asr),
        Arc::clone(&state.db),
        state.live_tx.clone(),
        config.storage.subtitle_history_limit,
        state.translation_dispatcher.clone(),
        config.translation.clone(),
        config.asr.api_profiles.clone(),
        state.subtitle_output.clone(),
    );
    let device = state
        .speaker_pipeline
        .lock()
        .await
        .start(
            config.audio.sample_rate,
            output_device_id,
            process_name,
            &config.vad,
            config.asr.clone(),
            dependencies.clone(),
        )
        .await
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
        match state
            .microphone_pipeline
            .lock()
            .await
            .start(
                config.audio.sample_rate,
                microphone_id,
                None,
                &config.vad,
                config.asr.clone(),
                dependencies,
            )
            .await
        {
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

pub(super) async fn capture_stop(State(state): State<Arc<AppState>>) -> Json<Value> {
    let _control = state.capture_control.lock().await;
    let mut speaker = state.speaker_pipeline.lock().await;
    let mut microphone = state.microphone_pipeline.lock().await;
    tokio::join!(speaker.stop(), microphone.stop());
    Json(json!({ "running": false }))
}
