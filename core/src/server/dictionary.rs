use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value};

use super::{
    api_error, api_error_with_params, db_call, dictionary_import_error, ApiResult, AppState,
};

const IMPORT_ID_HEADER: &str = "x-vrcs-import-id";
const IMPORT_PROGRESS_SCALE: u32 = 10_000;
static DICTIONARY_IMPORTS: LazyLock<Mutex<HashMap<String, Arc<AtomicU32>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn valid_import_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn register_import(import_id: &str) -> Arc<AtomicU32> {
    let progress = Arc::new(AtomicU32::new(0));
    let mut imports = DICTIONARY_IMPORTS.lock().expect("dictionary imports lock");
    if imports.len() >= 32 {
        imports.retain(|_, value| value.load(Ordering::Relaxed) < IMPORT_PROGRESS_SCALE);
    }
    imports.insert(import_id.to_owned(), Arc::clone(&progress));
    progress
}

fn remove_import(import_id: &str) {
    DICTIONARY_IMPORTS
        .lock()
        .expect("dictionary imports lock")
        .remove(import_id);
}

pub(super) async fn subtitle_history(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let limit = match params.get("limit") {
        None => 100,
        Some(raw) => raw
            .parse::<u32>()
            .ok()
            .filter(|value| (1..=10_000).contains(value))
            .ok_or_else(|| {
                api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "subtitles.invalid_limit",
                    "limit must be between 1 and 10000",
                )
            })?,
    };
    let before_id = match params.get("before_id") {
        None => None,
        Some(raw) => Some(
            raw.parse::<i64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    api_error(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "subtitles.invalid_before_id",
                        "before_id must be a positive integer",
                    )
                })?,
        ),
    };
    let subtitles = db_call(Arc::clone(&state.db), move |db| match before_id {
        Some(before_id) => db.subtitle_history_before(limit, Some(before_id)),
        None => db.subtitle_history(limit),
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "subtitles.history_failed",
            error.to_string(),
        )
    })?;
    Ok(Json(json!(subtitles)))
}

pub(super) async fn dictionary_lookup(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let query = params
        .get("q")
        .filter(|value| !value.is_empty() && value.chars().count() <= 100)
        .ok_or_else(|| {
            api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "dictionary.invalid_query",
                "q must contain 1 to 100 characters",
            )
        })?;
    let query = query.clone();
    let entries = db_call(Arc::clone(&state.db), move |db| db.lookup(&query, 10))
        .await
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "dictionary.lookup_failed",
                error.to_string(),
            )
        })?;
    Ok(Json(json!(entries)))
}

pub(super) async fn dictionary_list(State(state): State<Arc<AppState>>) -> ApiResult<Json<Value>> {
    let sources = db_call(Arc::clone(&state.db), |db| db.dictionary_sources())
        .await
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "dictionary.list_failed",
                error.to_string(),
            )
        })?;
    Ok(Json(json!(sources)))
}

pub(super) async fn dictionary_import(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    let import_id = headers
        .get(IMPORT_ID_HEADER)
        .map(|value| value.to_str().unwrap_or(""))
        .map(str::to_owned);
    if import_id
        .as_deref()
        .is_some_and(|value| !valid_import_id(value))
    {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "dictionary.import_id_invalid",
            "Dictionary import identifier is invalid",
        ));
    }
    let progress = import_id.as_deref().map(register_import);
    let worker_progress = progress.clone();
    let result = db_call(Arc::clone(&state.db), move |db| {
        db.import_yomitan_with_progress(&body, |value| {
            if let Some(progress) = &worker_progress {
                let scaled = (value.clamp(0.0, 1.0) * IMPORT_PROGRESS_SCALE as f64).round() as u32;
                progress.store(scaled, Ordering::Relaxed);
            }
        })
    })
    .await;
    match result {
        Ok(imported) => {
            if let Some(progress) = progress {
                progress.store(IMPORT_PROGRESS_SCALE, Ordering::Relaxed);
            }
            Ok(Json(json!(imported)))
        }
        Err(error) => {
            if let Some(import_id) = import_id {
                remove_import(&import_id);
            }
            Err(dictionary_import_error(error))
        }
    }
}

pub(super) async fn dictionary_import_progress(
    axum::extract::Path(import_id): axum::extract::Path<String>,
) -> ApiResult<Json<Value>> {
    let progress = DICTIONARY_IMPORTS
        .lock()
        .expect("dictionary imports lock")
        .get(&import_id)
        .map(|value| value.load(Ordering::Relaxed))
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "dictionary.import_progress_not_found",
                "Dictionary import task does not exist",
            )
        })?;
    Ok(Json(json!({
        "progress": progress as f64 / IMPORT_PROGRESS_SCALE as f64,
    })))
}

pub(super) async fn dictionary_delete(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(source_id): axum::extract::Path<i64>,
) -> ApiResult<Json<Value>> {
    let deleted = db_call(Arc::clone(&state.db), move |db| {
        db.delete_dictionary_source(source_id)
    })
    .await
    .map_err(|error| {
        api_error_with_params(
            StatusCode::INTERNAL_SERVER_ERROR,
            "dictionary.delete_failed",
            json!({ "source_id": source_id }),
            error.to_string(),
        )
    })?;
    if !deleted {
        return Err(api_error_with_params(
            StatusCode::NOT_FOUND,
            "dictionary.not_found",
            json!({ "source_id": source_id }),
            "Dictionary does not exist",
        ));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[cfg(test)]
mod tests {
    use super::valid_import_id;

    #[test]
    fn import_ids_are_bounded_and_header_safe() {
        assert!(valid_import_id("b313b783-0df5-421f-8a19-2274913a124f"));
        assert!(!valid_import_id(""));
        assert!(!valid_import_id("contains spaces"));
        assert!(!valid_import_id(&"x".repeat(65)));
    }
}
