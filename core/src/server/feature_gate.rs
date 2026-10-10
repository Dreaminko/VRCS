use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use super::{api_error_with_params, ApiResult, AppState};
use crate::config::{AppConfig, FeatureKey};

pub(super) fn require_feature(config: &AppConfig, feature: FeatureKey) -> ApiResult<()> {
    if feature.enabled(&config.features) {
        return Ok(());
    }
    Err(api_error_with_params(
        StatusCode::CONFLICT,
        "feature.disabled",
        json!({"feature": feature.name()}),
        "This feature is disabled in system settings",
    ))
}

fn required_features(path: &str, method: &Method) -> &'static [FeatureKey] {
    use FeatureKey::*;
    if path.starts_with("/api/learning/") {
        return if path.ends_with("/export") {
            &[Learning, Anki]
        } else {
            &[Learning]
        };
    }
    if path == "/api/dictionary"
        || path == "/api/dictionaries"
        || path.starts_with("/api/dictionaries/")
    {
        return &[Learning];
    }
    if path.starts_with("/api/anki/") {
        return &[Anki];
    }
    if path.starts_with("/api/chatbox/") || path.starts_with("/api/osc/") {
        return &[OscChatbox];
    }
    if path.starts_with("/api/glossaries/") || path.starts_with("/api/translations/glossar") {
        return &[Glossary];
    }
    if path.starts_with("/api/ocr/")
        && path != "/api/ocr/runtime"
        && !(path == "/api/ocr/token" && method == Method::GET)
    {
        return &[Ocr];
    }
    if path.starts_with("/api/vrcx/")
        && path != "/api/vrcx/status"
        && !(path == "/api/vrcx/token" && method == Method::GET)
    {
        return &[Vrcx];
    }
    if path == "/api/external-api/token" && method != Method::GET {
        return &[ExternalApi];
    }
    &[]
}

pub(super) async fn enforce(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() == Method::OPTIONS {
        return next.run(request).await;
    }
    let required = required_features(request.uri().path(), request.method());
    let mut changes = state.config.features_tx.subscribe();
    {
        let config = state.config.config.read().expect("config lock");
        for &feature in required {
            if let Err(error) = require_feature(&config, feature) {
                return error.into_response();
            }
        }
    }
    // Drop cloud work on disable. Anki exports must finish recording an external receipt.
    let cancellable = request.uri().path() == "/api/learning/selection-query"
        || request.uri().path().ends_with("/analysis")
        || request.uri().path().ends_with("/refresh");
    if !cancellable {
        return next.run(request).await;
    }
    let disabled = async {
        loop {
            if let Some(&feature) = required
                .iter()
                .find(|feature| !feature.enabled(&changes.borrow()))
            {
                return api_error_with_params(
                    StatusCode::CONFLICT,
                    "feature.disabled",
                    json!({"feature": feature.name()}),
                    "This feature was disabled while the request was running",
                )
                .into_response();
            }
            if changes.changed().await.is_err() {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            }
        }
    };
    tokio::select! { biased; response = disabled => response, response = next.run(request) => response }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{middleware, routing::post, Router};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn feature_switches_drop_in_flight_learning_work() {
        let directory = tempfile::tempdir().unwrap();
        let handle = crate::start(crate::CoreOptions {
            config_path: directory.path().join("config.json"),
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: Some("cancel-test".into()),
            vad_model_path: Some(directory.path().join("missing.onnx")),
            asr_model_dir: None,
        })
        .await
        .unwrap();
        let started = Arc::new(tokio::sync::Notify::new());
        let dropped = Arc::new(AtomicBool::new(false));
        let handler_started = started.clone();
        let handler_dropped = dropped.clone();
        let state = handle.state.clone();
        let router = Router::new()
            .route(
                "/api/learning/selection-query",
                post(move || {
                    let started = handler_started.clone();
                    let dropped = handler_dropped.clone();
                    async move {
                        let _guard = Dropped(dropped);
                        started.notify_one();
                        std::future::pending::<()>().await;
                        StatusCode::OK
                    }
                }),
            )
            .layer(middleware::from_fn_with_state(state, enforce));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let request = tokio::spawn(async move {
            reqwest::Client::new()
                .post(format!("http://{address}/api/learning/selection-query"))
                .send()
                .await
                .unwrap()
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        handle
            .state
            .config
            .config
            .write()
            .unwrap()
            .features
            .learning = false;
        handle
            .state
            .config
            .features_tx
            .send_modify(|features| features.learning = false);
        let response = tokio::time::timeout(std::time::Duration::from_secs(5), request)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["code"],
            "feature.disabled"
        );
        assert!(dropped.load(Ordering::SeqCst));
        server.abort();
        handle.shutdown().await.unwrap();
    }
}
