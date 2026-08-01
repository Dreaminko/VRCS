//! whisper.cpp 本地识别、GGML 模型状态与下载管理。

mod credentials;
mod cuda;
mod download;
mod engine;
mod manager;
mod migration;
mod model;
mod streaming;

pub use credentials::{credential_status, delete_credential, read_credential, write_credential};
pub use cuda::cuda_capability;
pub use engine::{AsrRuntimeState, AsrService};
pub use manager::ModelManager;
pub use model::is_supported_model;
pub use streaming::{
    spawn_streaming_session, test_streaming_connection, CloudEvent, StreamingSession,
};

use crate::config::AsrConfig;
use model::model_spec;

pub fn validate_config(config: &AsrConfig) -> Result<(), String> {
    model_spec(&config.local.model)?;
    let local_required =
        config.backend == "local_whisper" || config.cloud_failure_policy == "local";
    if local_required && config.local.device == "cuda" {
        if !cfg!(feature = "cuda") {
            return Err("当前构建未包含 CUDA 后端".into());
        }
        let capability = cuda_capability();
        if !capability.available {
            return Err(format!(
                "CUDA 预检失败：{}",
                capability.error.unwrap_or_else(|| "CUDA 不可用".into())
            ));
        }
    }
    if config.local.compute_type != "int8" {
        return Err("Rust Core 的 whisper.cpp 后端当前仅接受 int8 兼容配置".into());
    }
    Ok(())
}

#[cfg(test)]
use engine::WhisperEngine;
#[cfg(test)]
pub(crate) use engine::{AsrEngine, Transcription};
#[cfg(test)]
use manager::DownloadJob;
#[cfg(test)]
pub(crate) use model::cache_model_verification_for_test;
#[cfg(test)]
use model::{
    file_sha256, model_spec as test_model_spec, validate_download, verification_path,
    verify_model_file, ModelSpec, VerificationRecord, MODEL_BASE_URL, MODEL_REVISION,
};

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use tokio::sync::{watch, Notify};

    use super::*;

    #[test]
    fn validates_supported_configuration() {
        let config = AsrConfig {
            backend: "local_whisper".into(),
            language: "auto".into(),
            local: crate::config::LocalAsrConfig {
                model: "small".into(),
                device: "cpu".into(),
                compute_type: "int8".into(),
            },
            ..AsrConfig::default()
        };
        assert!(validate_config(&config).is_ok());

        let mut cuda = config.clone();
        cuda.local.device = "cuda".into();
        if !cfg!(feature = "cuda") {
            assert!(validate_config(&cuda).unwrap_err().contains("未包含 CUDA"));
        } else if cuda_capability().available {
            assert!(validate_config(&cuda).is_ok());
        } else {
            assert!(validate_config(&cuda)
                .unwrap_err()
                .contains("CUDA 预检失败"));
        }
    }

    #[test]
    fn model_manager_reports_local_models() {
        let directory = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(directory.path().to_path_buf()).unwrap();
        let tiny_spec = test_model_spec("tiny").unwrap();
        let file = std::fs::File::create(directory.path().join(tiny_spec.filename)).unwrap();
        file.set_len(tiny_spec.expected_bytes).unwrap();

        let tiny = manager.describe("tiny", "small", "not_loaded").unwrap();
        assert_eq!(tiny.status, "downloaded");
        assert_eq!(tiny.progress, 1.0);
        assert_eq!(tiny.downloaded_bytes, tiny_spec.expected_bytes);

        let small = manager.describe("small", "small", "not_loaded").unwrap();
        assert_eq!(small.status, "not_downloaded");
        assert!(small.active);
    }

    struct FakeEngine;

    impl AsrEngine for FakeEngine {
        fn transcribe(
            &mut self,
            _samples: &[f32],
            language: Option<&str>,
        ) -> Result<Transcription, String> {
            Ok(Transcription {
                text: "hello".into(),
                language: language.map(str::to_owned),
            })
        }
    }

    #[test]
    fn service_uses_engine_abstraction_and_resets_on_update() {
        let config = AsrConfig {
            language: "ja".into(),
            local: crate::config::LocalAsrConfig {
                model: "small".into(),
                device: "cpu".into(),
                compute_type: "int8".into(),
            },
            ..AsrConfig::default()
        };
        let mut service = AsrService::new(config.clone(), PathBuf::new());
        service.engine = Some(Box::new(FakeEngine));
        service.runtime.set("error", Some("old error".into()));

        let result = service.transcribe(&[0.0; 512]).unwrap();
        assert_eq!(result.language.as_deref(), Some("ja"));
        assert_eq!(service.runtime.snapshot(), ("ready", None));

        let mut changed = config;
        changed.local.model = "tiny".into();
        service.update(changed, PathBuf::new());
        assert_eq!(service.runtime.snapshot(), ("not_loaded", None));
        assert!(service.engine.is_none());
    }

    #[test]
    fn service_resets_when_model_directory_changes() {
        let config = AsrConfig::default();
        let mut service = AsrService::with_engine(config.clone(), Box::new(FakeEngine));

        service.update(config, PathBuf::from("custom-models"));

        assert_eq!(service.runtime.snapshot(), ("not_loaded", None));
        assert!(service.engine.is_none());
    }

    #[test]
    fn model_manager_switches_directories() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(first.path().to_path_buf()).unwrap();
        let tiny_spec = test_model_spec("tiny").unwrap();
        let source = first.path().join(tiny_spec.filename);
        let file = std::fs::File::create(&source).unwrap();
        file.set_len(tiny_spec.expected_bytes).unwrap();
        cache_model_verification_for_test(&source, "tiny");
        let destination_dir = second.path().join("custom-models");

        manager.move_model_dir(destination_dir.clone()).unwrap();

        assert_eq!(manager.model_dir(), destination_dir);
        assert!(!source.exists());
        assert!(manager.model_dir().join(tiny_spec.filename).is_file());
        assert!(manager.is_downloaded("tiny").unwrap());
    }

    #[test]
    fn model_manager_keeps_the_old_directory_when_the_target_conflicts() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(first.path().to_path_buf()).unwrap();
        let tiny_spec = test_model_spec("tiny").unwrap();
        let source = first.path().join(tiny_spec.filename);
        let source_file = std::fs::File::create(&source).unwrap();
        source_file.set_len(tiny_spec.expected_bytes).unwrap();
        cache_model_verification_for_test(&source, "tiny");
        let destination = second.path().join(tiny_spec.filename);
        std::fs::write(&destination, b"incomplete").unwrap();

        let error = manager
            .move_model_dir(second.path().to_path_buf())
            .unwrap_err();

        assert!(error.contains("不完整"));
        assert_eq!(manager.model_dir(), first.path());
        assert!(source.exists());
        assert_eq!(std::fs::read(destination).unwrap(), b"incomplete");
    }

    #[test]
    fn runtime_status_does_not_require_the_inference_lock() {
        let service = AsrService::with_engine(AsrConfig::default(), Box::new(FakeEngine));
        let runtime = service.runtime_state();
        let service = Mutex::new(service);
        let _inference_guard = service.lock().unwrap();

        assert_eq!(runtime.snapshot(), ("ready", None));
    }

    #[tokio::test]
    async fn model_manager_protects_active_model() {
        let directory = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(directory.path().to_path_buf()).unwrap();
        let error = manager.delete("small", "small").await.unwrap_err();
        assert!(error.contains("当前正在使用"));
    }

    #[tokio::test]
    async fn deleting_an_active_download_cancels_it() {
        let directory = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(directory.path().to_path_buf()).unwrap();
        let (cancel, mut cancelled) = watch::channel(false);
        let done = Arc::new(Notify::new());
        manager.jobs.lock().unwrap().insert(
            "small".into(),
            DownloadJob {
                status: "downloading",
                downloaded_bytes: 10,
                total_bytes: test_model_spec("small").unwrap().expected_bytes,
                error: None,
                cancel,
                done: Arc::clone(&done),
            },
        );
        tokio::spawn(async move {
            cancelled.changed().await.unwrap();
            done.notify_one();
        });

        manager.delete("small", "small").await.unwrap();

        assert!(!manager.jobs.lock().unwrap().contains_key("small"));
    }

    #[test]
    fn model_manager_allows_only_one_download_at_a_time() {
        let directory = tempfile::tempdir().unwrap();
        let manager = Arc::new(ModelManager::new(directory.path().to_path_buf()).unwrap());
        let (cancel, _) = watch::channel(false);
        manager.jobs.lock().unwrap().insert(
            "tiny".into(),
            DownloadJob {
                status: "downloading",
                downloaded_bytes: 10,
                total_bytes: test_model_spec("tiny").unwrap().expected_bytes,
                error: None,
                cancel,
                done: Arc::new(Notify::new()),
            },
        );

        assert!(manager
            .start_download("small")
            .unwrap_err()
            .contains("已有模型下载任务"));
    }

    #[test]
    fn download_validation_rejects_wrong_size_or_hash() {
        let spec = test_model_spec("tiny").unwrap();
        assert!(validate_download(spec, spec.expected_bytes, spec.sha256).is_ok());
        assert!(validate_download(spec, spec.expected_bytes - 1, spec.sha256).is_err());
        assert!(validate_download(spec, spec.expected_bytes, &"0".repeat(64)).is_err());
        assert!(
            format!("{MODEL_BASE_URL}/{MODEL_REVISION}/{}", spec.filename).contains(MODEL_REVISION)
        );
    }

    #[test]
    fn model_verification_detects_same_size_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("model.bin");
        std::fs::write(&path, b"good").unwrap();
        let digest = file_sha256(&path).unwrap();
        let digest: &'static str = Box::leak(digest.into_boxed_str());
        let spec = ModelSpec {
            id: "test",
            filename: "model.bin",
            expected_bytes: 4,
            sha256: digest,
        };
        assert!(verify_model_file(&path, spec, false).unwrap());

        std::fs::write(&path, b"evil").unwrap();
        let mut cached: VerificationRecord =
            serde_json::from_slice(&std::fs::read(verification_path(&path)).unwrap()).unwrap();
        cached.modified_nanos = cached.modified_nanos.saturating_sub(1);
        std::fs::write(
            verification_path(&path),
            serde_json::to_vec(&cached).unwrap(),
        )
        .unwrap();

        assert!(!verify_model_file(&path, spec, false).unwrap());
    }

    #[test]
    #[ignore = "需要通过 VRCS_WHISPER_MODEL 指定真实 GGML 模型"]
    fn whisper_model_transcribes_when_provided() {
        let path = std::env::var("VRCS_WHISPER_MODEL").expect("VRCS_WHISPER_MODEL");
        let mut engine = WhisperEngine::load(Path::new(&path), "cpu").unwrap();
        let result = engine.transcribe(&vec![0.0; 16_000], None).unwrap();
        assert!(result.text.is_empty());
    }

    #[test]
    #[ignore = "需要通过 VRCS_WHISPER_MODEL 和 VRCS_WHISPER_WAV 指定真实文件"]
    fn whisper_model_transcribes_wav_when_provided() {
        let model = std::env::var("VRCS_WHISPER_MODEL").expect("VRCS_WHISPER_MODEL");
        let wav = std::env::var("VRCS_WHISPER_WAV").expect("VRCS_WHISPER_WAV");
        let device = std::env::var("VRCS_WHISPER_DEVICE").unwrap_or_else(|_| "cpu".into());
        let mut reader = hound::WavReader::open(wav).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.bits_per_sample, 16);
        let samples = reader
            .samples::<i16>()
            .map(|sample| f32::from(sample.unwrap()) / 32_768.0)
            .collect::<Vec<_>>();
        let mut engine = WhisperEngine::load(Path::new(&model), &device).unwrap();

        let result = engine.transcribe(&samples, None).unwrap();

        assert!(
            !result.text.is_empty(),
            "Whisper auto-language mode returned no text"
        );
    }
}
