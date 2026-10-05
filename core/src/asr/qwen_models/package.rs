use std::path::{Path, PathBuf};

use super::{AssetSpec, PackageSpec};
use crate::asr::model::{file_sha256, modified_nanos, verification_path, VerificationRecord};

pub(crate) fn package_dir(root: &Path, spec: PackageSpec) -> PathBuf {
    root.join("qwen-asr").join(spec.id).join(spec.revision)
}

pub(crate) fn staging_dir(root: &Path, spec: PackageSpec) -> PathBuf {
    package_dir(root, spec).with_file_name(format!(".{}.staging", spec.revision))
}

pub(crate) fn is_installed(root: &Path, spec: PackageSpec, force: bool) -> Result<bool, String> {
    verify_package(&package_dir(root, spec), spec, force)
}

pub(crate) fn activate_staging(root: &Path, spec: PackageSpec) -> Result<(), String> {
    managed_parent(root, spec, false)?;
    let stage = staging_dir(root, spec);
    let metadata = std::fs::symlink_metadata(&stage)
        .map_err(|error| format!("Qwen ASR staging directory is unavailable: {error}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Qwen ASR staging path is not a managed directory".into());
    }
    if !verify_package(&stage, spec, false)? {
        return Err("The Qwen ASR package is incomplete or invalid".into());
    }
    check_owned_directory(&stage, spec, true)?;
    for file in spec.files {
        let partial = stage.join(format!(".{}.part", file.name));
        if partial.exists() {
            std::fs::remove_file(partial).map_err(|error| error.to_string())?;
        }
    }
    let installed = package_dir(root, spec);
    if std::fs::symlink_metadata(&installed).is_ok() {
        return Err("A Qwen ASR package already exists at the install path".into());
    }
    std::fs::rename(&stage, &installed)
        .map_err(|error| format!("Could not activate Qwen ASR package: {error}"))
}

pub(super) fn check_owned_directory(
    directory: &Path,
    spec: PackageSpec,
    allow_partials: bool,
) -> Result<bool, String> {
    let metadata = match std::fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Qwen ASR package path is not a managed directory".into());
    }
    for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let owned = spec.files.iter().any(|file| {
            name == file.name
                || name == format!("{}.vrcs-verified.json", file.name)
                || (allow_partials && name == format!(".{}.part", file.name))
        });
        if !owned
            || !entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
        {
            return Err("Qwen ASR package contains unmanaged files".into());
        }
    }
    Ok(true)
}

pub(super) fn managed_parent(root: &Path, spec: PackageSpec, create: bool) -> Result<(), String> {
    let managed = root.join("qwen-asr");
    let package = managed.join(spec.id);
    for path in [&managed, &package] {
        if create {
            match std::fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("Qwen ASR model path is not a managed directory".into());
        }
    }
    Ok(())
}

pub(super) fn verify_package(
    directory: &Path,
    spec: PackageSpec,
    force: bool,
) -> Result<bool, String> {
    if !directory.is_dir() {
        return Ok(false);
    }
    for asset in spec.files {
        if !verify_asset(&directory.join(asset.name), *asset, force)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn verify_asset(path: &Path, spec: AssetSpec, force: bool) -> Result<bool, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_file() || metadata.len() != spec.bytes {
        return Ok(false);
    }
    let modified = modified_nanos(&metadata)?;
    if !force {
        if let Ok(bytes) = std::fs::read(verification_path(path)) {
            if let Ok(record) = serde_json::from_slice::<VerificationRecord>(&bytes) {
                if record.bytes == spec.bytes
                    && record.modified_nanos == modified
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
    cache_verified_asset(path, spec, digest)?;
    Ok(true)
}

pub(super) fn cache_verified_asset(
    path: &Path,
    spec: AssetSpec,
    digest: String,
) -> Result<(), String> {
    let metadata = path.metadata().map_err(|error| error.to_string())?;
    if metadata.len() != spec.bytes || digest != spec.sha256 {
        return Err(format!("Qwen ASR asset failed verification: {}", spec.name));
    }
    let record = VerificationRecord {
        bytes: spec.bytes,
        modified_nanos: modified_nanos(&metadata)?,
        sha256: digest,
    };
    let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    if let Err(error) = std::fs::write(verification_path(path), bytes) {
        tracing::warn!(file = %path.display(), %error, "unable to cache Qwen ASR asset verification");
    }
    Ok(())
}
