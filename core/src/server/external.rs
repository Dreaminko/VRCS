use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;

use crate::credentials;

use super::{api_error, ApiResult, SettingsContext};

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
    State(state): State<SettingsContext>,
) -> Json<crate::external_api::ExternalApiRuntimeStatus> {
    Json(
        state
            .integrations
            .external_api_status
            .read()
            .expect("External API status lock")
            .clone(),
    )
}

pub(super) async fn token_write(
    State(state): State<SettingsContext>,
    Json(input): Json<TokenInput>,
) -> ApiResult<Json<credentials::CredentialStatus>> {
    let _config_control = state.config.config_control.lock().await;
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
    let previous = credentials::read_stored_external_api_token().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "external_api.token_status_failed",
            error,
        )
    })?;
    let token = input.token.trim().to_string();
    credentials::write_external_api_token(&token).map_err(|error| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "external_api.token_invalid",
            error,
        )
    })?;

    let config = state
        .config
        .config
        .read()
        .expect("config lock")
        .external_api
        .clone();
    if config.enabled && config.require_token {
        if let Err(error) =
            super::settings::reload_external_api_runtime(&state, &config, Some(token)).await
        {
            if let Err(recovery) = restore_token(previous) {
                return Err(api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "settings.rollback_failed",
                    format!("{error}; token rollback failed: {recovery}"),
                ));
            }
            return Err(api_error(
                StatusCode::CONFLICT,
                "external_api.token_reload_failed",
                error,
            ));
        }
    }
    token_status().await
}

pub(super) async fn token_delete(
    State(state): State<SettingsContext>,
) -> ApiResult<Json<credentials::CredentialStatus>> {
    let _config_control = state.config.config_control.lock().await;
    let token_required = {
        let config = state.config.config.read().expect("config lock");
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

fn restore_token(previous: Option<String>) -> Result<(), String> {
    match previous {
        Some(token) => credentials::write_external_api_token(&token),
        None => credentials::delete_external_api_token(),
    }
}
