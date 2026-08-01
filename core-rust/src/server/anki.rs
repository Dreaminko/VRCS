use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::anki as anki_service;
use crate::models::CardRequest;

use super::{api_error, api_error_with_params, ApiResult, AppState};

pub(super) async fn anki_status(State(state): State<Arc<AppState>>) -> Json<Value> {
    let config = state.config.read().expect("config lock").anki.clone();
    Json(anki_service::status(&state.http, &config).await)
}

pub(super) async fn anki_add_card(
    State(state): State<Arc<AppState>>,
    Json(card): Json<CardRequest>,
) -> ApiResult<Json<Value>> {
    card.validate()
        .map_err(|error| api_error(StatusCode::UNPROCESSABLE_ENTITY, "anki.card_invalid", error))?;
    let config = state.config.read().expect("config lock").anki.clone();
    let note_id = anki_service::create_card(&state.http, &card, &config)
        .await
        .map_err(|e| {
            api_error_with_params(
                StatusCode::from_u16(e.status_code).unwrap_or(StatusCode::BAD_GATEWAY),
                format!("anki.{}", e.code),
                e.params,
                e.message,
            )
        })?;
    Ok(Json(json!({ "note_id": note_id })))
}
