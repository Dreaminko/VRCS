use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    u64::try_from(nanos).map_err(|_| "Model file modification time is out of range".to_string())
}

pub(super) fn file_sha256(path: &Path) -> Result<String, String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("Failed to open model file {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Failed to read model file {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
