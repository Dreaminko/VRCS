use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::Json;
use serde_json::{json, Value};

use crate::config::AppConfig;
use crate::models::SettingsUpdate;
use crate::providers;

use super::{api_error, ApiResult, SettingsContext, CONFIG_REVISION_HEADER};

mod change_plan;

pub(super) async fn get_settings(State(state): State<SettingsContext>) -> (HeaderMap, Json<Value>) {
    let _config_control = state.config.config_control.lock().await;
    let config = state.config.config.read().expect("config lock").clone();
    let revision = state.config.config_revision.load(Ordering::SeqCst);
    (revision_headers(&state, revision), Json(json!(config)))
}

pub(super) async fn update_settings(
    State(state): State<SettingsContext>,
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
    let candidate = AppConfig {
        schema_version: update.schema_version,
        server: update.server,
        storage: update.storage,
        audio: update.audio,
        vad: update.vad,
        asr: update.asr,
        glossary: update.glossary,
        translation: update.translation,
        language_presets: update.language_presets,
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
    let _config_control = state.config.config_control.lock().await;
    let current_revision = state.config.config_revision.load(Ordering::SeqCst);
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
    let _capture_control = state.capture.capture_control.lock().await;
    let result = change_plan::SettingsChangePlan::prepare_update(
        &state,
        candidate,
        expected_revision.is_some(),
    )
    .await?
    .apply(&state)
    .await?;
    Ok((
        revision_headers(&state, result.revision),
        Json(json!(result.config)),
    ))
}

pub(super) async fn commit_candidate(
    state: &SettingsContext,
    candidate: AppConfig,
) -> ApiResult<u64> {
    let _capture_control = state.capture.capture_control.lock().await;
    let result = change_plan::SettingsChangePlan::prepare(state, candidate)
        .await?
        .apply(state)
        .await?;
    Ok(result.revision)
}

pub(super) async fn reload_external_api_runtime(
    state: &SettingsContext,
    config: &crate::config::ExternalApiConfig,
    token: Option<String>,
) -> Result<(), String> {
    change_plan::reload_external_api_runtime(state, config, token).await
}

fn revision_token(state: &SettingsContext, revision: u64) -> String {
    format!("{}:{revision}", state.config.config_epoch)
}

fn revision_headers(state: &SettingsContext, revision: u64) -> HeaderMap {
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

    let profile_exists = |profile_id: &str| {
        current
            .asr
            .api_profiles
            .iter()
            .any(|profile| profile.id == profile_id)
    };
    if candidate
        .translation
        .speaker_targets
        .iter()
        .chain(&candidate.translation.microphone_targets)
        .filter_map(|target| target.profile_id.as_deref())
        .any(|profile_id| !profile_exists(profile_id))
    {
        candidate.translation = current.translation.clone();
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
    use crate::config::{
        ApiProfile, GlossaryCategory, GlossaryEntry, GlossarySource, RecognitionServiceSettings,
    };
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
    fn settings_payload_reads_glossary_from_the_top_level() {
        let mut config = AppConfig::default();
        config.glossary.sources.push(GlossarySource::Local {
            id: "local".into(),
            name: "Local".into(),
            enabled: true,
            entries: vec![GlossaryEntry {
                source: "VRChat".into(),
                target: None,
                category: GlossaryCategory::Game,
                case_sensitive: false,
            }],
        });

        let update = parse_settings_update(&serde_json::to_vec(&config).unwrap()).unwrap();

        assert_eq!(update.glossary, config.glossary);
    }

    #[test]
    fn settings_payload_rejects_the_legacy_nested_glossary_sources() {
        let mut value = serde_json::to_value(AppConfig::default()).unwrap();
        value["translation"]["prompt"]["glossary_sources"] = serde_json::json!([]);

        let error = parse_settings_update(&serde_json::to_vec(&value).unwrap()).unwrap_err();

        assert!(error.contains("translation.prompt.glossary_sources"));
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
