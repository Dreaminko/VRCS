use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};

use super::{ApiResult, AppState, api_error};

pub(super) async fn test_image(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let answer = crate::vision::VisionService::new(state)
        .test_image()
        .await
        .map_err(|detail| {
            let (code, message) = match detail.split_once(": ") {
                Some((prefix, rest)) if prefix.starts_with("llm.") => {
                    (prefix.to_owned(), rest.to_owned())
                }
                _ => ("vision.test_failed".to_owned(), detail),
            };
            api_error(StatusCode::UNPROCESSABLE_ENTITY, code, message)
        })?;
    Ok(Json(json!({ "answer": answer })))
}
