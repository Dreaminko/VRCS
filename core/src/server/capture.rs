use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::audio;
use crate::config::{AppConfig, AsrConfig};
use crate::error::AppError;
use crate::pipeline::PipelineDependencies;

use super::{api_domain_error, api_error, api_error_with_params, ApiResult, AppState};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CaptureReloadPlan {
    speaker: bool,
    microphone: bool,
}

impl CaptureReloadPlan {
    pub(crate) fn between(current: &AppConfig, candidate: &AppConfig) -> Self {
        let shared = current.vad != candidate.vad
            || asr_runtime_changed(current, candidate)
            || current.storage.model_directory != candidate.storage.model_directory;
        let sample_rate_changed = current.audio.sample_rate != candidate.audio.sample_rate;
        Self {
            speaker: shared
                || sample_rate_changed
                || current.audio.output != candidate.audio.output,
            microphone: shared
                || sample_rate_changed
                || current.audio.microphone != candidate.audio.microphone,
        }
    }

    pub(crate) fn all() -> Self {
        Self {
            speaker: true,
            microphone: true,
        }
    }

    pub(crate) fn is_empty(self) -> bool {
        !self.speaker && !self.microphone
    }
}

pub(crate) fn asr_runtime_changed(current: &AppConfig, candidate: &AppConfig) -> bool {
    asr_config_runtime_changed(&current.asr, &candidate.asr)
}

fn asr_config_runtime_changed(current: &AsrConfig, candidate: &AsrConfig) -> bool {
    if current.backend != candidate.backend
        || current.language != candidate.language
        || current.cloud_failure_policy != candidate.cloud_failure_policy
    {
        return true;
    }

    let local_required = |config: &AsrConfig| {
        config.backend == "local_whisper" || config.cloud_failure_policy == "local"
    };
    if (local_required(current) || local_required(candidate)) && current.local != candidate.local {
        return true;
    }

    let backend_config_changed = match current.backend.as_str() {
        "local_whisper" => false,
        "qwen_realtime" => current.qwen != candidate.qwen,
        "fun_asr_realtime" => current.fun_asr != candidate.fun_asr,
        "openai_realtime" => current.openai != candidate.openai,
        _ => current != candidate,
    };
    backend_config_changed
        || active_asr_profile(current).map(asr_profile_runtime_config)
            != active_asr_profile(candidate).map(asr_profile_runtime_config)
}

fn asr_profile_runtime_config(profile: &crate::config::ApiProfile) -> crate::config::ApiProfile {
    let mut profile = profile.clone();
    profile.name.clear();
    profile
}

fn active_asr_profile(config: &AsrConfig) -> Option<&crate::config::ApiProfile> {
    let profile_id = match config.backend.as_str() {
        "qwen_realtime" | "fun_asr_realtime" => config.active_api_profiles.alibaba_cloud.as_deref(),
        "openai_realtime" => config.active_api_profiles.openai.as_deref(),
        _ => None,
    }?;
    config
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
}

pub(crate) fn uses_asr_profile(config: &AppConfig, profile_id: &str) -> bool {
    active_asr_profile(&config.asr).is_some_and(|profile| profile.id == profile_id)
}

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

pub(super) async fn microphone_test_start(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Value>> {
    let _control = state.capture_control.lock().await;
    if state.capture_requested.load(Ordering::SeqCst)
        || state.speaker_pipeline.lock().await.running()
        || state.microphone_pipeline.lock().await.running()
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            "audio.microphone_test_capture_running",
            "Stop transcription before testing the microphone",
        ));
    }
    let config = state.config.read().expect("config lock").clone();
    if config.audio.microphone.mode == "disabled" {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "audio.microphone_test_disabled",
            "Select a microphone before starting the test",
        ));
    }
    let device_id = (config.audio.microphone.mode == "device")
        .then_some(config.audio.microphone.device_id)
        .flatten();
    let device = state
        .microphone_monitor
        .lock()
        .await
        .start(config.audio.sample_rate, device_id, state.live_tx.clone())
        .await
        .map_err(|error| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                error.code(),
                error.to_string(),
            )
        })?;
    Ok(Json(json!({ "running": true, "device": device })))
}

pub(super) async fn microphone_test_stop(State(state): State<Arc<AppState>>) -> Json<Value> {
    let _control = state.capture_control.lock().await;
    state.microphone_monitor.lock().await.stop().await;
    Json(json!({ "running": false }))
}

pub(crate) async fn validate_capture_config(
    state: &Arc<AppState>,
    config: &AppConfig,
) -> ApiResult<()> {
    if config.audio.sample_rate != 16_000 {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "capture.invalid_sample_rate",
            "The Rust ASR pipeline requires a 16000 Hz sample rate",
        ));
    }
    if config.audio.output.mode == "disabled" && config.audio.microphone.mode == "disabled" {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "capture.no_audio_sources",
            "At least one audio source must be enabled",
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
    Ok(())
}

fn pipeline_dependencies(state: &Arc<AppState>) -> PipelineDependencies {
    PipelineDependencies::new(
        Arc::clone(&state.asr),
        Arc::clone(&state.db),
        state.live_tx.clone(),
        state.translation_dispatcher.clone(),
        Arc::clone(&state.config),
        state.subtitle_output.clone(),
    )
}

async fn start_speaker_pipeline(
    state: &Arc<AppState>,
    config: &AppConfig,
) -> ApiResult<Option<crate::models::AudioDevice>> {
    let output = &config.audio.output;
    if output.mode == "disabled" {
        return Ok(None);
    }
    let device_id = (output.mode == "system")
        .then_some(output.device_id)
        .flatten();
    let process_name = (output.mode == "vrchat").then_some("VRChat.exe");
    state
        .speaker_pipeline
        .lock()
        .await
        .start(
            config.audio.sample_rate,
            device_id,
            process_name,
            None,
            &config.vad,
            config.asr.clone(),
            pipeline_dependencies(state),
        )
        .await
        .map(Some)
        .map_err(|error| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                error.code(),
                error.to_string(),
            )
        })
}

async fn start_microphone_pipeline(
    state: &Arc<AppState>,
    config: &AppConfig,
) -> ApiResult<Option<crate::models::AudioDevice>> {
    if config.audio.microphone.mode == "disabled"
        || state.vrchat_mute_sync.status().muted == Some(true)
    {
        return Ok(None);
    }
    let microphone_id = (config.audio.microphone.mode == "device")
        .then_some(config.audio.microphone.device_id)
        .flatten();
    state
        .microphone_pipeline
        .lock()
        .await
        .start(
            config.audio.sample_rate,
            microphone_id,
            None,
            Some(config.audio.microphone.trigger_threshold_dbfs),
            &config.vad,
            config.asr.clone(),
            pipeline_dependencies(state),
        )
        .await
        .map(Some)
        .map_err(|error| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                error.code(),
                error.to_string(),
            )
        })
}

pub(crate) async fn stop_pipelines(state: &Arc<AppState>, plan: CaptureReloadPlan) {
    match (plan.speaker, plan.microphone) {
        (true, true) => {
            let mut speaker = state.speaker_pipeline.lock().await;
            let mut microphone = state.microphone_pipeline.lock().await;
            tokio::join!(speaker.stop(), microphone.stop());
        }
        (true, false) => state.speaker_pipeline.lock().await.stop().await,
        (false, true) => state.microphone_pipeline.lock().await.stop().await,
        (false, false) => {}
    }
}

pub(crate) async fn start_pipelines(
    state: &Arc<AppState>,
    config: &AppConfig,
    plan: CaptureReloadPlan,
) -> ApiResult<()> {
    if !state.capture_requested.load(Ordering::SeqCst) {
        return Ok(());
    }
    if plan.speaker {
        start_speaker_pipeline(state, config).await?;
    }
    if plan.microphone {
        if let Err(error) = start_microphone_pipeline(state, config).await {
            if plan.speaker {
                state.speaker_pipeline.lock().await.stop().await;
            }
            return Err(error);
        }
    }
    Ok(())
}

pub(super) async fn capture_start(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let _control = state.capture_control.lock().await;
    let config = state.config.read().expect("config lock").clone();
    validate_capture_config(&state, &config).await?;
    if state.speaker_pipeline.lock().await.running()
        || state.microphone_pipeline.lock().await.running()
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            "capture.already_running",
            "Transcription is already running",
        ));
    }
    state.microphone_monitor.lock().await.stop().await;

    let device = start_speaker_pipeline(&state, &config).await?;
    let microphone = match start_microphone_pipeline(&state, &config).await {
        Ok(device) => device,
        Err(error) => {
            state.speaker_pipeline.lock().await.stop().await;
            return Err(error);
        }
    };
    state.capture_requested.store(true, Ordering::SeqCst);
    Ok(Json(json!({
        "running": true,
        "device": device,
        "microphone_device": microphone,
    })))
}

pub(super) async fn capture_stop(State(state): State<Arc<AppState>>) -> Json<Value> {
    let _control = state.capture_control.lock().await;
    state.capture_requested.store(false, Ordering::SeqCst);
    let mut speaker = state.speaker_pipeline.lock().await;
    let mut microphone = state.microphone_pipeline.lock().await;
    tokio::join!(speaker.stop(), microphone.stop());
    Json(json!({ "running": false }))
}

pub(crate) async fn resume_microphone(state: &Arc<AppState>) -> Result<(), String> {
    if !state.capture_requested.load(Ordering::SeqCst)
        || state.microphone_pipeline.lock().await.running()
    {
        return Ok(());
    }
    let config = state.config.read().expect("config lock").clone();
    if config.audio.microphone.mode == "disabled" {
        return Ok(());
    }
    let microphone_id = (config.audio.microphone.mode == "device")
        .then_some(config.audio.microphone.device_id)
        .flatten();
    let dependencies = pipeline_dependencies(state);
    state
        .microphone_pipeline
        .lock()
        .await
        .start(
            config.audio.sample_rate,
            microphone_id,
            None,
            Some(config.audio.microphone.trigger_threshold_dbfs),
            &config.vad,
            config.asr,
            dependencies,
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::CaptureReloadPlan;
    use crate::config::AppConfig;

    #[test]
    fn translation_changes_use_the_shared_runtime_snapshot() {
        let current = AppConfig::default();
        let mut candidate = current.clone();
        candidate.translation.target_language = "ja".into();

        assert!(CaptureReloadPlan::between(&current, &candidate).is_empty());
    }

    #[test]
    fn inactive_asr_settings_do_not_reload_capture() {
        let current = AppConfig::default();
        let mut candidate = current.clone();
        candidate.asr.local.model = "tiny".into();
        candidate.asr.active_api_profiles.openai = Some("unused-profile".into());

        assert!(CaptureReloadPlan::between(&current, &candidate).is_empty());
    }

    #[test]
    fn active_profile_name_changes_do_not_reload_capture() {
        let mut current = AppConfig::default();
        current.asr.backend = "qwen_realtime".into();
        current.asr.active_api_profiles.alibaba_cloud = Some("profile-1".into());
        current.asr.api_profiles.push(crate::config::ApiProfile {
            id: "profile-1".into(),
            name: "Before".into(),
            provider: crate::providers::ALIBABA_PROVIDER.into(),
            ..crate::config::ApiProfile::default()
        });
        let mut candidate = current.clone();
        candidate.asr.api_profiles[0].name = "After".into();

        assert!(CaptureReloadPlan::between(&current, &candidate).is_empty());
    }

    #[test]
    fn output_changes_only_reload_the_speaker_pipeline() {
        let current = AppConfig::default();
        let mut candidate = current.clone();
        candidate.audio.output.mode = "disabled".into();

        assert_eq!(
            CaptureReloadPlan::between(&current, &candidate),
            CaptureReloadPlan {
                speaker: true,
                microphone: false,
            }
        );
    }

    #[test]
    fn microphone_threshold_changes_only_reload_the_microphone_pipeline() {
        let current = AppConfig::default();
        let mut candidate = current.clone();
        candidate.audio.microphone.trigger_threshold_dbfs -= 1.0;

        assert_eq!(
            CaptureReloadPlan::between(&current, &candidate),
            CaptureReloadPlan {
                speaker: false,
                microphone: true,
            }
        );
    }

    #[test]
    fn vad_changes_reload_both_pipelines() {
        let current = AppConfig::default();
        let mut candidate = current.clone();
        candidate.vad.silence_seconds += 0.1;

        assert_eq!(
            CaptureReloadPlan::between(&current, &candidate),
            CaptureReloadPlan::all()
        );
    }
}
