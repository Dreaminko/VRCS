use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::config::GlossaryConfig;
use crate::glossary::GlossaryRefreshError;

use super::{ApiResult, CaptureContext, api_error};

pub(super) async fn statuses(State(state): State<CaptureContext>) -> Json<Value> {
    let config = current_config(&state);
    Json(json!(state.content.glossary.statuses(&config)))
}

pub(super) async fn refresh(
    State(state): State<CaptureContext>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    if state
        .content
        .glossary
        .refresh(&id)
        .await
        .map_err(refresh_error)?
    {
        reload_asr_context(&state).await;
    }
    let config = current_config(&state);
    Ok(Json(json!(state.content.glossary.statuses(&config))))
}

async fn reload_asr_context(state: &CaptureContext) {
    if let Err((_, body)) = super::capture::reload_glossary_asr_context(state).await {
        tracing::warn!(detail = ?body, "ASR glossary context could not be reloaded");
    }
}

fn current_config(state: &CaptureContext) -> GlossaryConfig {
    state
        .config
        .config
        .read()
        .expect("config lock")
        .glossary
        .clone()
}

fn refresh_error(error: GlossaryRefreshError) -> (StatusCode, Json<Value>) {
    let status = match error.code {
        "glossary_subscription.not_found" => StatusCode::NOT_FOUND,
        "glossary_subscription.disabled" => StatusCode::CONFLICT,
        "glossary_subscription.invalid_url"
        | "glossary_subscription.invalid_redirect"
        | "glossary_subscription.invalid_json"
        | "glossary_subscription.unsupported_version"
        | "glossary_subscription.too_large"
        | "glossary_subscription.too_many_entries"
        | "glossary_subscription.invalid_entry"
        | "glossary_subscription.duplicate_entry" => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::BAD_GATEWAY,
    };
    api_error(status, error.code, error.detail)
}
