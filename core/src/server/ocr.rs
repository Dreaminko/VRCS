use super::{ApiResult, ServiceContext, api_error};
use axum::{Json, extract::State, http::StatusCode};
pub(super) async fn runtime_status(
    State(state): State<ServiceContext>,
) -> Json<crate::ocr::OcrExecutionStatus> {
    Json(state.config.local_ocr.execution_status())
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

pub(super) async fn model_delete(
    State(state): State<ServiceContext>,
) -> ApiResult<Json<crate::ocr::ModelStatus>> {
    state
        .config
        .local_ocr
        .delete_models()
        .await
        .map(Json)
        .map_err(|error| api_error(StatusCode::CONFLICT, "ocr.model_delete_failed", error))
}
