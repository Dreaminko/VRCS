//! AnkiConnect facade: status probing and card creation.
//! The HTTP protocol and note rendering live in focused child modules while
//! this module keeps the public API stable.

use serde_json::{json, Value};

use crate::config::AnkiConfig;
use crate::models::CardRequest;

mod client;
mod note;

use client::{discover, invoke};
use note::build_note;

#[derive(Debug)]
pub struct AnkiError {
    pub status_code: u16,
    pub code: &'static str,
    pub params: Value,
    pub message: String,
}

impl AnkiError {
    fn disabled() -> Self {
        Self {
            status_code: 403,
            code: "disabled",
            params: json!({}),
            message: "AnkiConnect integration is disabled".into(),
        }
    }

    fn unavailable(message: String) -> Self {
        Self {
            status_code: 503,
            code: "unavailable",
            params: json!({}),
            message,
        }
    }

    fn configuration(code: &'static str, params: Value, message: String) -> Self {
        Self {
            status_code: 422,
            code,
            params,
            message,
        }
    }

    fn duplicate(message: String) -> Self {
        Self {
            status_code: 409,
            code: "duplicate",
            params: json!({}),
            message,
        }
    }

    fn protocol(message: String) -> Self {
        Self {
            status_code: 502,
            code: "protocol_error",
            params: json!({}),
            message,
        }
    }

    fn with_params(mut self, params: Value) -> Self {
        self.params = params;
        self
    }
}

pub async fn status(client: &reqwest::Client, config: &AnkiConfig) -> Value {
    if !config.enabled {
        return json!({
            "connected": false,
            "version": null,
            "decks": [],
            "models": [],
            "fields": [],
            "configuration_valid": false,
            "error_code": "disabled",
            "status_code": "anki.disabled",
            "params": {},
            "detail": "AnkiConnect integration is disabled",
            "message": "AnkiConnect integration is disabled",
        });
    }
    match discover(client, config).await {
        Ok(discovery) => json!({
            "connected": true,
            "version": discovery.version,
            "decks": discovery.decks,
            "models": discovery.models,
            "fields": discovery.fields,
            "configuration_valid": discovery.configuration_valid,
            "error_code": discovery.error_code,
            "status_code": discovery.error_code
                .map(|code| format!("anki.{code}"))
                .unwrap_or_else(|| "anki.connected".into()),
            "params": discovery.params,
            "detail": discovery.message,
            "message": discovery.message,
        }),
        Err(error) => json!({
            "connected": false,
            "version": null,
            "decks": [],
            "models": [],
            "fields": [],
            "configuration_valid": false,
            "error_code": error.code,
            "status_code": format!("anki.{}", error.code),
            "params": error.params,
            "detail": error.message,
            "message": error.message,
        }),
    }
}

pub async fn create_card(
    client: &reqwest::Client,
    card: &CardRequest,
    config: &AnkiConfig,
    disabled: impl std::future::Future<Output = ()>,
) -> Result<i64, AnkiError> {
    if !config.enabled {
        return Err(AnkiError::disabled());
    }
    let prepare = async {
        let discovery = discover(client, config).await?;
        if !discovery.configuration_valid {
            return Err(AnkiError::configuration(
                discovery.error_code.unwrap_or("invalid_configuration"),
                discovery.params,
                discovery.message,
            ));
        }
        let note = build_note(card, config);
        let can_add = invoke(
            client,
            config,
            "canAddNotes",
            Some(json!({ "notes": [note] })),
        )
        .await?;
        let Some(flags) = can_add.as_array() else {
            return Err(AnkiError::protocol(
                "AnkiConnect returned an invalid card validation result".into(),
            ));
        };
        if flags.len() != 1 || flags[0].as_bool() != Some(true) {
            if flags.len() == 1 && flags[0].as_bool() == Some(false) {
                return Err(AnkiError::duplicate(
                    "This note already exists and was not added again".into(),
                ));
            }
            return Err(AnkiError::protocol(
                "AnkiConnect returned an invalid card validation result".into(),
            ));
        }
        Ok::<_, AnkiError>(note)
    };
    let note = tokio::select! {
        biased;
        _ = disabled => return Err(AnkiError::disabled()),
        note = prepare => note?,
    };
    let result = match invoke(client, config, "addNote", Some(json!({ "note": note }))).await {
        Ok(result) => result,
        Err(error)
            if error.code == "protocol_error"
                && error.message.to_lowercase().contains("duplicate") =>
        {
            return Err(AnkiError::duplicate(
                "This note already exists and was not added again".into(),
            ));
        }
        Err(error) => return Err(error),
    };
    result
        .as_i64()
        .ok_or_else(|| AnkiError::protocol("AnkiConnect did not return a valid note ID".into()))
}

pub fn client() -> reqwest::Client {
    client::http_client()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_integration_skips_status_checks_and_card_creation() {
        let config = AnkiConfig {
            enabled: false,
            ..AnkiConfig::default()
        };
        let http = client();

        let status = status(&http, &config).await;
        assert_eq!(status["status_code"], "anki.disabled");
        assert_eq!(status["connected"], false);

        let card = CardRequest {
            term: "学ぶ".into(),
            definition: "学习".into(),
            context: String::new(),
            reading: None,
            dictionary: None,
            language: None,
            labels: None,
        };
        let error = create_card(&http, &card, &config, std::future::pending())
            .await
            .unwrap_err();
        assert_eq!(error.code, "disabled");
        assert_eq!(error.status_code, 403);
    }
    #[tokio::test]
    async fn feature_switches_stop_anki_preparation_but_keep_a_sent_note_receipt() {
        use axum::{routing::post, Json, Router};
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        for stop_at in ["canAddNotes", "addNote"] {
            let reached = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let added = Arc::new(AtomicUsize::new(0));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let config = AnkiConfig {
                enabled: true,
                port: listener.local_addr().unwrap().port(),
                ..AnkiConfig::default()
            };
            let handler_config = config.clone();
            let handler_reached = reached.clone();
            let handler_release = release.clone();
            let handler_added = added.clone();
            let router = Router::new().route(
                "/",
                post(move |Json(body): Json<Value>| {
                    let config = handler_config.clone();
                    let reached = handler_reached.clone();
                    let release = handler_release.clone();
                    let added = handler_added.clone();
                    async move {
                        let action = body["action"].as_str().unwrap();
                        if action == "addNote" {
                            added.fetch_add(1, Ordering::SeqCst);
                        }
                        if action == stop_at {
                            reached.notify_one();
                            release.notified().await;
                        }
                        let result = match action {
                            "version" => json!(6),
                            "multi" => json!([[config.deck], [config.model]]),
                            "modelFieldNames" => json!([config.front_field, config.back_field]),
                            "canAddNotes" => json!([true]),
                            "addNote" => json!(123),
                            _ => panic!("Unexpected Anki action: {action}"),
                        };
                        Json(json!({"result": result, "error": null}))
                    }
                }),
            );
            let server = tokio::spawn(async move {
                axum::serve(listener, router).await.unwrap();
            });
            let (disabled_tx, mut disabled_rx) = tokio::sync::watch::channel(false);
            let task = tokio::spawn(async move {
                let card = CardRequest {
                    term: "test".into(),
                    definition: "definition".into(),
                    context: String::new(),
                    reading: None,
                    dictionary: None,
                    language: None,
                    labels: None,
                };
                create_card(&client(), &card, &config, async move {
                    let _ = disabled_rx.wait_for(|disabled| *disabled).await;
                })
                .await
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), reached.notified())
                .await
                .unwrap();
            disabled_tx.send_replace(true);
            release.notify_one();
            let result = tokio::time::timeout(std::time::Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
            if stop_at == "canAddNotes" {
                assert_eq!(result.unwrap_err().code, "disabled");
                assert_eq!(added.load(Ordering::SeqCst), 0);
            } else {
                assert_eq!(result.unwrap(), 123);
                assert_eq!(added.load(Ordering::SeqCst), 1);
            }
            server.abort();
        }
    }
}
