use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::audio;
use crate::error::AppError;

use super::{api_domain_error, api_error, api_error_with_params, ApiResult, AppState};

pub(super) async fn audio_devices() -> ApiResult<Json<Value>> {
    let devices = tokio::task::spawn_blocking(audio::list_devices)
        .await
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "audio.device_task_failed",
                format!("音频设备枚举任务失败：{error}"),
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
            "Rust ASR 管线要求 16000 Hz 采样率",
        ));
    }
    let manager = Arc::clone(&state.model_manager);
    let model = config.asr.model.clone();
    if !tokio::task::spawn_blocking(move || manager.is_downloaded(&model))
        .await
        .map_err(|error| {
            api_error_with_params(
                StatusCode::INTERNAL_SERVER_ERROR,
                "asr.model.inspect_task_failed",
                json!({ "model": config.asr.model }),
                format!("模型校验任务失败：{error}"),
            )
        })?
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
                Arc::clone(&state.asr),
                Arc::clone(&state.db),
                state.subtitles_tx.clone(),
                config.storage.subtitle_history_limit,
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
    state.speaker_pipeline.lock().await.stop().await;
    state.microphone_pipeline.lock().await.stop().await;
    Json(json!({ "running": false }))
}
