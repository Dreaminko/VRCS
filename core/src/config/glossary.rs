use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlossaryConfig {
    #[serde(default = "default_enabled")]
    pub llm_enabled: bool,
    #[serde(default = "default_enabled")]
    pub asr_enabled: bool,
    #[serde(default)]
    pub sources: Vec<GlossarySource>,
}

impl Default for GlossaryConfig {
    fn default() -> Self {
        Self {
            llm_enabled: true,
            asr_enabled: true,
            sources: Vec::new(),
        }
    }
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GlossarySource {
    Local {
        id: String,
        name: String,
        enabled: bool,
        entries: Vec<GlossaryEntry>,
    },
    Subscription {
        id: String,
        url: String,
        display_name: Option<String>,
        enabled: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlossaryEntry {
    pub source: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub category: GlossaryCategory,
    #[serde(default)]
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlossaryCategory {
    Person,
    World,
    Game,
    #[default]
    Custom,
}
