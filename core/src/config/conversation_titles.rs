use serde::{Deserialize, Serialize};

/// Explicit title languages and the names used in model instructions. Keep in sync with
/// the desktop settings options; "auto" is accepted in addition to these.
pub const TITLE_OUTPUT_LANGUAGES: [(&str, &str); 4] = [
    ("en-US", "English"),
    ("ja-JP", "Japanese"),
    ("zh-CN", "Simplified Chinese"),
    ("zh-Hant", "Traditional Chinese"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConversationTitleConfig {
    pub auto_generate: bool,
    pub profile_id: Option<String>,
    pub model: String,
    pub output_language: String,
}

impl Default for ConversationTitleConfig {
    fn default() -> Self {
        Self {
            auto_generate: false,
            profile_id: None,
            model: String::new(),
            output_language: "auto".into(),
        }
    }
}
