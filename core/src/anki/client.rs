use std::time::Duration;

use serde_json::{json, Value};

use crate::config::AnkiConfig;

use super::AnkiError;

const API_VERSION: i64 = 6;

pub(super) struct Discovery {
    pub(super) version: i64,
    pub(super) decks: Vec<String>,
    pub(super) models: Vec<String>,
    pub(super) fields: Vec<String>,
    pub(super) configuration_valid: bool,
    pub(super) error_code: Option<&'static str>,
    pub(super) params: Value,
    pub(super) message: String,
}

pub(super) async fn invoke(
    client: &reqwest::Client,
    config: &AnkiConfig,
    action: &str,
    params: Option<Value>,
) -> Result<Value, AnkiError> {
    let mut payload = json!({ "action": action, "version": API_VERSION });
    if let Some(params) = params {
        payload["params"] = params;
    }
    let url = format!("http://127.0.0.1:{}", config.port);
    let response = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|error| {
            if error.is_connect() || error.is_timeout() {
                AnkiError::unavailable(format!(
                    "Could not connect to AnkiConnect at 127.0.0.1:{}; start Anki first",
                    config.port
                ))
                .with_params(json!({ "port": config.port }))
            } else {
                AnkiError::unavailable(format!(
                    "Port {} did not respond as AnkiConnect; check whether VRCS Core or another service is using it",
                    config.port
                ))
                .with_params(json!({ "port": config.port }))
            }
        })?;
    let result: Value = response.json().await.map_err(|_| {
        AnkiError::protocol(
            "The local port returned an unrecognized response that is not AnkiConnect".into(),
        )
    })?;
    let Some(object) = result.as_object() else {
        return Err(AnkiError::protocol(
            "The local port returned an unrecognized response that is not AnkiConnect".into(),
        ));
    };
    if !object.contains_key("result") || !object.contains_key("error") {
        return Err(AnkiError::protocol(
            "The local port returned an unrecognized response that is not AnkiConnect".into(),
        ));
    }
    if !result["error"].is_null() {
        return Err(AnkiError::protocol(result["error"].to_string()));
    }
    Ok(result["result"].clone())
}

fn string_list(value: &Value, label: &str) -> Result<Vec<String>, AnkiError> {
    let Some(items) = value.as_array() else {
        return Err(AnkiError::protocol(format!(
            "AnkiConnect returned an invalid {label}"
        )));
    };
    items
        .iter()
        .map(|item| {
            item.as_str().map(str::to_owned).ok_or_else(|| {
                AnkiError::protocol(format!("AnkiConnect returned an invalid {label}"))
            })
        })
        .collect()
}

pub(super) async fn discover(
    client: &reqwest::Client,
    config: &AnkiConfig,
) -> Result<Discovery, AnkiError> {
    let version = invoke(client, config, "version", None)
        .await?
        .as_i64()
        .ok_or_else(|| {
            AnkiError::protocol("AnkiConnect returned an invalid version number".into())
        })?;
    if version < API_VERSION {
        return Ok(Discovery {
            version,
            decks: vec![],
            models: vec![],
            fields: vec![],
            configuration_valid: false,
            error_code: Some("incompatible_version"),
            params: json!({ "version": version, "minimum_version": API_VERSION }),
            message: format!(
                "AnkiConnect API v{version} is too old; v{API_VERSION} or later is required"
            ),
        });
    }

    let catalog = invoke(
        client,
        config,
        "multi",
        Some(json!({ "actions": [{ "action": "deckNames" }, { "action": "modelNames" }] })),
    )
    .await?;
    let Some(entries) = catalog.as_array() else {
        return Err(AnkiError::protocol(
            "AnkiConnect returned an invalid deck or note type list".into(),
        ));
    };
    if entries.len() != 2 {
        return Err(AnkiError::protocol(
            "AnkiConnect returned an invalid deck or note type list".into(),
        ));
    }
    let decks = string_list(&entries[0], "deck list")?;
    let models = string_list(&entries[1], "note type list")?;
    let fields = if models.contains(&config.model) {
        string_list(
            &invoke(
                client,
                config,
                "modelFieldNames",
                Some(json!({ "modelName": config.model })),
            )
            .await?,
            "field list",
        )?
    } else {
        Vec::new()
    };

    let invalid = |error_code, params: Value, message: String| Discovery {
        version,
        decks: decks.clone(),
        models: models.clone(),
        fields: fields.clone(),
        configuration_valid: false,
        error_code: Some(error_code),
        params,
        message,
    };
    if !decks.contains(&config.deck) {
        return Ok(invalid(
            "missing_deck",
            json!({ "deck": config.deck }),
            format!(
                "Deck '{}' was not found; create it in Anki or select another deck",
                config.deck
            ),
        ));
    }
    if !models.contains(&config.model) {
        return Ok(invalid(
            "missing_model",
            json!({ "model": config.model }),
            format!("Note type '{}' was not found", config.model),
        ));
    }
    let missing: Vec<&str> = [&config.front_field, &config.back_field]
        .into_iter()
        .filter(|name| !fields.contains(name))
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        return Ok(invalid(
            "missing_field",
            json!({ "model": config.model, "fields": missing }),
            format!(
                "Note type '{}' is missing fields: {}",
                config.model,
                missing.join(", ")
            ),
        ));
    }
    if config.front_field == config.back_field {
        return Ok(invalid(
            "duplicate_field_mapping",
            json!({
                "front_field": config.front_field,
                "back_field": config.back_field,
            }),
            "Front and back cannot map to the same field".into(),
        ));
    }
    Ok(Discovery {
        version,
        decks,
        models,
        fields,
        configuration_valid: true,
        error_code: None,
        params: json!({}),
        message: "AnkiConnect is connected and the card configuration is valid".into(),
    })
}

pub(super) fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client")
}
