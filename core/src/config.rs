//! 服务配置类型、默认值与结构校验。
//! 与 Python 版 `app/config.py` 行为保持一致。

mod audio;
mod integrations;
mod io;
mod migration;
mod profile;
mod recognition;
mod runtime;
mod schema;
mod translation;
mod validation;
mod vr_overlay;

#[cfg(test)]
mod io_tests;
#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod validation_tests;

pub use audio::{AudioConfig, MicrophoneConfig, OutputConfig, VadConfig};
pub use integrations::{AnkiConfig, DictionaryConfig, OscConfig};
pub use io::{load_config, save_config};
pub use profile::{ApiAuthMode, ApiProfile, HttpHeaderConfig, DEFAULT_PROFILE_TIMEOUT_MS};
#[allow(unused_imports)]
pub use recognition::{
    default_service_settings, AsrConfig, LocalAsrConfig, RecognitionServiceSettings,
};
pub use runtime::{ExternalApiConfig, ServerConfig, StorageConfig, VrcxConfig};
pub use schema::{AppConfig, SCHEMA_VERSION};
#[allow(unused_imports)]
pub use translation::{
    GlossaryCategory, GlossaryEntry, GlossarySource, TranslationConfig, TranslationPromptConfig,
    DEFAULT_TRANSLATION_SYSTEM_PROMPT,
};
pub use validation::{validate_glossary_source_url, validate_translation_prompt};
pub use vr_overlay::{VrOverlayConfig, VrOverlayHeadsetConfig, VrOverlayWristConfig};
