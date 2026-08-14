use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::asr;
use crate::config::{
    ApiProfile, TranslationConfig, ALIBABA_PROVIDER, API_PURPOSE_ASR, API_PURPOSE_LLM,
    API_PURPOSE_SHARED, GEMINI_PROVIDER, OPENAI_COMPATIBLE_PROVIDER,
};

use super::cloud::{ensure_asr_profile_ready, profile_api_key, profile_not_found};
use super::{api_error, ApiResult, AppState};

#[derive(Default, Deserialize)]
pub(super) struct TestProfileQuery {
    capability: Option<String>,
    backend: Option<String>,
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
    let capability = query.capability.as_deref().unwrap_or_else(|| {
        if profile.effective_purpose() == API_PURPOSE_ASR
            || (profile.effective_purpose() == API_PURPOSE_SHARED
                && profile.supports_realtime_asr()
                && (profile.provider != ALIBABA_PROVIDER
                    || profile
                        .workspace_id
                        .as_deref()
                        .is_some_and(|workspace| !workspace.trim().is_empty())))
        {
            API_PURPOSE_ASR
        } else {
            API_PURPOSE_LLM
        }
    });
    if capability == API_PURPOSE_LLM && profile.provider == OPENAI_COMPATIBLE_PROVIDER {
        return compatible_diagnostic(&state, profile, query.model.as_deref()).await;
    }
    if capability == API_PURPOSE_LLM {
        if !profile.supports_translation() {
            return Err(api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "translation.profile_invalid",
                "This API profile does not support translation",
            ));
        }
        let settings = TranslationConfig {
            mode: "manual".into(),
            target_language: "en".into(),
            profile_id: Some(profile_id.clone()),
            model: test_model(&state, profile, &config, query.model.as_deref()).await?,
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
            .map_err(|error| {
                api_error(StatusCode::SERVICE_UNAVAILABLE, error.code, error.detail)
            })?;
    } else if capability == API_PURPOSE_ASR {
        if !profile.supports_realtime_asr() {
            return Err(api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "asr.profile_invalid",
                "This API profile does not support realtime speech recognition",
            ));
        }
        ensure_asr_profile_ready(profile)?;
        asr::streaming_test_backend(&config.asr, &profile_id, query.backend.as_deref()).map_err(
            |error| {
                api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "asr.profile_invalid",
                    error,
                )
            },
        )?;
        asr::test_streaming_connection(&config.asr, &profile_id, query.backend.as_deref())
            .await
            .map_err(|error| {
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "asr.cloud_test_failed",
                    error,
                )
            })?;
    } else {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "asr.profile_invalid",
            "Capability must be asr or llm",
        ));
    }
    Ok(Json(json!({ "ok": true })))
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
            requested_model.or_else(|| models.into_iter().next())
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
    config: &crate::config::AppConfig,
    requested: Option<&str>,
) -> ApiResult<String> {
    if let Some(model) = requested.map(str::trim).filter(|model| !model.is_empty()) {
        return Ok(model.into());
    }
    if profile.provider == ALIBABA_PROVIDER {
        return Ok("qwen-plus".into());
    }
    if profile.provider != GEMINI_PROVIDER && !config.translation.model.trim().is_empty() {
        return Ok(config.translation.model.clone());
    }
    let api_key = profile_api_key(profile)?;
    crate::llm::LlmClient::new(state.http.clone())
        .list_models(profile, &api_key)
        .await
        .map_err(|error| api_error(StatusCode::BAD_GATEWAY, error.code, error.detail))?
        .into_iter()
        .next()
        .ok_or_else(|| {
            api_error(
                StatusCode::BAD_GATEWAY,
                "llm.invalid_response",
                "The LLM service did not return any models",
            )
        })
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
