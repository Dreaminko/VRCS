use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::audio;
use crate::config::{AppConfig, AsrConfig};
use crate::error::AppError;
use crate::pipeline::{AsrEchoGuard, PipelineDependencies};

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
            || glossary_asr_runtime_changed(current, candidate)
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

fn glossary_asr_runtime_changed(current: &AppConfig, candidate: &AppConfig) -> bool {
    if !supports_asr_context(&current.asr) && !supports_asr_context(&candidate.asr) {
        return false;
    }
    current.glossary.asr_enabled != candidate.glossary.asr_enabled
        || (candidate.glossary.asr_enabled
            && current.glossary.sources != candidate.glossary.sources)
}

fn supports_asr_context(config: &AsrConfig) -> bool {
    crate::providers::recognition_service(&config.backend)
        .and_then(|(_, service)| service.context_max_chars)
        .is_some()
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

    let backend_config_changed = current.backend != "local_whisper"
        && current.service_settings.get(&current.backend)
            != candidate.service_settings.get(&current.backend);
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
    if config.backend == "local_whisper" {
        return None;
    }
    let profile_id = config.active_profile_id.as_deref()?;
    let profile = config
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)?;
    crate::providers::resolve_profile_service(profile, &config.backend)
        .ok()
        .map(|_| profile)
}

pub(crate) fn uses_asr_profile(config: &AppConfig, profile_id: &str) -> bool {
    config.asr.backend != "local_whisper"
        && config.asr.active_profile_id.as_deref() == Some(profile_id)
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

fn effective_asr_config(state: &Arc<AppState>, config: &AppConfig) -> (AsrConfig, AsrEchoGuard) {
    let mut asr = config.asr.clone();
    let terms = state.glossary.terms_for_asr(&config.glossary).join("\n");
    crate::glossary::append_asr_context(&mut asr, &terms);
    let (signatures, repeated_world) = if config.vrcx.enabled && config.vrcx.include_in_asr_context
    {
        state.vrcx.apply_asr_context(&mut asr)
    } else {
        (Vec::new(), None)
    };
    (asr, AsrEchoGuard::new(signatures, repeated_world))
}

fn pipeline_dependencies(state: &Arc<AppState>) -> PipelineDependencies {
    PipelineDependencies::new(
        Arc::clone(&state.asr),
        Arc::clone(&state.db),
        state.live_tx.clone(),
        state.conversation_catalog_tx.clone(),
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
    let (asr, echo_guard) = effective_asr_config(state, config);
    state
        .speaker_pipeline
        .lock()
        .await
        .start(
            config.audio.sample_rate,
            device_id,
            process_name,
            Some(output.trigger_threshold_dbfs),
            &config.vad,
            asr,
            echo_guard,
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
    let (asr, echo_guard) = effective_asr_config(state, config);
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
            asr,
            echo_guard,
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

async fn start_planned_pipelines(
    state: &Arc<AppState>,
    config: &AppConfig,
    plan: CaptureReloadPlan,
) -> ApiResult<(
    Option<crate::models::AudioDevice>,
    Option<crate::models::AudioDevice>,
)> {
    let result = tokio::try_join!(
        async {
            if plan.speaker {
                start_speaker_pipeline(state, config).await
            } else {
                Ok(None)
            }
        },
        async {
            if plan.microphone {
                start_microphone_pipeline(state, config).await
            } else {
                Ok(None)
            }
        }
    );
    match result {
        Ok(devices) => Ok(devices),
        Err(error) => {
            stop_pipelines(state, plan).await;
            Err(error)
        }
    }
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
    start_planned_pipelines(state, config, plan)
        .await
        .map(|_| ())
}

pub(crate) async fn reload_glossary_asr_context(state: &Arc<AppState>) -> ApiResult<()> {
    let _control = state.capture_control.lock().await;
    let config = state.config.read().expect("config lock").clone();
    if !state.capture_requested.load(Ordering::SeqCst)
        || !config.glossary.asr_enabled
        || !supports_asr_context(&config.asr)
    {
        return Ok(());
    }
    let plan = CaptureReloadPlan::all();
    stop_pipelines(state, plan).await;
    start_pipelines(state, &config, plan).await
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

    let (device, microphone) =
        start_planned_pipelines(&state, &config, CaptureReloadPlan::all()).await?;
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
    let (asr, echo_guard) = effective_asr_config(state, &config);
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
            asr,
            echo_guard,
            dependencies,
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{asr_runtime_changed, CaptureReloadPlan};
    use crate::config::{AppConfig, GlossarySource};
    use crate::providers::{
        CAPABILITY_SPEECH_TO_TEXT, SERVICE_GROQ_TRANSCRIPTION, SERVICE_OPENAI_REALTIME,
        SERVICE_QWEN_REALTIME,
    };

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
        candidate.asr.active_profile_id = Some("unused-profile".into());
        candidate
            .asr
            .service_settings
            .get_mut(SERVICE_GROQ_TRANSCRIPTION)
            .unwrap()
            .context = "inactive".into();

        assert!(CaptureReloadPlan::between(&current, &candidate).is_empty());
    }

    #[test]
    fn active_profile_name_changes_do_not_reload_capture() {
        let mut current = AppConfig::default();
        current.asr.backend = SERVICE_QWEN_REALTIME.into();
        current.asr.active_profile_id = Some("profile-1".into());
        current.asr.api_profiles.push(crate::config::ApiProfile {
            id: "profile-1".into(),
            name: "Before".into(),
            provider: crate::providers::ALIBABA_PROVIDER.into(),
            region: Some("singapore".into()),
            enabled_capabilities: vec![CAPABILITY_SPEECH_TO_TEXT.into()],
            ..crate::config::ApiProfile::default()
        });
        let mut candidate = current.clone();
        candidate.asr.api_profiles[0].name = "After".into();

        assert!(CaptureReloadPlan::between(&current, &candidate).is_empty());
    }

    #[test]
    fn inactive_service_settings_do_not_reload_capture() {
        let mut current = AppConfig::default();
        current.asr.backend = SERVICE_QWEN_REALTIME.into();
        let mut candidate = current.clone();
        candidate
            .asr
            .service_settings
            .get_mut(SERVICE_GROQ_TRANSCRIPTION)
            .unwrap()
            .context = "inactive".into();

        assert!(CaptureReloadPlan::between(&current, &candidate).is_empty());
    }

    #[test]
    fn active_service_settings_reload_capture() {
        let mut current = AppConfig::default();
        current.asr.backend = SERVICE_QWEN_REALTIME.into();
        let mut candidate = current.clone();
        candidate
            .asr
            .service_settings
            .get_mut(SERVICE_QWEN_REALTIME)
            .unwrap()
            .context = "active".into();

        assert_eq!(
            CaptureReloadPlan::between(&current, &candidate),
            CaptureReloadPlan::all()
        );
    }

    #[test]
    fn glossary_asr_changes_reload_supported_cloud_pipelines_without_updating_local_runtime() {
        let mut current = AppConfig::default();
        current.asr.backend = SERVICE_QWEN_REALTIME.into();
        let mut candidate = current.clone();
        candidate.glossary.sources.push(GlossarySource::Local {
            id: "local".into(),
            name: "Local".into(),
            enabled: true,
            entries: Vec::new(),
        });

        assert_eq!(
            CaptureReloadPlan::between(&current, &candidate),
            CaptureReloadPlan::all()
        );
        assert!(!asr_runtime_changed(&current, &candidate));
    }

    #[test]
    fn glossary_changes_do_not_reload_services_without_context_support() {
        let mut current = AppConfig::default();
        current.asr.backend = SERVICE_OPENAI_REALTIME.into();
        let mut candidate = current.clone();
        candidate.glossary.asr_enabled = false;
        candidate.glossary.llm_enabled = false;

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
    fn output_threshold_changes_only_reload_the_speaker_pipeline() {
        let current = AppConfig::default();
        let mut candidate = current.clone();
        candidate.audio.output.trigger_threshold_dbfs -= 1.0;

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
