use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::asr;
use crate::config::ApiProfile;
use crate::llm::model_catalog::ModelCatalog;
use crate::providers::{
    self, ProviderServiceDefinition, ServiceAdapter, CAPABILITY_IMAGE_UNDERSTANDING,
    CAPABILITY_SPEECH_TO_TEXT, CAPABILITY_TEXT_GENERATION, CAPABILITY_TEXT_TRANSLATION,
    GROQ_PROVIDER, OPENROUTER_PROVIDER,
};

const DIAGNOSTIC_MAX_OUTPUT_TOKENS: u32 = 1_024;

use super::cloud::{ensure_asr_profile_ready, profile_api_key, profile_not_found};
use super::{api_error, ApiResult, ServiceContext};

#[derive(Default, Deserialize)]
pub(super) struct TestProfileQuery {
    capability: Option<String>,
    service_id: Option<String>,
    model: Option<String>,
}

#[derive(Serialize)]
struct DiagnosticCheck {
    name: &'static str,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

pub(super) async fn credential_test(
    State(state): State<ServiceContext>,
    Path(profile_id): Path<String>,
    Query(query): Query<TestProfileQuery>,
) -> ApiResult<Json<Value>> {
    let started = std::time::Instant::now();
    let mut config = state.config.config.read().expect("config lock").clone();
    config.resolve_model_assignments();
    let profile = config
        .asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(profile_not_found)?;
    let capability = query
        .capability
        .as_deref()
        .or_else(|| profile.enabled_capabilities.first().map(String::as_str))
        .ok_or_else(|| diagnostic_invalid("Select a capability to test".into()))?;
    let resolved = match query.service_id.as_deref() {
        Some(service_id) => {
            let resolved = providers::resolve_profile_service(profile, service_id)
                .map_err(diagnostic_invalid)?;
            if !resolved.service.capabilities.contains(&capability) {
                return Err(diagnostic_invalid(format!(
                    "Service {service_id} does not support capability {capability}"
                )));
            }
            resolved
        }
        None => providers::resolve_profile_capability(profile, capability)
            .map_err(diagnostic_invalid)?,
    };
    let service = resolved.service;
    let bindings = if capability == CAPABILITY_TEXT_GENERATION {
        [
            (&config.learning_ai.profile_id, &config.learning_ai.model),
            (
                &config.conversation_titles.profile_id,
                &config.conversation_titles.model,
            ),
            (
                &config.model_defaults.text.profile_id,
                &config.model_defaults.text.model,
            ),
        ]
    } else {
        [
            (
                &config.translation.model_selection.profile_id,
                &config.translation.model_selection.model,
            ),
            (
                &config.ocr.model_selection.profile_id,
                &config.ocr.model_selection.model,
            ),
            (
                &config.model_defaults.text.profile_id,
                &config.model_defaults.text.model,
            ),
        ]
    };
    let configured = bindings.into_iter().find_map(|(id, model)| {
        (id.as_deref() == Some(&profile.id) && !model.trim().is_empty()).then_some(model.as_str())
    });
    let text_model = configured_test_model(
        service,
        &config.translation,
        &profile.id,
        query
            .model
            .as_deref()
            .filter(|model| !model.trim().is_empty())
            .or(configured),
    );

    let result = match capability {
        CAPABILITY_SPEECH_TO_TEXT => {
            let mut asr_config = config.asr.clone();
            if providers::is_live_translation(service.id) {
                let language = state
                    .config
                    .language_session
                    .read()
                    .expect("language session lock")
                    .resolve(&config);
                asr_config.live_translation_target = language
                    .translation
                    .speaker_targets
                    .first()
                    .map(|t| t.target_language.clone());
            }
            test_recognition_service(&asr_config, profile, service).await
        }
        CAPABILITY_TEXT_GENERATION => {
            if matches!(
                service.adapter,
                ServiceAdapter::OpenAiChatCompletions { .. }
            ) {
                return compatible_diagnostic(&state, profile, text_model.as_deref()).await;
            }
            test_text_generation(&state, profile, service, text_model.as_deref()).await
        }
        CAPABILITY_TEXT_TRANSLATION => {
            test_translation(&state, profile, service, &config, text_model.as_deref()).await
        }
        CAPABILITY_IMAGE_UNDERSTANDING => {
            async {
                let key = profile_api_key(profile)?;
                let client = crate::llm::LlmClient::new(state.integrations.http.clone());
                let preferred = query
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .or_else(|| {
                        (config.vision.profile_id.as_deref() == Some(profile.id.as_str()))
                            .then_some(config.vision.model.as_str())
                    })
                    .or_else(|| {
                        (config.model_defaults.vision.profile_id.as_deref()
                            == Some(profile.id.as_str()))
                        .then_some(config.model_defaults.vision.model.as_str())
                    });
                let model = test_model(&state, profile, service, capability, preferred).await?;
                let details = client.openrouter_model_details(profile, &key, &model).await;
                let max_output_tokens = if profile.provider == OPENROUTER_PROVIDER
                    && crate::llm::openrouter::needs_reasoning_budget(
                        details.as_ref(),
                        false,
                        &model,
                    ) {
                    4096
                } else {
                    128
                };
                client
                    .generate_with_image(
                        profile,
                        &key,
                        crate::llm::LlmRequest {
                            model: &model,
                            instructions: "Describe the supplied test image briefly.",
                            input: "What color is the image?",
                            max_output_tokens,
                            thinking_enabled: false,
                            reasoning_control: None,
                        },
                        &crate::vision::test_png(),
                    )
                    .await
                    .map_err(|error| {
                        api_error(StatusCode::BAD_GATEWAY, error.code, error.detail)
                    })?;
                Ok(())
            }
            .await
        }
        _ => {
            return Err(diagnostic_invalid(format!(
                "Unsupported capability: {capability}"
            )));
        }
    };
    let terminal_check = if capability == CAPABILITY_SPEECH_TO_TEXT {
        "recognition"
    } else {
        "completion"
    };
    diagnostic_result(started, terminal_check, result)
}

async fn test_recognition_service(
    config: &crate::config::AsrConfig,
    profile: &ApiProfile,
    service: &ProviderServiceDefinition,
) -> ApiResult<()> {
    ensure_asr_profile_ready(profile)?;
    match service.adapter {
        ServiceAdapter::QwenRealtime
        | ServiceAdapter::QwenLiveTranslate
        | ServiceAdapter::AlibabaTokenPlanRealtime
        | ServiceAdapter::FunAsrRealtime
        | ServiceAdapter::OpenAiRealtime
        | ServiceAdapter::GeminiTranscribe
        | ServiceAdapter::GeminiLiveTranslate
        | ServiceAdapter::OpenAiRealtimeTranslate => {
            asr::streaming_test_backend(config, &profile.id, Some(service.id))
                .map_err(diagnostic_invalid)?;
            asr::test_streaming_connection(config, &profile.id, Some(service.id))
                .await
                .map_err(|error| {
                    api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "asr.cloud_test_failed",
                        error,
                    )
                })?;
        }
        ServiceAdapter::OpenAiAudioTranscriptions | ServiceAdapter::MimoTranscription => {
            asr::test_cloud_service(config, &profile.id, service.id)
                .await
                .map_err(|error| {
                    api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "asr.cloud_test_failed",
                        error,
                    )
                })?;
        }
        adapter => {
            return Err(diagnostic_invalid(format!(
                "Unsupported speech recognition adapter: {adapter:?}"
            )));
        }
    }
    Ok(())
}

async fn test_text_generation(
    state: &ServiceContext,
    profile: &ApiProfile,
    service: &ProviderServiceDefinition,
    requested_model: Option<&str>,
) -> ApiResult<()> {
    if !matches!(
        service.adapter,
        ServiceAdapter::AlibabaChatCompletions
            | ServiceAdapter::OpenAiResponses
            | ServiceAdapter::GeminiGenerateContent
    ) {
        return Err(diagnostic_invalid(format!(
            "Unsupported text generation adapter: {:?}",
            service.adapter
        )));
    }
    let api_key = profile_api_key(profile)?;
    let model = test_model(
        state,
        profile,
        service,
        CAPABILITY_TEXT_GENERATION,
        requested_model,
    )
    .await?;
    crate::llm::LlmClient::new(state.integrations.http.clone())
        .generate(
            profile,
            &api_key,
            crate::llm::LlmRequest {
                model: &model,
                instructions: "Reply with OK only.",
                input: "Connection test",
                max_output_tokens: DIAGNOSTIC_MAX_OUTPUT_TOKENS,
                thinking_enabled: false,
                reasoning_control: None,
            },
            None,
        )
        .await
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.code, error.detail))?;
    Ok(())
}

async fn test_translation(
    state: &ServiceContext,
    profile: &ApiProfile,
    service: &ProviderServiceDefinition,
    config: &crate::config::AppConfig,
    requested_model: Option<&str>,
) -> ApiResult<()> {
    if !matches!(
        service.adapter,
        ServiceAdapter::AlibabaChatCompletions
            | ServiceAdapter::OpenAiResponses
            | ServiceAdapter::OpenAiChatCompletions { .. }
            | ServiceAdapter::GeminiGenerateContent
            | ServiceAdapter::DeepLTextTranslation
            | ServiceAdapter::MicrosoftTextTranslation
    ) {
        return Err(diagnostic_invalid(format!(
            "Unsupported translation adapter: {:?}",
            service.adapter
        )));
    }
    let model = if matches!(
        service.adapter,
        ServiceAdapter::DeepLTextTranslation | ServiceAdapter::MicrosoftTextTranslation
    ) {
        String::new()
    } else {
        test_model(
            state,
            profile,
            service,
            CAPABILITY_TEXT_TRANSLATION,
            requested_model,
        )
        .await?
    };
    let target = crate::config::TranslationTargetConfig {
        target_language: "en".into(),
        use_default_model: false,
        profile_id: Some(profile.id.clone()),
        model,
        model_by_profile: Default::default(),
        thinking_enabled: false,
        reasoning_control: None,
    };
    let prompt = crate::config::TranslationPromptConfig::default();
    state
        .content
        .translation_service
        .translate(
            &target,
            &prompt,
            &config.asr.api_profiles,
            "こんにちは",
            Some("ja"),
            &[],
        )
        .await
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.code, error.detail))?;
    Ok(())
}

async fn compatible_diagnostic(
    state: &ServiceContext,
    profile: &ApiProfile,
    requested_model: Option<&str>,
) -> ApiResult<Json<Value>> {
    let started = std::time::Instant::now();
    let mut checks = vec![passed("configuration")];
    let api_key = match profile_api_key(profile) {
        Ok(api_key) => api_key,
        Err((_, error)) => {
            checks.push(skipped("endpoint"));
            checks.push(failed(
                "authentication",
                error["code"]
                    .as_str()
                    .unwrap_or("profile.credential_store_failed"),
                error["detail"]
                    .as_str()
                    .unwrap_or("Could not read the API key")
                    .into(),
            ));
            checks.push(skipped("models"));
            checks.push(skipped("completion"));
            checks.push(skipped("streaming"));
            return Ok(diagnostic_value(false, started, checks));
        }
    };
    let client = crate::llm::LlmClient::new(state.integrations.http.clone());
    let requested_model = requested_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_owned);
    let model = match client.list_model_catalog(profile, &api_key).await {
        Ok(catalog) => {
            checks.push(passed("endpoint"));
            checks.push(skipped("authentication"));
            checks.push(passed("models"));
            select_test_model(
                profile,
                &catalog,
                CAPABILITY_TEXT_GENERATION,
                requested_model.as_deref(),
            )
        }
        Err(error) if endpoint_error(error.code) => {
            checks.push(failed("endpoint", error.code, error.detail));
            checks.push(skipped("authentication"));
            checks.push(skipped("models"));
            checks.push(skipped("completion"));
            checks.push(skipped("streaming"));
            return Ok(diagnostic_value(false, started, checks));
        }
        Err(error) if error.code == "llm.authentication_failed" => {
            checks.push(passed("endpoint"));
            checks.push(failed("authentication", error.code, error.detail));
            checks.push(skipped("models"));
            checks.push(skipped("completion"));
            checks.push(skipped("streaming"));
            return Ok(diagnostic_value(false, started, checks));
        }
        Err(error) => {
            checks.push(passed("endpoint"));
            checks.push(skipped("authentication"));
            checks.push(warning("models", "llm.models_unsupported", error.detail));
            requested_model
        }
    };
    let Some(model) = model else {
        checks.push(failed(
            "completion",
            "llm.test_model_unavailable",
            "No available test model accepts text input and produces text output".into(),
        ));
        checks.push(skipped("streaming"));
        return Ok(diagnostic_value(false, started, checks));
    };
    let details = client
        .openrouter_model_details(profile, &api_key, &model)
        .await;
    let max_output_tokens = if profile.provider == OPENROUTER_PROVIDER
        && crate::llm::openrouter::needs_reasoning_budget(details.as_ref(), false, &model)
    {
        4096
    } else {
        DIAGNOSTIC_MAX_OUTPUT_TOKENS
    };
    let request = || crate::llm::LlmRequest {
        model: &model,
        instructions: "Reply with OK only.",
        input: "Connection test",
        max_output_tokens,
        thinking_enabled: false,
        reasoning_control: None,
    };
    if let Err(error) = client.generate(profile, &api_key, request(), None).await {
        if error.code == "llm.authentication_failed" {
            if let Some(check) = checks
                .iter_mut()
                .find(|check| check.name == "authentication")
            {
                *check = failed("authentication", error.code, error.detail.clone());
            }
        }
        checks.push(failed("completion", error.code, error.detail));
        checks.push(skipped("streaming"));
        return Ok(diagnostic_value(false, started, checks));
    }
    if let Some(check) = checks
        .iter_mut()
        .find(|check| check.name == "authentication")
    {
        *check = passed("authentication");
    }
    checks.push(passed("completion"));
    let progress = |_: &str| {};
    match client
        .test_openai_compatible_streaming(profile, &api_key, request(), &progress)
        .await
    {
        Ok(_) => {
            checks.push(passed("streaming"));
            Ok(diagnostic_value(true, started, checks))
        }
        Err(error) => {
            checks.push(failed("streaming", error.code, error.detail));
            Ok(diagnostic_value(false, started, checks))
        }
    }
}

async fn test_model(
    state: &ServiceContext,
    profile: &ApiProfile,
    service: &ProviderServiceDefinition,
    capability: &str,
    requested: Option<&str>,
) -> ApiResult<String> {
    let requested = requested.map(str::trim).filter(|model| !model.is_empty());
    if !service.supports_model_listing {
        return requested
            .map(str::to_owned)
            .ok_or_else(|| test_model_unavailable(capability));
    }
    let api_key = profile_api_key(profile)?;
    let catalog = match crate::llm::LlmClient::new(state.integrations.http.clone())
        .list_model_catalog(profile, &api_key)
        .await
    {
        Ok(mut catalog) => {
            catalog.models = providers::compatible_service_models(service, catalog.models);
            catalog
        }
        Err(error) => {
            return requested
                .map(str::to_owned)
                .ok_or_else(|| api_error(StatusCode::BAD_GATEWAY, error.code, error.detail))
        }
    };
    select_test_model(profile, &catalog, capability, requested)
        .ok_or_else(|| test_model_unavailable(capability))
}

fn configured_test_model(
    service: &ProviderServiceDefinition,
    translation: &crate::config::TranslationConfig,
    profile_id: &str,
    requested: Option<&str>,
) -> Option<String> {
    requested
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .or_else(|| {
            let targets = translation
                .speaker_targets
                .iter()
                .chain(&translation.microphone_targets);
            targets
                .clone()
                .filter(|target| target.profile_id.as_deref() == Some(profile_id))
                .map(|target| target.model.trim())
                .find(|model| !model.is_empty())
                .or_else(|| {
                    targets
                        .filter_map(|target| target.model_by_profile.get(profile_id))
                        .map(|model| model.trim())
                        .find(|model| !model.is_empty())
                })
        })
        .or_else(|| service.models.first().copied())
        .map(str::to_owned)
}

fn select_test_model(
    profile: &ApiProfile,
    catalog: &ModelCatalog,
    capability: &str,
    preferred: Option<&str>,
) -> Option<String> {
    let input = if capability == CAPABILITY_IMAGE_UNDERSTANDING {
        "image"
    } else {
        "text"
    };
    let recommendations = providers::definition(&profile.provider)?.diagnostic_models;
    catalog
        .models
        .iter()
        .enumerate()
        .filter_map(|(index, model)| {
            if model.trim().is_empty() {
                return None;
            }
            let details = catalog.model_details.get(model);
            if details.is_some_and(|details| {
                (!details.input_modalities.is_empty()
                    && !details.input_modalities.iter().any(|kind| kind == input))
                    || (!details.output_modalities.is_empty()
                        && !details.output_modalities.iter().any(|kind| kind == "text"))
            }) {
                return None;
            }
            let known_input = details
                .is_some_and(|details| details.input_modalities.iter().any(|kind| kind == input));
            let recommendation = recommendations
                .iter()
                .position(|candidate| model == candidate)
                .unwrap_or(usize::MAX);
            if !known_input
                && recommendation == usize::MAX
                && preferred != Some(model.as_str())
                && !matches!(
                    profile.provider.as_str(),
                    GROQ_PROVIDER
                        | providers::OPENAI_COMPATIBLE_PROVIDER
                        | providers::LM_STUDIO_PROVIDER
                        | providers::OLLAMA_PROVIDER
                )
            {
                return None;
            }
            // Name heuristics are a fallback only when capability metadata is missing.
            let normalized = model.to_ascii_lowercase();
            if !known_input
                && [
                    "embed",
                    "rerank",
                    "whisper",
                    "transcri",
                    "asr",
                    "tts",
                    "speech",
                    "orpheus",
                    "guard",
                    "audio",
                    "realtime",
                    "image",
                    "moderation",
                ]
                .iter()
                .any(|kind| normalized.contains(kind))
            {
                return None;
            }
            let deprecated =
                details.and_then(|details| details.status.as_deref()) == Some("deprecated");
            Some((
                (
                    !known_input,
                    deprecated,
                    preferred != Some(model.as_str()),
                    recommendation,
                    index,
                ),
                model,
            ))
        })
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, model)| model.clone())
}

fn test_model_unavailable(capability: &str) -> (StatusCode, Json<Value>) {
    api_error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "llm.test_model_unavailable",
        format!("No available test model supports {capability} with text output"),
    )
}

fn diagnostic_invalid(detail: String) -> (StatusCode, Json<Value>) {
    api_error(StatusCode::UNPROCESSABLE_ENTITY, "profile.invalid", detail)
}

fn diagnostic_result(
    started: std::time::Instant,
    terminal_check: &'static str,
    result: ApiResult<()>,
) -> ApiResult<Json<Value>> {
    let terminal = match result {
        Ok(()) => passed(terminal_check),
        Err((status, body)) if !status.is_server_error() => return Err((status, body)),
        Err((_, body)) => failed(
            terminal_check,
            body["code"].as_str().unwrap_or("profile.test_failed"),
            body["detail"]
                .as_str()
                .unwrap_or("The diagnostic request failed")
                .to_owned(),
        ),
    };
    Ok(diagnostic_value(
        terminal.status == "passed",
        started,
        vec![passed("configuration"), terminal],
    ))
}

fn diagnostic_value(
    ok: bool,
    started: std::time::Instant,
    checks: Vec<DiagnosticCheck>,
) -> Json<Value> {
    Json(json!({
        "ok": ok,
        "latency_ms": started.elapsed().as_millis() as u64,
        "checks": checks,
    }))
}

fn passed(name: &'static str) -> DiagnosticCheck {
    DiagnosticCheck {
        name,
        status: "passed",
        code: None,
        detail: None,
    }
}

fn skipped(name: &'static str) -> DiagnosticCheck {
    DiagnosticCheck {
        name,
        status: "skipped",
        code: None,
        detail: None,
    }
}

fn warning(name: &'static str, code: &'static str, detail: String) -> DiagnosticCheck {
    DiagnosticCheck {
        name,
        status: "warning",
        code: Some(code.into()),
        detail: Some(detail),
    }
}

fn failed(name: &'static str, code: impl Into<String>, detail: String) -> DiagnosticCheck {
    DiagnosticCheck {
        name,
        status: "failed",
        code: Some(code.into()),
        detail: Some(detail),
    }
}

fn endpoint_error(code: &str) -> bool {
    matches!(
        code,
        "llm.timeout"
            | "llm.network_failed"
            | "llm.dns_failed"
            | "llm.connection_refused"
            | "llm.tls_failed"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{
        ALIBABA_TOKEN_PLAN_PROVIDER, DEEPSEEK_PROVIDER, GEMINI_PROVIDER, LM_STUDIO_PROVIDER,
        OLLAMA_PROVIDER, OPENAI_COMPATIBLE_PROVIDER, OPENAI_PROVIDER, SERVICE_GROQ_TRANSCRIPTION,
    };

    fn select_listed_model(provider: &str, models: Vec<String>) -> Option<String> {
        let profile = ApiProfile {
            provider: provider.into(),
            ..Default::default()
        };
        let mut catalog = ModelCatalog {
            models,
            ..Default::default()
        };
        crate::llm::models_dev::supplement(&profile, &mut catalog);
        select_test_model(&profile, &catalog, CAPABILITY_TEXT_GENERATION, None)
    }

    #[test]
    fn image_diagnostics_use_public_capabilities_and_prefer_current_models() {
        let profile = ApiProfile {
            provider: DEEPSEEK_PROVIDER.into(),
            ..Default::default()
        };
        let mut catalog = crate::llm::model_catalog::ModelCatalog {
            models: vec![
                "deepseek-v4-pro".into(),
                "deepseek-v4-flash".into(),
                "deepseek-flash".into(),
            ],
            ..Default::default()
        };
        crate::llm::models_dev::supplement(&profile, &mut catalog);
        assert_eq!(
            select_test_model(&profile, &catalog, CAPABILITY_IMAGE_UNDERSTANDING, None).as_deref(),
            Some("deepseek-flash")
        );
        assert_eq!(
            select_test_model(
                &profile,
                &catalog,
                CAPABILITY_IMAGE_UNDERSTANDING,
                Some("deepseek-v4-pro")
            )
            .as_deref(),
            Some("deepseek-flash")
        );
        catalog.models = vec!["deepseek-v4-pro".into()];
        assert!(
            select_test_model(&profile, &catalog, CAPABILITY_IMAGE_UNDERSTANDING, None).is_none()
        );
    }

    #[test]
    fn text_diagnostics_accept_current_models_outside_fixed_recommendations() {
        assert_eq!(
            select_listed_model(DEEPSEEK_PROVIDER, vec!["deepseek-flash".into()]).as_deref(),
            Some("deepseek-flash")
        );
    }

    #[test]
    fn image_diagnostics_respect_native_capabilities_and_available_models() {
        let profile = ApiProfile {
            id: "diagnostic-native-capabilities".into(),
            provider: DEEPSEEK_PROVIDER.into(),
            ..Default::default()
        };
        let mut catalog = ModelCatalog {
            models: vec!["deepseek-flash".into(), "available-vision".into()],
            model_details: [
                ("deepseek-flash".into(), crate::llm::model_catalog::ModelDetails::from_native(
                    &json!({"input_modalities": ["text"], "output_modalities": ["text"]}),
                    DEEPSEEK_PROVIDER,
                )),
                ("available-vision".into(), crate::llm::model_catalog::ModelDetails::from_native(
                    &json!({"input_modalities": ["text", "image"], "output_modalities": ["text"]}),
                    DEEPSEEK_PROVIDER,
                )),
            ].into(),
        };
        crate::llm::models_dev::supplement(&profile, &mut catalog);
        for preferred in [Some("deepseek-flash"), Some("removed-vision"), None] {
            assert_eq!(
                select_test_model(
                    &profile,
                    &catalog,
                    CAPABILITY_IMAGE_UNDERSTANDING,
                    preferred
                )
                .as_deref(),
                Some("available-vision")
            );
        }
        catalog.models.clear();
        assert!(select_test_model(
            &profile,
            &catalog,
            CAPABILITY_IMAGE_UNDERSTANDING,
            Some("deepseek-flash")
        )
        .is_none());
        let (status, Json(error)) = test_model_unavailable(CAPABILITY_IMAGE_UNDERSTANDING);
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["code"], "llm.test_model_unavailable");
    }

    #[test]
    fn image_diagnostics_prefer_image_input_and_text_output() {
        use crate::llm::model_catalog::{ModelCatalog, ModelDetails};

        let profile = ApiProfile {
            provider: OPENAI_COMPATIBLE_PROVIDER.into(),
            ..Default::default()
        };
        let mut catalog = ModelCatalog {
            models: vec![
                "chat-model".into(),
                "image-generator".into(),
                "vision-model".into(),
            ],
            model_details: [
                (
                    "chat-model".into(),
                    ModelDetails {
                        input_modalities: vec!["text".into()],
                        ..Default::default()
                    },
                ),
                (
                    "image-generator".into(),
                    ModelDetails {
                        input_modalities: vec!["text".into(), "image".into()],
                        output_modalities: vec!["image".into()],
                        ..Default::default()
                    },
                ),
                (
                    "vision-model".into(),
                    ModelDetails {
                        input_modalities: vec!["text".into(), "image".into()],
                        output_modalities: vec!["text".into()],
                        ..Default::default()
                    },
                ),
            ]
            .into(),
        };
        assert_eq!(
            select_test_model(&profile, &catalog, CAPABILITY_IMAGE_UNDERSTANDING, None).as_deref(),
            Some("vision-model")
        );
        catalog.models.pop();
        assert!(
            select_test_model(&profile, &catalog, CAPABILITY_IMAGE_UNDERSTANDING, None).is_none()
        );
        catalog.model_details.clear();
        assert_eq!(
            select_test_model(&profile, &catalog, CAPABILITY_IMAGE_UNDERSTANDING, None).as_deref(),
            Some("chat-model")
        );
    }

    #[test]
    fn native_diagnostics_return_uniform_success_and_upstream_failure_checks() {
        for (terminal_check, status, code) in [
            (
                "recognition",
                StatusCode::SERVICE_UNAVAILABLE,
                "asr.cloud_test_failed",
            ),
            (
                "completion",
                StatusCode::SERVICE_UNAVAILABLE,
                "translation.upstream_failed",
            ),
            ("completion", StatusCode::BAD_GATEWAY, "llm.upstream_failed"),
        ] {
            let Json(success) =
                diagnostic_result(std::time::Instant::now(), terminal_check, Ok(())).unwrap();
            assert_eq!(success.as_object().unwrap().len(), 3);
            assert_eq!(success["ok"], true);
            assert!(success["latency_ms"].is_u64());
            assert_eq!(
                success["checks"],
                json!([
                    { "name": "configuration", "status": "passed" },
                    { "name": terminal_check, "status": "passed" },
                ])
            );

            let Json(failure) = diagnostic_result(
                std::time::Instant::now(),
                terminal_check,
                Err(api_error(
                    status,
                    code.to_owned(),
                    "Upstream is unavailable",
                )),
            )
            .unwrap();
            assert_eq!(failure.as_object().unwrap().len(), 3);
            assert_eq!(failure["ok"], false);
            assert!(failure["latency_ms"].is_u64());
            assert_eq!(
                failure["checks"],
                json!([
                    { "name": "configuration", "status": "passed" },
                    {
                        "name": terminal_check,
                        "status": "failed",
                        "code": code,
                        "detail": "Upstream is unavailable",
                    },
                ])
            );
        }
    }

    #[test]
    fn invalid_diagnostic_parameters_remain_http_errors() {
        let (status, Json(body)) = diagnostic_result(
            std::time::Instant::now(),
            "completion",
            Err(diagnostic_invalid("Select a capability to test".into())),
        )
        .unwrap_err();

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "profile.invalid");
        assert_eq!(body["detail"], "Select a capability to test");
        assert!(body.get("checks").is_none());
    }

    #[test]
    fn mimo_diagnostic_selects_only_available_standard_chat_models() {
        assert_eq!(
            select_listed_model(
                "mimo",
                vec![
                    "mimo-v2.5-asr".into(),
                    "mimo-v2.5-tts".into(),
                    "mimo-v2.6-pro-ultraspeed".into(),
                    "mimo-v2.6-pro".into(),
                    "mimo-v2.6-flash".into(),
                ]
            )
            .as_deref(),
            Some("mimo-v2.6-flash")
        );
        assert!(select_listed_model(
            "mimo",
            vec!["mimo-v2.5-asr".into(), "mimo-v2.6-pro-ultraspeed".into()]
        )
        .is_none());
    }
    #[test]
    fn explicit_service_must_match_requested_capability() {
        let profile = ApiProfile {
            id: "groq".into(),
            name: "Groq".into(),
            provider: GROQ_PROVIDER.into(),
            enabled_capabilities: vec![
                CAPABILITY_SPEECH_TO_TEXT.into(),
                CAPABILITY_TEXT_GENERATION.into(),
            ],
            ..ApiProfile::default()
        };
        let resolved =
            providers::resolve_profile_service(&profile, SERVICE_GROQ_TRANSCRIPTION).unwrap();

        assert!(resolved
            .service
            .capabilities
            .contains(&CAPABILITY_SPEECH_TO_TEXT));
        assert!(!resolved
            .service
            .capabilities
            .contains(&CAPABILITY_TEXT_GENERATION));
    }

    #[test]
    fn groq_diagnostic_prefers_a_general_chat_model() {
        let models = vec![
            "whisper-large-v3-turbo".into(),
            "meta-llama/llama-guard-4-12b".into(),
            "openai/gpt-oss-120b".into(),
            "openai/gpt-oss-20b".into(),
        ];

        assert_eq!(
            select_listed_model(GROQ_PROVIDER, models).as_deref(),
            Some("openai/gpt-oss-20b")
        );
    }

    #[test]
    fn compatible_and_local_diagnostics_select_a_listed_text_model() {
        let non_text_models = vec![
            " ".into(),
            "text-embedding-3-small".into(),
            "NOMIC-EMBED-TEXT".into(),
            "bge-reranker".into(),
            "whisper-large-v3".into(),
            "qwen3-asr".into(),
            "tts-model".into(),
            "audio-model".into(),
        ];

        for provider in [
            OPENAI_COMPATIBLE_PROVIDER,
            LM_STUDIO_PROVIDER,
            OLLAMA_PROVIDER,
        ] {
            let mut models = non_text_models.clone();
            models.extend(["qwen3:8b".into(), "chat-model".into()]);

            assert_eq!(
                select_listed_model(provider, models).as_deref(),
                Some("qwen3:8b"),
                "{provider}"
            );
            assert_eq!(
                select_listed_model(provider, non_text_models.clone()),
                None,
                "{provider}"
            );
        }
    }

    #[test]
    fn provider_diagnostics_select_only_known_text_models() {
        assert_eq!(
            select_listed_model(
                OPENAI_PROVIDER,
                vec![
                    "text-embedding-3-small".into(),
                    "gpt-5-mini".into(),
                    "gpt-4.1-mini".into(),
                ],
            )
            .as_deref(),
            Some("gpt-4.1-mini")
        );
        assert_eq!(
            select_listed_model(
                DEEPSEEK_PROVIDER,
                vec!["deepseek-embedding".into(), "deepseek-v4-pro".into()],
            )
            .as_deref(),
            Some("deepseek-v4-pro")
        );
        assert_eq!(
            select_listed_model(GEMINI_PROVIDER, vec!["text-embedding-004".into()]),
            None
        );
        assert_eq!(
            select_listed_model(OPENAI_PROVIDER, vec!["unknown-chat-model".into()]),
            None
        );
        assert_eq!(
            select_listed_model(
                ALIBABA_TOKEN_PLAN_PROVIDER,
                vec!["qwen-audio-3.0-asr-flash".into(), "qwen3.6-flash".into()],
            )
            .as_deref(),
            Some("qwen3.6-flash")
        );
    }

    #[test]
    fn diagnostic_prefers_requested_then_active_profile_models() {
        let service = &providers::definition(GROQ_PROVIDER).unwrap().services[0];
        let mut translation = crate::config::TranslationConfig {
            speaker_targets: vec![crate::config::TranslationTargetConfig {
                profile_id: Some("other".into()),
                model: "other-model".into(),
                model_by_profile: [("groq".into(), "speaker-memory".into())].into(),
                ..crate::config::TranslationTargetConfig::new("en")
            }],
            microphone_targets: vec![crate::config::TranslationTargetConfig {
                profile_id: Some("groq".into()),
                model: " microphone-model ".into(),
                model_by_profile: [("groq".into(), "stale-model".into())].into(),
                ..crate::config::TranslationTargetConfig::new("ja")
            }],
            ..Default::default()
        };

        assert_eq!(
            configured_test_model(service, &translation, "groq", Some(" custom-model ")).as_deref(),
            Some("custom-model")
        );
        assert_eq!(
            configured_test_model(service, &translation, "groq", Some(" ")).as_deref(),
            Some("microphone-model")
        );

        translation.speaker_targets[0].profile_id = Some("groq".into());
        translation.speaker_targets[0].model = "speaker-model".into();
        assert_eq!(
            configured_test_model(service, &translation, "groq", None).as_deref(),
            Some("speaker-model")
        );
    }

    #[test]
    fn diagnostic_uses_profile_model_memory_before_service_defaults() {
        let service = ProviderServiceDefinition {
            models: &["service-default"],
            ..providers::definition(GROQ_PROVIDER).unwrap().services[0]
        };
        let mut translation = crate::config::TranslationConfig {
            speaker_targets: vec![crate::config::TranslationTargetConfig {
                profile_id: Some("other".into()),
                model: "other-model".into(),
                model_by_profile: [("groq".into(), " ".into())].into(),
                ..crate::config::TranslationTargetConfig::new("en")
            }],
            microphone_targets: vec![crate::config::TranslationTargetConfig {
                model_by_profile: [("groq".into(), " remembered-model ".into())].into(),
                ..crate::config::TranslationTargetConfig::new("ja")
            }],
            ..Default::default()
        };

        assert_eq!(
            configured_test_model(&service, &translation, "groq", None).as_deref(),
            Some("remembered-model")
        );
        translation.microphone_targets.clear();
        assert_eq!(
            configured_test_model(&service, &translation, "groq", None).as_deref(),
            Some("service-default")
        );
    }
}
