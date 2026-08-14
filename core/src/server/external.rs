use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::credentials;

use super::{api_error, ApiResult, AppState};

#[derive(Deserialize)]
pub(super) struct TokenInput {
    token: String,
}

pub(super) async fn token_status() -> ApiResult<Json<credentials::CredentialStatus>> {
    credentials::external_api_token_status()
        .map(Json)
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "external_api.token_status_failed",
                error,
            )
        })
}

pub(super) async fn runtime_status(
    State(state): State<Arc<AppState>>,
) -> Json<crate::external_api::ExternalApiRuntimeStatus> {
    Json(state.external_api_status.clone())
}

pub(super) async fn token_write(
    Json(input): Json<TokenInput>,
) -> ApiResult<Json<credentials::CredentialStatus>> {
    let current = credentials::external_api_token_status().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "external_api.token_status_failed",
            error,
        )
    })?;
    if current.environment_override {
        return Err(api_error(
            StatusCode::CONFLICT,
            "external_api.token_environment_override",
            "VRCS_EXTERNAL_API_TOKEN overrides stored credentials",
        ));
    }
    credentials::write_external_api_token(&input.token).map_err(|error| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "external_api.token_invalid",
            error,
        )
    })?;
    token_status().await
}

pub(super) async fn token_delete(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<credentials::CredentialStatus>> {
    let token_required = {
        let config = state.config.read().expect("config lock");
        config.external_api.enabled && config.external_api.require_token
    };
    if token_required {
        return Err(api_error(
            StatusCode::CONFLICT,
            "external_api.token_required",
            "Disable token authentication or the External API before deleting its token",
        ));
    }
    let current = credentials::external_api_token_status().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "external_api.token_status_failed",
            error,
        )
    })?;
    if current.environment_override {
        return Err(api_error(
            StatusCode::CONFLICT,
            "external_api.token_environment_override",
            "VRCS_EXTERNAL_API_TOKEN overrides stored credentials",
        ));
    }
    credentials::delete_external_api_token().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "external_api.token_delete_failed",
            error,
        )
    })?;
    token_status().await
}
