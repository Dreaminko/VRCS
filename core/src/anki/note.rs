use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Value};

use crate::config::AnkiConfig;
use crate::models::CardRequest;

const FONT_STACK: &str = "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', \
    'Hiragino Sans GB', 'Noto Sans CJK SC', sans-serif";

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

fn definition_html(value: &str) -> String {
    let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = normalized.trim();
    let block_splitter = Regex::new(r"\n\s*\n").unwrap();
    let mut rendered = String::new();
    for raw_block in block_splitter.split(normalized) {
        let lines: Vec<&str> = raw_block
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
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

pub(super) fn build_note(card: &CardRequest, config: &AnkiConfig) -> Value {
    let definition_label = card
        .labels
        .as_ref()
        .map(|labels| labels.definition.as_str())
        .unwrap_or("释义");
    let context_label = card
        .labels
        .as_ref()
        .map(|labels| labels.context.as_str())
        .unwrap_or("语境");
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
        section_label(definition_label),
        definition_html(&card.definition)
    );
    if !card.context.is_empty() {
        back.push_str(&format!(
            r#"<section class="vrcs-context" style="margin-top:1.45rem;padding-top:1rem;border-top:1px solid rgba(127,127,127,0.24);">{}<div class="vrcs-context-content" style="padding:0.82rem 0.95rem;border-radius:0.65rem;background:rgba(61,115,168,0.08);font-size:0.95rem;line-height:1.7;">{}</div></section>"#,
            section_label(context_label),
            escaped_lines(&card.context)
        ));
    }
    let metadata = [
        card.dictionary.clone(),
        card.language
            .as_ref()
            .map(|language| language.to_uppercase()),
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
            labels: Some(crate::models::CardLabels {
                definition: "Definition".into(),
                context: "Context".into(),
            }),
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
        assert!(back.contains("Definition"));
        assert!(back.contains("Context"));
        assert_eq!(note["tags"], json!(["vrcs"]));
    }
}
