use super::*;

#[tokio::test]
async fn embedded_core_starts_and_stops() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: None,
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    assert!(handle.address().port() > 0);
    assert!(handle.external_api_address().is_none());
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn vr_overlay_watch_starts_with_current_config_and_updates_after_commit() {
    let directory = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(port),
        session_token: Some("vr-overlay-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let mut updates = handle.subscribe_vr_overlay_config();
    assert_eq!(*updates.borrow(), VrOverlayConfig::default());

    let client = reqwest::Client::new();
    let settings_url = format!("http://{}/api/settings", handle.address());
    let mut settings: serde_json::Value = client
        .get(&settings_url)
        .bearer_auth("vr-overlay-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    settings["vr_overlay"]["wrist"]["max_entries"] = serde_json::json!(2);
    let rejected = client
        .put(&settings_url)
        .bearer_auth("vr-overlay-token")
        .json(&settings)
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), updates.changed())
            .await
            .is_err()
    );

    settings["vr_overlay"]["enabled"] = serde_json::json!(true);
    settings["vr_overlay"]["headset"]["content_mode"] = serde_json::json!("translation");
    settings["vr_overlay"]["wrist"]["max_entries"] = serde_json::json!(5);
    let response = client
        .put(&settings_url)
        .bearer_auth("vr-overlay-token")
        .json(&settings)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(status.is_success(), "{status}: {body}");

    tokio::time::timeout(std::time::Duration::from_secs(1), updates.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(updates.borrow().enabled);
    assert_eq!(updates.borrow().headset.content_mode, "translation");
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn storage_stats_quota_update_and_history_clear_are_available_over_http() {
    let directory = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(port),
        session_token: Some("storage-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let client = reqwest::Client::new();
    let base_url = format!("http://{}", handle.address());
    let mut settings: serde_json::Value = client
        .get(format!("{base_url}/api/settings"))
        .bearer_auth("storage-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    settings["storage"]["subtitle_history_max_bytes"] = serde_json::json!(128_u64 * 1024 * 1024);
    let saved = client
        .put(format!("{base_url}/api/settings"))
        .bearer_auth("storage-token")
        .json(&settings)
        .send()
        .await
        .unwrap();
    let saved_status = saved.status();
    let saved_body = saved.text().await.unwrap();
    assert!(saved_status.is_success(), "{saved_status}: {saved_body}");

    let stats: serde_json::Value = client
        .get(format!("{base_url}/api/storage/stats"))
        .bearer_auth("storage-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(stats["max_bytes"], serde_json::json!(128_u64 * 1024 * 1024));
    assert!(stats["used_bytes"].as_u64().unwrap() > 0);

    handle
        .state
        .content
        .db
        .lock()
        .unwrap()
        .add_subtitle(&crate::models::Subtitle {
            id: None,
            conversation_id: None,
            text: "temporary history".into(),
            language: Some("en".into()),
            started_at: None,
            ended_at: None,
            source: "speaker".into(),
            created_at: crate::models::now_iso8601(),
            translations: Vec::new(),
        })
        .unwrap();
    let cleared = client
        .delete(format!("{base_url}/api/subtitles"))
        .bearer_auth("storage-token")
        .send()
        .await
        .unwrap();
    assert!(cleared.status().is_success());
    assert!(handle
        .state
        .content
        .db
        .lock()
        .unwrap()
        .subtitle_history(10)
        .unwrap()
        .is_empty());

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn subtitle_range_deletion_validates_and_deletes_only_the_requested_messages() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: Some("subtitle-delete-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let client = reqwest::Client::new();
    let endpoint = format!("http://{}/api/subtitles/range", handle.address());

    {
        let db = handle.state.content.db.lock().unwrap();
        for (text, created_at) in [
            ("older", "2026-01-01T00:00:00.000000Z"),
            ("target", "2026-01-02T00:00:00.000000Z"),
            ("newer", "2026-01-03T00:00:00.000000Z"),
        ] {
            db.add_subtitle(&crate::models::Subtitle {
                id: None,
                conversation_id: None,
                text: text.into(),
                language: Some("en".into()),
                started_at: None,
                ended_at: None,
                source: "speaker".into(),
                created_at: created_at.into(),
                translations: Vec::new(),
            })
            .unwrap();
        }
    }

    let deleted = client
        .delete(&endpoint)
        .bearer_auth("subtitle-delete-token")
        .json(&serde_json::json!({
            "started_at": "2026-01-02T08:00:00+08:00",
            "ended_at": "2026-01-03T08:00:00+08:00"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), reqwest::StatusCode::OK);
    assert_eq!(
        deleted.json::<serde_json::Value>().await.unwrap()["deleted"],
        1
    );
    let remaining = handle
        .state
        .content
        .db
        .lock()
        .unwrap()
        .subtitle_history(10)
        .unwrap()
        .into_iter()
        .map(|subtitle| subtitle.text)
        .collect::<Vec<_>>();
    assert_eq!(remaining, vec!["newer", "older"]);

    for input in [
        serde_json::json!({
            "started_at": "not-a-timestamp",
            "ended_at": null
        }),
        serde_json::json!({
            "started_at": "2026-01-03T00:00:00Z",
            "ended_at": "2026-01-02T00:00:00Z"
        }),
    ] {
        let rejected = client
            .delete(&endpoint)
            .bearer_auth("subtitle-delete-token")
            .json(&input)
            .send()
            .await
            .unwrap();
        assert_eq!(rejected.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            rejected.json::<serde_json::Value>().await.unwrap()["code"],
            "subtitles.invalid_range"
        );
    }

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn conversation_http_catalog_and_pagination_are_stable() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: Some("conversation-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    {
        let database = handle.state.content.db.lock().unwrap();
        for (text, created_at) in [
            ("before", "2026-01-01T00:00:00.000000Z"),
            ("middle", "2026-01-01T01:30:00.000000Z"),
            ("latest", "2026-01-01T03:00:00.000000Z"),
        ] {
            database
                .add_subtitle(&crate::models::Subtitle {
                    id: None,
                    conversation_id: None,
                    text: text.into(),
                    language: Some("en".into()),
                    started_at: None,
                    ended_at: None,
                    source: "speaker".into(),
                    created_at: created_at.into(),
                    translations: Vec::new(),
                })
                .unwrap();
        }
    }

    let client = reqwest::Client::new();
    let base_url = format!("http://{}", handle.address());
    let catalog: serde_json::Value = client
        .get(format!("{base_url}/api/conversations"))
        .bearer_auth("conversation-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(catalog.get("legacy_imported").is_none());
    assert_eq!(catalog["conversations"].as_array().unwrap().len(), 1);
    let active = &catalog["conversations"][0];
    assert_eq!(active["subtitle_count"], 3);
    assert_eq!(active["automatic_title"], "before");
    let conversation_id = active["id"].as_str().unwrap();

    let page: serde_json::Value = client
        .get(format!(
            "{base_url}/api/conversations/{conversation_id}/subtitles?limit=2"
        ))
        .bearer_auth("conversation-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    assert_eq!(page["items"][0]["text"], "latest");
    assert_eq!(page["items"][1]["text"], "middle");
    assert_eq!(page["items"][0]["conversation_id"], conversation_id);
    assert_eq!(page["has_more"], true);
    let before_id = page["next_before_id"].as_i64().unwrap();

    let older_page: serde_json::Value = client
        .get(format!(
            "{base_url}/api/conversations/{conversation_id}/subtitles?limit=2&before_id={before_id}"
        ))
        .bearer_auth("conversation-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(older_page["items"].as_array().unwrap().len(), 1);
    assert_eq!(older_page["items"][0]["text"], "before");
    assert_eq!(older_page["has_more"], false);
    assert!(older_page["next_before_id"].is_null());

    let patched: serde_json::Value = client
        .patch(format!("{base_url}/api/conversations/{conversation_id}"))
        .bearer_auth("conversation-token")
        .json(&serde_json::json!({
            "custom_title": "Renamed",
            "icon": "music"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let active = &patched["conversations"][0];
    assert_eq!(active["custom_title"], "Renamed");
    assert_eq!(active["icon"], "music");

    let patched_without_icon: serde_json::Value = client
        .patch(format!("{base_url}/api/conversations/{conversation_id}"))
        .bearer_auth("conversation-token")
        .json(&serde_json::json!({ "custom_title": "Renamed again" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let active = &patched_without_icon["conversations"][0];
    assert_eq!(active["custom_title"], "Renamed again");
    assert_eq!(active["icon"], "music");

    let cleared_icon: serde_json::Value = client
        .patch(format!("{base_url}/api/conversations/{conversation_id}"))
        .bearer_auth("conversation-token")
        .json(&serde_json::json!({ "icon": null }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let active = &cleared_icon["conversations"][0];
    assert_eq!(active["custom_title"], "Renamed again");
    assert!(active["icon"].is_null());

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn external_api_uses_a_separate_listener_and_route_surface() {
    let directory = tempfile::tempdir().unwrap();
    let config_path: PathBuf = directory.path().join("config.json");
    let external_port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut config = config::AppConfig::default();
    config.external_api.enabled = true;
    config.external_api.port = external_port;
    config::save_config(&config_path, &config).unwrap();
    let handle = start(CoreOptions {
        config_path,
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: Some("internal-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let external = handle.external_api_address().unwrap();
    assert_ne!(external, handle.address());

    let client = reqwest::Client::new();
    let health: serde_json::Value = client
        .get(format!("http://{external}/v1/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["api_version"], "1.0");
    assert_eq!(
        client
            .get(format!("http://{external}/api/settings"))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::NOT_FOUND
    );
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn external_api_settings_reload_without_restarting_core() {
    let directory = tempfile::tempdir().unwrap();
    let core_port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let first_external_port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let second_external_port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let occupied = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let occupied_port = occupied.local_addr().unwrap().port();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(core_port),
        session_token: Some("internal-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let client = reqwest::Client::new();
    let base_url = format!("http://{}", handle.address());
    let mut settings: serde_json::Value = client
        .get(format!("{base_url}/api/settings"))
        .bearer_auth("internal-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    settings["external_api"]["enabled"] = serde_json::json!(true);
    settings["external_api"]["port"] = serde_json::json!(first_external_port);
    let response = client
        .put(format!("{base_url}/api/settings"))
        .bearer_auth("internal-token")
        .json(&settings)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    settings = response.json().await.unwrap();
    assert_eq!(
        client
            .get(format!("http://127.0.0.1:{first_external_port}/v1/health"))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::OK
    );

    settings["external_api"]["port"] = serde_json::json!(second_external_port);
    let response = client
        .put(format!("{base_url}/api/settings"))
        .bearer_auth("internal-token")
        .json(&settings)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    settings = response.json().await.unwrap();
    assert_eq!(
        client
            .get(format!("http://127.0.0.1:{second_external_port}/v1/health"))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::OK
    );

    settings["external_api"]["port"] = serde_json::json!(occupied_port);
    let response = client
        .put(format!("{base_url}/api/settings"))
        .bearer_auth("internal-token")
        .json(&settings)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(
        client
            .get(format!("http://127.0.0.1:{second_external_port}/v1/health"))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::OK
    );

    settings["external_api"]["port"] = serde_json::json!(second_external_port);
    settings["external_api"]["enabled"] = serde_json::json!(false);
    let response = client
        .put(format!("{base_url}/api/settings"))
        .bearer_auth("internal-token")
        .json(&settings)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let status: serde_json::Value = client
        .get(format!("{base_url}/api/external-api/status"))
        .bearer_auth("internal-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["state"], "disabled");

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn external_api_startup_failure_does_not_stop_the_core() {
    let directory = tempfile::tempdir().unwrap();
    let config_path: PathBuf = directory.path().join("config.json");
    let occupied = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let mut config = config::AppConfig::default();
    config.external_api.enabled = true;
    config.external_api.port = occupied.local_addr().unwrap().port();
    config::save_config(&config_path, &config).unwrap();

    let handle = start(CoreOptions {
        config_path,
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: Some("internal-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();

    assert!(handle.external_api_address().is_none());
    let status: serde_json::Value = reqwest::Client::new()
        .get(format!(
            "http://{}/api/external-api/status",
            handle.address()
        ))
        .bearer_auth("internal-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["state"], "failed");
    assert!(status["error"]
        .as_str()
        .unwrap()
        .contains("Failed to listen for External API"));

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn chatbox_send_is_stored_and_broadcast_as_conversation_message() {
    let directory = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(port),
        session_token: Some("test-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let client = reqwest::Client::new();
    let base_url = format!("http://{}", handle.address());
    let udp = tokio::net::UdpSocket::bind(("127.0.0.1", 0)).await.unwrap();
    let mut settings: serde_json::Value = client
        .get(format!("{base_url}/api/settings"))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    settings["osc"]["enabled"] = serde_json::json!(true);
    settings["osc"]["mute_sync_enabled"] = serde_json::json!(true);
    settings["osc"]["port"] = serde_json::json!(udp.local_addr().unwrap().port());
    let updated = client
        .put(format!("{base_url}/api/settings"))
        .bearer_auth("test-token")
        .json(&settings)
        .send()
        .await
        .unwrap();
    let updated_status = updated.status();
    let updated_body = updated.text().await.unwrap();
    assert_eq!(
        updated_status,
        reqwest::StatusCode::OK,
        "settings update failed: {updated_body}"
    );
    handle.state.integrations.osc.update_mute_status(Some(true));

    let mut subtitles = handle.state.content.subtitle_output.subscribe_subtitles();
    let sent = client
        .post(format!("{base_url}/api/chatbox/messages"))
        .bearer_auth("test-token")
        .json(&serde_json::json!({
            "original": "hello",
            "translation": "こんにちは",
            "source_language": "en",
            "target_language": "ja",
            "send_mode": "bilingual",
            "message_format": "original_newline_translation",
            "custom_format": null,
            "overflow_policy": "smart_truncate"
        }))
        .send()
        .await
        .unwrap();
    let sent_status = sent.status();
    let sent_body = sent.text().await.unwrap();
    assert_eq!(
        sent_status,
        reqwest::StatusCode::OK,
        "Chatbox send failed: {sent_body}"
    );

    let streamed = tokio::time::timeout(std::time::Duration::from_secs(1), subtitles.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(streamed.source, "chatbox");
    assert_eq!(streamed.text, "hello");
    assert_eq!(streamed.translations[0].text, "こんにちは");

    let history: serde_json::Value = client
        .get(format!("{base_url}/api/subtitles?limit=10"))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(history[0]["source"], "chatbox");
    assert_eq!(history[0]["translations"][0]["text"], "こんにちは");
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn microphone_test_requires_a_selected_input_and_stop_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: Some("test-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let client = reqwest::Client::new();
    let base_url = format!("http://{}", handle.address());
    let rejected = client
        .post(format!("{base_url}/api/audio/microphone-test/start"))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = rejected.json().await.unwrap();
    assert_eq!(body["code"], "audio.microphone_test_disabled");

    let stopped = client
        .post(format!("{base_url}/api/audio/microphone-test/stop"))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(stopped.status(), reqwest::StatusCode::OK);
    let health: serde_json::Value = client
        .get(format!("{base_url}/health"))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["microphone_test_running"], false);
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn deferred_vad_does_not_block_core_startup() {
    let directory = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let handle = start_with_deferred_vad(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: None,
        vad_model_path: None,
        asr_model_dir: None,
    })
    .await
    .unwrap();

    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "deferred startup should not wait for the managed VAD download"
    );
    assert_eq!(handle.vad_backend(), "energy");
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn settings_update_switches_the_live_model_directory() {
    let directory = tempfile::tempdir().unwrap();
    let config_path = directory.path().join("config.json");
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = start(CoreOptions {
        config_path,
        host: Some("127.0.0.1".into()),
        port: Some(port),
        session_token: None,
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let token = handle.session_token().to_owned();
    let tiny = handle
        .model_manager
        .list("small", "not_loaded")
        .into_iter()
        .find(|model| model.id == "tiny")
        .unwrap();
    let old_model_path = handle.model_manager.model_dir().join("ggml-tiny.bin");
    let model_file = std::fs::File::create(&old_model_path).unwrap();
    model_file.set_len(tiny.total_bytes).unwrap();
    asr::cache_model_verification_for_test(&old_model_path, "tiny");
    let client = reqwest::Client::new();
    let settings_url = format!("http://{}/api/settings", handle.address());
    let mut settings: serde_json::Value = client
        .get(&settings_url)
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    settings["storage"]["model_directory"] = serde_json::json!("custom/models");

    let response = client
        .put(settings_url)
        .bearer_auth(&token)
        .json(&settings)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        handle.model_manager.model_dir(),
        directory.path().join("custom").join("models")
    );
    assert!(handle.model_manager.model_dir().is_dir());
    assert!(!old_model_path.exists());
    assert!(handle
        .model_manager
        .model_dir()
        .join("ggml-tiny.bin")
        .is_file());
    assert!(handle.model_manager.is_downloaded("tiny").unwrap());

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn websocket_accepts_query_token_without_authorization_header() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: Some("test-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let response = reqwest::Client::new()
        .get(format!("http://{}/ws?token=test-token", handle.address()))
        .header(reqwest::header::CONNECTION, "Upgrade")
        .header(reqwest::header::UPGRADE, "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::SWITCHING_PROTOCOLS);
    drop(response);
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn websocket_query_token_does_not_weaken_rest_authentication() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: Some("test-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let client = reqwest::Client::new();
    let base_url = format!("http://{}", handle.address());
    let health = client
        .get(format!("{base_url}/health"))
        .send()
        .await
        .unwrap();
    let websocket = client
        .get(format!("{base_url}/ws?token=wrong-token"))
        .header(reqwest::header::CONNECTION, "Upgrade")
        .header(reqwest::header::UPGRADE, "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await
        .unwrap();

    assert_eq!(health.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert_eq!(websocket.status(), reqwest::StatusCode::UNAUTHORIZED);
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn websocket_rejects_untrusted_browser_origin() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: Some("test-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let response = reqwest::Client::new()
        .get(format!("http://{}/ws?token=test-token", handle.address()))
        .header(reqwest::header::ORIGIN, "https://example.com")
        .header(reqwest::header::CONNECTION, "Upgrade")
        .header(reqwest::header::UPGRADE, "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn dictionary_import_accepts_bodies_larger_than_axum_default() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: None,
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let token = handle.session_token().to_owned();
    let response = reqwest::Client::new()
        .post(format!(
            "http://{}/api/dictionaries/import",
            handle.address()
        ))
        .bearer_auth(token)
        .body(vec![0; 3 * 1024 * 1024])
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn loopback_without_supplied_token_is_still_authenticated() {
    let directory = tempfile::tempdir().unwrap();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: None,
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let client = reqwest::Client::new();
    let url = format!("http://{}/health", handle.address());
    let unauthorized = client.get(&url).send().await.unwrap();
    let authorized = client
        .get(url)
        .bearer_auth(handle.session_token())
        .send()
        .await
        .unwrap();

    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert_eq!(authorized.status(), reqwest::StatusCode::OK);
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_closes_an_active_websocket() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let directory = tempfile::tempdir().unwrap();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(0),
        session_token: Some("test-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let mut stream = tokio::net::TcpStream::connect(handle.address())
        .await
        .unwrap();
    let request = format!(
        "GET /ws?token=test-token HTTP/1.1\r\nHost: {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
        handle.address()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = [0u8; 512];
    let read = stream.read(&mut response).await.unwrap();
    assert!(String::from_utf8_lossy(&response[..read]).contains("101 Switching Protocols"));

    tokio::time::timeout(std::time::Duration::from_secs(2), handle.shutdown())
        .await
        .expect("shutdown timed out")
        .unwrap();
}

#[tokio::test]
async fn startup_rejects_invalid_persisted_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let config_path = directory.path().join("config.json");
    let mut config = crate::config::AppConfig::default();
    config.storage.subtitle_history_max_bytes = 0;
    std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();

    let error = start(CoreOptions {
        config_path,
        host: None,
        port: Some(0),
        session_token: None,
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .err()
    .expect("invalid config must fail");

    assert!(error.contains("subtitle_history_max_bytes"));
}

#[tokio::test]
async fn non_loopback_binding_requires_session_token() {
    let directory = tempfile::tempdir().unwrap();
    let error = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("0.0.0.0".into()),
        port: Some(0),
        session_token: None,
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .err()
    .expect("unauthenticated external binding must fail");

    assert!(error.contains("VRCS_SESSION_TOKEN"));
}

#[tokio::test]
async fn concurrent_api_profile_creates_do_not_lose_updates() {
    let directory = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(port),
        session_token: Some("test-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let client = reqwest::Client::new();
    let url = format!("http://{}/api/asr/profiles", handle.address());
    let mut requests = tokio::task::JoinSet::new();
    for index in 0..16 {
        let client = client.clone();
        let url = url.clone();
        requests.spawn(async move {
            let response = client
                .post(url)
                .bearer_auth("test-token")
                .json(&serde_json::json!({
                    "name": format!("DeepL {index}"),
                    "provider": "deepl",
                    "enabled_capabilities": ["text_translation"]
                }))
                .send()
                .await
                .unwrap();
            let status = response.status();
            let body = response.text().await.unwrap();
            (status, body)
        });
    }
    while let Some(result) = requests.join_next().await {
        let (status, body) = result.unwrap();
        assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    }

    let profiles: serde_json::Value = client
        .get(url)
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(profiles["profiles"].as_array().unwrap().len(), 16);
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn stale_settings_cannot_restore_a_deleted_api_profile() {
    let directory = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = start(CoreOptions {
        config_path: directory.path().join("config.json"),
        host: Some("127.0.0.1".into()),
        port: Some(port),
        session_token: Some("test-token".into()),
        vad_model_path: Some(directory.path().join("missing-silero.onnx")),
        asr_model_dir: None,
    })
    .await
    .unwrap();
    let client = reqwest::Client::new();
    let base_url = format!("http://{}", handle.address());
    let preflight = client
        .request(reqwest::Method::OPTIONS, format!("{base_url}/api/settings"))
        .header("Origin", "http://tauri.localhost")
        .header("Access-Control-Request-Method", "PUT")
        .header(
            "Access-Control-Request-Headers",
            "content-type,x-vrcs-config-revision",
        )
        .send()
        .await
        .unwrap();
    assert_eq!(preflight.status(), reqwest::StatusCode::OK);
    assert!(preflight
        .headers()
        .get("access-control-allow-headers")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("x-vrcs-config-revision"));

    let initial_response = client
        .get(format!("{base_url}/api/settings"))
        .bearer_auth("test-token")
        .header("Origin", "http://tauri.localhost")
        .send()
        .await
        .unwrap();
    assert!(initial_response
        .headers()
        .get("access-control-expose-headers")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("x-vrcs-config-revision"));
    let initial_revision = initial_response
        .headers()
        .get("x-vrcs-config-revision")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let initial_settings: serde_json::Value = initial_response.json().await.unwrap();
    let response = client
        .post(format!("{base_url}/api/asr/profiles"))
        .bearer_auth("test-token")
        .json(&serde_json::json!({
            "name": "Temporary Alibaba",
            "provider": "alibaba_cloud",
            "enabled_capabilities": ["speech_to_text"],
            "region": "china_beijing",
            "workspace_id": "test-workspace"
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert_eq!(status, reqwest::StatusCode::OK, "{body}");
    let created: serde_json::Value = serde_json::from_str(&body).unwrap();
    let profile_id = created["id"].as_str().unwrap();
    let rejected = client
        .put(format!("{base_url}/api/settings"))
        .bearer_auth("test-token")
        .header("x-vrcs-config-revision", initial_revision)
        .json(&initial_settings)
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), reqwest::StatusCode::CONFLICT);
    let rejected_body: serde_json::Value = rejected.json().await.unwrap();
    assert_eq!(rejected_body["code"], "settings.stale");

    let settings_response = client
        .get(format!("{base_url}/api/settings"))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap();
    let profile_revision = settings_response
        .headers()
        .get("x-vrcs-config-revision")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let mut stale_settings: serde_json::Value = settings_response.json().await.unwrap();
    stale_settings["asr"]["backend"] = serde_json::json!("qwen_realtime");
    stale_settings["asr"]["active_profile_id"] = serde_json::json!(profile_id);
    stale_settings = client
        .put(format!("{base_url}/api/settings"))
        .bearer_auth("test-token")
        .header("x-vrcs-config-revision", profile_revision)
        .json(&stale_settings)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(stale_settings["asr"]["backend"], "qwen_realtime");
    assert_eq!(
        stale_settings["asr"]["active_profile_id"],
        serde_json::json!(profile_id)
    );

    let deleted = client
        .delete(format!("{base_url}/api/asr/profiles/{profile_id}"))
        .bearer_auth("test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), reqwest::StatusCode::OK);

    stale_settings["osc"]["port"] = serde_json::json!(9001);
    let saved: serde_json::Value = client
        .put(format!("{base_url}/api/settings"))
        .bearer_auth("test-token")
        .json(&stale_settings)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(saved["asr"]["api_profiles"].as_array().unwrap().is_empty());
    assert_eq!(saved["asr"]["backend"], "local_whisper");
    assert!(saved["asr"]["active_profile_id"].is_null());
    handle.shutdown().await.unwrap();
}
