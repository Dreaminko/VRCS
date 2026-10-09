use std::path::Path;

use super::{launch_args, probe_ready, QwenRuntime};

#[tokio::test]
async fn managed_qwen_gpu_discovery_respects_the_vulkan_feature() {
    let root = tempfile::tempdir().unwrap();
    let runtime = QwenRuntime::new(root.path().to_path_buf());
    std::fs::create_dir_all(&runtime.directory).unwrap();
    for name in super::super::qwen_runtime_download::REQUIRED_FILES {
        std::fs::write(runtime.directory.join(name), b"fixture").unwrap();
    }
    std::fs::write(
        runtime.directory.join("version.txt"),
        super::super::qwen_runtime_download::SHA256,
    )
    .unwrap();
    runtime
        .devices
        .set(vec!["Vulkan0: test GPU".into()])
        .unwrap();
    assert_eq!(
        runtime.devices().await,
        if cfg!(feature = "vulkan") {
            vec!["Vulkan0: test GPU".to_owned()]
        } else {
            Vec::new()
        }
    );
}

#[cfg(not(feature = "vulkan"))]
#[tokio::test]
async fn managed_qwen_explicit_gpu_is_rejected_without_vulkan() {
    let runtime = QwenRuntime::new(std::path::PathBuf::new());
    runtime
        .devices
        .set(vec!["Vulkan0: test GPU".into()])
        .unwrap();
    let selection = super::Selection {
        executable: "unused.exe".into(),
        model: "unused.gguf".into(),
        mmproj: "unused-mmproj.gguf".into(),
        alias: "vrcs-qwen".into(),
        device: "gpu".into(),
    };
    let error = runtime
        .start_selection(selection, 0, std::time::Duration::from_secs(1))
        .await
        .unwrap_err();
    assert!(
        error.contains("does not include the Vulkan backend"),
        "{error}"
    );
    assert!(runtime.state.lock().await.child.is_none());
    assert_eq!(runtime.snapshot().await.status, "error");
}

#[test]
fn managed_qwen_gpu_arguments_respect_the_vulkan_feature() {
    let arguments = launch_args(
        Path::new("main.gguf"),
        Path::new("mmproj.gguf"),
        43210,
        "vrcs-qwen",
        "session-token",
        "gpu",
    )
    .iter()
    .map(|argument| argument.to_string_lossy().into_owned())
    .collect::<Vec<_>>();
    let layers = if cfg!(feature = "vulkan") { "all" } else { "0" };
    assert!(arguments
        .windows(2)
        .any(|pair| pair == ["--gpu-layers", layers]));
    assert_eq!(
        arguments.contains(&"--no-mmproj-offload".into()),
        !cfg!(feature = "vulkan")
    );
}

#[tokio::test]
async fn loading_status_and_cancellation_do_not_wait_for_the_startup_lock() {
    let runtime = QwenRuntime::new(std::path::PathBuf::new());
    let _startup = runtime.startup.lock().await;
    runtime.state.lock().await.status = "loading";
    let snapshot = tokio::time::timeout(std::time::Duration::from_millis(100), runtime.snapshot())
        .await
        .unwrap();
    assert_eq!(snapshot.status, "loading");
    tokio::time::timeout(
        std::time::Duration::from_millis(100),
        runtime.cancel_loading(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(runtime.snapshot().await.status, "not_loaded");
}

#[tokio::test]
async fn explicit_stop_prevents_reconnecting_an_old_session() {
    let runtime = QwenRuntime::new(std::path::PathBuf::new());
    let previous = super::QwenConnection {
        base_url: String::new(),
        token: String::new(),
        model: "vrcs-qwen".into(),
        generation: 0,
        session: 0,
    };
    runtime.stop().await.unwrap();
    assert!(runtime
        .reconnect(&previous)
        .await
        .unwrap_err()
        .contains("stopped or replaced"));
}

#[tokio::test]
#[ignore = "requires staged llama.cpp runtime, verified Qwen model package, and models/qwen-asr/jfk.wav"]
async fn real_managed_qwen_transcribes_both_channels_and_recovers_after_exit() {
    use crate::asr::segmented_upload::SegmentedUploadSession;
    use crate::asr::{CloudEvent, ModelManager};
    use std::sync::Arc;
    use std::time::Duration;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("models");
    let manager = Arc::new(ModelManager::new(root.clone()).unwrap());
    let runtime = Arc::new(QwenRuntime::new(std::path::PathBuf::new()));
    let mut reader = hound::WavReader::open(root.join("qwen-asr/jfk.wav")).unwrap();
    assert_eq!(reader.spec().sample_rate, 16_000);
    let samples = Arc::new(
        reader
            .samples::<i16>()
            .map(|v| f32::from(v.unwrap()) / 32768.0)
            .collect::<Vec<_>>(),
    );
    let mut devices = vec!["cpu"];
    if !runtime.devices().await.is_empty() {
        devices.push("gpu");
    }
    let spec = crate::asr::qwen_models::package_spec("qwen3-asr-0.6b-q8_0").unwrap();
    let package = crate::asr::qwen_models::package_dir(&root, spec);
    let starting = runtime.clone();
    let assets = package.clone();
    let startup = tokio::spawn(async move {
        starting
            .ensure_started(
                &super::executable_path().unwrap(),
                &assets.join(spec.files[0].name),
                &assets.join(spec.files[1].name),
                spec.id,
                "cpu",
                Duration::from_secs(120),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while runtime.state.lock().await.child.is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let snapshot = tokio::time::timeout(Duration::from_millis(100), runtime.snapshot())
        .await
        .unwrap();
    assert_eq!(snapshot.status, "loading");
    runtime.cancel_loading().await.unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(3), startup)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert_eq!(runtime.snapshot().await.status, "not_loaded");
    for device in devices {
        let mut config = crate::config::AsrConfig::default();
        config.backend = crate::config::QWEN_MANAGED_BACKEND.into();
        config.managed_qwen.device = device.into();
        let connection = crate::asr::prepare_managed_qwen(&config, &runtime, &manager)
            .await
            .unwrap();
        let shared = crate::asr::prepare_managed_qwen(&config, &runtime, &manager)
            .await
            .unwrap();
        assert_eq!(connection.generation, shared.generation);
        assert_eq!(runtime.snapshot().await.device, Some(device));
        let speaker = SegmentedUploadSession::spawn_managed_qwen(
            connection.clone(),
            runtime.clone(),
            "auto".into(),
        );
        let microphone =
            SegmentedUploadSession::spawn_managed_qwen(shared, runtime.clone(), "auto".into());
        speaker.send(samples.clone()).await.unwrap();
        microphone.send(samples.clone()).await.unwrap();
        speaker.commit().await.unwrap();
        microphone.commit().await.unwrap();
        let (speaker_events, microphone_events) =
            tokio::join!(speaker.stop_and_drain(), microphone.stop_and_drain());
        for events in [speaker_events, microphone_events] {
            assert_eq!(events.len(), 1);
            let CloudEvent::Final { text, language, .. } = &events[0] else {
                panic!("{events:?}")
            };
            eprintln!("{device}: {text} ({language:?})");
            assert!(text.to_ascii_lowercase().contains("country"));
            assert_eq!(language.as_deref(), Some("en"));
        }
        runtime
            .state
            .lock()
            .await
            .child
            .as_mut()
            .unwrap()
            .kill()
            .await
            .unwrap();
        assert_eq!(runtime.snapshot().await.status, "error");
        let recovered = runtime.reconnect(&connection).await.unwrap();
        assert_ne!(recovered.generation, connection.generation);
        assert_eq!(recovered.session, connection.session);
        let result = crate::asr::openai_audio_transcriptions::transcribe_managed_qwen(
            &recovered, "en", &samples,
        )
        .await
        .unwrap();
        assert!(result.text.to_ascii_lowercase().contains("country"));
        runtime.stop().await.unwrap();
        assert!(runtime.connection().await.unwrap().is_none());
        assert_eq!(runtime.snapshot().await.status, "not_loaded");
        assert!(
            tokio::time::timeout(Duration::from_secs(1), runtime.reconnect(&recovered))
                .await
                .unwrap()
                .is_err()
        );
    }
    let corrupt = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(corrupt.path(), b"invalid model").unwrap();
    let error = runtime
        .ensure_started(
            &super::executable_path().unwrap(),
            corrupt.path(),
            &package.join(spec.files[1].name),
            spec.id,
            "cpu",
            Duration::from_secs(10),
        )
        .await
        .unwrap_err();
    assert!(error.contains("runtime exited"), "{error}");
    assert!(
        error.lines().count() > 1,
        "runtime diagnostics were discarded: {error}"
    );
    let snapshot = runtime.snapshot().await;
    assert_eq!(snapshot.status, "error");
    assert_eq!(snapshot.error.as_deref(), Some(error.as_str()));
    assert!(runtime.connection().await.unwrap().is_none());
    runtime.stop().await.unwrap();
}

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
    assert!(arguments
        .windows(2)
        .any(|pair| pair == ["--device", "none"]));
    assert!(arguments.contains(&"--no-mmproj-offload".into()));
    assert!(arguments.contains(&"--no-op-offload".into()));
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
    let runtime = QwenRuntime::new(std::path::PathBuf::new());
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
