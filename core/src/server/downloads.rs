use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use super::SettingsContext;
use crate::downloads::{DownloadSourceStatus, ProbeResult};

#[derive(Default, Deserialize)]
pub(super) struct ProbeQuery {
    #[serde(default)]
    refresh: bool,
}

pub(super) async fn sources(State(state): State<SettingsContext>) -> Json<DownloadSourceStatus> {
    Json(state.config.download_sources.status().await)
}

pub(super) async fn probe(
    State(state): State<SettingsContext>,
    Query(query): Query<ProbeQuery>,
) -> Json<ProbeResult> {
    Json(state.config.download_sources.probe(query.refresh).await)
}

#[cfg(test)]
mod tests {
    use crate::{config, start, CoreOptions};
    use reqwest::StatusCode;
    use serde_json::Value;

    #[tokio::test]
    async fn source_preference_is_loaded_persisted_and_applied_only_after_commit() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.json");
        let mut config = config::AppConfig::default();
        config.storage.model_download_source = config::ModelDownloadSource::Official;
        config::save_config(&config_path, &config).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let handle = start(CoreOptions {
            config_path: config_path.clone(),
            host: Some("127.0.0.1".into()),
            port: Some(port),
            session_token: Some("download-source-test".into()),
            vad_model_path: Some(directory.path().join("missing-silero.onnx")),
            asr_model_dir: None,
        })
        .await
        .unwrap();
        let client = reqwest::Client::new();
        let base = format!("http://{}", handle.address());
        for (method, path) in [
            (reqwest::Method::GET, "/api/downloads/sources"),
            (reqwest::Method::POST, "/api/downloads/probe"),
        ] {
            assert_eq!(
                client
                    .request(method, format!("{base}{path}"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
        let status: Value = client
            .get(format!("{base}/api/downloads/sources"))
            .bearer_auth("download-source-test")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(status["configured_source"], "official");
        assert_eq!(status["active_source"], "official");
        assert!(status["probe"].is_null());

        let response = client
            .get(format!("{base}/api/settings"))
            .bearer_auth("download-source-test")
            .send()
            .await
            .unwrap();
        let revision = response.headers()["x-vrcs-config-revision"].clone();
        let mut settings: Value = response.json().await.unwrap();
        settings["storage"]["model_download_source"] = "modelscope".into();
        let response = client
            .put(format!("{base}/api/settings"))
            .bearer_auth("download-source-test")
            .header("x-vrcs-config-revision", revision)
            .json(&settings)
            .send()
            .await
            .unwrap();
        let status = response.status();
        let revision = response.headers().get("x-vrcs-config-revision").cloned();
        let mut settings: Value = response.json().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{settings}");
        let revision = revision.unwrap();
        assert_eq!(settings["storage"]["model_download_source"], "modelscope");
        assert_eq!(
            handle
                .model_manager
                .download_sources
                .status()
                .await
                .active_source,
            Some(config::ModelDownloadSource::Modelscope)
        );
        let saved: Value = serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
        assert_eq!(saved["storage"]["model_download_source"], "modelscope");

        // Block the atomic replacement to verify a failed save keeps the live preference.
        let saved_path = directory.path().join("saved.json");
        std::fs::rename(&config_path, &saved_path).unwrap();
        std::fs::create_dir(&config_path).unwrap();
        settings["storage"]["model_download_source"] = "official".into();
        let response = client
            .put(format!("{base}/api/settings"))
            .bearer_auth("download-source-test")
            .header("x-vrcs-config-revision", revision)
            .json(&settings)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let error: Value = response.json().await.unwrap();
        assert_eq!(error["code"], "settings.invalid");
        let status: Value = client
            .get(format!("{base}/api/downloads/sources"))
            .bearer_auth("download-source-test")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(status["configured_source"], "modelscope");
        assert_eq!(status["active_source"], "modelscope");
        assert_eq!(
            handle
                .model_manager
                .download_sources
                .status()
                .await
                .active_source,
            Some(config::ModelDownloadSource::Modelscope)
        );
        std::fs::remove_dir(&config_path).unwrap();
        std::fs::rename(&saved_path, &config_path).unwrap();
        handle.shutdown().await.unwrap();
    }
}
