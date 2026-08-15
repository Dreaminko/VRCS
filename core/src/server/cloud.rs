use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::asr;
use crate::config::{
    save_config, ApiAuthMode, ApiProfile, HttpHeaderConfig, DEFAULT_PROFILE_TIMEOUT_MS,
};
use crate::providers::{
    self, ALIBABA_PROVIDER, API_PURPOSE_LLM, DEEPL_PROVIDER, GEMINI_PROVIDER, MICROSOFT_PROVIDER,
    OPENAI_COMPATIBLE_PROVIDER, OPENAI_PROVIDER,
};

use super::{api_error, ApiResult, AppState};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateProfileInput {
    name: String,
    provider: String,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    purpose: Option<String>,
    #[serde(default)]
    preset_id: Option<String>,
    #[serde(default)]
    auth_mode: Option<ApiAuthMode>,
    #[serde(default)]
    is_local: Option<bool>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    headers: Option<Vec<HttpHeaderConfig>>,
    #[serde(default)]
    api_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateProfileInput {
    name: String,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    purpose: Option<String>,
    #[serde(default)]
    preset_id: Option<String>,
    #[serde(default)]
    auth_mode: Option<ApiAuthMode>,
    #[serde(default)]
    is_local: Option<bool>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    headers: Option<Vec<HttpHeaderConfig>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CredentialInput {
    api_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActiveProfileInput {
    profile_id: Option<String>,
}

pub(super) async fn provider_list() -> Json<Value> {
    Json(json!({ "providers": crate::providers::catalog() }))
}

fn profile_value(profile: &ApiProfile, config: &crate::config::AppConfig) -> Result<Value, String> {
    let status = asr::credential_status(&profile.id, &profile.provider)?;
    let provider = crate::providers::definition(&profile.provider)
        .ok_or_else(|| format!("Unsupported API provider: {}", profile.provider))?;
    let active = match profile.provider.as_str() {
        ALIBABA_PROVIDER => config.asr.active_api_profiles.alibaba_cloud.as_deref(),
        OPENAI_PROVIDER => config.asr.active_api_profiles.openai.as_deref(),
        _ => None,
    } == Some(profile.id.as_str());
    let translation_active = config.translation.profile_id.as_deref() == Some(profile.id.as_str());
    Ok(json!({
        "id": profile.id,
        "name": profile.name,
        "provider": profile.provider,
        "provider_display_name": provider.display_name,
        "region": profile.region,
        "workspace_id": profile.workspace_id,
        "base_url": profile.base_url,
        "purpose": providers::effective_purpose(profile),
        "preset_id": profile.preset_id,
        "auth_mode": profile.auth_mode,
        "is_local": profile.is_local,
        "timeout_ms": profile.timeout_ms,
        "headers": profile.headers,
        "active": active,
        "translation_active": translation_active,
        "credential": status,
        "capabilities": crate::providers::profile_capabilities(profile),
        "support_levels": crate::providers::profile_support_levels(profile),
    }))
}

pub(super) async fn profile_list(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let config = state.config.read().expect("config lock").clone();
    let profiles = config
        .asr
        .api_profiles
        .iter()
        .map(|profile| profile_value(profile, &config))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| credential_error("list", error))?;
    Ok(Json(json!({ "profiles": profiles })))
}

pub(super) async fn profile_create(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateProfileInput>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config_control.lock().await;
    let preset = input.preset_id.as_deref().and_then(|id| {
        crate::providers::OPENAI_COMPATIBLE_PRESETS
            .iter()
            .find(|item| item.id == id)
    });
    let mut profile = ApiProfile {
        id: uuid::Uuid::new_v4().to_string(),
        name: input.name.trim().to_string(),
        provider: input.provider,
        region: input.region,
        workspace_id: input.workspace_id.map(|value| value.trim().to_string()),
        base_url: input
            .base_url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                preset
                    .filter(|item| !item.base_url.is_empty())
                    .map(|item| item.base_url.into())
            }),
        purpose: input.purpose,
        preset_id: input.preset_id,
        auth_mode: input
            .auth_mode
            .unwrap_or_else(|| preset.map(|item| item.auth_mode).unwrap_or_default()),
        is_local: input
            .is_local
            .unwrap_or_else(|| preset.is_some_and(|item| item.is_local)),
        timeout_ms: input.timeout_ms.unwrap_or(DEFAULT_PROFILE_TIMEOUT_MS),
        headers: input.headers.unwrap_or_default(),
    };
    normalize_profile_fields(&mut profile);
    apply_default_name_for_new_llm_profile(&mut profile);
    let mut candidate = state.config.read().expect("config lock").clone();
    candidate.asr.api_profiles.push(profile.clone());
    commit_profile_config(&state, candidate).await?;
    if let Some(api_key) = input.api_key.filter(|value| !value.trim().is_empty()) {
        if let Err(error) = asr::write_credential(&profile.id, &profile.provider, &api_key) {
            let mut rollback = state.config.read().expect("config lock").clone();
            rollback
                .asr
                .api_profiles
                .retain(|item| item.id != profile.id);
            let _ = commit_profile_config(&state, rollback).await;
            return Err(credential_error(&profile.id, error));
        }
    }
    let config = state.config.read().expect("config lock").clone();
    Ok(Json(
        profile_value(&profile, &config).map_err(|error| credential_error(&profile.id, error))?,
    ))
}

pub(super) async fn profile_update(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
    Json(input): Json<UpdateProfileInput>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config_control.lock().await;
    let mut candidate = state.config.read().expect("config lock").clone();
    let profile = candidate
        .asr
        .api_profiles
        .iter_mut()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(profile_not_found)?;
    profile.name = input.name.trim().to_string();
    if profile.provider == ALIBABA_PROVIDER || profile.provider == MICROSOFT_PROVIDER {
        profile.region = input.region;
    }
    if profile.provider == ALIBABA_PROVIDER {
        profile.workspace_id = input.workspace_id.map(|value| value.trim().to_string());
    }
    if matches!(
        profile.provider.as_str(),
        OPENAI_PROVIDER | OPENAI_COMPATIBLE_PROVIDER
    ) {
        profile.base_url = input.base_url.map(|value| value.trim().to_string());
    }
    if profile.provider == OPENAI_COMPATIBLE_PROVIDER {
        profile.preset_id = input.preset_id;
        if let Some(auth_mode) = input.auth_mode {
            profile.auth_mode = auth_mode;
        }
        if let Some(is_local) = input.is_local {
            profile.is_local = is_local;
        }
        if let Some(timeout_ms) = input.timeout_ms {
            profile.timeout_ms = timeout_ms;
        }
        if let Some(headers) = input.headers {
            profile.headers = headers;
        }
    }
    if input.purpose.is_some() {
        profile.purpose = input.purpose;
    }
    normalize_profile_fields(profile);
    let disable_realtime = !providers::supports_realtime_asr(profile);
    let disable_translation = !providers::supports_translation(profile);
    let updated = profile.clone();
    if disable_realtime {
        if candidate.asr.active_api_profiles.openai.as_deref() == Some(profile_id.as_str()) {
            candidate.asr.active_api_profiles.openai = None;
            if candidate.asr.backend == "openai_realtime" {
                candidate.asr.backend = "local_whisper".into();
            }
        }
        if candidate.asr.active_api_profiles.alibaba_cloud.as_deref() == Some(profile_id.as_str()) {
            candidate.asr.active_api_profiles.alibaba_cloud = None;
            if matches!(
                candidate.asr.backend.as_str(),
                "qwen_realtime" | "fun_asr_realtime"
            ) {
                candidate.asr.backend = "local_whisper".into();
            }
        }
    }
    if disable_translation
        && candidate.translation.profile_id.as_deref() == Some(profile_id.as_str())
    {
        candidate.translation.profile_id = None;
        candidate.translation.mode = "disabled".into();
        candidate.translation.translate_microphone = false;
    }
    commit_profile_config(&state, candidate).await?;
    let config = state.config.read().expect("config lock").clone();
    Ok(Json(
        profile_value(&updated, &config).map_err(|error| credential_error(&profile_id, error))?,
    ))
}

pub(super) async fn profile_delete(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config_control.lock().await;
    let mut candidate = state.config.read().expect("config lock").clone();
    let profile = candidate
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .cloned()
        .ok_or_else(profile_not_found)?;
    candidate
        .asr
        .api_profiles
        .retain(|item| item.id != profile_id);
    if candidate.asr.active_api_profiles.alibaba_cloud.as_deref() == Some(profile_id.as_str()) {
        candidate.asr.active_api_profiles.alibaba_cloud = None;
    }
    if candidate.asr.active_api_profiles.openai.as_deref() == Some(profile_id.as_str()) {
        candidate.asr.active_api_profiles.openai = None;
    }
    if candidate.translation.profile_id.as_deref() == Some(profile_id.as_str()) {
        candidate.translation.profile_id = None;
        candidate.translation.mode = "disabled".into();
        candidate.translation.translate_microphone = false;
    }
    commit_profile_config(&state, candidate).await?;
    asr::delete_credential(&profile.id, &profile.provider)
        .map_err(|error| credential_error(&profile_id, error))?;
    Ok(Json(json!({ "deleted": true })))
}

pub(super) async fn credential_write(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
    Json(input): Json<CredentialInput>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config_control.lock().await;
    ensure_capture_stopped(&state).await?;
    let config = state.config.read().expect("config lock").clone();
    let profile = config
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(profile_not_found)?;
    asr::write_credential(&profile.id, &profile.provider, &input.api_key)
        .map_err(|error| credential_error(&profile_id, error))?;
    Ok(Json(
        profile_value(profile, &config).map_err(|error| credential_error(&profile_id, error))?,
    ))
}

pub(super) async fn credential_delete(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config_control.lock().await;
    ensure_capture_stopped(&state).await?;
    let config = state.config.read().expect("config lock").clone();
    let profile = config
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(profile_not_found)?;
    if asr::credential_status(&profile.id, &profile.provider)
        .map_err(|error| credential_error(&profile_id, error))?
        .environment_override
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            "asr.credential_environment_managed",
            "This API key is managed by an environment variable",
        ));
    }
    asr::delete_credential(&profile.id, &profile.provider)
        .map_err(|error| credential_error(&profile_id, error))?;
    Ok(Json(
        profile_value(profile, &config).map_err(|error| credential_error(&profile_id, error))?,
    ))
}

pub(super) async fn profile_activate(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<String>,
    Json(input): Json<ActiveProfileInput>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config_control.lock().await;
    if ![ALIBABA_PROVIDER, OPENAI_PROVIDER].contains(&provider.as_str()) {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "asr.profile_invalid",
            "This API provider does not support speech recognition",
        ));
    }
    let mut candidate = state.config.read().expect("config lock").clone();
    if let Some(profile_id) = input.profile_id.as_deref() {
        let profile = candidate
            .asr
            .api_profiles
            .iter()
            .find(|profile| profile.id == profile_id && profile.provider == provider)
            .ok_or_else(profile_not_found)?;
        let status = asr::credential_status(&profile.id, &profile.provider)
            .map_err(|error| credential_error(profile_id, error))?;
        if !status.configured {
            return Err(api_error(
                StatusCode::CONFLICT,
                "asr.credential_missing",
                "Configure an API key before activating this profile",
            ));
        }
        if !providers::supports_realtime_asr(profile) {
            return Err(api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "asr.profile_invalid",
                "This API profile does not support realtime speech recognition",
            ));
        }
        ensure_asr_profile_ready(profile)?;
    }
    match provider.as_str() {
        ALIBABA_PROVIDER => candidate.asr.active_api_profiles.alibaba_cloud = input.profile_id,
        OPENAI_PROVIDER => candidate.asr.active_api_profiles.openai = input.profile_id,
        _ => unreachable!(),
    }
    commit_profile_config(&state, candidate).await?;
    profile_list(State(Arc::clone(&state))).await
}

pub(super) async fn profile_models(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let config = state.config.read().expect("config lock").clone();
    let profile = config
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(profile_not_found)?;
    if !providers::supports_llm_models(profile) {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "llm.models_unsupported",
            "This API provider does not expose LLM models",
        ));
    }
    let api_key = profile_api_key(profile)?;
    let models = crate::llm::LlmClient::new(state.http.clone())
        .list_models(profile, &api_key)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.code, error.detail))?;
    Ok(Json(json!({ "models": models })))
}

pub(super) fn profile_api_key(profile: &ApiProfile) -> ApiResult<String> {
    if !profile.requires_api_key() {
        return Ok(String::new());
    }
    asr::read_credential(&profile.id, &profile.provider)
        .map_err(|error| credential_error(&profile.id, error))?
        .ok_or_else(|| {
            api_error(
                StatusCode::CONFLICT,
                "translation.credential_missing",
                "Configure an API key before using this profile",
            )
        })
}

async fn commit_profile_config(
    state: &Arc<AppState>,
    candidate: crate::config::AppConfig,
) -> ApiResult<()> {
    ensure_capture_stopped(state).await?;
    candidate.validate_settings().map_err(|error| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "asr.profile_invalid",
            error,
        )
    })?;
    save_config(&state.config_path, &candidate).map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "asr.profile_save_failed",
            error,
        )
    })?;
    *state.config.write().expect("config lock") = candidate;
    state
        .config_revision
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

async fn ensure_capture_stopped(state: &Arc<AppState>) -> ApiResult<()> {
    let _control = state.capture_control.lock().await;
    if state.speaker_pipeline.lock().await.running()
        || state.microphone_pipeline.lock().await.running()
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            "settings.capture_must_be_stopped",
            "Stop transcription before changing API profiles",
        ));
    }
    Ok(())
}

pub(super) fn profile_not_found() -> (StatusCode, Json<Value>) {
    api_error(
        StatusCode::NOT_FOUND,
        "asr.profile_not_found",
        "The API profile does not exist",
    )
}

pub(super) fn ensure_asr_profile_ready(profile: &ApiProfile) -> ApiResult<()> {
    if profile.provider == ALIBABA_PROVIDER
        && !profile
            .workspace_id
            .as_deref()
            .is_some_and(|workspace| !workspace.trim().is_empty())
    {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "asr.alibaba_workspace_missing",
            "Configure an Alibaba Cloud Workspace ID before using speech recognition",
        ));
    }
    Ok(())
}

fn credential_error(profile_id: &str, detail: String) -> (StatusCode, Json<Value>) {
    let status = if detail.contains("Unsupported") || detail.contains("length") {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    api_error(
        status,
        "asr.credential_failed",
        format!("{profile_id}: {detail}"),
    )
}

fn normalize_profile_fields(profile: &mut ApiProfile) {
    match profile.provider.as_str() {
        ALIBABA_PROVIDER => profile.base_url = None,
        MICROSOFT_PROVIDER => {
            profile.workspace_id = None;
            profile.base_url = None;
        }
        OPENAI_PROVIDER => {
            profile.region = None;
            profile.workspace_id = None;
            profile.base_url = None;
        }
        OPENAI_COMPATIBLE_PROVIDER => {
            profile.region = None;
            profile.workspace_id = None;
            profile.purpose = Some(API_PURPOSE_LLM.into());
            if profile.base_url.as_deref().is_some_and(str::is_empty) {
                profile.base_url = None;
            }
            for header in &mut profile.headers {
                header.name = header.name.trim().to_string();
            }
        }
        GEMINI_PROVIDER | DEEPL_PROVIDER => {
            profile.region = None;
            profile.workspace_id = None;
            profile.base_url = None;
        }
        _ => profile.base_url = None,
    }
    if matches!(
        profile.provider.as_str(),
        DEEPL_PROVIDER | GEMINI_PROVIDER | MICROSOFT_PROVIDER | OPENAI_COMPATIBLE_PROVIDER
    ) {
        profile.purpose = Some(API_PURPOSE_LLM.into());
    }
    if profile.provider != OPENAI_COMPATIBLE_PROVIDER {
        profile.preset_id = None;
        profile.auth_mode = ApiAuthMode::Bearer;
        profile.is_local = false;
        profile.headers.clear();
    }
}

fn apply_default_name_for_new_llm_profile(profile: &mut ApiProfile) {
    if !profile.name.trim().is_empty() || providers::effective_purpose(profile) != API_PURPOSE_LLM {
        return;
    }
    profile.name = if profile.provider == OPENAI_COMPATIBLE_PROVIDER {
        profile
            .preset_id
            .as_deref()
            .and_then(|id| {
                providers::OPENAI_COMPATIBLE_PRESETS
                    .iter()
                    .find(|preset| preset.id == id)
            })
            .map(|preset| preset.display_name)
            .or_else(|| {
                providers::definition(&profile.provider).map(|provider| provider.display_name)
            })
    } else {
        providers::definition(&profile.provider).map(|provider| provider.display_name)
    }
    .unwrap_or("LLM API")
    .to_string();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::API_PURPOSE_SHARED;

    #[test]
    fn new_llm_profile_uses_its_preset_name_when_name_is_blank() {
        let mut profile = ApiProfile {
            name: "  ".into(),
            provider: OPENAI_COMPATIBLE_PROVIDER.into(),
            purpose: Some(API_PURPOSE_LLM.into()),
            preset_id: Some("deepseek".into()),
            ..ApiProfile::default()
        };

        apply_default_name_for_new_llm_profile(&mut profile);

        assert_eq!(profile.name, "DeepSeek");
    }

    #[test]
    fn new_non_llm_profile_still_requires_an_explicit_name() {
        let mut profile = ApiProfile {
            name: String::new(),
            provider: OPENAI_PROVIDER.into(),
            purpose: Some("asr".into()),
            ..ApiProfile::default()
        };

        apply_default_name_for_new_llm_profile(&mut profile);

        assert!(profile.name.is_empty());
    }

    #[test]
    fn alibaba_asr_requires_a_workspace() {
        let profile = ApiProfile {
            id: "alibaba".into(),
            name: "Alibaba".into(),
            provider: ALIBABA_PROVIDER.into(),
            region: Some("china_beijing".into()),
            workspace_id: None,
            base_url: None,
            purpose: Some(API_PURPOSE_SHARED.into()),
            ..ApiProfile::default()
        };

        let (_, body) = ensure_asr_profile_ready(&profile).unwrap_err();
        assert_eq!(body["code"], "asr.alibaba_workspace_missing");

        let ready = ApiProfile {
            workspace_id: Some("workspace-one".into()),
            ..profile
        };
        assert!(ensure_asr_profile_ready(&ready).is_ok());
    }
}
