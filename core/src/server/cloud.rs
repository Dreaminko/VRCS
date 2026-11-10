use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::asr;
use crate::config::{ApiAuthMode, ApiProfile, HttpHeaderConfig, DEFAULT_PROFILE_TIMEOUT_MS};
use crate::providers::{
    self, BaseUrlPolicy, ProviderCategory, ProviderDefinition, ProviderServiceDefinition,
    ServiceAdapter, ALIBABA_PROVIDER, CAPABILITY_IMAGE_UNDERSTANDING, CAPABILITY_SPEECH_TO_TEXT,
    CAPABILITY_TEXT_GENERATION, CAPABILITY_TEXT_TRANSLATION, MICROSOFT_PROVIDER,
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
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    enabled_capabilities: Option<Vec<String>>,
    #[serde(default, deserialize_with = "nullable_field")]
    region: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable_field")]
    workspace_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable_field")]
    base_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable_field")]
    preset_id: Option<Option<String>>,
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

fn nullable_field<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
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

#[derive(Default, Deserialize)]
pub(super) struct ModelCatalogQuery {
    #[serde(default)]
    refresh: bool,
    #[serde(default)]
    cached_only: bool,
}

#[derive(Default, Deserialize)]
pub(super) struct ProfileMutationQuery {
    #[serde(default)]
    dry_run: bool,
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
                "label_key": "settings.apiManagement.connectionFields.region",
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
                "label_key": "settings.apiManagement.connectionFields.workspace_id",
                "type": "text",
                "required": false,
                "default": null
            }),
        ],
        providers::QWEN_CLOUD_PROVIDER => vec![json!({
            "id": "region",
            "label_key": "settings.apiManagement.connectionFields.region",
            "type": "select",
            "required": true,
            "default": "ap-southeast-1",
            "options": providers::QWEN_CLOUD_REGIONS.iter().map(|(value, label, _)| {
                json!({ "value": value, "label": label })
            }).collect::<Vec<_>>()
        })],
        MICROSOFT_PROVIDER => vec![json!({
            "id": "region",
            "label_key": "settings.apiManagement.connectionFields.region",
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
        "diagnostic_models": provider.diagnostic_models,
    })
}

fn service_value(service: &ProviderServiceDefinition) -> Value {
    json!({
        "id": service.id,
        "display_name": service.display_name,
        "capabilities": service.capabilities,
        "adapter": adapter_id(service.adapter),
        "recognition_transport": service.recognition_transport,
        "recognition_languages": service.recognition_languages,
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
        ServiceAdapter::QwenLiveTranslate => "qwen_live_translate",
        ServiceAdapter::AlibabaTokenPlanRealtime => "alibaba_token_plan_realtime",
        ServiceAdapter::FunAsrRealtime => "fun_asr_realtime",
        ServiceAdapter::OpenAiRealtime => "openai_realtime",
        ServiceAdapter::GeminiTranscribe => "gemini_transcribe",
        ServiceAdapter::GeminiLiveTranslate => "gemini_live_translate",
        ServiceAdapter::OpenAiRealtimeTranslate => "openai_realtime_translate",
        ServiceAdapter::OpenAiAudioTranscriptions => "openai_audio_transcriptions",
        ServiceAdapter::MimoTranscription => "mimo_transcription",
    }
}

fn profile_value(profile: &ApiProfile, config: &crate::config::AppConfig) -> Result<Value, String> {
    let status = asr::credential_status(&profile.id, &profile.provider)?;
    let provider = providers::definition(&profile.provider)
        .ok_or_else(|| format!("Unsupported API provider: {}", profile.provider))?;
    let active = config.asr.backend != crate::config::QWEN_MANAGED_BACKEND
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
        "status": "ready",
        "usages": profile_usages(config, &profile.id),
    }))
}

fn unavailable_profile_value(profile: &ApiProfile, config: &crate::config::AppConfig) -> Value {
    let provider = providers::definition(&profile.provider);
    let mut value = json!(profile);
    let object = value.as_object_mut().expect("profile object");
    object.extend(json!({
        "provider_display_name": provider.as_ref().map_or(profile.provider.as_str(), |provider| provider.display_name),
        "active": false,
        "translation_active": false,
        "status": if provider.is_some() { "credential_error" } else { "unsupported" },
        "credential": { "configured": false, "stored_configured": false, "environment_override": false, "source": null,
            "storage_available": false, "reason_code": "credentials.read_failed" },
        "capabilities": providers::profile_capabilities(profile).map_or_else(|| json!({
            "supports_asr": false, "supports_translation": false, "supports_text_generation": false,
            "supports_image_understanding": false, "supports_model_listing": false,
            "supports_context": false, "supports_streaming": false, "requires_api_key": false,
            "is_local": false, "supports_custom_translation_language": false, "supported_languages": []
        }), |capabilities| json!(capabilities)),
        "support_levels": { "asr": null, "translation": null },
        "usages": profile_usages(config, &profile.id),
    }).as_object().expect("profile status object").clone());
    value
}

fn profile_usages(config: &crate::config::AppConfig, profile_id: &str) -> Vec<Value> {
    let mut config = config.clone();
    config.resolve_model_assignments();
    let mut usages = Vec::new();
    if config.asr.active_profile_id.as_deref() == Some(profile_id) {
        usages.push(json!({ "kind": "transcription", "capability": CAPABILITY_SPEECH_TO_TEXT }));
    }
    for (binding, kind, capability) in [
        (
            &config.model_defaults.text,
            "model_default_text",
            CAPABILITY_TEXT_GENERATION,
        ),
        (
            &config.model_defaults.vision,
            "model_default_vision",
            CAPABILITY_IMAGE_UNDERSTANDING,
        ),
        (
            &config.model_defaults.translation,
            "model_default_translation",
            CAPABILITY_TEXT_TRANSLATION,
        ),
    ] {
        if binding.profile_id.as_deref() == Some(profile_id) {
            usages.push(json!({ "kind": kind, "capability": capability }));
        }
    }
    for (selection, kind) in [
        (&config.translation.model_selection, "translation"),
        (&config.ocr.model_selection, "ocr_translation"),
    ] {
        if selection.profile_id.as_deref() == Some(profile_id) {
            usages.push(json!({
                "kind": kind, "capability": CAPABILITY_TEXT_TRANSLATION,
                "scope": "model_selection",
            }));
        }
    }
    for (scope, targets) in [
        ("speaker", &config.translation.speaker_targets),
        ("microphone", &config.translation.microphone_targets),
    ] {
        append_target_usages(
            &mut usages,
            targets,
            profile_id,
            "translation",
            Some(scope),
            None,
        );
    }
    append_target_usages(
        &mut usages,
        &config.ocr.targets,
        profile_id,
        "ocr_translation",
        None,
        None,
    );
    for preset in &config.language_presets {
        for (scope, targets) in [
            ("speaker", &preset.speaker_targets),
            ("microphone", &preset.microphone_targets),
        ] {
            append_target_usages(
                &mut usages,
                targets,
                profile_id,
                "language_preset",
                Some(scope),
                Some(preset),
            );
        }
    }
    for (used, kind, capability) in [
        (
            config.vision.profile_id.as_deref() == Some(profile_id),
            "vision",
            CAPABILITY_IMAGE_UNDERSTANDING,
        ),
        (
            config.conversation_titles.profile_id.as_deref() == Some(profile_id),
            "conversation_titles",
            CAPABILITY_TEXT_GENERATION,
        ),
        (
            config.learning_ai.profile_id.as_deref() == Some(profile_id),
            "learning_ai",
            CAPABILITY_TEXT_GENERATION,
        ),
    ] {
        if used {
            usages.push(json!({ "kind": kind, "capability": capability }));
        }
    }
    usages
}

fn append_target_usages(
    usages: &mut Vec<Value>,
    targets: &[crate::config::TranslationTargetConfig],
    profile_id: &str,
    kind: &str,
    scope: Option<&str>,
    preset: Option<&crate::config::LanguagePreset>,
) {
    for (index, target) in targets.iter().enumerate() {
        if target.profile_id.as_deref() != Some(profile_id) {
            continue;
        }
        let mut usage = json!({
            "kind": kind, "capability": CAPABILITY_TEXT_TRANSLATION,
            "index": index, "target_language": target.target_language,
        });
        if let Some(scope) = scope {
            usage["scope"] = json!(scope);
        }
        if let Some(preset) = preset {
            usage["preset_id"] = json!(preset.id);
            usage["preset_name"] = json!(preset.name);
        }
        usages.push(usage);
    }
}

fn profile_side_effects(
    current: &crate::config::AppConfig,
    candidate: &crate::config::AppConfig,
    profile_id: &str,
) -> Vec<Value> {
    let remaining = profile_usages(candidate, profile_id);
    profile_usages(current, profile_id)
        .into_iter()
        .filter(|usage| {
            !remaining.contains(usage)
                || usage["capability"].as_str().is_some_and(|capability| {
                    candidate
                        .asr
                        .api_profiles
                        .iter()
                        .find(|profile| profile.id == profile_id)
                        .is_none_or(|profile| {
                            providers::resolve_profile_capability(profile, capability).is_err()
                        })
                })
        })
        .collect()
}

fn connection_origin_changed(previous: &ApiProfile, updated: &ApiProfile) -> bool {
    let origin = |profile: &ApiProfile| {
        profile
            .base_url
            .as_deref()
            .and_then(|url| reqwest::Url::parse(url).ok())
            .map(|url| url.origin())
    };
    origin(previous) != origin(updated)
}

fn credential_reconfirmation_required(
    previous: &ApiProfile,
    updated: &ApiProfile,
    api_key_supplied: bool,
    credential_configured: bool,
) -> bool {
    updated.requires_api_key()
        && !api_key_supplied
        && credential_configured
        && (connection_origin_changed(previous, updated) || !previous.requires_api_key())
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
        .map(|profile| {
            profile_value(profile, &config)
                .unwrap_or_else(|_| unavailable_profile_value(profile, &config))
        })
        .collect::<Vec<_>>();
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
    normalize_profile_fields(&mut profile).map_err(profile_validation_error)?;
    if profile.requires_api_key()
        && input
            .api_key
            .as_ref()
            .is_some_and(|key| key.trim().len() > 4096)
    {
        return Err(profile_validation_error(
            providers::ProfileValidationError::new("api_key", "API key length is invalid"),
        ));
    }
    let mut candidate = state.config.config.read().expect("config lock").clone();
    candidate.asr.api_profiles.push(profile.clone());
    commit_profile_config(&state, candidate).await?;
    if let Some(api_key) = input
        .api_key
        .filter(|value| profile.requires_api_key() && !value.trim().is_empty())
    {
        if let Err(error) = asr::write_credential(&profile.id, &profile.provider, &api_key) {
            let mut rollback = state.config.config.read().expect("config lock").clone();
            rollback
                .asr
                .api_profiles
                .retain(|item| item.id != profile.id);
            let _ = commit_profile_config(&state, rollback).await;
            return Err(credential_error(error));
        }
    }
    let config = state.config.config.read().expect("config lock").clone();
    Ok(Json(
        profile_value(&profile, &config).map_err(credential_error)?,
    ))
}

pub(super) async fn profile_update(
    State(state): State<SettingsContext>,
    Path(profile_id): Path<String>,
    Query(query): Query<ProfileMutationQuery>,
    Json(input): Json<UpdateProfileInput>,
) -> ApiResult<Json<Value>> {
    let _config_control = state.config.config_control.lock().await;
    let current = state.config.config.read().expect("config lock").clone();
    let mut candidate = current.clone();
    let updated = {
        let profile = candidate
            .asr
            .api_profiles
            .iter_mut()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(profile_not_found)?;
        if let Some(name) = input.name {
            profile.name = name;
        }
        if let Some(enabled_capabilities) = input.enabled_capabilities {
            profile.enabled_capabilities = enabled_capabilities;
        }
        if let Some(region) = input.region {
            profile.region = region;
        }
        if let Some(workspace_id) = input.workspace_id {
            profile.workspace_id = workspace_id;
        }
        if let Some(base_url) = input.base_url {
            profile.base_url = base_url;
        }
        if let Some(preset_id) = input.preset_id {
            profile.preset_id = preset_id;
        }
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
        normalize_profile_fields(profile).map_err(profile_validation_error)?;
        profile.clone()
    };
    let api_key = input
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| updated.requires_api_key() && !value.is_empty());
    let previous = current
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .expect("profile exists");
    if credential_reconfirmation_required(
        previous,
        &updated,
        api_key.is_some(),
        asr::credential_status(&profile_id, &updated.provider)
            .map_err(credential_error)?
            .configured,
    ) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "profile.credential_reconfirm_required",
            "Enter an API key again before changing this authenticated connection",
        ));
    }
    if let Some(api_key) = api_key {
        if api_key.len() > 4096 {
            return Err(profile_validation_error(
                providers::ProfileValidationError::new("api_key", "API key length is invalid"),
            ));
        }
        if asr::credential_status(&profile_id, &updated.provider)
            .map_err(credential_error)?
            .environment_override
        {
            return Err(api_error(
                StatusCode::CONFLICT,
                "profile.credential_environment_managed",
                "This API key is managed by an environment variable",
            ));
        }
    }
    apply_profile_compatibility_fallbacks(&mut candidate, &updated);
    let side_effects = profile_side_effects(&current, &candidate, &profile_id);
    if query.dry_run {
        candidate.validate_settings().map_err(profile_invalid)?;
        return Ok(Json(json!({ "side_effects": side_effects })));
    }
    if let Some(api_key) = api_key {
        candidate.validate_settings().map_err(profile_invalid)?;
        super::settings::commit_profile_candidate(&state, candidate, &updated, api_key).await?;
    } else {
        commit_profile_config(&state, candidate).await?;
    }
    let config = state.config.config.read().expect("config lock").clone();
    let mut response = profile_value(&updated, &config).map_err(credential_error)?;
    response["side_effects"] = json!(side_effects);
    Ok(Json(response))
}

pub(super) async fn profile_delete(
    State(state): State<SettingsContext>,
    Path(profile_id): Path<String>,
    Query(query): Query<ProfileMutationQuery>,
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
    clear_model_bindings(&mut candidate, &profile_id);
    if candidate.asr.active_profile_id.as_deref() == Some(profile_id.as_str()) {
        disable_cloud_recognition(&mut candidate);
    }
    if uses_translation_profile(&candidate, &profile_id) {
        disable_translation_profile(&mut candidate, &profile_id);
    }
    disable_vision_profile(&mut candidate, &profile_id);
    disable_title_profile(&mut candidate, &profile_id);
    candidate.resolve_model_assignments();
    if query.dry_run {
        candidate.validate_settings().map_err(profile_invalid)?;
        return Ok(Json(
            json!({ "side_effects": profile_usages(&current, &profile_id) }),
        ));
    }
    let previous_credential = crate::credentials::read_profile_stored_for_use(&profile.id, &profile.provider)
        .map_err(credential_error)?;
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
        return Err(credential_error(error));
    }
    Ok(Json(
        json!({ "deleted": true, "side_effects": profile_usages(&current, &profile_id) }),
    ))
}

pub(super) async fn credential_write(
    State(state): State<SettingsContext>,
    Path(profile_id): Path<String>,
    Json(input): Json<CredentialInput>,
) -> ApiResult<Json<Value>> {
    if input.api_key.trim().is_empty() || input.api_key.trim().len() > 4096 {
        return Err(profile_validation_error(
            providers::ProfileValidationError::new("api_key", "API key length is invalid"),
        ));
    }
    let _config_control = state.config.config_control.lock().await;
    let config = state.config.config.read().expect("config lock").clone();
    let profile = config
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .cloned()
        .ok_or_else(profile_not_found)?;
    providers::definition(&profile.provider).ok_or_else(|| {
        profile_invalid(format!("Unsupported API provider: {}", profile.provider))
    })?;
    let previous =
        crate::credentials::read_profile_stored_for_use(&profile.id, &profile.provider).map_err(credential_error)?;
    asr::write_credential(&profile.id, &profile.provider, &input.api_key)
        .map_err(credential_error)?;
    reload_after_credential_change(&state, &config, &profile, previous).await?;
    Ok(Json(
        profile_value(&profile, &config).map_err(credential_error)?,
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
        .map_err(credential_error)?
        .environment_override
    {
        return Err(api_error(
            StatusCode::CONFLICT,
            "profile.credential_environment_managed",
            "This API key is managed by an environment variable",
        ));
    }
    let previous =
        crate::credentials::read_profile_stored_for_use(&profile.id, &profile.provider).map_err(credential_error)?;
    asr::delete_credential(&profile.id, &profile.provider).map_err(credential_error)?;
    reload_after_credential_change(&state, &config, &profile, previous).await?;
    Ok(Json(
        profile_value(&profile, &config).map_err(credential_error)?,
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
    if profile.requires_api_key() {
        crate::credentials::read_credential_for_use(&profile.id, &profile.provider)
            .map_err(credential_error)?;
    }
    let status =
        asr::credential_status(&profile.id, &profile.provider).map_err(credential_error)?;
    if profile.requires_api_key() && !status.configured {
        return Err(api_error(
            StatusCode::CONFLICT,
            "profile.credential_missing",
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
    if candidate.asr.backend == providers::SERVICE_FUN_ASR_REALTIME {
        candidate.vad.silence_seconds = candidate.vad.silence_seconds.max(0.2);
    }
    commit_profile_config(&state, candidate).await?;
    profile_list_response(&state)
}

pub(super) async fn profile_models(
    State(state): State<SettingsContext>,
    Path(profile_id): Path<String>,
    Query(query): Query<ModelCatalogQuery>,
) -> ApiResult<Json<Value>> {
    let config = state.config.config.read().expect("config lock").clone();
    let profile = config
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(profile_not_found)?;
    let resolved = [
        CAPABILITY_TEXT_GENERATION,
        CAPABILITY_TEXT_TRANSLATION,
        CAPABILITY_IMAGE_UNDERSTANDING,
    ]
    .into_iter()
    .find_map(|capability| providers::resolve_profile_capability(profile, capability).ok())
    .ok_or_else(|| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "llm.models_unsupported",
            "This API profile does not expose LLM models",
        )
    })?;
    service_models(&state, profile, resolved.service, query).await
}

pub(super) async fn profile_service_models(
    State(state): State<SettingsContext>,
    Path((profile_id, service_id)): Path<(String, String)>,
    Query(query): Query<ModelCatalogQuery>,
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
    service_models(&state, profile, resolved.service, query).await
}

async fn service_models(
    state: &SettingsContext,
    profile: &ApiProfile,
    service: &ProviderServiceDefinition,
    query: ModelCatalogQuery,
) -> ApiResult<Json<Value>> {
    let api_key = if service.supports_model_listing
        || service.id == providers::SERVICE_OPENROUTER_TRANSCRIPTION
    {
        if query.refresh {
            profile_api_key(profile)?
        } else {
            match asr::read_credential(&profile.id, &profile.provider) {
                Ok(Some(key)) => key,
                Ok(None) => String::new(),
                Err(error) if query.cached_only => {
                    return Ok(Json(json!({
                        "models": [], "model_details": {}, "updated_at": null,
                        "reason_code": crate::credentials::error_reason_code(&error),
                    })));
                }
                Err(error) => return Err(credential_error(error)),
            }
        }
    } else {
        String::new()
    };
    if !query.refresh {
        if let Some(catalog) = state
            .integrations
            .model_catalogs
            .get(profile, service.id, &api_key)
        {
            return Ok(Json(catalog));
        }
    }
    if query.cached_only {
        return Ok(Json(
            json!({ "models": [], "model_details": {}, "updated_at": null }),
        ));
    }
    let mut catalog = fetch_service_models(state, profile, service, &api_key).await?;
    catalog.0["updated_at"] = json!(chrono::Utc::now().timestamp());
    state
        .integrations
        .model_catalogs
        .insert(profile, service.id, &api_key, catalog.0.clone());
    Ok(catalog)
}

async fn fetch_service_models(
    state: &SettingsContext,
    profile: &ApiProfile,
    service: &ProviderServiceDefinition,
    api_key: &str,
) -> ApiResult<Json<Value>> {
    if service.id == providers::SERVICE_OPENROUTER_TRANSCRIPTION {
        let catalog = crate::llm::openrouter::fetch_transcription_catalog(
            &state.integrations.http,
            profile,
            api_key,
        )
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.code, error.detail))?;
        return Ok(Json(json!(catalog)));
    }
    if service.supports_model_listing {
        let mut catalog = crate::llm::LlmClient::new(state.integrations.http.clone())
            .list_model_catalog(profile, api_key)
            .await
            .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.code, error.detail))?;
        catalog.models = providers::compatible_service_models(service, catalog.models);
        catalog
            .model_details
            .retain(|model, _| catalog.models.contains(model));
        if !catalog.models.is_empty() {
            return Ok(Json(json!(catalog)));
        }
    }
    if !service.models.is_empty() {
        let mut catalog = crate::llm::model_catalog::ModelCatalog {
            models: service
                .models
                .iter()
                .map(|model| (*model).to_owned())
                .collect(),
            ..Default::default()
        };
        crate::llm::models_dev::supplement(profile, &mut catalog);
        return Ok(Json(json!(catalog)));
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
    crate::credentials::read_credential_for_use(&profile.id, &profile.provider)
        .map_err(credential_error)?
        .ok_or_else(|| {
            api_error(
                StatusCode::CONFLICT,
                "profile.credential_missing",
                "Configure an API key before using this profile",
            )
        })
}

async fn commit_profile_config(
    state: &SettingsContext,
    mut candidate: crate::config::AppConfig,
) -> ApiResult<()> {
    candidate.resolve_model_assignments();
    candidate
        .validate_settings()
        .map_err(|error| api_error(StatusCode::UNPROCESSABLE_ENTITY, "profile.invalid", error))?;
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

pub(crate) fn restore_credential(
    profile: &ApiProfile,
    previous: Option<String>,
) -> Result<(), String> {
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
        "profile.not_found",
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

fn credential_error(detail: String) -> (StatusCode, Json<Value>) {
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        crate::credentials::error_reason_code(&detail).unwrap_or("profile.credential_store_failed"),
        detail,
    )
}

fn profile_invalid(detail: String) -> (StatusCode, Json<Value>) {
    api_error(StatusCode::UNPROCESSABLE_ENTITY, "profile.invalid", detail)
}

fn profile_validation_error(error: providers::ProfileValidationError) -> (StatusCode, Json<Value>) {
    let mut response = profile_invalid(error.detail);
    response.1["field"] = json!(error.field);
    response
}

fn normalize_profile_fields(
    profile: &mut ApiProfile,
) -> Result<(), providers::ProfileValidationError> {
    let provider = providers::definition(&profile.provider).ok_or_else(|| {
        providers::ProfileValidationError::new(
            "provider",
            format!("Unsupported API provider: {}", profile.provider),
        )
    })?;
    profile.name = profile.name.trim().to_string();
    if profile.name.is_empty() {
        profile.name = provider.display_name.to_string();
    }
    if profile.name.chars().count() > 50 {
        return Err(providers::ProfileValidationError::new(
            "name",
            "An API profile name must contain 1 to 50 characters",
        ));
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
                providers::QWEN_CLOUD_PROVIDER => {}
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
    providers::validate_profile_fields(profile)
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
    config.resolve_model_assignments();
    if config.asr.active_profile_id.as_deref() == Some(profile.id.as_str())
        && providers::resolve_profile_service(profile, &config.asr.backend).is_err()
    {
        disable_cloud_recognition(config);
    }
    if uses_translation_profile(config, &profile.id) && !providers::supports_translation(profile) {
        disable_translation_profile(config, &profile.id);
    }
    if providers::resolve_profile_capability(profile, CAPABILITY_IMAGE_UNDERSTANDING).is_err() {
        disable_vision_profile(config, &profile.id);
    }
    if providers::resolve_profile_capability(profile, CAPABILITY_TEXT_GENERATION).is_err() {
        disable_title_profile(config, &profile.id);
    }
    config.resolve_model_assignments();
}

fn clear_model_bindings(config: &mut crate::config::AppConfig, profile_id: &str) {
    for binding in [
        &mut config.model_defaults.text,
        &mut config.model_defaults.translation,
        &mut config.model_defaults.vision,
    ] {
        if binding.profile_id.as_deref() == Some(profile_id) {
            binding.profile_id = None;
        }
    }
    for selection in [
        &mut config.translation.model_selection,
        &mut config.ocr.model_selection,
    ] {
        if selection.profile_id.as_deref() == Some(profile_id) {
            selection.profile_id = None;
        }
    }
    if config.learning_ai.profile_id.as_deref() == Some(profile_id) {
        config.learning_ai.profile_id = None;
    }
}

fn disable_vision_profile(config: &mut crate::config::AppConfig, profile_id: &str) {
    if config.vision.profile_id.as_deref() == Some(profile_id) {
        config.vision.enabled = false;
        config.vision.profile_id = None;
    }
}

fn disable_title_profile(config: &mut crate::config::AppConfig, profile_id: &str) {
    let titles = &mut config.conversation_titles;
    if titles.profile_id.as_deref() == Some(profile_id) {
        titles.auto_generate = false;
        titles.profile_id = None;
    }
}

fn disable_cloud_recognition(config: &mut crate::config::AppConfig) {
    config.asr.backend = crate::config::QWEN_MANAGED_BACKEND.into();
    config.asr.active_profile_id = None;
}

fn disable_translation_profile(config: &mut crate::config::AppConfig, profile_id: &str) {
    for target in &mut config.ocr.targets {
        if target.profile_id.as_deref() == Some(profile_id) {
            target.profile_id = None;
            config.ocr.enabled = false;
            config.ocr.desktop_enabled = false;
        }
    }
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
        || config
            .ocr
            .targets
            .iter()
            .any(|target| target.profile_id.as_deref() == Some(profile_id))
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
    fn model_assignment_profile_impact_keeps_invalid_followers_without_rebinding() {
        let mut config = crate::config::AppConfig::default();
        let profile = ApiProfile {
            id: "shared".into(),
            name: "Shared".into(),
            provider: OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some("http://127.0.0.1:9000/v1".into()),
            auth_mode: ApiAuthMode::None,
            enabled_capabilities: vec![
                CAPABILITY_TEXT_GENERATION.into(),
                CAPABILITY_TEXT_TRANSLATION.into(),
                CAPABILITY_IMAGE_UNDERSTANDING.into(),
            ],
            ..Default::default()
        };
        config.asr.api_profiles = vec![
            profile.clone(),
            ApiProfile {
                id: "other".into(),
                name: "Other".into(),
                ..profile
            },
        ];
        config.model_defaults.text = crate::config::ModelBinding {
            backend: crate::config::TranslationBackend::Api,
            profile_id: Some("shared".into()),
            model: "text-model".into(),
        };
        config.model_defaults.translation = crate::config::ModelBinding {
            backend: crate::config::TranslationBackend::Api,
            profile_id: Some("shared".into()),
            model: "translation-model".into(),
        };
        config.model_defaults.vision = crate::config::ModelBinding {
            backend: crate::config::TranslationBackend::Api,
            profile_id: Some("shared".into()),
            model: "vision-model".into(),
        };
        config.translation.mode = "manual".into();
        config.ocr.enabled = true;
        config.conversation_titles.auto_generate = true;
        let custom = &mut config.translation.microphone_targets[0];
        custom.use_default_model = false;
        custom.profile_id = Some("other".into());
        custom.model = "custom-model".into();
        config.resolve_model_assignments();
        let current = config.clone();
        config.asr.api_profiles[0].enabled_capabilities =
            vec![CAPABILITY_IMAGE_UNDERSTANDING.into()];
        let changed = config.asr.api_profiles[0].clone();
        apply_profile_compatibility_fallbacks(&mut config, &changed);
        assert_eq!(config.translation.mode, "disabled");
        assert!(!config.ocr.enabled && !config.conversation_titles.auto_generate);
        assert_eq!(config.learning_ai.profile_id.as_deref(), Some("shared"));
        assert_eq!(
            config.translation.microphone_targets[0],
            current.translation.microphone_targets[0]
        );
        config.validate_settings().unwrap();
        let effects = profile_side_effects(&current, &config, "shared");
        for kind in [
            "model_default_text",
            "learning_ai",
            "translation",
            "ocr_translation",
            "conversation_titles",
        ] {
            assert!(effects.iter().any(|usage| usage["kind"] == kind), "{kind}");
        }
        assert!(profile_usages(&config, "shared")
            .iter()
            .any(|usage| usage["kind"] == "model_default_vision"));
        clear_model_bindings(&mut config, "shared");
        disable_translation_profile(&mut config, "shared");
        disable_vision_profile(&mut config, "shared");
        disable_title_profile(&mut config, "shared");
        config
            .asr
            .api_profiles
            .retain(|profile| profile.id != "shared");
        config.resolve_model_assignments();
        assert!(profile_usages(&config, "shared").is_empty());
        assert!(config.learning_ai.use_default_model && config.learning_ai.profile_id.is_none());
        assert_eq!(config.translation.microphone_targets[0].profile_id, None);
        assert!(config.translation.microphone_targets[0].use_default_model);
        config.validate_settings().unwrap();
    }

    #[test]
    fn profile_validation_errors_identify_the_rejected_field() {
        let base = ApiProfile {
            provider: OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some("https://example.com/v1".into()),
            enabled_capabilities: vec![CAPABILITY_TEXT_GENERATION.into()],
            ..Default::default()
        };
        for (field, mut profile) in [
            (
                "base_url",
                ApiProfile {
                    base_url: Some("ftp://example.com".into()),
                    ..base.clone()
                },
            ),
            (
                "timeout_ms",
                ApiProfile {
                    timeout_ms: 1,
                    ..base.clone()
                },
            ),
            (
                "enabled_capabilities",
                ApiProfile {
                    enabled_capabilities: Vec::new(),
                    ..base.clone()
                },
            ),
            (
                "headers",
                ApiProfile {
                    headers: vec![HttpHeaderConfig {
                        name: "Authorization".into(),
                        value: "secret".into(),
                    }],
                    ..base.clone()
                },
            ),
        ] {
            let error = normalize_profile_fields(&mut profile).unwrap_err();
            let (status, Json(body)) = profile_validation_error(error);
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(body["code"], "profile.invalid");
            assert_eq!(body["field"], field);
        }
    }

    #[test]
    fn profile_update_distinguishes_omitted_fields_from_explicit_nulls() {
        let omitted: UpdateProfileInput = serde_json::from_value(json!({})).unwrap();
        assert!(omitted.name.is_none());
        assert!(omitted.region.is_none());
        assert!(omitted.base_url.is_none());
        assert!(omitted.preset_id.is_none());
        let cleared: UpdateProfileInput = serde_json::from_value(json!({
            "region": null, "workspace_id": null, "base_url": null, "preset_id": null
        }))
        .unwrap();
        assert_eq!(cleared.region, Some(None));
        assert_eq!(cleared.workspace_id, Some(None));
        assert_eq!(cleared.base_url, Some(None));
        assert_eq!(cleared.preset_id, Some(None));
    }

    #[test]
    fn profile_usages_cover_features_cleared_by_capability_changes() {
        let mut config = crate::config::AppConfig::default();
        config.translation.microphone_targets[0].use_default_model = false;
        config.ocr.targets[0].use_default_model = false;
        config.vision.use_default_model = false;
        config.conversation_titles.use_default_model = false;
        config.asr.active_profile_id = Some("shared".into());
        config.translation.microphone_targets[0].profile_id = Some("shared".into());
        config.ocr.targets[0].profile_id = Some("shared".into());
        config.vision.profile_id = Some("shared".into());
        config.conversation_titles.profile_id = Some("shared".into());
        config.language_presets.push(crate::config::LanguagePreset {
            id: "meeting".into(),
            name: "Meeting".into(),
            recognition_language: "auto".into(),
            translation_mode: "manual".into(),
            speaker_targets: config.translation.microphone_targets.clone(),
            microphone_targets: Vec::new(),
            osc_translation_strategy: "preferred_only".into(),
        });
        let usages = profile_usages(&config, "shared");
        assert_eq!(
            usages
                .iter()
                .map(|usage| usage["kind"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "transcription",
                "translation",
                "ocr_translation",
                "language_preset",
                "vision",
                "conversation_titles"
            ]
        );
        assert_eq!(usages[1]["scope"], "microphone");
        assert_eq!(usages[1]["index"], 0);
        assert_eq!(usages[1]["target_language"], "en");
        assert_eq!(usages[3]["scope"], "speaker");
        assert_eq!(usages[3]["preset_id"], "meeting");
        assert_eq!(usages[3]["preset_name"], "Meeting");
        let profile = ApiProfile {
            id: "shared".into(),
            provider: OPENAI_PROVIDER.into(),
            enabled_capabilities: vec![CAPABILITY_TEXT_GENERATION.into()],
            ..Default::default()
        };
        apply_profile_compatibility_fallbacks(&mut config, &profile);
        assert_eq!(
            profile_usages(&config, "shared"),
            vec![json!({
                "kind": "conversation_titles", "capability": CAPABILITY_TEXT_GENERATION
            })]
        );
    }

    #[test]
    fn credential_reconfirmation_tracks_origin_changes_but_allows_path_changes() {
        let mut profile = ApiProfile {
            base_url: Some("https://example.com/v1".into()),
            ..Default::default()
        };
        let original = profile.clone();
        profile.base_url = Some("https://example.com/v2".into());
        assert!(!connection_origin_changed(&original, &profile));
        for url in [
            "https://other.example/v1",
            "https://example.com:8443/v1",
            "http://example.com/v1",
        ] {
            profile.base_url = Some(url.into());
            assert!(connection_origin_changed(&original, &profile));
        }
    }

    #[test]
    fn credential_reconfirmation_survives_a_no_auth_origin_change() {
        let requiring_auth = ApiProfile {
            base_url: Some("https://first.example/v1".into()),
            auth_mode: ApiAuthMode::Bearer,
            ..Default::default()
        };
        let no_auth = ApiProfile {
            base_url: Some("https://second.example/v1".into()),
            auth_mode: ApiAuthMode::None,
            ..requiring_auth.clone()
        };
        let restored_auth = ApiProfile {
            auth_mode: ApiAuthMode::Bearer,
            ..no_auth.clone()
        };

        assert!(credential_reconfirmation_required(
            &no_auth,
            &restored_auth,
            false,
            true
        ));
        assert!(!credential_reconfirmation_required(
            &no_auth,
            &restored_auth,
            true,
            true
        ));
    }

    #[test]
    fn unavailable_profiles_keep_identity_and_usage_information() {
        let profile = ApiProfile {
            id: "removed".into(),
            name: "Removed".into(),
            provider: "removed-provider".into(),
            ..Default::default()
        };
        let mut config = crate::config::AppConfig::default();
        config.vision.profile_id = Some(profile.id.clone());
        config.vision.use_default_model = false;
        let value = unavailable_profile_value(&profile, &config);
        assert_eq!(value["id"], "removed");
        assert_eq!(value["status"], "unsupported");
        assert_eq!(value["usages"][0]["kind"], "vision");
        assert_eq!(value["credential"]["configured"], false);
    }

    #[test]
    fn losing_ocr_translation_profile_disables_ocr_without_changing_voice_translation() {
        let mut config = crate::config::AppConfig::default();
        config.ocr.enabled = true;
        config.ocr.targets[0].profile_id = Some("removed".into());
        assert!(uses_translation_profile(&config, "removed"));
        disable_translation_profile(&mut config, "removed");
        assert!(!config.ocr.enabled);
        assert_eq!(config.ocr.targets[0].profile_id, None);
        assert_eq!(
            config.translation,
            crate::config::AppConfig::default().translation
        );
    }

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
        config.translation.speaker_targets[0].use_default_model = false;
        config.translation.microphone_targets[0].use_default_model = false;
        config.translation.speaker_targets[0].model = "gpt-5-mini".into();
        config.translation.microphone_targets[0].model = "gpt-5-mini".into();
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

        assert_eq!(config.asr.backend, crate::config::QWEN_MANAGED_BACKEND);
        assert_eq!(config.asr.active_profile_id, None);
        assert_eq!(config.translation.mode, "disabled");
        assert_eq!(config.translation.speaker_targets[0].profile_id, None);
        assert_eq!(config.translation.microphone_targets[0].profile_id, None);
    }

    #[test]
    fn losing_or_deleting_image_profile_disables_only_its_visual_questions() {
        let mut config = crate::config::AppConfig::default();
        config.vision.use_default_model = false;
        config.vision.enabled = true;
        config.vision.profile_id = Some("image".into());
        config.vision.model = "image-model".into();
        let profile = ApiProfile {
            id: "image".into(),
            provider: providers::OPENAI_COMPATIBLE_PROVIDER.into(),
            enabled_capabilities: vec![CAPABILITY_TEXT_GENERATION.into()],
            ..Default::default()
        };
        let ocr = config.ocr.clone();
        apply_profile_compatibility_fallbacks(&mut config, &profile);
        assert!(!config.vision.enabled);
        assert!(config.vision.profile_id.is_none());
        assert_eq!(config.vision.model, "image-model");
        assert_eq!(config.ocr, ocr);
        config.vision.profile_id = Some("image".into());
        disable_vision_profile(&mut config, "unrelated");
        assert!(config.vision.profile_id.is_some());
        disable_vision_profile(&mut config, "image");
        assert!(config.vision.profile_id.is_none());
    }

    #[test]
    fn losing_or_deleting_text_profile_disables_conversation_titles() {
        let mut config = crate::config::AppConfig::default();
        config.conversation_titles.use_default_model = false;
        config.conversation_titles.auto_generate = true;
        config.conversation_titles.profile_id = Some("text".into());
        config.conversation_titles.model = "title-model".into();
        let profile = ApiProfile {
            id: "text".into(),
            provider: providers::OPENAI_COMPATIBLE_PROVIDER.into(),
            enabled_capabilities: vec![CAPABILITY_IMAGE_UNDERSTANDING.into()],
            ..Default::default()
        };
        apply_profile_compatibility_fallbacks(&mut config, &profile);
        assert!(!config.conversation_titles.auto_generate);
        assert!(config.conversation_titles.profile_id.is_none());
        assert_eq!(config.conversation_titles.model, "title-model");
        config.conversation_titles.profile_id = Some("text".into());
        disable_title_profile(&mut config, "unrelated");
        assert!(config.conversation_titles.profile_id.is_some());
        disable_title_profile(&mut config, "text");
        assert!(config.conversation_titles.profile_id.is_none());
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
    fn qwen_ai_catalog_and_readiness_do_not_require_model_studio_metadata() {
        let value = provider_value(providers::definition(providers::QWEN_AI_PROVIDER).unwrap());
        assert_eq!(value["display_name"], "千问AI平台");
        assert_eq!(value["connection"]["fields"], json!([]));
        assert_eq!(
            value["connection"]["base_url"]["default"],
            "https://maas.qianwenaiapi.com/compatible-mode/v1"
        );
        assert!(value["services"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == "qwen_live_translate"));
        let profile = ApiProfile {
            provider: providers::QWEN_AI_PROVIDER.into(),
            ..Default::default()
        };
        assert!(ensure_asr_profile_ready(&profile).is_ok());
    }

    #[test]
    fn qwen_cloud_catalog_and_normalization_preserve_the_selected_region() {
        let value = provider_value(providers::definition("qwen_cloud").unwrap());
        let fields = value["connection"]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0]["id"], "region");
        assert_eq!(fields[0]["default"], "ap-southeast-1");
        let regions = fields[0]["options"].as_array().unwrap();
        assert_eq!(regions.len(), 6);
        for region in regions {
            let mut profile = ApiProfile {
                provider: "qwen_cloud".into(),
                region: Some(format!(" {} ", region["value"].as_str().unwrap())),
                enabled_capabilities: vec![CAPABILITY_SPEECH_TO_TEXT.into()],
                ..Default::default()
            };
            normalize_profile_fields(&mut profile).unwrap();
            assert_eq!(profile.region.as_deref(), region["value"].as_str());
            assert!(profile.workspace_id.is_none());
            assert!(ensure_asr_profile_ready(&profile).is_ok());
        }
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
