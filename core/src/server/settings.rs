use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::Json;
use serde_json::{json, Value};

use crate::config::{save_config, AppConfig};
use crate::db::conversations::publish_latest_catalog;
use crate::models::SettingsUpdate;
use crate::providers;
use crate::{asr, audio, credentials};

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
        external_api: update.external_api,
        vrcx: update.vrcx,
        vr_overlay: update.vr_overlay,
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
    if expected_revision
        .as_deref()
        .is_some_and(|revision| revision != current_revision_token)
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            "settings.stale",
            "Settings changed since they were loaded; reload and try again",
        ));
    }
    let current = state.config.read().expect("config lock").clone();
    protect_profile_owned_settings(&mut candidate, &current, expected_revision.is_some());
    let model_directory_changed =
        candidate.storage.model_directory != current.storage.model_directory;

    if candidate.server != current.server {
        return Err(unprocessable(
            "The Core address is a startup setting and cannot be changed at runtime".into(),
        ));
    }
    if candidate.storage.database_path != current.storage.database_path {
        return Err(unprocessable(
            "The database path cannot be changed at runtime".into(),
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
    asr::validate_config(&mut candidate.asr).map_err(unprocessable)?;
    if candidate.audio != current.audio {
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
    }
    let revision = commit_candidate(&state, candidate.clone()).await?;
    Ok((revision_headers(&state, revision), Json(json!(candidate))))
}

pub(super) async fn commit_candidate(
    state: &Arc<AppState>,
    candidate: AppConfig,
) -> ApiResult<u64> {
    let _capture_control = state.capture_control.lock().await;
    let current = state.config.read().expect("config lock").clone();
    let plan = super::capture::CaptureReloadPlan::between(&current, &candidate);
    let reload_capture = state.capture_requested.load(Ordering::SeqCst) && !plan.is_empty();
    let reload_external_api = current.external_api != candidate.external_api;
    if reload_capture {
        super::capture::validate_capture_config(state, &candidate).await?;
        super::capture::stop_pipelines(state, plan).await;
    }

    let model_directory_changed =
        candidate.storage.model_directory != current.storage.model_directory;
    let candidate_model_dir = state.asr_model_dir_override.clone().unwrap_or_else(|| {
        crate::resolve_config_path(&state.config_path, &candidate.storage.model_directory)
    });
    let previous_model_dir = state.model_manager.model_dir();
    if model_directory_changed {
        if let Err(error) = move_model_directory(state, candidate_model_dir.clone()).await {
            if let Err(recovery) = restore_previous(
                state,
                &current,
                &previous_model_dir,
                RestorePreviousOptions::default(),
                plan,
            )
            .await
            {
                return Err(rollback_error(error, recovery));
            }
            return Err(api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "settings.model_directory_migration_failed",
                error,
            ));
        }
    }

    let asr_changed =
        super::capture::asr_runtime_changed(&current, &candidate) || model_directory_changed;
    let current_local_required = local_asr_required(&current);
    let local_required = local_asr_required(&candidate);
    let local_runtime_changed = model_directory_changed
        || current.asr.local != candidate.asr.local
        || (!current_local_required && local_required);
    let prepared_engine = if reload_capture && local_runtime_changed && local_required {
        match prepare_asr_runtime(&candidate, candidate_model_dir.clone()).await {
            Ok(engine) => Some(engine),
            Err(error) => {
                if let Err(recovery) = restore_previous(
                    state,
                    &current,
                    &previous_model_dir,
                    RestorePreviousOptions {
                        model_directory: model_directory_changed,
                        ..Default::default()
                    },
                    plan,
                )
                .await
                {
                    return Err(rollback_error(error, recovery));
                }
                return Err(api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "settings.asr_preload_failed",
                    error,
                ));
            }
        }
    } else {
        None
    };

    if let Err(error) = save_config(&state.config_path, &candidate) {
        if let Err(recovery) = restore_previous(
            state,
            &current,
            &previous_model_dir,
            RestorePreviousOptions {
                model_directory: model_directory_changed,
                ..Default::default()
            },
            plan,
        )
        .await
        {
            return Err(rollback_error(error, recovery));
        }
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "settings.invalid",
            error,
        ));
    }

    let previous_engine = if asr_changed {
        match update_asr_runtime(state, &candidate, candidate_model_dir, prepared_engine).await {
            Ok(engine) => engine,
            Err(error) => {
                if let Err(recovery) = restore_previous(
                    state,
                    &current,
                    &previous_model_dir,
                    RestorePreviousOptions {
                        model_directory: model_directory_changed,
                        persisted_config: true,
                        asr_runtime: true,
                        ..Default::default()
                    },
                    plan,
                )
                .await
                {
                    return Err(rollback_error(error, recovery));
                }
                return Err(api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "settings.asr_update_failed",
                    error,
                ));
            }
        }
    } else {
        None
    };

    if reload_capture {
        if let Err(error) = super::capture::start_pipelines(state, &candidate, plan).await {
            let detail = api_detail(&error);
            if let Err(recovery) = restore_previous(
                state,
                &current,
                &previous_model_dir,
                RestorePreviousOptions {
                    model_directory: model_directory_changed,
                    persisted_config: true,
                    asr_runtime: asr_changed,
                    prepared_engine: previous_engine,
                },
                plan,
            )
            .await
            {
                return Err(rollback_error(detail, recovery));
            }
            return Err(error);
        }
    }

    if reload_external_api {
        let token = if candidate.external_api.enabled && candidate.external_api.require_token {
            match credentials::read_external_api_token() {
                Ok(token) => token,
                Err(error) => {
                    let error = api_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "external_api.token_status_failed",
                        error,
                    );
                    let detail = api_detail(&error);
                    if let Err(recovery) = restore_previous(
                        state,
                        &current,
                        &previous_model_dir,
                        RestorePreviousOptions {
                            model_directory: model_directory_changed,
                            persisted_config: true,
                            asr_runtime: asr_changed,
                            prepared_engine: previous_engine,
                        },
                        plan,
                    )
                    .await
                    {
                        return Err(rollback_error(detail, recovery));
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };
        if let Err(error) = reload_external_api_runtime(state, &candidate.external_api, token).await
        {
            let api_error = api_error(
                StatusCode::CONFLICT,
                "settings.external_api_reload_failed",
                error,
            );
            let detail = api_detail(&api_error);
            if let Err(recovery) = restore_previous(
                state,
                &current,
                &previous_model_dir,
                RestorePreviousOptions {
                    model_directory: model_directory_changed,
                    persisted_config: true,
                    asr_runtime: asr_changed,
                    prepared_engine: previous_engine,
                },
                plan,
            )
            .await
            {
                return Err(rollback_error(detail, recovery));
            }
            return Err(api_error);
        }
    }

    let storage_quota_changed =
        candidate.storage.subtitle_history_max_bytes != current.storage.subtitle_history_max_bytes;
    let glossary_refresh_ids = state
        .glossary_subscription
        .set_sources(candidate.translation.prompt.glossary_sources.clone());
    *state.config.write().expect("config lock") = candidate.clone();
    if storage_quota_changed {
        let max_bytes = candidate.storage.subtitle_history_max_bytes;
        let conversation_catalog = state.conversation_catalog_tx.clone();
        if let Err(error) = super::db_call(Arc::clone(&state.db), move |db| {
            if db.set_subtitle_history_max_bytes(max_bytes)? {
                publish_latest_catalog(db, &conversation_catalog);
            }
            Ok(())
        })
        .await
        {
            tracing::warn!(%error, "subtitle history storage quota could not be enforced immediately");
        }
    }
    state.osc.update_config(candidate.osc.clone());
    state
        .vrchat_mute_sync
        .update_enabled(candidate.osc.mute_sync_enabled);
    if candidate.vrcx != current.vrcx {
        let token = credentials::read_vrcx_token().unwrap_or_else(|error| {
            tracing::warn!(%error, "VRCX-0 token could not be read after settings update");
            None
        });
        state.vrcx.reconfigure(candidate.vrcx.clone(), token).await;
    }
    if !glossary_refresh_ids.is_empty() {
        let glossary_subscription = Arc::clone(&state.glossary_subscription);
        tokio::spawn(async move {
            for id in glossary_refresh_ids {
                if let Err(error) = glossary_subscription.refresh(&id).await {
                    tracing::warn!(subscription_id = %id, code = error.code, detail = %error.detail, "glossary subscription refresh failed");
                }
            }
        });
    }
    if candidate.vr_overlay != current.vr_overlay {
        state
            .vr_overlay_config_tx
            .send_replace(candidate.vr_overlay.clone());
    }
    Ok(state.config_revision.fetch_add(1, Ordering::SeqCst) + 1)
}

pub(super) async fn reload_external_api_runtime(
    state: &Arc<AppState>,
    config: &crate::config::ExternalApiConfig,
    token: Option<String>,
) -> Result<(), String> {
    let mut server = state.external_api_server.lock().await;
    let result = crate::external_api::reconfigure(
        &mut server,
        config,
        state.domain_events.clone(),
        token,
        state.shutdown.clone(),
    )
    .await;
    let status = match server.as_ref() {
        Some(server) => crate::external_api::ExternalApiRuntimeStatus::running(server.address),
        None if config.enabled => crate::external_api::ExternalApiRuntimeStatus::failed(
            result
                .as_ref()
                .err()
                .cloned()
                .unwrap_or_else(|| "External API listener is unavailable".into()),
        ),
        None => crate::external_api::ExternalApiRuntimeStatus::disabled(),
    };
    *state
        .external_api_status
        .write()
        .expect("External API status lock") = status;
    result
}

async fn move_model_directory(
    state: &Arc<AppState>,
    path: std::path::PathBuf,
) -> Result<(), String> {
    let manager = Arc::clone(&state.model_manager);
    tokio::task::spawn_blocking(move || manager.move_model_dir(path))
        .await
        .map_err(|error| format!("Model directory migration task failed: {error}"))?
}

fn local_asr_required(config: &AppConfig) -> bool {
    config.asr.backend == "local_whisper" || config.asr.cloud_failure_policy == "local"
}

async fn prepare_asr_runtime(
    config: &AppConfig,
    model_directory: std::path::PathBuf,
) -> Result<Box<dyn crate::asr::AsrEngine>, String> {
    let asr_config = config.asr.clone();
    tokio::task::spawn_blocking(move || {
        crate::asr::prepare_local_engine(&asr_config, &model_directory)
    })
    .await
    .map_err(|error| format!("ASR model preload task failed: {error}"))?
}

async fn update_asr_runtime(
    state: &Arc<AppState>,
    config: &AppConfig,
    model_directory: std::path::PathBuf,
    prepared_engine: Option<Box<dyn crate::asr::AsrEngine>>,
) -> Result<Option<Box<dyn crate::asr::AsrEngine>>, String> {
    let asr = Arc::clone(&state.asr);
    let asr_config = config.asr.clone();
    tokio::task::spawn_blocking(move || {
        let previous_engine = asr
            .lock()
            .map_err(|_| "The ASR inference lock is unavailable".to_string())?
            .update(asr_config, model_directory, prepared_engine);
        Ok::<_, String>(previous_engine)
    })
    .await
    .map_err(|error| format!("ASR configuration update task failed: {error}"))?
}

#[derive(Default)]
struct RestorePreviousOptions {
    model_directory: bool,
    persisted_config: bool,
    asr_runtime: bool,
    prepared_engine: Option<Box<dyn crate::asr::AsrEngine>>,
}

async fn restore_previous(
    state: &Arc<AppState>,
    previous: &AppConfig,
    previous_model_dir: &std::path::Path,
    options: RestorePreviousOptions,
    plan: super::capture::CaptureReloadPlan,
) -> Result<(), String> {
    let mut errors = Vec::new();
    super::capture::stop_pipelines(state, plan).await;
    if options.model_directory {
        if let Err(error) = move_model_directory(state, previous_model_dir.to_path_buf()).await {
            errors.push(error);
        }
    }
    if options.persisted_config {
        if let Err(error) = save_config(&state.config_path, previous) {
            errors.push(format!("Previous settings could not be restored: {error}"));
        }
    }
    if options.asr_runtime {
        let model_directory = state.asr_model_dir_override.clone().unwrap_or_else(|| {
            crate::resolve_config_path(&state.config_path, &previous.storage.model_directory)
        });
        if let Err(error) =
            update_asr_runtime(state, previous, model_directory, options.prepared_engine).await
        {
            errors.push(error);
        }
    }
    if state.capture_requested.load(Ordering::SeqCst) {
        if let Err(error) = super::capture::start_pipelines(state, previous, plan).await {
            errors.push(api_detail(&error));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn api_detail(error: &(StatusCode, Json<Value>)) -> String {
    error
        .1
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or("Capture reconfiguration failed")
        .to_string()
}

fn rollback_error(
    error: impl std::fmt::Display,
    recovery: impl std::fmt::Display,
) -> (StatusCode, Json<Value>) {
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "settings.rollback_failed",
        format!("{error}; rollback failed: {recovery}"),
    )
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

fn protect_profile_owned_settings(
    candidate: &mut AppConfig,
    current: &AppConfig,
    has_current_revision: bool,
) {
    candidate.asr.api_profiles = current.asr.api_profiles.clone();

    if !has_current_revision {
        candidate.asr.backend = current.asr.backend.clone();
        candidate.asr.active_profile_id = current.asr.active_profile_id.clone();
        candidate.asr.service_settings = current.asr.service_settings.clone();
    } else {
        for (service_id, settings) in &current.asr.service_settings {
            candidate
                .asr
                .service_settings
                .entry(service_id.clone())
                .or_insert_with(|| settings.clone());
        }
        if candidate.asr.backend == "local_whisper" {
            candidate.asr.active_profile_id = None;
        } else if !valid_active_selection(&candidate.asr) {
            candidate.asr.backend = current.asr.backend.clone();
            candidate.asr.active_profile_id = current.asr.active_profile_id.clone();
        }
    }

    if candidate
        .translation
        .profile_id
        .as_deref()
        .is_some_and(|profile_id| {
            !current
                .asr
                .api_profiles
                .iter()
                .any(|profile| profile.id == profile_id)
        })
    {
        candidate.translation.profile_id = current.translation.profile_id.clone();
        candidate.translation.mode = current.translation.mode.clone();
    }
}

fn valid_active_selection(asr: &crate::config::AsrConfig) -> bool {
    let Some(profile_id) = asr.active_profile_id.as_deref() else {
        return false;
    };
    asr.api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .is_some_and(|profile| providers::resolve_profile_service(profile, &asr.backend).is_ok())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiProfile, RecognitionServiceSettings};
    use crate::providers::{CAPABILITY_SPEECH_TO_TEXT, GROQ_PROVIDER, SERVICE_GROQ_TRANSCRIPTION};

    fn groq_profile() -> ApiProfile {
        ApiProfile {
            id: "groq-profile".into(),
            name: "Groq".into(),
            provider: GROQ_PROVIDER.into(),
            enabled_capabilities: vec![CAPABILITY_SPEECH_TO_TEXT.into()],
            ..ApiProfile::default()
        }
    }

    #[test]
    fn unversioned_payload_cannot_restore_deleted_active_profile_or_service_settings() {
        let mut current = AppConfig::default();
        current.asr.backend = "local_whisper".into();
        current.asr.active_profile_id = None;
        current.asr.service_settings.insert(
            SERVICE_GROQ_TRANSCRIPTION.into(),
            RecognitionServiceSettings {
                model: "whisper-large-v3".into(),
                context: "current".into(),
            },
        );
        let mut candidate = current.clone();
        candidate.asr.api_profiles.push(groq_profile());
        candidate.asr.backend = SERVICE_GROQ_TRANSCRIPTION.into();
        candidate.asr.active_profile_id = Some("groq-profile".into());
        candidate
            .asr
            .service_settings
            .get_mut(SERVICE_GROQ_TRANSCRIPTION)
            .unwrap()
            .context = "stale".into();

        protect_profile_owned_settings(&mut candidate, &current, false);

        assert!(candidate.asr.api_profiles.is_empty());
        assert_eq!(candidate.asr.backend, "local_whisper");
        assert_eq!(candidate.asr.active_profile_id, None);
        assert_eq!(
            candidate.asr.service_settings[SERVICE_GROQ_TRANSCRIPTION].context,
            "current"
        );
    }

    #[test]
    fn versioned_payload_can_update_service_settings_but_keeps_missing_entries() {
        let current = AppConfig::default();
        let mut candidate = current.clone();
        candidate
            .asr
            .service_settings
            .remove(SERVICE_GROQ_TRANSCRIPTION);
        candidate
            .asr
            .service_settings
            .get_mut(crate::providers::SERVICE_QWEN_REALTIME)
            .unwrap()
            .context = "updated".into();

        protect_profile_owned_settings(&mut candidate, &current, true);

        assert_eq!(
            candidate.asr.service_settings[crate::providers::SERVICE_QWEN_REALTIME].context,
            "updated"
        );
        assert!(candidate
            .asr
            .service_settings
            .contains_key(SERVICE_GROQ_TRANSCRIPTION));
    }
}
