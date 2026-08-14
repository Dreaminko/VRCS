use serde::{Deserialize, Serialize};

pub const DEFAULT_PROFILE_TIMEOUT_MS: u64 = 8_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiProfile {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    #[serde(default)]
    pub auth_mode: ApiAuthMode,
    #[serde(default)]
    pub is_local: bool,
    #[serde(default = "default_profile_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub headers: Vec<HttpHeaderConfig>,
}

impl Default for ApiProfile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            provider: String::new(),
            region: None,
            workspace_id: None,
            base_url: None,
            purpose: None,
            preset_id: None,
            auth_mode: ApiAuthMode::Bearer,
            is_local: false,
            timeout_ms: DEFAULT_PROFILE_TIMEOUT_MS,
            headers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiAuthMode {
    #[default]
    Bearer,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpHeaderConfig {
    pub name: String,
    pub value: String,
}

fn default_profile_timeout_ms() -> u64 {
    DEFAULT_PROFILE_TIMEOUT_MS
}

impl ApiProfile {
    pub fn requires_api_key(&self) -> bool {
        self.auth_mode == ApiAuthMode::Bearer
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ActiveApiProfiles {
    #[serde(default)]
    pub alibaba_cloud: Option<String>,
    #[serde(default)]
    pub openai: Option<String>,
}
