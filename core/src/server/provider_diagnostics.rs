use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::asr;
use crate::config::{ApiProfile, TranslationConfig};
use crate::providers::{
    self, ProviderServiceDefinition, ServiceAdapter, ALIBABA_PROVIDER, CAPABILITY_SPEECH_TO_TEXT,
    CAPABILITY_TEXT_GENERATION, CAPABILITY_TEXT_TRANSLATION, DEEPSEEK_PROVIDER, GEMINI_PROVIDER,
    GROQ_PROVIDER, OPENAI_PROVIDER, OPENROUTER_PROVIDER,
};

const OPENAI_DIAGNOSTIC_MODELS: &[&str] = &["gpt-5-mini", "gpt-4.1-mini", "gpt-4o-mini"];
const GROQ_DIAGNOSTIC_MODELS: &[&str] = &["openai/gpt-oss-20b", "openai/gpt-oss-120b"];
const DEEPSEEK_DIAGNOSTIC_MODELS: &[&str] = &["deepseek-v4-flash", "deepseek-v4-pro"];
const GEMINI_DIAGNOSTIC_MODELS: &[&str] =
    &["gemini-3.7-flash", "gemini-3.6-flash", "gemini-2.5-flash"];
const ALIBABA_DIAGNOSTIC_MODELS: &[&str] = &["qwen3.6-flash", "qwen3.7-plus", "qwen3.7-max"];
const OPENROUTER_DIAGNOSTIC_MODELS: &[&str] = &[
    "openai/gpt-5-mini",
    "google/gemini-2.5-flash",
    "openai/gpt-4o-mini",
];

use super::cloud::{ensure_asr_profile_ready, profile_api_key, profile_not_found};
use super::{api_error, ApiResult, AppState};

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
    code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

pub(super) async fn credential_test(
    State(state): State<Arc<AppState>>,
    Path(profile_id): Path<String>,
    Query(query): Query<TestProfileQuery>,
) -> ApiResult<Json<Value>> {
    let config = state.config.read().expect("config lock").clone();
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

    match capability {
        CAPABILITY_SPEECH_TO_TEXT => {
            test_recognition_service(&config.asr, profile, service).await?;
        }
        CAPABILITY_TEXT_GENERATION => {
            if matches!(
                service.adapter,
                ServiceAdapter::OpenAiChatCompletions { .. }
            ) {
                return compatible_diagnostic(&state, profile, query.model.as_deref()).await;
            }
            test_text_generation(&state, profile, service, query.model.as_deref()).await?;
        }
        CAPABILITY_TEXT_TRANSLATION => {
            test_translation(&state, profile, service, &config, query.model.as_deref()).await?;
        }
        _ => {
            return Err(diagnostic_invalid(format!(
                "Unsupported capability: {capability}"
            )));
        }
    }
    Ok(Json(json!({ "ok": true })))
}

async fn test_recognition_service(
    config: &crate::config::AsrConfig,
    profile: &ApiProfile,
    service: &ProviderServiceDefinition,
) -> ApiResult<()> {
    ensure_asr_profile_ready(profile)?;
    match service.adapter {
        ServiceAdapter::QwenRealtime
        | ServiceAdapter::FunAsrRealtime
        | ServiceAdapter::OpenAiRealtime => {
            asr::streaming_test_backend(config, &profile.id, Some(service.id)).map_err(
                |error| {
                    api_error(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "asr.profile_invalid",
                        error,
                    )
                },
            )?;
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
        ServiceAdapter::OpenAiAudioTranscriptions => {
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
    state: &Arc<AppState>,
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
    let model = test_model(state, profile, service, requested_model).await?;
    crate::llm::LlmClient::new(state.http.clone())
        .generate(
            profile,
            &api_key,
            crate::llm::LlmRequest {
                model: &model,
                instructions: "Reply with OK only.",
                input: "Connection test",
                max_output_tokens: 8,
                thinking_enabled: false,
            },
            None,
        )
        .await
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.code, error.detail))?;
    Ok(())
}

async fn test_translation(
    state: &Arc<AppState>,
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
        test_model(state, profile, service, requested_model).await?
    };
    let settings = TranslationConfig {
        mode: "manual".into(),
        target_language: "en".into(),
        profile_id: Some(profile.id.clone()),
        model,
        thinking_enabled: false,
        ..TranslationConfig::default()
    };
    state
        .translation_service
        .translate(
            &settings,
            &config.asr.api_profiles,
            "こんにちは",
            Some("ja"),
            None,
            &[],
        )
        .await
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, error.code, error.detail))?;
    Ok(())
}

async fn compatible_diagnostic(
    state: &Arc<AppState>,
    profile: &ApiProfile,
    requested_model: Option<&str>,
) -> ApiResult<Json<Value>> {
    let started = std::time::Instant::now();
    let mut checks = vec![passed("configuration")];
    let api_key = match profile_api_key(profile) {
        Ok(api_key) => api_key,
        Err(_) => {
            checks.push(skipped("endpoint"));
            checks.push(failed(
                "authentication",
                "translation.credential_missing",
                "Configure an API key before testing this profile".into(),
            ));
            checks.push(skipped("models"));
            checks.push(skipped("completion"));
            checks.push(skipped("streaming"));
            return Ok(diagnostic_value(false, started, checks));
        }
    };
    let client = crate::llm::LlmClient::new(state.http.clone());
    let requested_model = requested_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_owned);
    let model = match client.list_models(profile, &api_key).await {
        Ok(models) => {
            checks.push(passed("endpoint"));
            checks.push(passed("authentication"));
            checks.push(passed("models"));
            requested_model.or_else(|| select_listed_model(&profile.provider, models))
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
            checks.push(passed("authentication"));
            checks.push(warning("models", "llm.models_unsupported", error.detail));
            requested_model
        }
    };
    let Some(model) = model else {
        checks.push(failed(
            "completion",
            "llm.model_required",
            "Enter a model name because the service did not return a model list".into(),
        ));
        checks.push(skipped("streaming"));
        return Ok(diagnostic_value(false, started, checks));
    };
    let request = || crate::llm::LlmRequest {
        model: &model,
        instructions: "Reply with OK only.",
        input: "Connection test",
        max_output_tokens: 8,
        thinking_enabled: false,
    };
    if let Err(error) = client.generate(profile, &api_key, request(), None).await {
        checks.push(failed("completion", error.code, error.detail));
        checks.push(skipped("streaming"));
        return Ok(diagnostic_value(false, started, checks));
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
            let code = if error.code == "llm.invalid_response" {
                "llm.sse_incompatible"
            } else {
                error.code
            };
            checks.push(failed("streaming", code, error.detail));
            Ok(diagnostic_value(false, started, checks))
        }
    }
}

async fn test_model(
    state: &Arc<AppState>,
    profile: &ApiProfile,
    service: &ProviderServiceDefinition,
    requested: Option<&str>,
) -> ApiResult<String> {
    if let Some(model) = configured_test_model(service, requested) {
        return Ok(model);
    }
    if !service.supports_model_listing {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "llm.model_required",
            format!("Enter a model name for service {}", service.id),
        ));
    }
    let api_key = profile_api_key(profile)?;
    let models = crate::llm::LlmClient::new(state.http.clone())
        .list_models(profile, &api_key)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.code, error.detail))?;
    select_listed_model(&profile.provider, models).ok_or_else(|| {
        api_error(
            StatusCode::BAD_GATEWAY,
            "llm.invalid_response",
            "The LLM service did not return a usable text model",
        )
    })
}

fn configured_test_model(
    service: &ProviderServiceDefinition,
    requested: Option<&str>,
) -> Option<String> {
    requested
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_owned)
        .or_else(|| service.models.first().map(|model| (*model).into()))
}

fn select_listed_model(provider: &str, models: Vec<String>) -> Option<String> {
    let preferred = match provider {
        OPENAI_PROVIDER => OPENAI_DIAGNOSTIC_MODELS,
        GROQ_PROVIDER => GROQ_DIAGNOSTIC_MODELS,
        DEEPSEEK_PROVIDER => DEEPSEEK_DIAGNOSTIC_MODELS,
        GEMINI_PROVIDER => GEMINI_DIAGNOSTIC_MODELS,
        ALIBABA_PROVIDER => ALIBABA_DIAGNOSTIC_MODELS,
        OPENROUTER_PROVIDER => OPENROUTER_DIAGNOSTIC_MODELS,
        _ => return None,
    };
    let selected = preferred
        .iter()
        .find_map(|candidate| models.iter().find(|model| model == candidate).cloned());
    if selected.is_some() || provider != GROQ_PROVIDER {
        return selected;
    }
    models.into_iter().find(|model| {
        let model = model.to_ascii_lowercase();
        !["whisper", "tts", "speech", "orpheus", "guard"]
            .iter()
            .any(|kind| model.contains(kind))
    })
}

fn diagnostic_invalid(detail: String) -> (StatusCode, Json<Value>) {
    api_error(
        StatusCode::UNPROCESSABLE_ENTITY,
        "asr.profile_invalid",
        detail,
    )
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
        code: Some(code),
        detail: Some(detail),
    }
}

fn failed(name: &'static str, code: &'static str, detail: String) -> DiagnosticCheck {
    DiagnosticCheck {
        name,
        status: "failed",
        code: Some(code),
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
        LM_STUDIO_PROVIDER, OPENAI_COMPATIBLE_PROVIDER, SERVICE_GROQ_TRANSCRIPTION,
    };

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
    fn diagnostics_do_not_guess_unknown_or_local_models() {
        let models = vec![
            "embedding-model".into(),
            "audio-model".into(),
            "chat-model".into(),
        ];

        assert_eq!(
            select_listed_model(LM_STUDIO_PROVIDER, models.clone()),
            None
        );
        assert_eq!(
            select_listed_model(OPENAI_COMPATIBLE_PROVIDER, models),
            None
        );
    }

    #[test]
    fn provider_diagnostics_select_only_known_text_models() {
        assert_eq!(
            select_listed_model(
                OPENAI_PROVIDER,
                vec!["text-embedding-3-small".into(), "gpt-5-mini".into()],
            )
            .as_deref(),
            Some("gpt-5-mini")
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
    }

    #[test]
    fn diagnostic_uses_only_an_explicit_or_service_model() {
        let profile = ApiProfile {
            id: "groq".into(),
            provider: GROQ_PROVIDER.into(),
            enabled_capabilities: vec![CAPABILITY_TEXT_TRANSLATION.into()],
            ..ApiProfile::default()
        };
        let service = providers::resolve_profile_capability(&profile, CAPABILITY_TEXT_TRANSLATION)
            .unwrap()
            .service;

        assert_eq!(configured_test_model(service, None), None);
        assert_eq!(
            configured_test_model(service, Some(" custom-model ")).as_deref(),
            Some("custom-model")
        );
    }
}
