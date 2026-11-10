use std::sync::Arc;

use axum::{extract::State, Json};

use crate::platform_capabilities::{
    self as platform, AudioCapabilities, Capability, ChatboxCapabilities, InferenceCapabilities,
    PlatformCapabilities,
};

use super::AppState;

pub(super) async fn get(State(state): State<Arc<AppState>>) -> Json<PlatformCapabilities> {
    Json(snapshot(&state).await)
}

pub(crate) async fn snapshot(state: &AppState) -> PlatformCapabilities {
    let (microphone, system_output, audio_reason) = if cfg!(any(windows, target_os = "linux")) {
        let devices = tokio::task::spawn_blocking(crate::audio::list_devices).await;
        match devices {
            Ok(Ok(devices)) => (
                devices.iter().any(|device| !device.is_loopback),
                devices.iter().any(|device| device.is_loopback),
                "audio.device_unavailable",
            ),
            Ok(Err(error)) => (false, false, error.code()),
            Err(_) => (false, false, "audio.device_task_failed"),
        }
    } else {
        (false, false, platform::AUDIO_UNAVAILABLE)
    };
    let qwen = state.capture.qwen_runtime.snapshot().await;
    let plamo = state.content.plamo_runtime.snapshot().await;
    let ocr = state.config.local_ocr.execution_status();
    let managed = platform::managed_inference_supported();
    let (credentials_available, credentials_reason) = tokio::task::spawn_blocking(crate::credentials::storage_capability)
        .await.unwrap_or((false, Some("credentials.no_service")));
    let qwen_installed = if managed {
        let manager = Arc::clone(&state.capture.model_manager);
        let package = state
            .config
            .config
            .read()
            .expect("config lock")
            .asr
            .managed_qwen
            .package_id
            .clone();
        state
            .capture
            .qwen_runtime
            .executable_path()
            .is_ok_and(|path| path.is_file())
            && tokio::task::spawn_blocking(move || manager.describe_qwen(&package))
                .await
                .ok()
                .and_then(Result::ok)
                .is_some_and(|model| model.status == "installed")
    } else {
        false
    };
    let local_installed = if managed && plamo.runtime_available {
        let models = Arc::clone(&state.content.local_translation_models);
        tokio::task::spawn_blocking(move || {
            let model = models.status();
            model.status == "installed" && model.license_accepted
        })
        .await
        .unwrap_or(false)
    } else {
        false
    };
    PlatformCapabilities {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        audio: AudioCapabilities {
            microphone: Capability::runtime(cfg!(any(windows, target_os = "linux")), microphone, audio_reason),
            system_output: Capability::runtime(
                cfg!(any(windows, target_os = "linux")),
                system_output,
                audio_reason,
            ),
            application_output: Capability::runtime(
                cfg!(windows),
                system_output,
                "platform.application_audio_unavailable",
            ),
            player_audio: Capability::runtime(
                cfg!(windows),
                cfg!(windows),
                platform::NETWORK_CAPTURE_UNSUPPORTED,
            ),
        },
        credentials: Capability::runtime(
            cfg!(any(windows, target_os = "linux")),
            credentials_available,
            credentials_reason.unwrap_or(platform::CREDENTIALS_UNAVAILABLE),
        ),
        inference: InferenceCapabilities {
            vad_cpu: Capability::runtime(
                true,
                state.capture.vad_runtime.backend() == "silero-onnx",
                "platform.vad_unavailable",
            ),
            qwen_cpu: Capability::runtime(
                managed,
                qwen_installed && qwen.status != "error",
                platform::QWEN_UNAVAILABLE,
            ),
            qwen_vulkan: Capability::runtime(
                platform::qwen_vulkan_supported(),
                qwen.status == "ready" && qwen.device == Some("gpu"),
                platform::QWEN_UNAVAILABLE,
            ),
            local_translation: Capability::runtime(
                platform::local_translation_supported(),
                local_installed && plamo.status != "error",
                platform::LOCAL_TRANSLATION_UNAVAILABLE,
            ),
            ocr_cpu: Capability::runtime(
                true,
                ocr.active_device == Some(crate::config::OcrDevice::Cpu),
                "platform.ocr_unavailable",
            ),
            ocr_directml: Capability::runtime(
                cfg!(windows),
                ocr.active_device == Some(crate::config::OcrDevice::Directml),
                platform::DIRECTML_UNSUPPORTED,
            ),
        },
        chatbox: ChatboxCapabilities {
            osc_send: Capability::runtime(true, true, "platform.osc_unavailable"),
            network_receive: Capability::runtime(
                cfg!(windows),
                cfg!(windows),
                platform::NETWORK_CAPTURE_UNSUPPORTED,
            ),
        },
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use serde_json::{json, Value};

    #[tokio::test]
    async fn linux_keeps_imported_preferences_and_rejects_unimplemented_backends() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.json");
        let mut imported = crate::config::AppConfig::default();
        imported.audio.output.mode = "vrchat_players".into();
        imported.audio.output.player_audio.enabled = true;
        imported.audio.output.player_audio.selected_tracks = Some(vec!["usr_saved".into()]);
        imported.audio.output.player_audio.proximity.enabled = true;
        imported.vrchat_chatbox.enabled = true;
        imported.model_defaults.translation.backend = crate::config::TranslationBackend::PlamoLocal;
        imported.model_defaults.translation.model = crate::translation::MODEL_ID.into();
        imported.resolve_model_assignments();
        crate::config::save_config(&config_path, &imported).unwrap();
        let handle = crate::start(crate::CoreOptions {
            config_path,
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: Some("linux-platform".into()),
            vad_model_path: Some(directory.path().join("missing.onnx")),
            asr_model_dir: None,
        })
        .await
        .unwrap();
        // Runtime port overrides are ephemeral; settings still require a nonzero port.
        handle.state.config.config.write().unwrap().server.port = handle.address().port();
        let client = reqwest::Client::new();
        let base = format!("http://{}", handle.address());
        assert_eq!(
            client
                .get(format!("{base}/api/capabilities"))
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::UNAUTHORIZED
        );
        let request = |method, path: &str| {
            client
                .request(method, format!("{base}{path}"))
                .bearer_auth("linux-platform")
        };
        let capabilities: Value = request(reqwest::Method::GET, "/api/capabilities")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(capabilities["inference"]["vad_cpu"]["available"], false);
        assert_eq!(capabilities["inference"]["local_translation"]["supported"], crate::platform_capabilities::local_translation_supported());
        assert_eq!(capabilities["inference"]["local_translation"]["available"], false);
        assert_eq!(capabilities["chatbox"]["osc_send"]["supported"], true);
        assert_eq!(capabilities["audio"]["player_audio"]["supported"], false);
        assert_eq!(
            capabilities["chatbox"]["network_receive"]["supported"],
            false
        );
        let before: Value = request(reqwest::Method::GET, "/api/settings")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        for (path, body, code) in [
            (
                "/api/capture/start",
                json!({}),
                "platform.network_capture_unsupported",
            ),
            (
                "/api/audio/microphone-test/start",
                json!({}),
                "audio.microphone_test_disabled",
            ),
        ] {
            let response = request(reqwest::Method::POST, path)
                .json(&body)
                .send()
                .await
                .unwrap();
            assert!(!response.status().is_success(), "{path}");
            assert_eq!(
                response.json::<Value>().await.unwrap()["code"],
                code,
                "{path}"
            );
        }
        let preview: Value = request(reqwest::Method::POST, "/api/translations/preview")
            .json(&json!({"text": "hello", "source_language": "en", "target_language": "ja"}))
            .send().await.unwrap().json().await.unwrap();
        assert_eq!(preview["code"], if crate::platform_capabilities::local_translation_supported() {
            "translation.local_model_missing"
        } else {
            "platform.local_translation_unavailable"
        });
        let mut selection = before.clone();
        selection["audio"]["output"]["player_audio"]["proximity"]["radius_m"] = json!(5.0);
        let rejected: Value = request(reqwest::Method::PUT, "/api/settings")
            .json(&selection)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(rejected["code"], "platform.network_capture_unsupported");
        let mut repair = before.clone();
        repair["audio"]["output"]["trigger_threshold_dbfs"] = json!(-50.0);
        assert!(request(reqwest::Method::PUT, "/api/settings")
            .json(&repair)
            .send()
            .await
            .unwrap()
            .status()
            .is_success());
        let after: Value = request(reqwest::Method::GET, "/api/settings")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            before["audio"]["output"]["player_audio"],
            after["audio"]["output"]["player_audio"]
        );
        assert_eq!(before["vrchat_chatbox"], after["vrchat_chatbox"]);
        assert_eq!(before["translation"], after["translation"]);
        handle.shutdown().await.unwrap();
    }
}
