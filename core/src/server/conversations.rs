use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;

use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};

use crate::db::conversations::publish_catalog;

use super::{api_error, db_call, ApiResult, AppState};

const ICONS: [&str; 16] = [
    "message",
    "game",
    "headphones",
    "languages",
    "study",
    "users",
    "bookmark",
    "sparkles",
    "mic",
    "music",
    "video",
    "globe",
    "heart",
    "star",
    "coffee",
    "trophy",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmptyRequest {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateConversationRequest {
    #[serde(default, deserialize_with = "deserialize_present_option")]
    custom_title: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_present_option")]
    icon: Option<Option<String>>,
}

#[derive(Deserialize, Default)]
pub(super) struct SubtitlePageQuery {
    limit: Option<String>,
    before_id: Option<String>,
}

pub(super) async fn catalog(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let catalog = db_call(Arc::clone(&state.db), |db| db.conversation_catalog())
        .await
        .map_err(conversation_error("conversations.catalog_failed"))?;
    Ok(Json(json!(catalog)))
}

pub(super) async fn create(
    State(state): State<Arc<AppState>>,
    Json(_input): Json<EmptyRequest>,
) -> ApiResult<Json<Value>> {
    let conversation_catalog = state.conversation_catalog_tx.clone();
    let catalog = db_call(Arc::clone(&state.db), move |db| {
        let catalog = db.create_conversation()?;
        publish_catalog(&conversation_catalog, &catalog);
        Ok(catalog)
    })
    .await
    .map_err(conversation_error("conversations.create_failed"))?;
    Ok(Json(json!(catalog)))
}

pub(super) async fn update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<UpdateConversationRequest>,
) -> ApiResult<Json<Value>> {
    validate_path_id(&id)?;
    let custom_title = normalize_title_update(input.custom_title)?;
    let icon = validate_icon_update(input.icon)?;
    let conversation_catalog = state.conversation_catalog_tx.clone();
    let catalog = db_call(Arc::clone(&state.db), move |db| {
        let catalog = db.update_conversation(
            &id,
            patch_value_as_deref(&custom_title),
            patch_value_as_deref(&icon),
        )?;
        if let Some(catalog) = &catalog {
            publish_catalog(&conversation_catalog, catalog);
        }
        Ok(catalog)
    })
    .await
    .map_err(conversation_error("conversations.update_failed"))?
    .ok_or_else(conversation_not_found)?;
    Ok(Json(json!(catalog)))
}

pub(super) async fn delete_conversation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    validate_path_id(&id)?;
    let conversation_catalog = state.conversation_catalog_tx.clone();
    let catalog = db_call(Arc::clone(&state.db), move |db| {
        let catalog = db.delete_conversation(&id)?;
        if let Some(catalog) = &catalog {
            publish_catalog(&conversation_catalog, catalog);
        }
        Ok(catalog)
    })
    .await
    .map_err(conversation_error("conversations.delete_failed"))?
    .ok_or_else(conversation_not_found)?;
    Ok(Json(json!(catalog)))
}

pub(super) async fn subtitles(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<SubtitlePageQuery>,
) -> ApiResult<Json<Value>> {
    validate_path_id(&id)?;
    let limit = parse_limit(query.limit.as_deref())?;
    let before_id = parse_before_id(query.before_id.as_deref())?;
    let page = db_call(Arc::clone(&state.db), move |db| {
        db.conversation_subtitles(&id, limit, before_id)
    })
    .await
    .map_err(conversation_error("conversations.subtitles_failed"))?
    .ok_or_else(conversation_not_found)?;
    Ok(Json(json!(page)))
}

fn patch_value_as_deref(value: &Option<Option<String>>) -> Option<Option<&str>> {
    match value {
        None => None,
        Some(None) => Some(None),
        Some(Some(value)) => Some(Some(value.as_str())),
    }
}

fn deserialize_present_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

fn normalize_title_update(
    value: Option<Option<String>>,
) -> Result<Option<Option<String>>, (StatusCode, Json<Value>)> {
    value.map(normalize_title).transpose()
}

fn validate_icon_update(
    value: Option<Option<String>>,
) -> Result<Option<Option<String>>, (StatusCode, Json<Value>)> {
    value.map(validate_icon).transpose()
}

fn normalize_title(value: Option<String>) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    let Some(value) = value else {
        return Ok(None);
    };
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() > 40 {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "conversations.invalid_title",
            "Conversation titles may contain at most 40 characters",
        ));
    }
    Ok((!normalized.is_empty()).then_some(normalized))
}

fn validate_icon(value: Option<String>) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    if value.as_deref().is_some_and(|icon| !ICONS.contains(&icon)) {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "conversations.invalid_icon",
            "Conversation icon is not supported",
        ));
    }
    Ok(value)
}

fn valid_public_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn validate_path_id(value: &str) -> Result<(), (StatusCode, Json<Value>)> {
    if valid_public_id(value) {
        Ok(())
    } else {
        Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "conversations.invalid_id",
            "Conversation id must be an ASCII identifier of 1 to 64 characters",
        ))
    }
}

fn parse_limit(value: Option<&str>) -> Result<u32, (StatusCode, Json<Value>)> {
    match value {
        None => Ok(100),
        Some(value) => value
            .parse::<u32>()
            .ok()
            .filter(|limit| (1..=1_000).contains(limit))
            .ok_or_else(|| {
                api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "conversations.invalid_limit",
                    "limit must be between 1 and 1000",
                )
            }),
    }
}

fn parse_before_id(value: Option<&str>) -> Result<Option<i64>, (StatusCode, Json<Value>)> {
    value
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(|| {
                    api_error(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "conversations.invalid_before_id",
                        "before_id must be a positive integer",
                    )
                })
        })
        .transpose()
}

fn conversation_error(
    code: &'static str,
) -> impl FnOnce(crate::error::AppError) -> (StatusCode, Json<Value>) {
    move |error| api_error(StatusCode::INTERNAL_SERVER_ERROR, code, error.to_string())
}

fn conversation_not_found() -> (StatusCode, Json<Value>) {
    api_error(
        StatusCode::NOT_FOUND,
        "conversations.not_found",
        "Conversation was not found",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_validation_matches_the_public_contract() {
        assert!(valid_public_id("conversation-123_test"));
        assert!(valid_public_id("import"));
        assert!(!valid_public_id("conversation/123"));
        assert!(!valid_public_id(&"x".repeat(65)));
        assert_eq!(
            normalize_title(Some("  hello   world  ".into())).unwrap(),
            Some("hello world".into())
        );
        assert!(normalize_title(Some("界".repeat(41))).is_err());
        for icon in ICONS {
            assert_eq!(
                validate_icon(Some(icon.into())).unwrap().as_deref(),
                Some(icon)
            );
        }
        assert!(validate_icon(Some("unsupported".into())).is_err());
    }
}
