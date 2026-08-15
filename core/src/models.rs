//! 对外 API 的数据模型，与 Python 版 `app/models.py` 的 JSON 形状保持一致。

use serde::{Deserialize, Serialize};

use crate::config::{
    AnkiConfig, AsrConfig, AudioConfig, DictionaryConfig, ExternalApiConfig, OscConfig,
    ServerConfig, StorageConfig, TranslationConfig, VadConfig,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtitle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub text: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub started_at: Option<f64>,
    #[serde(default)]
    pub ended_at: Option<f64>,
    #[serde(default = "default_source")]
    pub source: String,
    pub created_at: String,
    #[serde(default)]
    pub translations: Vec<SubtitleTranslation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubtitleTranslation {
    pub text: String,
    pub source_language: Option<String>,
    pub target_language: String,
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    pub created_at: String,
}

fn default_source() -> String {
    "speaker".into()
}

/// 音频设备枚举结果，供后续阶段的 /api/audio/devices 使用。
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub struct AudioDevice {
    pub id: i64,
    pub name: String,
    pub is_default: bool,
    pub is_loopback: bool,
    pub sample_rate: u32,
    pub channels: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryEntry {
    pub term: String,
    pub language: String,
    pub definition: String,
    #[serde(default)]
    pub reading: Option<String>,
    #[serde(default)]
    pub dictionary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DictionarySource {
    pub id: i64,
    pub title: String,
    pub revision: String,
    pub source_language: String,
    pub target_language: Option<String>,
    pub entry_count: i64,
    pub imported_at: String,
}

/// PUT /api/settings 的请求体。
/// server/storage/audio/asr 为必填，vad/dictionary/anki 可省略。
#[derive(Debug, Deserialize)]
pub struct SettingsUpdate {
    pub schema_version: u32,
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub audio: AudioConfig,
    #[serde(default)]
    pub vad: VadConfig,
    pub asr: AsrConfig,
    #[serde(default)]
    pub dictionary: DictionaryConfig,
    #[serde(default)]
    pub translation: TranslationConfig,
    #[serde(default)]
    pub osc: OscConfig,
    #[serde(default)]
    pub anki: AnkiConfig,
    #[serde(default)]
    pub external_api: ExternalApiConfig,
    #[serde(default)]
    pub vrcx: crate::config::VrcxConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardLabels {
    pub definition: String,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LiveTranscription {
    Partial {
        utterance_id: String,
        source: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
    },
    Failed {
        source: String,
        code: String,
        detail: String,
    },
    AudioLevel {
        source: String,
        rms_dbfs: f32,
        peak_dbfs: f32,
        speech: bool,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardRequest {
    pub term: String,
    pub definition: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub reading: Option<String>,
    #[serde(default)]
    pub dictionary: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub labels: Option<CardLabels>,
}

impl CardRequest {
    pub fn validate(&self) -> Result<(), String> {
        validate_text("term", &self.term, 1, 500)?;
        validate_text("definition", &self.definition, 1, 20_000)?;
        validate_text("context", &self.context, 0, 20_000)?;
        validate_optional_text("reading", self.reading.as_deref(), 500)?;
        validate_optional_text("dictionary", self.dictionary.as_deref(), 500)?;
        validate_optional_text("language", self.language.as_deref(), 20)?;
        if let Some(labels) = &self.labels {
            validate_text("definition label", &labels.definition, 1, 100)?;
            validate_text("context label", &labels.context, 1, 100)?;
        }
        Ok(())
    }
}

fn validate_text(label: &str, value: &str, minimum: usize, maximum: usize) -> Result<(), String> {
    let length = value.chars().count();
    if !(minimum..=maximum).contains(&length) {
        return Err(format!(
            "{label} length must be between {minimum} and {maximum} characters"
        ));
    }
    Ok(())
}

fn validate_optional_text(label: &str, value: Option<&str>, maximum: usize) -> Result<(), String> {
    if value.is_some_and(|value| value.chars().count() > maximum) {
        return Err(format!("{label} cannot exceed {maximum} characters"));
    }
    Ok(())
}

pub fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> CardRequest {
        CardRequest {
            term: "学ぶ".into(),
            definition: "学习".into(),
            context: String::new(),
            reading: None,
            dictionary: None,
            language: None,
            labels: None,
        }
    }

    #[test]
    fn card_limits_count_unicode_characters() {
        let mut card = card();
        card.term = "学".repeat(500);
        assert!(card.validate().is_ok());
        card.term.push('ぶ');
        assert!(card.validate().is_err());
    }

    #[test]
    fn card_rejects_unknown_fields_and_oversized_content() {
        let unknown = serde_json::from_value::<CardRequest>(serde_json::json!({
            "term": "hello",
            "definition": "greeting",
            "typo": true
        }));
        assert!(unknown.is_err());

        let mut card = card();
        card.definition = "x".repeat(20_001);
        assert!(card.validate().is_err());
    }
}
