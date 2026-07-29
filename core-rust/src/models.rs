//! 对外 API 的数据模型，与 Python 版 `app/models.py` 的 JSON 形状保持一致。

use serde::{Deserialize, Serialize};

use crate::config::{AnkiConfig, AsrConfig, AudioConfig, ServerConfig, StorageConfig, VadConfig};

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
/// server/storage/audio/asr 为必填（与 Python 的 SettingsUpdate 一致），vad/anki 可省略。
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
    pub anki: AnkiConfig,
}

#[derive(Debug, Clone, Deserialize)]
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
}

pub fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}
