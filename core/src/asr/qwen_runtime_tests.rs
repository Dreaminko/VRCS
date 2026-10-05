use std::path::Path;

use super::qwen_runtime::{launch_args, probe_ready, QwenRuntime};

#[test]
fn managed_qwen_command_binds_loopback_and_uses_both_assets() {
    let arguments = launch_args(
        Path::new("C:/models/main.gguf"),
        Path::new("C:/models/mmproj.gguf"),
        43210,
        "vrcs-qwen",
        "session-token",
        "cpu",
    )
    .iter()
    .map(|argument| argument.to_string_lossy().into_owned())
    .collect::<Vec<_>>();
    assert!(arguments
        .windows(2)
        .any(|pair| pair == ["--model", "C:/models/main.gguf"]));
    assert!(arguments
        .windows(2)
        .any(|pair| pair == ["--mmproj", "C:/models/mmproj.gguf"]));
    assert!(arguments
        .windows(2)
        .any(|pair| pair == ["--host", "127.0.0.1"]));
    assert!(arguments.windows(2).any(|pair| pair == ["--port", "43210"]));
    assert!(arguments
        .windows(2)
        .any(|pair| pair == ["--api-key", "session-token"]));
    assert!(arguments.contains(&"--no-webui".into()));
    assert!(arguments
        .windows(2)
        .any(|pair| pair == ["--gpu-layers", "0"]));
}

#[tokio::test]
async fn managed_qwen_readiness_requires_the_expected_model_alias() {
    use axum::{routing::get, Json, Router};
    use serde_json::json;

    let app = Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route(
            "/v1/models",
            get(|| async { Json(json!({ "data": [{ "id": "other" }] })) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();

    let ready = probe_ready(&client, &format!("http://{address}"), "token", "vrcs-qwen")
        .await
        .unwrap();
    assert!(!ready);
    server.abort();
}

#[tokio::test]
async fn managed_qwen_readiness_checks_the_session_token() {
    use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
    use axum::{routing::get, Json, Router};
    use serde_json::json;

    let app = Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route(
            "/v1/models",
            get(|headers: HeaderMap| async move {
                if headers
                    .get(AUTHORIZATION)
                    .is_none_or(|value| value != "Bearer secret")
                {
                    return (StatusCode::UNAUTHORIZED, Json(json!({ "data": [] })));
                }
                (
                    StatusCode::OK,
                    Json(json!({ "data": [{ "id": "vrcs-qwen" }] })),
                )
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let origin = format!("http://{address}");

    assert!(probe_ready(&client, &origin, "secret", "vrcs-qwen")
        .await
        .unwrap());
    assert!(probe_ready(&client, &origin, "wrong", "vrcs-qwen")
        .await
        .is_err());
    server.abort();
}

#[tokio::test]
async fn managed_qwen_rejects_missing_assets_before_starting_a_process() {
    let runtime = QwenRuntime::new();
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing.gguf");
    assert!(runtime
        .ensure_started(
            &missing,
            &missing,
            &missing,
            "vrcs-qwen",
            "auto",
            std::time::Duration::from_secs(1),
        )
        .await
        .is_err());
    assert!(runtime.connection().await.unwrap().is_none());
}
