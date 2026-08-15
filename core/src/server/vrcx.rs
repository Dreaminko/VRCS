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
    credentials::vrcx_token_status().map(Json).map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "vrcx.token_status_failed",
            error,
        )
    })
}

pub(super) async fn runtime_status(
    State(state): State<Arc<AppState>>,
) -> Json<crate::vrcx::VrcxRuntimeStatus> {
    Json(state.vrcx.status())
}

pub(super) async fn token_write(
    State(state): State<Arc<AppState>>,
    Json(input): Json<TokenInput>,
) -> ApiResult<Json<credentials::CredentialStatus>> {
    let _config_control = state.config_control.lock().await;
    let current = credentials::vrcx_token_status().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "vrcx.token_status_failed",
            error,
        )
    })?;
    if current.environment_override {
        return Err(api_error(
            StatusCode::CONFLICT,
            "vrcx.token_environment_override",
            "VRCS_VRCX_INTEGRATION_TOKEN overrides stored credentials",
        ));
    }
    let token = input.token.trim().to_string();
    credentials::write_vrcx_token(&token).map_err(|error| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "vrcx.token_invalid",
            error,
        )
    })?;
    let config = state.config.read().expect("config lock").vrcx.clone();
    state.vrcx.reconfigure(config, Some(token)).await;
    token_status().await
}

pub(super) async fn token_delete(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<credentials::CredentialStatus>> {
    let _config_control = state.config_control.lock().await;
    let current = credentials::vrcx_token_status().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "vrcx.token_status_failed",
            error,
        )
    })?;
    if current.environment_override {
        return Err(api_error(
            StatusCode::CONFLICT,
            "vrcx.token_environment_override",
            "VRCS_VRCX_INTEGRATION_TOKEN overrides stored credentials",
        ));
    }
    credentials::delete_vrcx_token().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "vrcx.token_delete_failed",
            error,
        )
    })?;
    let config = state.config.read().expect("config lock").vrcx.clone();
    state.vrcx.reconfigure(config, None).await;
    token_status().await
}

pub(super) async fn test_connection(
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<crate::vrcx::VrcxRuntimeStatus>> {
    let config = state.config.read().expect("config lock").vrcx.clone();
    let token = credentials::read_vrcx_token()
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "vrcx.token_status_failed",
                error,
            )
        })?
        .ok_or_else(|| {
            api_error(
                StatusCode::CONFLICT,
                "vrcx.token_required",
                "Save a VRCX-0 token before testing the connection",
            )
        })?;
    crate::vrcx::VrcxIntegration::test_connection(&config, &token)
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, "vrcx.test_failed", error))
}
