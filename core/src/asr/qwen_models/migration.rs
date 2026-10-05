use std::path::{Path, PathBuf};

use super::package::{
    check_owned_directory, is_installed, managed_parent, package_dir, verify_asset,
};
use super::PackageSpec;

struct Transfer {
    spec: PackageSpec,
    source: PathBuf,
    destination: PathBuf,
    created: bool,
}

pub(crate) struct QwenMigration {
    transfers: Vec<Transfer>,
}

impl QwenMigration {
    pub(crate) fn rollback(self) -> Result<(), String> {
        for transfer in self.transfers.iter().rev().filter(|item| item.created) {
            remove_package(&transfer.destination, transfer.spec)?;
        }
        Ok(())
    }

    pub(crate) fn commit(self) -> Result<(), String> {
        for transfer in &self.transfers {
            remove_package(&transfer.source, transfer.spec)?;
            let _ = std::fs::remove_dir(transfer.source.parent().unwrap());
        }
        Ok(())
    }
}

pub(crate) fn prepare_migration(
    source_root: &Path,
    target_root: &Path,
    specs: &[PackageSpec],
) -> Result<QwenMigration, String> {
    let mut migration = QwenMigration {
        transfers: Vec::new(),
    };
    let sources = discover_sources(source_root, specs)?;
    for (spec, source) in sources {
        let destination = package_dir(target_root, spec);
        let result = prepare_one(target_root, spec, &source, &destination);
        match result {
            Ok(created) => migration.transfers.push(Transfer {
                spec,
                source,
                destination,
                created,
            }),
            Err(error) => {
                if let Err(rollback_error) = migration.rollback() {
                    return Err(format!(
                        "{error}; Qwen migration rollback failed: {rollback_error}"
                    ));
                }
                return Err(error);
            }
        }
    }
    Ok(migration)
}

fn discover_sources(
    root: &Path,
    specs: &[PackageSpec],
) -> Result<Vec<(PackageSpec, PathBuf)>, String> {
    let managed = root.join("qwen-asr");
    let metadata = match std::fs::symlink_metadata(&managed) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Qwen ASR model root is not a managed directory".into());
    }
    let mut sources = Vec::new();
    for package_entry in std::fs::read_dir(&managed).map_err(|error| error.to_string())? {
        let package_entry = package_entry.map_err(|error| error.to_string())?;
        let id = package_entry.file_name();
        let id = id.to_string_lossy();
        let spec = specs
            .iter()
            .copied()
            .find(|spec| spec.id == id)
            .ok_or_else(|| format!("Unknown Qwen ASR package directory: {id}"))?;
        managed_parent(root, spec, false)?;
        for revision_entry in
            std::fs::read_dir(package_entry.path()).map_err(|error| error.to_string())?
        {
            let revision_entry = revision_entry.map_err(|error| error.to_string())?;
            if revision_entry.file_name() != spec.revision {
                return Err("Unfinished or unknown Qwen ASR revision blocks migration".into());
            }
            let source = revision_entry.path();
            check_owned_directory(&source, spec, false)?;
            if !is_installed(root, spec, false)? {
                return Err(format!("Qwen ASR package {} failed verification", spec.id));
            }
            sources.push((spec, source));
        }
    }
    Ok(sources)
}

fn prepare_one(
    target_root: &Path,
    spec: PackageSpec,
    source: &Path,
    destination: &Path,
) -> Result<bool, String> {
    managed_parent(target_root, spec, true)?;
    if check_owned_directory(destination, spec, false)? {
        if is_installed(target_root, spec, false)? {
            return Ok(false);
        }
        return Err(format!(
            "Conflicting Qwen ASR package: {}",
            destination.display()
        ));
    }
    let temporary = destination.with_file_name(format!(".{}.moving", spec.revision));
    if std::fs::symlink_metadata(&temporary).is_ok() {
        return Err(format!(
            "Unfinished Qwen ASR migration exists: {}",
            temporary.display()
        ));
    }
    std::fs::create_dir(&temporary).map_err(|error| error.to_string())?;
    let result = (|| {
        for file in spec.files {
            let target_file = temporary.join(file.name);
            std::fs::copy(source.join(file.name), &target_file)
                .map_err(|error| format!("Could not copy Qwen ASR asset {}: {error}", file.name))?;
            if !verify_asset(&target_file, *file, true)? {
                return Err(format!(
                    "Qwen ASR asset copy failed verification: {}",
                    file.name
                ));
            }
        }
        std::fs::rename(&temporary, destination)
            .map_err(|error| format!("Could not activate migrated Qwen ASR package: {error}"))
    })();
    match result {
        Ok(()) => Ok(true),
        Err(error) => match remove_package(&temporary, spec) {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(format!(
                "{error}; unfinished Qwen ASR migration was preserved: {cleanup_error}"
            )),
        },
    }
}

fn remove_package(directory: &Path, spec: PackageSpec) -> Result<(), String> {
    if check_owned_directory(directory, spec, false)? {
        std::fs::remove_dir_all(directory).map_err(|error| error.to_string())?;
    }
    Ok(())
}
