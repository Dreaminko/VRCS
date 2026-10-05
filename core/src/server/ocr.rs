use super::{api_error, ApiResult, ServiceContext};
use crate::credentials;
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TokenInput {
    token: String,
}

pub(super) async fn model_status(
    State(state): State<ServiceContext>,
) -> ApiResult<Json<crate::ocr::ModelStatus>> {
    state
        .config
        .local_ocr
        .model_status()
        .await
        .map(Json)
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ocr.model_status_failed",
                error,
            )
        })
}

pub(super) async fn model_download(
    State(state): State<ServiceContext>,
) -> ApiResult<(StatusCode, Json<crate::ocr::ModelStatus>)> {
    state
        .config
        .local_ocr
        .download_models()
        .map(|status| (StatusCode::ACCEPTED, Json(status)))
        .map_err(|error| api_error(StatusCode::CONFLICT, "ocr.model_download_failed", error))
}

pub(super) async fn token_status() -> ApiResult<Json<credentials::CredentialStatus>> {
    credentials::ocr_token_status().map(Json).map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ocr.token_status_failed",
            error,
        )
    })
}

pub(super) async fn token_write(
    State(state): State<ServiceContext>,
    Json(input): Json<TokenInput>,
) -> ApiResult<Json<credentials::CredentialStatus>> {
    let _control = state.config.config_control.lock().await;
    ensure_stored_token_editable()?;
    credentials::write_ocr_token(&input.token)
        .map_err(|error| api_error(StatusCode::UNPROCESSABLE_ENTITY, "ocr.token_invalid", error))?;
    token_status().await
}

pub(super) async fn token_delete(
    State(state): State<ServiceContext>,
) -> ApiResult<Json<credentials::CredentialStatus>> {
    let _control = state.config.config_control.lock().await;
    ensure_stored_token_editable()?;
    credentials::delete_ocr_token().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ocr.token_delete_failed",
            error,
        )
    })?;
    token_status().await
}

fn ensure_stored_token_editable() -> ApiResult<()> {
    let status = credentials::ocr_token_status().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "ocr.token_status_failed",
            error,
        )
    })?;
    if status.environment_override {
        return Err(api_error(
            StatusCode::CONFLICT,
            "ocr.token_environment_override",
            "PADDLEOCR_ACCESS_TOKEN overrides stored credentials",
        ));
    }
    Ok(())
}
