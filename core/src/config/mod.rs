//! Service configuration types, defaults, and structural validation.

mod audio;
mod conversation_titles;
mod features;
mod glossary;
mod integrations;
mod io;
mod language;
mod migration;
mod model_assignment;
mod ocr;
mod profile;
mod recognition;
mod runtime;
mod schema;
mod translation;
mod validation;
mod vr_overlay;
mod vrchat_chatbox;
mod vrchat_context;

#[cfg(test)]
mod io_tests;
#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod validation_tests;

pub use audio::{
    AudioConfig, MicrophoneConfig, OutputConfig, PlayerAudioConfig, PlayerProximityConfig,
    PlayerSourceOverride, PlayerViewPriorityConfig, VadConfig,
};
pub use conversation_titles::{ConversationTitleConfig, TITLE_OUTPUT_LANGUAGES};
pub use features::{FeatureConfig, FeatureKey, apply_feature_gates};
pub use glossary::{GlossaryCategory, GlossaryConfig, GlossaryEntry, GlossarySource};
pub use integrations::{AnkiConfig, DictionaryConfig, OscConfig, UniTextConfig};
pub use io::{load_config, save_config};
pub use language::LanguagePreset;
#[cfg(test)]
pub use model_assignment::ModelBinding;
pub use model_assignment::{LearningAiConfig, ModelDefaultsConfig, ModelSelection};
pub use profile::{ApiAuthMode, ApiProfile, DEFAULT_PROFILE_TIMEOUT_MS, HttpHeaderConfig};
pub use recognition::{
    AsrConfig, ManagedQwenConfig, QWEN_MANAGED_BACKEND, RecognitionServiceSettings,
    default_service_settings,
};
pub use runtime::{
    ExternalApiConfig, ModelDownloadSource, ServerConfig, StorageConfig, VrcxConfig,
};
pub use schema::{AppConfig, SCHEMA_VERSION};
#[cfg(test)]
pub use translation::DEFAULT_TRANSLATION_SYSTEM_PROMPT;
pub use translation::{
    LocalTranslationConfig, ReasoningSetting, TranslationBackend, TranslationConfig,
    TranslationPromptConfig, TranslationTargetConfig,
};
pub use validation::{validate_glossary_source_url, validate_translation_prompt};
pub use vr_overlay::{
    VrOverlayConfig, VrOverlayHeadsetConfig, VrOverlayWristConfig, VrOverlayWristMenuConfig,
};
pub use vrchat_chatbox::VrchatChatboxConfig;
pub use vrchat_context::VrchatContextConfig;

pub use ocr::{OcrDevice, VisionConfig, VrOcrConfig, VrOcrDisplayMode, VrOcrWristConfig};
