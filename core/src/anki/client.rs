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
                    "无法连接 AnkiConnect（127.0.0.1:{}），请先启动 Anki",
                    config.port
                ))
                .with_params(json!({ "port": config.port }))
            } else {
                AnkiError::unavailable(format!(
                    "端口 {} 没有响应 AnkiConnect，请检查端口是否被 VRCS Core 或其他服务占用",
                    config.port
                ))
                .with_params(json!({ "port": config.port }))
            }
        })?;
    let result: Value = response.json().await.map_err(|_| {
        AnkiError::protocol("本地端口返回了无法识别的响应，不像 AnkiConnect".into())
    })?;
    let Some(object) = result.as_object() else {
        return Err(AnkiError::protocol(
            "本地端口返回了无法识别的响应，不像 AnkiConnect".into(),
        ));
    };
    if !object.contains_key("result") || !object.contains_key("error") {
        return Err(AnkiError::protocol(
            "本地端口返回了无法识别的响应，不像 AnkiConnect".into(),
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
            "AnkiConnect 返回的{label}格式无效"
        )));
    };
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| AnkiError::protocol(format!("AnkiConnect 返回的{label}格式无效")))
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
        .ok_or_else(|| AnkiError::protocol("AnkiConnect 返回了无效版本号".into()))?;
    if version < API_VERSION {
        return Ok(Discovery {
            version,
            decks: vec![],
            models: vec![],
            fields: vec![],
            configuration_valid: false,
            error_code: Some("incompatible_version"),
            params: json!({ "version": version, "minimum_version": API_VERSION }),
            message: format!("AnkiConnect API v{version} 过旧，需要 v{API_VERSION} 或更高版本"),
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
            "AnkiConnect 返回的牌组或笔记类型列表无效".into(),
        ));
    };
    if entries.len() != 2 {
        return Err(AnkiError::protocol(
            "AnkiConnect 返回的牌组或笔记类型列表无效".into(),
        ));
    }
    let decks = string_list(&entries[0], "牌组列表")?;
    let models = string_list(&entries[1], "笔记类型列表")?;
    let fields = if models.contains(&config.model) {
        string_list(
            &invoke(
                client,
                config,
                "modelFieldNames",
                Some(json!({ "modelName": config.model })),
            )
            .await?,
            "字段列表",
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
                "找不到牌组“{}”，请先在 Anki 中创建或选择其他牌组",
                config.deck
            ),
        ));
    }
    if !models.contains(&config.model) {
        return Ok(invalid(
            "missing_model",
            json!({ "model": config.model }),
            format!("找不到笔记类型“{}”", config.model),
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
            format!("笔记类型“{}”缺少字段：{}", config.model, missing.join("、")),
        ));
    }
    if config.front_field == config.back_field {
        return Ok(invalid(
            "duplicate_field_mapping",
            json!({
                "front_field": config.front_field,
                "back_field": config.back_field,
            }),
            "正面和背面不能映射到同一个字段".into(),
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
        message: "AnkiConnect 已连接，制卡配置有效".into(),
    })
}

pub(super) fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client")
}
