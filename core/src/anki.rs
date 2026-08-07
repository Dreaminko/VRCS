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
            message: "AnkiConnect 集成已关闭".into(),
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
            "detail": "AnkiConnect 集成已关闭",
            "message": "AnkiConnect 集成已关闭",
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
) -> Result<i64, AnkiError> {
    if !config.enabled {
        return Err(AnkiError::disabled());
    }
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
            "AnkiConnect 返回的制卡校验结果无效".into(),
        ));
    };
    if flags.len() != 1 || flags[0].as_bool() != Some(true) {
        if flags.len() == 1 && flags[0].as_bool() == Some(false) {
            return Err(AnkiError::duplicate("这条笔记已存在，未重复添加".into()));
        }
        return Err(AnkiError::protocol(
            "AnkiConnect 返回的制卡校验结果无效".into(),
        ));
    }
    let result = match invoke(client, config, "addNote", Some(json!({ "note": note }))).await {
        Ok(result) => result,
        Err(error)
            if error.code == "protocol_error"
                && error.message.to_lowercase().contains("duplicate") =>
        {
            return Err(AnkiError::duplicate("这条笔记已存在，未重复添加".into()));
        }
        Err(error) => return Err(error),
    };
    result
        .as_i64()
        .ok_or_else(|| AnkiError::protocol("AnkiConnect 未返回有效的笔记 ID".into()))
}

pub fn client() -> reqwest::Client {
    client::http_client()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_integration_skips_status_checks_and_card_creation() {
        let mut config = AnkiConfig::default();
        config.enabled = false;
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
        let error = create_card(&http, &card, &config).await.unwrap_err();
        assert_eq!(error.code, "disabled");
        assert_eq!(error.status_code, 403);
    }
}
