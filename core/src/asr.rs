//! Recognition sessions and managed local Qwen models and runtime.

mod manager;
mod migration;
mod openai_audio_transcriptions;
mod qwen_models;
mod qwen_runtime;
mod qwen_runtime_download;
mod segmented_upload;
mod session;
mod streaming;
mod transcription;
mod verification;

#[cfg(test)]
mod qwen_models_tests;
#[cfg(test)]
mod qwen_runtime_download_tests;

pub(crate) use crate::credentials::read_stored_credential;
pub use crate::credentials::{
    credential_status, delete_credential, read_credential, write_credential,
};
pub use manager::ModelManager;
pub(crate) use qwen_models::is_supported as is_supported_qwen_package;
pub(crate) use qwen_runtime::QwenRuntime;
pub(crate) use session::{prepare_managed_qwen, spawn_managed_qwen_session};
pub use session::{
    spawn_cloud_recognition_session, test_cloud_service, validate_cloud_connection,
    CloudRecognitionSession,
};
pub use streaming::{
    streaming_test_backend, test_streaming_connection, CloudEvent, LiveTranslationResult,
    SegmentationMode,
};

pub(crate) type SharedAudio = std::sync::Arc<Vec<f32>>;

pub(crate) fn share_audio(samples: Vec<f32>) -> SharedAudio {
    std::sync::Arc::new(samples)
}

#[cfg(test)]
use manager::DownloadJob;
