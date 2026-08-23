use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::asr;
use crate::config::{ApiAuthMode, ApiProfile, HttpHeaderConfig, DEFAULT_PROFILE_TIMEOUT_MS};
use crate::providers::{
    self, BaseUrlPolicy, ProviderCategory, ProviderDefinition, ProviderServiceDefinition,
    ServiceAdapter, ALIBABA_PROVIDER, CAPABILITY_SPEECH_TO_TEXT, CAPABILITY_TEXT_GENERATION,
    CAPABILITY_TEXT_TRANSLATION, MICROSOFT_PROVIDER,
};

use super::{api_error, ApiResult, SettingsContext};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateProfileInput {
    name: String,
    provider: String,
    #[serde(default)]
    enabled_capabilities: Option<Vec<String>>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
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
    enabled_capabilities: Option<Vec<String>>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
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
    profile_id: String,
    service_id: String,
}

pub(super) async fn provider_list() -> Json<Value> {
    let providers = providers::catalog()
        .into_iter()
        .map(provider_value)
        .collect::<Vec<_>>();
    Json(json!({ "providers": providers }))
}

fn provider_value(provider: ProviderDefinition) -> Value {
    let (base_url_mode, base_url_default) = match provider.connection.base_url {
        BaseUrlPolicy::Fixed(value) => ("fixed", Some(value)),
        BaseUrlPolicy::Regional => ("fixed", None),
        BaseUrlPolicy::Editable(value) => ("editable", (!value.is_empty()).then_some(value)),
    };
    let auth_modes = if provider.category == ProviderCategory::CustomProtocol {
        vec![ApiAuthMode::Bearer, ApiAuthMode::None]
    } else {
        vec![provider.connection.auth_mode]
    };
    let fields = match provider.id {
        ALIBABA_PROVIDER => vec![
            json!({
                "id": "region",
                "label": "Region",
                "type": "select",
                "required": true,
                "default": "singapore",
                "options": [
                    { "value": "singapore", "label": "Singapore" },
                    { "value": "china_beijing", "label": "China (Beijing)" }
                ]
            }),
            json!({
                "id": "workspace_id",
                "label": "Workspace ID",
                "type": "text",
                "required": false,
                "default": null
            }),
        ],
        MICROSOFT_PROVIDER => vec![json!({
            "id": "region",
            "label": "Region",
            "type": "text",
            "required": true,
            "default": null
        })],
        _ => Vec::new(),
    };
    let services = provider
        .services
        .iter()
        .map(service_value)
        .collect::<Vec<_>>();
    json!({
        "id": provider.id,
        "display_name": provider.display_name,
        "category": provider.category,
        "connection": {
            "base_url": {
                "mode": base_url_mode,
                "default": base_url_default,
            },
            "auth_modes": auth_modes,
            "default_auth_mode": provider.connection.auth_mode,
            "fields": fields,
        },
        "services": services,
        "support_levels": provider.support_levels,
        "capabilities": provider.capabilities,
        "presets": provider.presets,
    })
}

fn service_value(service: &ProviderServiceDefinition) -> Value {
    json!({
        "id": service.id,
        "display_name": service.display_name,
        "capabilities": service.capabilities,
        "adapter": adapter_id(service.adapter),
        "recognition_transport": service.recognition_transport,
        "partial_results": service.partial_results,
        "models": service.models,
        "model_listing": service.supports_model_listing,
        "supports_context": service.supports_context,
    })
}

fn adapter_id(adapter: ServiceAdapter) -> &'static str {
    match adapter {
        ServiceAdapter::AlibabaChatCompletions => "alibaba_chat_completions",
        ServiceAdapter::OpenAiResponses => "openai_responses",
        ServiceAdapter::OpenAiChatCompletions { .. } => "openai_chat_completions",
        ServiceAdapter::GeminiGenerateContent => "gemini_generate_content",
        ServiceAdapter::DeepLTextTranslation => "deepl_text_translation",
        ServiceAdapter::MicrosoftTextTranslation => "microsoft_text_translation",
        ServiceAdapter::QwenRealtime => "qwen_realtime",
        ServiceAdapter::AlibabaTokenPlanRealtime => "alibaba_token_plan_realtime",
        ServiceAdapter::FunAsrRealtime => "fun_asr_realtime",
        ServiceAdapter::OpenAiRealtime => "openai_realtime",
        ServiceAdapter::OpenAiAudioTranscriptions => "openai_audio_transcriptions",
    }
}

fn profile_value(profile: &ApiProfile, config: &crate::config::AppConfig) -> Result<Value, String> {
    let status = asr::credential_status(&profile.id, &profile.provider)?;
    let provider = providers::definition(&profile.provider)
        .ok_or_else(|| format!("Unsupported API provider: {}", profile.provider))?;
    let active = config.asr.backend != "local_whisper"
        && config.asr.active_profile_id.as_deref() == Some(profile.id.as_str())
        && providers::resolve_profile_service(profile, &config.asr.backend).is_ok();
    let translation_active = uses_global_translation_profile(config, &profile.id);
    let services = provider
        .services
        .iter()
        .filter(|service| {
            service.capabilities.iter().any(|capability| {
                profile
                    .enabled_capabilities
                    .iter()
                    .any(|enabled| enabled == capability)
            })
        })
        .map(service_value)
        .collect::<Vec<_>>();
    Ok(json!({
        "id": profile.id,
        "name": profile.name,
        "provider": profile.provider,
        "provider_display_name": provider.display_name,
        "region": profile.region,
        "workspace_id": profile.workspace_id,
        "base_url": profile.base_url,
        "enabled_capabilities": profile.enabled_capabilities,
        "preset_id": profile.preset_id,
        "auth_mode": profile.auth_mode,
        "is_local": profile.is_local,
        "timeout_ms": profile.timeout_ms,
        "headers": profile.headers,
        "active": active,
        "translation_active": translation_active,
        "credential": status,
        "capabilities": providers::profile_capabilities(profile),
        "support_levels": providers::profile_support_levels(profile),
        "services": services,
    }))
}

pub(super) async fn profile_list(State(state): State<SettingsContext>) -> ApiResult<Json<Value>> {
    profile_list_response(&state)
}

fn profile_list_response(state: &SettingsContext) -> ApiResult<Json<Value>> {
    let config = state.config.config.read().expect("config lock").clone();
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
    State(state): State<SettingsContext>,
    Json(input): Json<CreateProfileInput>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config.config_control.lock().await;
    let enabled_capabilities = match input.enabled_capabilities {
        Some(capabilities) => capabilities,
        None => providers::provider_capability_ids(&input.provider)
            .ok_or_else(|| {
                profile_invalid(format!("Unsupported API provider: {}", input.provider))
            })?
            .into_iter()
            .map(str::to_owned)
            .collect(),
    };
    let mut profile = ApiProfile {
        id: uuid::Uuid::new_v4().to_string(),
        name: input.name,
        provider: input.provider,
        region: input.region,
        workspace_id: input.workspace_id,
        base_url: input.base_url,
        enabled_capabilities,
        preset_id: input.preset_id,
        auth_mode: input.auth_mode.unwrap_or_default(),
        is_local: input.is_local.unwrap_or(false),
        timeout_ms: input.timeout_ms.unwrap_or(DEFAULT_PROFILE_TIMEOUT_MS),
        headers: input.headers.unwrap_or_default(),
    };
    normalize_profile_fields(&mut profile).map_err(profile_invalid)?;
    let mut candidate = state.config.config.read().expect("config lock").clone();
    candidate.asr.api_profiles.push(profile.clone());
    commit_profile_config(&state, candidate).await?;
    if let Some(api_key) = input.api_key.filter(|value| !value.trim().is_empty()) {
        if let Err(error) = asr::write_credential(&profile.id, &profile.provider, &api_key) {
            let mut rollback = state.config.config.read().expect("config lock").clone();
            rollback
                .asr
                .api_profiles
                .retain(|item| item.id != profile.id);
            let _ = commit_profile_config(&state, rollback).await;
            return Err(credential_error(&profile.id, error));
        }
    }
    let config = state.config.config.read().expect("config lock").clone();
    Ok(Json(
        profile_value(&profile, &config).map_err(|error| credential_error(&profile.id, error))?,
    ))
}

pub(super) async fn profile_update(
    State(state): State<SettingsContext>,
    Path(profile_id): Path<String>,
    Json(input): Json<UpdateProfileInput>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config.config_control.lock().await;
    let mut candidate = state.config.config.read().expect("config lock").clone();
    let updated = {
        let profile = candidate
            .asr
            .api_profiles
            .iter_mut()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(profile_not_found)?;
        profile.name = input.name;
        if let Some(enabled_capabilities) = input.enabled_capabilities {
            profile.enabled_capabilities = enabled_capabilities;
        }
        profile.region = input.region;
        profile.workspace_id = input.workspace_id;
        profile.base_url = input.base_url;
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
        normalize_profile_fields(profile).map_err(profile_invalid)?;
        profile.clone()
    };
    apply_profile_compatibility_fallbacks(&mut candidate, &updated);
    commit_profile_config(&state, candidate).await?;
    let config = state.config.config.read().expect("config lock").clone();
    Ok(Json(
        profile_value(&updated, &config).map_err(|error| credential_error(&profile_id, error))?,
    ))
}

pub(super) async fn profile_delete(
    State(state): State<SettingsContext>,
    Path(profile_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config.config_control.lock().await;
    let current = state.config.config.read().expect("config lock").clone();
    let mut candidate = current.clone();
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
    if candidate.asr.active_profile_id.as_deref() == Some(profile_id.as_str()) {
        disable_cloud_recognition(&mut candidate);
    }
    if uses_translation_profile(&candidate, &profile_id) {
        disable_translation_profile(&mut candidate, &profile_id);
    }
    let previous_credential = asr::read_stored_credential(&profile.id, &profile.provider)
        .map_err(|error| credential_error(&profile_id, error))?;
    commit_profile_config(&state, candidate).await?;
    if let Err(error) = asr::delete_credential(&profile.id, &profile.provider) {
        let mut recovery_errors = Vec::new();
        if let Err(recovery) = restore_credential(&profile, previous_credential) {
            recovery_errors.push(format!("credential rollback failed: {recovery}"));
        }
        if let Err(recovery) = commit_profile_config(&state, current).await {
            recovery_errors.push(api_detail(&recovery));
        }
        if !recovery_errors.is_empty() {
            return Err(api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "settings.rollback_failed",
                format!("{error}; {}", recovery_errors.join("; ")),
            ));
        }
        return Err(credential_error(&profile_id, error));
    }
    Ok(Json(json!({ "deleted": true })))
}

pub(super) async fn credential_write(
    State(state): State<SettingsContext>,
    Path(profile_id): Path<String>,
    Json(input): Json<CredentialInput>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config.config_control.lock().await;
    let config = state.config.config.read().expect("config lock").clone();
    let profile = config
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .cloned()
        .ok_or_else(profile_not_found)?;
    let previous = asr::read_stored_credential(&profile.id, &profile.provider)
        .map_err(|error| credential_error(&profile_id, error))?;
    asr::write_credential(&profile.id, &profile.provider, &input.api_key)
        .map_err(|error| credential_error(&profile_id, error))?;
    reload_after_credential_change(&state, &config, &profile, previous).await?;
    Ok(Json(
        profile_value(&profile, &config).map_err(|error| credential_error(&profile_id, error))?,
    ))
}

pub(super) async fn credential_delete(
    State(state): State<SettingsContext>,
    Path(profile_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config.config_control.lock().await;
    let config = state.config.config.read().expect("config lock").clone();
    let profile = config
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .cloned()
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
    let previous = asr::read_stored_credential(&profile.id, &profile.provider)
        .map_err(|error| credential_error(&profile_id, error))?;
    asr::delete_credential(&profile.id, &profile.provider)
        .map_err(|error| credential_error(&profile_id, error))?;
    reload_after_credential_change(&state, &config, &profile, previous).await?;
    Ok(Json(
        profile_value(&profile, &config).map_err(|error| credential_error(&profile_id, error))?,
    ))
}

pub(super) async fn profile_activate(
    State(state): State<SettingsContext>,
    Json(input): Json<ActiveProfileInput>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config.config_control.lock().await;
    let mut candidate = state.config.config.read().expect("config lock").clone();
    let profile = candidate
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == input.profile_id)
        .ok_or_else(profile_not_found)?;
    let resolved =
        providers::resolve_profile_service(profile, &input.service_id).map_err(profile_invalid)?;
    if !resolved
        .service
        .capabilities
        .contains(&CAPABILITY_SPEECH_TO_TEXT)
        || resolved.service.recognition_transport.is_none()
    {
        return Err(profile_invalid(
            "The selected service does not support speech recognition".into(),
        ));
    }
    let status = asr::credential_status(&profile.id, &profile.provider)
        .map_err(|error| credential_error(&profile.id, error))?;
    if profile.requires_api_key() && !status.configured {
        return Err(api_error(
            StatusCode::CONFLICT,
            "asr.credential_missing",
            "Configure an API key before activating this profile",
        ));
    }
    ensure_asr_profile_ready(profile)?;
    let settings = candidate
        .asr
        .service_settings
        .get(&input.service_id)
        .ok_or_else(|| {
            profile_invalid(format!(
                "Recognition service settings are missing for {}",
                input.service_id
            ))
        })?;
    if !providers::recognition_model_supported(resolved.service, &settings.model) {
        return Err(profile_invalid(format!(
            "Unsupported model for recognition service {}: {}",
            input.service_id, settings.model
        )));
    }
    candidate.asr.active_profile_id = Some(input.profile_id);
    candidate.asr.backend = input.service_id;
    commit_profile_config(&state, candidate).await?;
    profile_list_response(&state)
}

pub(super) async fn profile_models(
    State(state): State<SettingsContext>,
    Path(profile_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let config = state.config.config.read().expect("config lock").clone();
    let profile = config
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(profile_not_found)?;
    let resolved = [CAPABILITY_TEXT_GENERATION, CAPABILITY_TEXT_TRANSLATION]
        .into_iter()
        .find_map(|capability| providers::resolve_profile_capability(profile, capability).ok())
        .ok_or_else(|| {
            api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "llm.models_unsupported",
                "This API profile does not expose LLM models",
            )
        })?;
    service_models(&state, profile, resolved.service).await
}

pub(super) async fn profile_service_models(
    State(state): State<SettingsContext>,
    Path((profile_id, service_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let config = state.config.config.read().expect("config lock").clone();
    let profile = config
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(profile_not_found)?;
    let resolved =
        providers::resolve_profile_service(profile, &service_id).map_err(profile_invalid)?;
    service_models(&state, profile, resolved.service).await
}

async fn service_models(
    state: &SettingsContext,
    profile: &ApiProfile,
    service: &ProviderServiceDefinition,
) -> ApiResult<Json<Value>> {
    if service.supports_model_listing {
        let api_key = profile_api_key(profile)?;
        let models = crate::llm::LlmClient::new(state.integrations.http.clone())
            .list_provider_models(profile, &api_key)
            .await
            .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.code, error.detail))?;
        let models = if service.recognition_transport.is_some() {
            providers::compatible_service_models(service, models)
        } else {
            models
        };
        if !models.is_empty() {
            return Ok(Json(json!({ "models": models })));
        }
    }
    if !service.models.is_empty() {
        return Ok(Json(json!({ "models": service.models })));
    }
    if !service.supports_model_listing {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "llm.models_unsupported",
            format!("Service {} does not expose model listing", service.id),
        ));
    }
    Err(api_error(
        StatusCode::BAD_GATEWAY,
        "llm.invalid_response",
        format!("Service {} did not return compatible models", service.id),
    ))
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
    state: &SettingsContext,
    candidate: crate::config::AppConfig,
) -> ApiResult<()> {
    candidate.validate_settings().map_err(|error| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "asr.profile_invalid",
            error,
        )
    })?;
    super::settings::commit_candidate(state, candidate).await?;
    Ok(())
}

async fn reload_after_credential_change(
    state: &SettingsContext,
    config: &crate::config::AppConfig,
    profile: &ApiProfile,
    previous: Option<String>,
) -> ApiResult<()> {
    if !state
        .capture
        .capture_requested
        .load(std::sync::atomic::Ordering::SeqCst)
        || !super::capture::uses_asr_profile(config, &profile.id)
    {
        return Ok(());
    }
    let _capture_control = state.capture.capture_control.lock().await;
    if !state
        .capture
        .capture_requested
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Ok(());
    }
    if let Err(error) = super::capture::validate_capture_config(state, config).await {
        restore_credential(profile, previous).map_err(|recovery| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "settings.rollback_failed",
                format!(
                    "{}; credential rollback failed: {recovery}",
                    api_detail(&error)
                ),
            )
        })?;
        return Err(error);
    }

    let plan = super::capture::CaptureReloadPlan::all();
    super::capture::stop_pipelines(state, plan).await;
    if let Err(error) = super::capture::start_pipelines(state, config, plan).await {
        let mut recovery_errors = Vec::new();
        if let Err(recovery) = restore_credential(profile, previous) {
            recovery_errors.push(format!("credential rollback failed: {recovery}"));
        } else {
            super::capture::stop_pipelines(state, plan).await;
            if let Err(recovery) = super::capture::start_pipelines(state, config, plan).await {
                recovery_errors.push(api_detail(&recovery));
            }
        }
        if !recovery_errors.is_empty() {
            return Err(api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "settings.rollback_failed",
                format!("{}; {}", api_detail(&error), recovery_errors.join("; ")),
            ));
        }
        return Err(error);
    }
    Ok(())
}

fn restore_credential(profile: &ApiProfile, previous: Option<String>) -> Result<(), String> {
    match previous {
        Some(api_key) => asr::write_credential(&profile.id, &profile.provider, &api_key),
        None => asr::delete_credential(&profile.id, &profile.provider),
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

pub(super) fn profile_not_found() -> (StatusCode, Json<Value>) {
    api_error(
        StatusCode::NOT_FOUND,
        "asr.profile_not_found",
        "The API profile does not exist",
    )
}

pub(super) fn ensure_asr_profile_ready(profile: &ApiProfile) -> ApiResult<()> {
    if profile.provider == ALIBABA_PROVIDER
        && profile
            .workspace_id
            .as_deref()
            .is_none_or(|workspace| workspace.trim().is_empty())
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

fn profile_invalid(detail: String) -> (StatusCode, Json<Value>) {
    api_error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "asr.profile_invalid",
        detail,
    )
}

fn normalize_profile_fields(profile: &mut ApiProfile) -> Result<(), String> {
    let provider = providers::definition(&profile.provider)
        .ok_or_else(|| format!("Unsupported API provider: {}", profile.provider))?;
    profile.name = profile.name.trim().to_string();
    if profile.name.is_empty() {
        profile.name = provider.display_name.to_string();
    }
    profile.region = trimmed_option(profile.region.take());
    profile.workspace_id = trimmed_option(profile.workspace_id.take());
    profile.base_url = trimmed_option(profile.base_url.take());
    for capability in &mut profile.enabled_capabilities {
        *capability = capability.trim().to_string();
    }

    match provider.connection.base_url {
        BaseUrlPolicy::Fixed(_) => {
            profile.region = None;
            profile.workspace_id = None;
            profile.base_url = None;
        }
        BaseUrlPolicy::Regional => {
            profile.base_url = None;
            match profile.provider.as_str() {
                ALIBABA_PROVIDER => {
                    if profile.region.is_none() {
                        profile.region = Some("singapore".into());
                    }
                }
                MICROSOFT_PROVIDER => profile.workspace_id = None,
                _ => {
                    profile.region = None;
                    profile.workspace_id = None;
                }
            }
        }
        BaseUrlPolicy::Editable(default) => {
            profile.region = None;
            profile.workspace_id = None;
            if profile.base_url.is_none() && !default.is_empty() {
                profile.base_url = Some(default.into());
            }
        }
    }

    match provider.category {
        ProviderCategory::CloudProvider => {
            profile.auth_mode = provider.connection.auth_mode;
            profile.is_local = false;
        }
        ProviderCategory::LocalService => {
            profile.auth_mode = provider.connection.auth_mode;
            profile.is_local = true;
        }
        ProviderCategory::CustomProtocol => {}
    }
    profile.preset_id = profile
        .preset_id
        .take()
        .filter(|preset_id| provider.presets.iter().any(|preset| preset.id == preset_id));
    if provider.connection.allow_custom_headers {
        for header in &mut profile.headers {
            header.name = header.name.trim().to_string();
        }
        profile.headers.retain(|header| !header.name.is_empty());
    } else {
        profile.headers.clear();
    }
    providers::validate_profile(profile)
}

fn trimmed_option(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn apply_profile_compatibility_fallbacks(
    config: &mut crate::config::AppConfig,
    profile: &ApiProfile,
) {
    if config.asr.active_profile_id.as_deref() == Some(profile.id.as_str())
        && providers::resolve_profile_service(profile, &config.asr.backend).is_err()
    {
        disable_cloud_recognition(config);
    }
    if uses_translation_profile(config, &profile.id) && !providers::supports_translation(profile) {
        disable_translation_profile(config, &profile.id);
    }
}

fn disable_cloud_recognition(config: &mut crate::config::AppConfig) {
    config.asr.backend = "local_whisper".into();
    config.asr.active_profile_id = None;
}

fn disable_translation_profile(config: &mut crate::config::AppConfig, profile_id: &str) {
    let global_uses_profile = config
        .translation
        .speaker_targets
        .iter()
        .chain(&config.translation.microphone_targets)
        .any(|target| target.profile_id.as_deref() == Some(profile_id));
    if global_uses_profile {
        config.translation.mode = "disabled".into();
    }
    for target in config
        .translation
        .speaker_targets
        .iter_mut()
        .chain(&mut config.translation.microphone_targets)
    {
        if target.profile_id.as_deref() == Some(profile_id) {
            target.profile_id = None;
        }
    }
    for preset in &mut config.language_presets {
        let uses_profile = preset
            .speaker_targets
            .iter()
            .chain(&preset.microphone_targets)
            .any(|target| target.profile_id.as_deref() == Some(profile_id));
        if uses_profile {
            preset.translation_mode = "disabled".into();
        }
        for target in preset
            .speaker_targets
            .iter_mut()
            .chain(&mut preset.microphone_targets)
        {
            if target.profile_id.as_deref() == Some(profile_id) {
                target.profile_id = None;
            }
        }
    }
}

fn uses_translation_profile(config: &crate::config::AppConfig, profile_id: &str) -> bool {
    uses_global_translation_profile(config, profile_id)
        || config.language_presets.iter().any(|preset| {
            preset
                .speaker_targets
                .iter()
                .chain(&preset.microphone_targets)
                .any(|target| target.profile_id.as_deref() == Some(profile_id))
        })
}

fn uses_global_translation_profile(config: &crate::config::AppConfig, profile_id: &str) -> bool {
    config
        .translation
        .speaker_targets
        .iter()
        .chain(&config.translation.microphone_targets)
        .any(|target| target.profile_id.as_deref() == Some(profile_id))
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::providers::{
        GROQ_PROVIDER, OPENAI_COMPATIBLE_PROVIDER, OPENAI_PROVIDER, SERVICE_GROQ_TRANSCRIPTION,
    };

    #[test]
    fn fixed_provider_ignores_client_base_url_and_uses_provider_name() {
        let mut profile = ApiProfile {
            name: "  ".into(),
            provider: OPENAI_PROVIDER.into(),
            base_url: Some("https://example.invalid/v1".into()),
            enabled_capabilities: vec![CAPABILITY_TEXT_GENERATION.into()],
            ..ApiProfile::default()
        };

        normalize_profile_fields(&mut profile).unwrap();

        assert_eq!(profile.name, "OpenAI");
        assert_eq!(profile.base_url, None);
    }

    #[test]
    fn editable_provider_keeps_custom_connection_metadata() {
        let mut profile = ApiProfile {
            name: "Custom".into(),
            provider: OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some("  http://127.0.0.1:9000/v1  ".into()),
            enabled_capabilities: vec![CAPABILITY_TEXT_GENERATION.into()],
            auth_mode: ApiAuthMode::None,
            is_local: true,
            headers: vec![HttpHeaderConfig {
                name: "  x-client  ".into(),
                value: "vrcs".into(),
            }],
            ..ApiProfile::default()
        };

        normalize_profile_fields(&mut profile).unwrap();

        assert_eq!(
            profile.base_url.as_deref(),
            Some("http://127.0.0.1:9000/v1")
        );
        assert_eq!(profile.auth_mode, ApiAuthMode::None);
        assert!(profile.is_local);
        assert_eq!(profile.headers[0].name, "x-client");
    }

    #[test]
    fn losing_active_capabilities_falls_back_to_local_services() {
        let mut config = crate::config::AppConfig::default();
        config.asr.backend = SERVICE_GROQ_TRANSCRIPTION.into();
        config.asr.active_profile_id = Some("groq".into());
        config.translation.mode = "automatic".into();
        config.translation.speaker_targets[0].profile_id = Some("groq".into());
        config.translation.microphone_targets[0].profile_id = Some("groq".into());
        let profile = ApiProfile {
            id: "groq".into(),
            name: "Groq".into(),
            provider: GROQ_PROVIDER.into(),
            enabled_capabilities: vec![CAPABILITY_TEXT_GENERATION.into()],
            ..ApiProfile::default()
        };

        apply_profile_compatibility_fallbacks(&mut config, &profile);

        assert_eq!(config.asr.backend, "local_whisper");
        assert_eq!(config.asr.active_profile_id, None);
        assert_eq!(config.translation.mode, "disabled");
        assert_eq!(config.translation.speaker_targets[0].profile_id, None);
        assert_eq!(config.translation.microphone_targets[0].profile_id, None);
    }

    #[test]
    fn provider_catalog_uses_desktop_connection_and_service_shape() {
        let value = provider_value(providers::definition(GROQ_PROVIDER).unwrap());

        assert_eq!(value["connection"]["base_url"]["mode"], "fixed");
        assert_eq!(value["services"][1]["id"], SERVICE_GROQ_TRANSCRIPTION);
        assert_eq!(value["services"][1]["model_listing"], false);
        assert!(value["services"][1]["adapter"].is_string());
    }

    #[test]
    fn alibaba_asr_requires_a_workspace() {
        let profile = ApiProfile {
            id: "alibaba".into(),
            name: "Alibaba".into(),
            provider: ALIBABA_PROVIDER.into(),
            region: Some("china_beijing".into()),
            workspace_id: None,
            enabled_capabilities: vec![CAPABILITY_SPEECH_TO_TEXT.into()],
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
