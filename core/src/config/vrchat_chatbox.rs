use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VrchatChatboxConfig {
    #[serde(default)]
    pub enabled: bool,
}
