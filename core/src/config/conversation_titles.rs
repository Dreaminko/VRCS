use serde::{Deserialize, Serialize};

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
