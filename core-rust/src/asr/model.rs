use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(super) const MODEL_REPOSITORY: &str = "ggerganov/whisper.cpp";
pub(super) const MODEL_REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
pub(super) const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve";

#[derive(Clone, Copy)]
pub(super) struct ModelSpec {
    pub(super) id: &'static str,
    pub(super) filename: &'static str,
    pub(super) expected_bytes: u64,
    pub(super) sha256: &'static str,
}

pub(super) const MODELS: [ModelSpec; 5] = [
    ModelSpec {
        id: "tiny",
        filename: "ggml-tiny.bin",
        expected_bytes: 77_691_713,
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
    },
    ModelSpec {
        id: "base",
        filename: "ggml-base.bin",
        expected_bytes: 147_951_465,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
    },
    ModelSpec {
        id: "small",
        filename: "ggml-small.bin",
        expected_bytes: 487_601_967,
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
    },
    ModelSpec {
        id: "medium",
        filename: "ggml-medium.bin",
        expected_bytes: 1_533_763_059,
        sha256: "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
    },
    ModelSpec {
        id: "large-v3",
        filename: "ggml-large-v3.bin",
        expected_bytes: 3_095_033_483,
        sha256: "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
    },
];

pub(super) fn model_spec(model: &str) -> Result<ModelSpec, String> {
    MODELS
        .iter()
        .copied()
        .find(|spec| spec.id == model)
        .ok_or_else(|| format!("不支持的识别模型：{model}"))
}

pub fn is_supported_model(model: &str) -> bool {
    MODELS.iter().any(|spec| spec.id == model)
}

#[derive(Deserialize, Serialize)]
pub(super) struct VerificationRecord {
    pub(super) bytes: u64,
    pub(super) modified_nanos: u64,
    pub(super) sha256: String,
}

pub(super) fn verification_path(model_path: &Path) -> PathBuf {
    let mut path = model_path.as_os_str().to_os_string();
    path.push(".vrcs-verified.json");
    PathBuf::from(path)
}

pub(super) fn modified_nanos(metadata: &std::fs::Metadata) -> Result<u64, String> {
    let nanos = metadata
        .modified()
        .map_err(|error| error.to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    u64::try_from(nanos).map_err(|_| "模型文件修改时间超出支持范围".to_string())
}

pub(super) fn file_sha256(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("无法打开模型文件 {}：{error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("无法读取模型文件 {}：{error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(super) fn record_verification(
    path: &Path,
    spec: ModelSpec,
    digest: &str,
) -> Result<(), String> {
    let metadata = path.metadata().map_err(|error| error.to_string())?;
    validate_download(spec, metadata.len(), digest)?;
    let record = VerificationRecord {
        bytes: metadata.len(),
        modified_nanos: modified_nanos(&metadata)?,
        sha256: digest.to_owned(),
    };
    let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    std::fs::write(verification_path(path), bytes).map_err(|error| error.to_string())
}

pub(super) fn verify_model_file(path: &Path, spec: ModelSpec, force: bool) -> Result<bool, String> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() || metadata.len() != spec.expected_bytes {
        return Ok(false);
    }
    let modified_nanos = modified_nanos(&metadata)?;
    if !force {
        if let Ok(bytes) = std::fs::read(verification_path(path)) {
            if let Ok(record) = serde_json::from_slice::<VerificationRecord>(&bytes) {
                if record.bytes == metadata.len()
                    && record.modified_nanos == modified_nanos
                    && record.sha256 == spec.sha256
                {
                    return Ok(true);
                }
            }
        }
    }

    let digest = file_sha256(path)?;
    if digest != spec.sha256 {
        let _ = std::fs::remove_file(verification_path(path));
        return Ok(false);
    }
    if let Err(error) = record_verification(path, spec, &digest) {
        tracing::warn!(model = spec.id, %error, "unable to cache model verification");
    }
    Ok(true)
}

#[cfg(test)]
pub(crate) fn cache_model_verification_for_test(path: &Path, model: &str) {
    let spec = model_spec(model).unwrap();
    record_verification(path, spec, spec.sha256).unwrap();
}

pub(super) fn validate_download(
    spec: ModelSpec,
    downloaded: u64,
    sha256: &str,
) -> Result<(), String> {
    if downloaded != spec.expected_bytes {
        return Err(format!(
            "模型文件大小不符：应为 {} 字节，实际为 {downloaded} 字节",
            spec.expected_bytes
        ));
    }
    if sha256 != spec.sha256 {
        return Err("模型文件 SHA-256 校验失败".into());
    }
    Ok(())
}
