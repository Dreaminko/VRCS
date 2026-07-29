//! AnkiConnect 客户端：状态探测与制卡。
//! HTTP 协议、笔记 HTML 组装与 Python 版 `app/anki.py` 保持一致。

use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use serde_json::{json, Value};

use crate::config::AnkiConfig;
use crate::models::CardRequest;

const API_VERSION: i64 = 6;
const FONT_STACK: &str = "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', \
    'Hiragino Sans GB', 'Noto Sans CJK SC', sans-serif";

#[derive(Debug)]
pub struct AnkiError {
    pub status_code: u16,
    pub code: &'static str,
    pub message: String,
}

impl AnkiError {
    fn unavailable(message: String) -> Self {
        Self {
            status_code: 503,
            code: "unavailable",
            message,
        }
    }
    fn configuration(message: String) -> Self {
        Self {
            status_code: 422,
            code: "invalid_configuration",
            message,
        }
    }
    fn duplicate(message: String) -> Self {
        Self {
            status_code: 409,
            code: "duplicate",
            message,
        }
    }
    fn protocol(message: String) -> Self {
        Self {
            status_code: 502,
            code: "protocol_error",
            message,
        }
    }
}

struct Discovery {
    version: i64,
    decks: Vec<String>,
    models: Vec<String>,
    fields: Vec<String>,
    configuration_valid: bool,
    error_code: Option<&'static str>,
    message: String,
}

async fn invoke(
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
    let response = client.post(&url).json(&payload).send().await.map_err(|e| {
        if e.is_connect() || e.is_timeout() {
            AnkiError::unavailable(format!(
                "无法连接 AnkiConnect（127.0.0.1:{}），请先启动 Anki",
                config.port
            ))
        } else {
            AnkiError::unavailable(format!(
                "端口 {} 没有响应 AnkiConnect，请检查端口是否被 VRCS Core 或其他服务占用",
                config.port
            ))
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

async fn discover(client: &reqwest::Client, config: &AnkiConfig) -> Result<Discovery, AnkiError> {
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

    let invalid = |error_code, message: String| Discovery {
        version,
        decks: decks.clone(),
        models: models.clone(),
        fields: fields.clone(),
        configuration_valid: false,
        error_code: Some(error_code),
        message,
    };
    if !decks.contains(&config.deck) {
        return Ok(invalid(
            "missing_deck",
            format!(
                "找不到牌组“{}”，请先在 Anki 中创建或选择其他牌组",
                config.deck
            ),
        ));
    }
    if !models.contains(&config.model) {
        return Ok(invalid(
            "missing_model",
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
            format!("笔记类型“{}”缺少字段：{}", config.model, missing.join("、")),
        ));
    }
    if config.front_field == config.back_field {
        return Ok(invalid(
            "duplicate_field_mapping",
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
        message: "AnkiConnect 已连接，制卡配置有效".into(),
    })
}

pub async fn status(client: &reqwest::Client, config: &AnkiConfig) -> Value {
    match discover(client, config).await {
        Ok(d) => json!({
            "connected": true,
            "version": d.version,
            "decks": d.decks,
            "models": d.models,
            "fields": d.fields,
            "configuration_valid": d.configuration_valid,
            "error_code": d.error_code,
            "message": d.message,
        }),
        Err(e) => json!({
            "connected": false,
            "version": null,
            "decks": [],
            "models": [],
            "fields": [],
            "configuration_valid": false,
            "error_code": e.code,
            "message": e.message,
        }),
    }
}

pub async fn create_card(
    client: &reqwest::Client,
    card: &CardRequest,
    config: &AnkiConfig,
) -> Result<i64, AnkiError> {
    let discovery = discover(client, config).await?;
    if !discovery.configuration_valid {
        return Err(AnkiError::configuration(discovery.message));
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
        Err(e) if e.code == "protocol_error" && e.message.to_lowercase().contains("duplicate") => {
            return Err(AnkiError::duplicate("这条笔记已存在，未重复添加".into()));
        }
        Err(e) => return Err(e),
    };
    result
        .as_i64()
        .ok_or_else(|| AnkiError::protocol("AnkiConnect 未返回有效的笔记 ID".into()))
}

/// 与 Python `html.escape(value, quote=True)` 一致
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn escaped_lines(value: &str) -> String {
    escape_html(value)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "<br>")
}

fn section_label(label: &str) -> String {
    format!(
        r#"<div class="vrcs-section-label" style="display:flex;align-items:center;gap:0.45rem;margin-bottom:0.75rem;font-size:0.75rem;font-weight:700;line-height:1.4;opacity:0.62;"><span aria-hidden="true" style="display:inline-block;width:0.42rem;height:0.42rem;border-radius:999px;background:#3d73a8;"></span>{label}</div>"#
    )
}

fn dictionary_heading_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^【(?P<label>.+)】$").unwrap())
}

fn numbered_gloss_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"^\d+[.)、]\s*(?P<text>.+)$").unwrap())
}

/// 把多段释义文本渲染成分块 HTML：
/// 支持「【词典名】」标题行与编号义项列表。
fn definition_html(value: &str) -> String {
    let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = normalized.trim();
    let block_splitter = Regex::new(r"\n\s*\n").unwrap();
    let mut rendered = String::new();

    for raw_block in block_splitter.split(normalized) {
        let lines: Vec<&str> = raw_block
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            continue;
        }
        let heading = dictionary_heading_pattern().captures(lines[0]);
        let has_heading = heading.is_some();
        let content_lines: &[&str] = if has_heading { &lines[1..] } else { &lines };
        let glosses: Vec<Option<regex::Captures>> = content_lines
            .iter()
            .map(|line| numbered_gloss_pattern().captures(line))
            .collect();
        let is_numbered_list = !glosses.is_empty() && glosses.iter().all(Option::is_some);
        let block_margin = if rendered.is_empty() { "0" } else { "1.15rem" };

        rendered.push_str(r#"<div class="vrcs-definition-block" style="margin-top:"#);
        rendered.push_str(block_margin);
        rendered.push_str(r#";">"#);

        if let Some(heading) = heading {
            rendered.push_str(&format!(
                r#"<div class="vrcs-dictionary-label" style="display:inline-block;padding:0.2rem 0.52rem;border-radius:999px;background:rgba(61,115,168,0.12);font-size:0.76rem;font-weight:600;line-height:1.45;opacity:0.82;">{}</div>"#,
                escape_html(&heading["label"])
            ));
        }

        if is_numbered_list {
            rendered.push_str(
                r#"<ol class="vrcs-gloss-list" style="margin:0.65rem 0 0;padding-left:1.45rem;">"#,
            );
            for gloss in glosses.iter().flatten() {
                rendered.push_str(&format!(
                    r#"<li style="margin:0.34rem 0;padding-left:0.18rem;">{}</li>"#,
                    escape_html(&gloss["text"])
                ));
            }
            rendered.push_str("</ol>");
        } else if !content_lines.is_empty() {
            let content = content_lines
                .iter()
                .map(|line| escape_html(line))
                .collect::<Vec<_>>()
                .join("<br>");
            let content_margin = if has_heading { "0.62rem" } else { "0" };
            rendered.push_str(&format!(
                r#"<div class="vrcs-definition-text" style="margin-top:{content_margin};">{content}</div>"#
            ));
        }
        rendered.push_str("</div>");
    }
    rendered
}

fn build_note(card: &CardRequest, config: &AnkiConfig) -> Value {
    let mut front = format!(
        r#"<div class="vrcs-note vrcs-note-front" style="max-width:42rem;margin:0 auto;padding:0.35rem 0;text-align:center;font-family:{FONT_STACK};color:inherit;overflow-wrap:anywhere;"><div class="vrcs-term" style="font-size:2rem;font-weight:700;line-height:1.25;">{}</div>"#,
        escaped_lines(&card.term)
    );
    if let Some(reading) = &card.reading {
        front.push_str(&format!(
            r#"<div class="vrcs-reading" style="margin-top:0.48rem;font-size:0.95rem;line-height:1.5;opacity:0.62;">{}</div>"#,
            escaped_lines(reading)
        ));
    }
    front.push_str("</div>");

    let mut back = format!(
        r#"<div class="vrcs-note vrcs-note-back" style="max-width:42rem;margin:0 auto;text-align:left;font-family:{FONT_STACK};font-size:1rem;line-height:1.72;color:inherit;overflow-wrap:anywhere;"><section class="vrcs-definition">{}<div class="vrcs-definition-content">{}</div></section>"#,
        section_label("释义"),
        definition_html(&card.definition)
    );
    if !card.context.is_empty() {
        back.push_str(&format!(
            r#"<section class="vrcs-context" style="margin-top:1.45rem;padding-top:1rem;border-top:1px solid rgba(127,127,127,0.24);">{}<div class="vrcs-context-content" style="padding:0.82rem 0.95rem;border-radius:0.65rem;background:rgba(61,115,168,0.08);font-size:0.95rem;line-height:1.7;">{}</div></section>"#,
            section_label("语境"),
            escaped_lines(&card.context)
        ));
    }
    let metadata = [
        card.dictionary.clone(),
        card.language.as_ref().map(|l| l.to_uppercase()),
    ]
    .into_iter()
    .flatten()
    .map(|value| escaped_lines(&value))
    .collect::<Vec<_>>()
    .join(" · ");
    if !metadata.is_empty() {
        back.push_str(&format!(
            r#"<footer class="vrcs-source" style="margin-top:1.15rem;font-size:0.76rem;line-height:1.5;opacity:0.56;">{metadata}</footer>"#
        ));
    }
    back.push_str("</div>");

    json!({
        "deckName": config.deck,
        "modelName": config.model,
        "fields": {
            config.front_field.clone(): front,
            config.back_field.clone(): back,
        },
        "options": { "allowDuplicate": false },
        "tags": ["vrcs"],
    })
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_like_python_html_escape() {
        assert_eq!(
            escape_html("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#x27;&amp;&#x27;&lt;/a&gt;"
        );
    }

    #[test]
    fn definition_html_renders_numbered_glosses() {
        let html = definition_html("【TestDict】\n1. first\n2. second");
        assert!(html.contains("vrcs-dictionary-label"));
        assert!(html.contains("TestDict"));
        assert!(html.contains("<ol"));
        assert!(html.contains("first"));
        assert!(html.contains("second"));
    }

    #[test]
    fn definition_html_renders_plain_blocks() {
        let html = definition_html("alpha\nbeta\n\ngamma");
        assert_eq!(html.matches("vrcs-definition-block").count(), 2);
        assert!(html.contains("alpha<br>beta"));
        assert!(html.contains("gamma"));
    }

    #[test]
    fn note_contains_term_reading_context_and_metadata() {
        let card = CardRequest {
            term: "食べる".into(),
            definition: "1. 吃".into(),
            context: "ご飯を食べる".into(),
            reading: Some("たべる".into()),
            dictionary: Some("TestDict".into()),
            language: Some("ja".into()),
        };
        let config = AnkiConfig::default();
        let note = build_note(&card, &config);
        let fields = &note["fields"];
        let front = fields["Front"].as_str().unwrap();
        let back = fields["Back"].as_str().unwrap();
        assert!(front.contains("食べる"));
        assert!(front.contains("たべる"));
        assert!(back.contains("ご飯を食べる"));
        assert!(back.contains("TestDict · JA"));
        assert_eq!(note["tags"], json!(["vrcs"]));
    }
}
