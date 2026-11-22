use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;

use crate::credentials;

use super::{ApiResult, ServiceContext, api_error};

#[derive(Deserialize)]
pub(super) struct TokenInput {
    token: String,
}

pub(super) async fn token_status() -> ApiResult<Json<credentials::CredentialStatus>> {
    credentials::blocking(credentials::vrcx_token_status)
        .await
        .map(Json)
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                credentials::error_reason_code(&error).unwrap_or("vrcx.token_status_failed"),
                error,
            )
        })
}

pub(super) async fn runtime_status(
    State(state): State<ServiceContext>,
) -> Json<crate::vrcx::VrcxRuntimeStatus> {
    Json(state.integrations.vrcx.status())
}

pub(super) async fn token_write(
    State(state): State<ServiceContext>,
    Json(input): Json<TokenInput>,
) -> ApiResult<Json<credentials::CredentialStatus>> {
    credentials::finish_mutation(async move {
        let _config_control = state.config.config_control.lock().await;
        let current = credentials::blocking(credentials::vrcx_token_status)
            .await
            .map_err(|error| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    credentials::error_reason_code(&error).unwrap_or("vrcx.token_status_failed"),
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
        let stored_token = token.clone();
        credentials::blocking(move || credentials::write_vrcx_token(&stored_token))
            .await
            .map_err(|error| {
                api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    credentials::error_reason_code(&error).unwrap_or("vrcx.token_invalid"),
                    error,
                )
            })?;
        let config =
            crate::config::apply_feature_gates(&state.config.config.read().expect("config lock"))
                .vrcx;
        state
            .integrations
            .vrcx
            .reconfigure(config, Some(token))
            .await;
        token_status().await
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "credentials.task_failed",
            error,
        )
    })?
}

pub(super) async fn token_delete(
    State(state): State<ServiceContext>,
) -> ApiResult<Json<credentials::CredentialStatus>> {
    credentials::finish_mutation(async move {
        let _config_control = state.config.config_control.lock().await;
        let current = credentials::blocking(credentials::vrcx_token_status)
            .await
            .map_err(|error| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    credentials::error_reason_code(&error).unwrap_or("vrcx.token_status_failed"),
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
        credentials::blocking(credentials::delete_vrcx_token)
            .await
            .map_err(|error| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    credentials::error_reason_code(&error).unwrap_or("vrcx.token_delete_failed"),
                    error,
                )
            })?;
        let config =
            crate::config::apply_feature_gates(&state.config.config.read().expect("config lock"))
                .vrcx;
        state.integrations.vrcx.reconfigure(config, None).await;
        token_status().await
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "credentials.task_failed",
            error,
        )
    })?
}

pub(super) async fn test_connection(
    State(state): State<ServiceContext>,
) -> ApiResult<Json<crate::vrcx::VrcxRuntimeStatus>> {
    let _config_control = state.config.config_control.lock().await;
    let token = credentials::blocking(credentials::read_vrcx_token_for_use)
        .await
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                credentials::error_reason_code(&error).unwrap_or("vrcx.token_status_failed"),
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
    let config =
        crate::config::apply_feature_gates(&state.config.config.read().expect("config lock")).vrcx;
    crate::vrcx::VrcxIntegration::test_connection(&config, &token)
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::SERVICE_UNAVAILABLE, "vrcx.test_failed", error))
}
