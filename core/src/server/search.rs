use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{api_error, db_call, ApiResult, ContentState};

const MAX_QUERY_CHARS: usize = 200;
const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 100;
const MAX_OFFSET: u32 = 10_000;

#[derive(Debug, Deserialize)]
pub(super) struct SubtitleSearchQuery {
    q: String,
    limit: Option<String>,
    offset: Option<String>,
}

pub(super) async fn subtitles(
    State(state): State<ContentState>,
    Query(input): Query<SubtitleSearchQuery>,
) -> ApiResult<Json<Value>> {
    let query = input.q.trim().to_string();
    let length = query.chars().count();
    if length == 0 || length > MAX_QUERY_CHARS {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "subtitles.search.invalid_query",
            format!("Search query must contain 1 to {MAX_QUERY_CHARS} characters"),
        ));
    }
    let limit = parse_bounded(input.limit.as_deref(), DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let offset = parse_bounded(input.offset.as_deref(), 0, MAX_OFFSET, "offset")?;
    let page = db_call(Arc::clone(&state.db), move |db| {
        db.search_subtitles(&query, limit, offset)
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "subtitles.search.failed",
            error.to_string(),
        )
    })?;
    Ok(Json(json!(page)))
}

fn parse_bounded(
    value: Option<&str>,
    default: u32,
    max: u32,
    name: &str,
) -> Result<u32, (StatusCode, Json<Value>)> {
    match value {
        None => Ok(default),
        Some(value) => value
            .parse::<u32>()
            .ok()
            .filter(|parsed| *parsed <= max && (name != "limit" || *parsed > 0))
            .ok_or_else(|| {
                api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "subtitles.search.invalid_pagination",
                    format!("{name} is outside the supported range"),
                )
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_validation_enforces_public_limits() {
        assert_eq!(
            parse_bounded(None, DEFAULT_LIMIT, MAX_LIMIT, "limit").unwrap(),
            50
        );
        assert_eq!(
            parse_bounded(Some("100"), DEFAULT_LIMIT, MAX_LIMIT, "limit").unwrap(),
            100
        );
        assert!(parse_bounded(Some("0"), DEFAULT_LIMIT, MAX_LIMIT, "limit").is_err());
        assert!(parse_bounded(Some("101"), DEFAULT_LIMIT, MAX_LIMIT, "limit").is_err());
        assert_eq!(
            parse_bounded(Some("0"), 0, MAX_OFFSET, "offset").unwrap(),
            0
        );
    }
}
