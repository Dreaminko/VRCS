use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VrchatContextConfig {
    pub include_in_asr_context: bool,
    pub include_in_translation_context: bool,
    pub include_world_name: bool,
}

impl Default for VrchatContextConfig {
    fn default() -> Self {
        Self {
            include_in_asr_context: false,
            include_in_translation_context: false,
            include_world_name: true,
        }
    }
}
