use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::audio;
use crate::config::{AppConfig, AsrConfig, QWEN_MANAGED_BACKEND};
use crate::error::AppError;
use crate::pipeline::{AsrEchoGuard, PipelineDependencies};

use super::{api_domain_error, api_error, ApiResult, CaptureContext};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CaptureReloadPlan {
    speaker: bool,
    microphone: bool,
}

impl CaptureReloadPlan {
    pub(crate) fn between(current: &AppConfig, candidate: &AppConfig) -> Self {
        let shared = current.vad != candidate.vad
            || (current.vrcx.include_in_asr_context || candidate.vrcx.include_in_asr_context)
                && current.vrcx.enabled != candidate.vrcx.enabled
            || asr_runtime_changed(current, candidate)
            || glossary_asr_runtime_changed(current, candidate)
            || current.storage.model_directory != candidate.storage.model_directory;
        let live = crate::providers::is_live_translation(&current.asr.backend)
            || crate::providers::is_live_translation(&candidate.asr.backend);
        let live_mode_changed = live && current.translation.mode != candidate.translation.mode;
        let target = |targets: &[crate::config::TranslationTargetConfig]| {
            targets.first().map(|t| t.target_language.clone())
        };
        let sample_rate_changed = current.audio.sample_rate != candidate.audio.sample_rate;
        Self {
            speaker: shared
                || live_mode_changed
                || (live
                    && target(&current.translation.speaker_targets)
                        != target(&candidate.translation.speaker_targets))
                || sample_rate_changed
                || current.audio.output != candidate.audio.output,
            microphone: shared
                || live_mode_changed
                || (live
                    && target(&current.translation.microphone_targets)
                        != target(&candidate.translation.microphone_targets))
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
    if supports_live_translation_glossary(&current.asr)
        || supports_live_translation_glossary(&candidate.asr)
    {
        return current.glossary.llm_enabled != candidate.glossary.llm_enabled
            || current.glossary.asr_enabled != candidate.glossary.asr_enabled
            || ((candidate.glossary.llm_enabled || candidate.glossary.asr_enabled)
                && current.glossary.sources != candidate.glossary.sources);
    }
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

fn supports_live_translation_glossary(config: &AsrConfig) -> bool {
    config.backend == crate::providers::SERVICE_QWEN_LIVE_TRANSLATE
}

fn glossary_used_by_capture(config: &AppConfig) -> bool {
    (config.glossary.asr_enabled && supports_asr_context(&config.asr))
        || (supports_live_translation_glossary(&config.asr)
            && (config.glossary.llm_enabled || config.glossary.asr_enabled))
}

fn asr_config_runtime_changed(current: &AsrConfig, candidate: &AsrConfig) -> bool {
    if current.backend != candidate.backend || current.language != candidate.language {
        return true;
    }

    if (current.backend == QWEN_MANAGED_BACKEND || candidate.backend == QWEN_MANAGED_BACKEND)
        && current.managed_qwen != candidate.managed_qwen
    {
        return true;
    }

    let backend_config_changed = current.backend != QWEN_MANAGED_BACKEND
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
    if config.backend == QWEN_MANAGED_BACKEND {
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
    config.asr.backend != QWEN_MANAGED_BACKEND
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
    State(state): State<CaptureContext>,
) -> ApiResult<Json<Value>> {
    let _control = state.capture.capture_control.lock().await;
    if state.capture.capture_requested.load(Ordering::SeqCst)
        || state.capture.speaker_pipeline.lock().await.running()
        || state.capture.microphone_pipeline.lock().await.running()
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            "audio.microphone_test_capture_running",
            "Stop transcription before testing the microphone",
        ));
    }
    let config =
        crate::config::apply_feature_gates(&state.config.config.read().expect("config lock"));
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
        .capture
        .microphone_monitor
        .lock()
        .await
        .start(
            config.audio.sample_rate,
            device_id,
            state.capture.live_tx.clone(),
        )
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

pub(super) async fn microphone_test_stop(State(state): State<CaptureContext>) -> Json<Value> {
    let _control = state.capture.capture_control.lock().await;
    state.capture.microphone_monitor.lock().await.stop().await;
    Json(json!({ "running": false }))
}

pub(crate) async fn validate_capture_config(
    state: &CaptureContext,
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
    if crate::providers::is_live_translation(&config.asr.backend) {
        if config.translation.mode != "automatic" {
            return Err(api_error(
                StatusCode::CONFLICT,
                "asr.live_translation_config",
                "Live translation requires automatic translation",
            ));
        }
        for targets in [
            &config.translation.speaker_targets,
            &config.translation.microphone_targets,
        ] {
            let target = targets.first().ok_or_else(|| {
                api_error(
                    StatusCode::CONFLICT,
                    "asr.live_translation_config",
                    "Select a translation target",
                )
            })?;
            crate::providers::validate_live_translation_language(
                &config.asr.backend,
                &target.target_language,
            )
            .map_err(|error| {
                api_error(StatusCode::CONFLICT, "asr.live_translation_config", error)
            })?;
        }
    }
    if config.asr.backend == QWEN_MANAGED_BACKEND {
        let manager = Arc::clone(&state.capture.model_manager);
        let package = config.asr.managed_qwen.package_id.clone();
        let installed = tokio::task::spawn_blocking(move || manager.describe_qwen(&package))
            .await
            .map_err(|error| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "asr.qwen_model.inspect_task_failed",
                    error.to_string(),
                )
            })?
            .map_err(|error| {
                api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "asr.qwen_model.inspect_failed",
                    error,
                )
            })?;
        if installed.status != "installed" {
            return Err(api_error(
                StatusCode::CONFLICT,
                "asr.qwen_model.not_installed",
                "The selected Qwen ASR package is not installed",
            ));
        }
    } else {
        crate::asr::validate_cloud_connection(&config.asr)
            .map_err(|error| api_error(StatusCode::CONFLICT, "asr.cloud_profile_invalid", error))?;
    }
    Ok(())
}

fn effective_asr_config(
    state: &CaptureContext,
    config: &AppConfig,
    source: &str,
) -> (AsrConfig, AsrEchoGuard) {
    let mut asr = config.asr.clone();
    if crate::providers::is_live_translation(&asr.backend) {
        let targets = if source == "microphone" {
            &config.translation.microphone_targets
        } else {
            &config.translation.speaker_targets
        };
        asr.live_translation_target = targets.first().map(|target| target.target_language.clone());
    }
    if supports_live_translation_glossary(&asr) {
        asr.live_translation_phrases = state
            .content
            .glossary
            .phrases_for_live_translation(&config.glossary);
    }
    let terms = state
        .content
        .glossary
        .terms_for_asr(&config.glossary)
        .join("\n");
    crate::glossary::append_asr_context(&mut asr, &terms);
    let (signatures, repeated_world) = if config.vrcx.enabled && config.vrcx.include_in_asr_context
    {
        state.integrations.vrcx.apply_asr_context(&mut asr)
    } else {
        (Vec::new(), None)
    };
    (asr, AsrEchoGuard::new(signatures, repeated_world))
}

fn pipeline_dependencies(state: &CaptureContext) -> PipelineDependencies {
    PipelineDependencies::new(
        Arc::clone(&state.content.db),
        state.capture.live_tx.clone(),
        state.content.conversation_catalog_tx.clone(),
        state.content.translation_dispatcher.clone(),
        Arc::clone(&state.config.config),
        Arc::clone(&state.config.language_session),
        state.content.subtitle_output.clone(),
    )
    .with_managed_qwen(
        Arc::clone(&state.capture.qwen_runtime),
        Arc::clone(&state.capture.model_manager),
    )
}

async fn start_speaker_pipeline(
    state: &CaptureContext,
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
    let (asr, echo_guard) = effective_asr_config(state, config, "speaker");
    state
        .capture
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
    state: &CaptureContext,
    config: &AppConfig,
) -> ApiResult<Option<crate::models::AudioDevice>> {
    if config.audio.microphone.mode == "disabled"
        || state.integrations.vrchat_mute_sync.status().muted == Some(true)
    {
        return Ok(None);
    }
    let microphone_id = (config.audio.microphone.mode == "device")
        .then_some(config.audio.microphone.device_id)
        .flatten();
    let (asr, echo_guard) = effective_asr_config(state, config, "microphone");
    state
        .capture
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
    state: &CaptureContext,
    config: &AppConfig,
    plan: CaptureReloadPlan,
) -> ApiResult<(
    Option<crate::models::AudioDevice>,
    Option<crate::models::AudioDevice>,
)> {
    if config.asr.backend == QWEN_MANAGED_BACKEND
        && (config.audio.output.mode != "disabled" || config.audio.microphone.mode != "disabled")
    {
        crate::asr::prepare_managed_qwen(
            &config.asr,
            &state.capture.qwen_runtime,
            &state.capture.model_manager,
        )
        .await
        .map_err(|error| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "asr.cloud_connect_failed",
                error,
            )
        })?;
    }
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

pub(crate) async fn stop_pipelines(state: &CaptureContext, plan: CaptureReloadPlan) {
    match (plan.speaker, plan.microphone) {
        (true, true) => {
            let mut speaker = state.capture.speaker_pipeline.lock().await;
            let mut microphone = state.capture.microphone_pipeline.lock().await;
            tokio::join!(speaker.stop(), microphone.stop());
        }
        (true, false) => state.capture.speaker_pipeline.lock().await.stop().await,
        (false, true) => state.capture.microphone_pipeline.lock().await.stop().await,
        (false, false) => {}
    }
}

pub(crate) async fn start_pipelines(
    state: &CaptureContext,
    config: &AppConfig,
    plan: CaptureReloadPlan,
) -> ApiResult<()> {
    if !state.capture.capture_requested.load(Ordering::SeqCst) {
        return Ok(());
    }
    start_planned_pipelines(state, config, plan)
        .await
        .map(|_| ())
}

pub(crate) async fn reload_glossary_asr_context(state: &CaptureContext) -> ApiResult<()> {
    let _control = state.capture.capture_control.lock().await;
    let global = state.config.config.read().expect("config lock").clone();
    let config = state
        .config
        .language_session
        .read()
        .expect("language session lock")
        .apply_to(&global);
    if !state.capture.capture_requested.load(Ordering::SeqCst) || !glossary_used_by_capture(&config)
    {
        return Ok(());
    }
    let plan = CaptureReloadPlan::all();
    stop_pipelines(state, plan).await;
    start_pipelines(state, &config, plan).await
}

pub(super) async fn capture_start(
    State(state): State<CaptureContext>,
    input: Option<Json<crate::language_session::CaptureStartInput>>,
) -> ApiResult<Json<Value>> {
    let _control = state.capture.capture_control.lock().await;
    let global = state.config.config.read().expect("config lock").clone();
    let input = input.map(|Json(input)| input).unwrap_or_default();
    let session = crate::language_session::select_session(input, &global).map_err(|detail| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "capture.language_session_invalid",
            detail,
        )
    })?;
    let config = session.apply_to(&global);
    validate_capture_config(&state, &config).await?;
    if state.capture.speaker_pipeline.lock().await.running()
        || state.capture.microphone_pipeline.lock().await.running()
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            "capture.already_running",
            "Transcription is already running",
        ));
    }
    state.capture.microphone_monitor.lock().await.stop().await;

    *state
        .config
        .language_session
        .write()
        .expect("language session lock") = session;
    state.integrations.osc.update_config(config.osc.clone());
    let started = start_planned_pipelines(&state, &config, CaptureReloadPlan::all()).await;
    let (device, microphone) = match started {
        Ok(devices) => devices,
        Err(error) => {
            if let Err(stop_error) = state.capture.qwen_runtime.stop().await {
                tracing::warn!(%stop_error, "could not stop Qwen ASR after capture start failed");
            }
            *state
                .config
                .language_session
                .write()
                .expect("language session lock") =
                crate::language_session::ActiveLanguageSession::Global;
            state
                .integrations
                .osc
                .update_config(crate::config::apply_feature_gates(&global).osc);
            return Err(error);
        }
    };
    state
        .capture
        .capture_requested
        .store(true, Ordering::SeqCst);
    Ok(Json(json!({
        "running": true,
        "device": device,
        "microphone_device": microphone,
    })))
}

pub(super) async fn capture_stop(State(state): State<CaptureContext>) -> Json<Value> {
    if let Err(error) = state.capture.qwen_runtime.cancel_loading().await {
        tracing::warn!(%error, "could not cancel Qwen ASR startup");
    }
    let _control = state.capture.capture_control.lock().await;
    state
        .capture
        .capture_requested
        .store(false, Ordering::SeqCst);
    let mut speaker = state.capture.speaker_pipeline.lock().await;
    let mut microphone = state.capture.microphone_pipeline.lock().await;
    tokio::join!(speaker.stop(), microphone.stop());
    if let Err(error) = state.capture.qwen_runtime.stop().await {
        tracing::warn!(%error, "could not stop managed Qwen ASR runtime after capture");
    }
    *state
        .config
        .language_session
        .write()
        .expect("language session lock") = crate::language_session::ActiveLanguageSession::Global;
    let osc =
        crate::config::apply_feature_gates(&state.config.config.read().expect("config lock")).osc;
    state.integrations.osc.update_config(osc);
    Json(json!({ "running": false }))
}

pub(crate) async fn resume_microphone(state: &CaptureContext) -> Result<(), String> {
    if !state.capture.capture_requested.load(Ordering::SeqCst)
        || state.capture.microphone_pipeline.lock().await.running()
    {
        return Ok(());
    }
    let global = state.config.config.read().expect("config lock").clone();
    let config = state
        .config
        .language_session
        .read()
        .expect("language session lock")
        .apply_to(&global);
    if config.audio.microphone.mode == "disabled" {
        return Ok(());
    }
    let microphone_id = (config.audio.microphone.mode == "device")
        .then_some(config.audio.microphone.device_id)
        .flatten();
    let dependencies = pipeline_dependencies(state);
    let (asr, echo_guard) = effective_asr_config(state, &config, "microphone");
    state
        .capture
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

    #[test]
    fn qwen_glossary_changes_reload_live_sessions_for_either_consumer() {
        let mut current = crate::config::AppConfig::default();
        current.asr.backend = crate::providers::SERVICE_QWEN_LIVE_TRANSLATE.into();
        for translation_only in [false, true] {
            current.glossary.asr_enabled = !translation_only;
            let mut next = current.clone();
            next.glossary.sources.push(GlossarySource::Local {
                id: "local".into(),
                name: "local".into(),
                enabled: true,
                entries: Vec::new(),
            });
            assert_eq!(
                CaptureReloadPlan::between(&current, &next),
                CaptureReloadPlan::all()
            );
            assert!(super::glossary_used_by_capture(&current));
            next = current.clone();
            next.glossary.llm_enabled = false;
            assert_eq!(
                CaptureReloadPlan::between(&current, &next),
                CaptureReloadPlan::all()
            );
        }
        current.glossary.asr_enabled = false;
        current.glossary.llm_enabled = false;
        assert!(!super::glossary_used_by_capture(&current));
        let mut next = current.clone();
        next.glossary.sources.push(GlossarySource::Local {
            id: "disabled-consumers".into(),
            name: "local".into(),
            enabled: true,
            entries: Vec::new(),
        });
        assert!(CaptureReloadPlan::between(&current, &next).is_empty());
        next.glossary.asr_enabled = true;
        assert_eq!(
            CaptureReloadPlan::between(&current, &next),
            CaptureReloadPlan::all()
        );
    }

    #[test]
    fn live_translation_restarts_only_the_changed_audio_target() {
        for service in [
            crate::providers::SERVICE_GEMINI_LIVE_TRANSLATE,
            crate::providers::SERVICE_OPENAI_REALTIME_TRANSLATE,
            crate::providers::SERVICE_QWEN_LIVE_TRANSLATE,
        ] {
            let mut current = crate::config::AppConfig::default();
            current.asr.backend = service.into();
            let mut next = current.clone();
            next.translation.speaker_targets[0].target_language = "ja".into();
            let plan = CaptureReloadPlan::between(&current, &next);
            assert!(plan.speaker);
            assert!(!plan.microphone);
            next = current.clone();
            next.translation.speaker_targets[0].model = "another-text-model".into();
            assert!(CaptureReloadPlan::between(&current, &next).is_empty());
            next = current.clone();
            next.asr.service_settings.get_mut(service).unwrap().model = "changed-model".into();
            let plan = CaptureReloadPlan::between(&current, &next);
            assert!(plan.speaker && plan.microphone);
        }
    }

    use crate::config::{AppConfig, GlossarySource};
    use crate::providers::{
        CAPABILITY_SPEECH_TO_TEXT, SERVICE_GROQ_TRANSCRIPTION, SERVICE_OPENAI_REALTIME,
        SERVICE_QWEN_REALTIME,
    };

    #[test]
    fn translation_changes_use_the_shared_runtime_snapshot() {
        let current = AppConfig::default();
        let mut candidate = current.clone();
        candidate.translation.speaker_targets[0].target_language = "ja".into();

        assert!(CaptureReloadPlan::between(&current, &candidate).is_empty());
    }

    #[test]
    fn inactive_asr_settings_do_not_reload_capture() {
        let current = AppConfig::default();
        let mut candidate = current.clone();
        candidate.asr.managed_qwen.device = "cpu".into();
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
