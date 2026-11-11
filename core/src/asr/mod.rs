//! Recognition sessions and managed local Qwen models and runtime.

mod manager;
mod migration;
mod mimo;
mod openai_audio_transcriptions;
mod question;
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

pub use crate::credentials::{
    credential_status, delete_credential, read_credential, write_credential,
};
pub use manager::ModelManager;
pub(crate) use migration::paths_equivalent;
pub(crate) use question::{
    recognize_live_question, validate_backend as validate_question_backend, QuestionDraft,
};
pub(crate) use qwen_models::is_supported as is_supported_qwen_package;
pub(crate) use qwen_models::{
    download_file as download_model_file, verify_file as verify_model_file,
    verify_file_forced as verify_model_file_forced,
    verify_file_readonly as verify_model_file_readonly, AssetSpec as ModelAssetSpec,
};
pub(crate) use qwen_runtime::QwenRuntime;
pub(crate) use session::{prepare_managed_qwen, spawn_managed_qwen_session};
pub use session::{
    spawn_cloud_recognition_session, test_cloud_service, validate_cloud_connection,
    CloudRecognitionSession,
};
#[cfg(any(windows, target_os = "linux"))]
pub(crate) use streaming::connection_rate_controlled;
pub use streaming::{
    streaming_test_backend, test_streaming_connection, CloudEvent, LiveTranslationResult,
    SegmentationMode,
};

pub(crate) type SharedAudio = std::sync::Arc<Vec<f32>>;

pub(crate) fn share_audio(samples: Vec<f32>) -> SharedAudio {
    std::sync::Arc::new(samples)
}

pub(crate) fn llama_server_executable_path(
    runtime_directory: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    QwenRuntime::new(runtime_directory.to_path_buf()).executable_path()
}

#[cfg(test)]
use manager::DownloadJob;
