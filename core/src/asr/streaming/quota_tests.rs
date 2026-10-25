use super::*;

#[tokio::test]
async fn qwen_rate_close_is_shared_with_new_tracks_and_stop_cancels_reconnect_wait() {
    use tokio_tungstenite::tungstenite::protocol::{frame::coding::CloseCode, CloseFrame};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let profile = ApiProfile {
        provider: providers::QWEN_AI_PROVIDER.into(),
        ..Default::default()
    };
    let mut config = AsrConfig {
        backend: providers::SERVICE_QWEN_LIVE_TRANSLATE.into(),
        single_speaker: true,
        live_translation_target: Some("zh".into()),
        api_profiles: vec![profile.clone()],
        active_profile_id: Some(profile.id.clone()),
        ..Default::default()
    };
    config
        .service_settings
        .get_mut(&config.backend)
        .unwrap()
        .model = format!(
        "qwen3.8-livetranslate-flash-realtime-test-{}",
        uuid::Uuid::new_v4()
    );
    TEST_CONNECT_URL
        .scope(url, async {
            let connect = connect_initialized(
                Provider::QwenLiveTranslate,
                &config,
                &profile,
                0.4,
                "mock-key",
            );
            let server = async {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                socket.next().await.unwrap().unwrap();
                socket
                    .send(Message::Text(
                        serde_json::json!({"type":"session.updated"})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
                socket
            };
            let (client, mut server) = tokio::join!(connect, server);
            let (socket, task_id) = client.unwrap();
            let (audio_tx, audio_rx) = mpsc::channel(32);
            let (event_tx, mut events) = mpsc::channel(32);
            let (stop_tx, stop_rx) = watch::channel(false);
            let worker = run_with_reconnect(
                Provider::QwenLiveTranslate,
                config.clone(),
                profile.clone(),
                0.4,
                "mock-key".into(),
                socket,
                task_id,
                audio_rx,
                event_tx,
                stop_rx,
            );
            let driver = async {
                audio_tx
                    .send(StreamingInput::AudioActivity(
                        super::super::share_audio(vec![0.1; 1600]),
                        true,
                    ))
                    .await
                    .unwrap();
                server.next().await.unwrap().unwrap();
                server
                    .send(Message::Close(Some(CloseFrame {
                        code: CloseCode::Invalid,
                        reason: "Requests rate limit exceeded, please try again later.".into(),
                    })))
                    .await
                    .unwrap();
                let Some(CloudEvent::Failed { detail, .. }) = events.recv().await else {
                    panic!("expected cloud failure")
                };
                assert!(detail.contains("1007") && detail.contains("Requests rate limit"));
                audio_tx
                    .send(StreamingInput::AudioActivity(
                        super::super::share_audio(vec![0.1; 1600]),
                        true,
                    ))
                    .await
                    .unwrap();
                let (commit_tx, commit_rx) = oneshot::channel();
                audio_tx
                    .send(StreamingInput::Commit(commit_tx))
                    .await
                    .unwrap();
                assert_eq!(
                    tokio::time::timeout(Duration::from_millis(200), commit_rx)
                        .await
                        .unwrap()
                        .unwrap(),
                    Ok(())
                );
                // A different player cannot bypass the same account/model gate.
                assert!(tokio::time::timeout(
                    Duration::from_millis(100),
                    connect_initialized(
                        Provider::QwenLiveTranslate,
                        &config,
                        &profile,
                        0.4,
                        "mock-key"
                    )
                )
                .await
                .is_err());
                assert!(
                    tokio::time::timeout(Duration::from_millis(30), listener.accept())
                        .await
                        .is_err()
                );
                stop_tx.send(true).unwrap();
            };
            tokio::time::timeout(Duration::from_secs(2), async {
                tokio::join!(worker, driver);
            })
            .await
            .unwrap();
        })
        .await;
}
