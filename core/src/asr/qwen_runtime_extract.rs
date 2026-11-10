use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Component, Path};
use sha2::{Digest, Sha256};

use super::assets::{ArchiveFormat, RuntimeAsset};

const MAX_EXTRACTED_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ENTRIES: usize = 256;

pub(in crate::asr) fn installed(directory: &Path, asset: &RuntimeAsset) -> bool {
    if !directory.symlink_metadata().is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
        return false;
    }
    let Ok(root) = directory.canonicalize() else { return false; };
    std::fs::read_to_string(directory.join("version.txt"))
        .is_ok_and(|value| value.trim() == asset.archive.sha256)
        && asset.required_files().all(|name| {
            let path = directory.join(name);
            path.canonicalize().is_ok_and(|path| path.starts_with(&root))
                && path.metadata().is_ok_and(|file| file.is_file() && file.len() > 0)
        })
        && executable_valid(&directory.join(asset.executable), asset)
}

fn executable_valid(path: &Path, asset: &RuntimeAsset) -> bool {
    if asset.os != "linux" { return true; }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !path.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0) { return false; }
    }
    elf_valid(path, asset).is_ok()
}

fn elf_valid(path: &Path, asset: &RuntimeAsset) -> Result<(), String> {
    let mut header = [0u8; 20];
    std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| format!("Could not read runtime ELF {}: {error}", path.display()))?;
    let machine = match asset.arch { "x86_64" => 62, "aarch64" => 183, _ => 0 };
    if &header[..4] != b"\x7fELF" || header[4] != 2 || header[5] != 1
        || u16::from_le_bytes([header[18], header[19]]) != machine {
        return Err(format!("Runtime ELF architecture mismatch: {}", path.display()));
    }
    Ok(())
}

pub(in crate::asr) fn extract_archive(archive: &Path, directory: &Path, asset: &RuntimeAsset) -> Result<(), String> {
    match asset.format {
        ArchiveFormat::Zip => extract_zip(archive, directory, asset)?,
        ArchiveFormat::TarGz => extract_tar(archive, directory, asset)?,
    }
    for name in asset.required_files() {
        if !directory.join(name).metadata().is_ok_and(|m| m.is_file() && m.len() > 0) {
            return Err(format!("Qwen runtime archive is missing required file: {name}"));
        }
    }
    for license in asset.licenses {
        let bytes = std::fs::read(directory.join(license.name)).map_err(|e| e.to_string())?;
        if bytes.len() as u64 != license.bytes || format!("{:x}", Sha256::digest(&bytes)) != license.sha256 {
            return Err(format!("Qwen runtime license failed verification: {}", license.name));
        }
    }
    std::fs::write(directory.join("version.txt"), asset.archive.sha256).map_err(|e| e.to_string())
}

fn add_size(total: &mut u64, bytes: u64) -> Result<(), String> {
    *total = total.checked_add(bytes).filter(|n| *n <= MAX_EXTRACTED_BYTES)
        .ok_or("Qwen runtime archive exceeds its extraction size limit")?;
    Ok(())
}

fn extract_zip(archive: &Path, directory: &Path, asset: &RuntimeAsset) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(archive).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if archive.len() > MAX_ENTRIES { return Err("Too many runtime archive entries".into()); }
    let mut total = 0;
    let mut seen = HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        add_size(&mut total, entry.size())?;
        if !asset.runtime_file(entry.name()) || entry.name() == "LICENSE" { continue; }
        if entry.is_symlink() || !seen.insert(entry.name().to_owned()) {
            return Err("Duplicate or symbolic runtime ZIP entry".into());
        }
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true)
            .open(directory.join(entry.name())).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// The pinned tar archives have one fixed root and flat members. Do not delegate
// path handling, hard links, devices or permissions to tar::Entry::unpack.
fn tar_name(path: &Path) -> Result<Option<&str>, String> {
    let mut parts = path.components();
    if parts.next() != Some(Component::Normal(std::ffi::OsStr::new("llama-b11501"))) {
        return Err("Invalid runtime archive root".into());
    }
    let Some(Component::Normal(name)) = parts.next() else {
        if parts.next().is_none() && path == Path::new("llama-b11501") { return Ok(None); }
        return Err("Invalid runtime archive path".into());
    };
    if parts.next().is_some() { return Err("Nested runtime archive path is not allowed".into()); }
    let name = name.to_str().ok_or("Non-UTF8 runtime archive path")?;
    if name.contains(['/', '\\', ':']) { return Err("Invalid runtime archive filename".into()); }
    Ok(Some(name))
}

fn extract_tar(archive: &Path, directory: &Path, asset: &RuntimeAsset) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    // Bound the decompressed stream as well as declared file sizes (including
    // skipped entries and metadata); malformed padding cannot grow unbounded.
    let gzip = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gzip.take(MAX_EXTRACTED_BYTES + 1024 * 1024));
    let mut total = 0;
    let mut seen = HashSet::new();
    let mut links = HashMap::new();
    for (index, entry) in archive.entries().map_err(|e| e.to_string())?.raw(true).enumerate() {
        if index >= MAX_ENTRIES { return Err("Too many runtime archive entries".into()); }
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        // components() normalizes '.', so reject it before normalization too.
        if path.to_str().is_none_or(|p| p.split('/').any(|c| c == "." || c == "..")) {
            return Err("Runtime archive traversal path".into());
        }
        let name = tar_name(&path)?;
        let kind = entry.header().entry_type();
        add_size(&mut total, entry.size())?;
        if name.is_none() {
            if !kind.is_dir() { return Err("Runtime root is not a directory".into()); }
            continue;
        }
        let name = name.expect("checked name");
        if !seen.insert(name.to_owned()) { return Err("Duplicate runtime archive entry".into()); }
        if !(kind.is_file() || kind.is_symlink()) {
            return Err("Unsupported runtime archive entry type".into());
        }
        if kind.is_symlink() {
            let target = entry.link_name().map_err(|e| e.to_string())?
                .ok_or("Runtime symbolic link has no target")?.into_owned();
            let target = target.to_str().ok_or("Non-UTF8 runtime symbolic link")?;
            // A flat relative filename is sufficient for all fixed upstream
            // links and makes the full-chain containment rule unambiguous.
            if target.is_empty() || target == "." || target == ".."
                || target.contains(['/', '\\', ':']) || !asset.runtime_file(target) {
                return Err("Runtime symbolic link escapes its managed directory".into());
            }
            if asset.runtime_file(name) { links.insert(name.to_owned(), target.to_owned()); }
            continue;
        }
        if !asset.runtime_file(name) { continue; }
        let path = directory.join(name);
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true)
            .open(&path).map_err(|e| e.to_string())?;
        let expected = entry.size();
        if std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())? != expected {
            return Err("Truncated runtime archive entry".into());
        }
        file.flush().and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
        if name == asset.executable || name.contains(".so") { elf_valid(&path, asset)?; }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(
                if name == asset.executable { 0o755 } else { 0o644 }
            )).map_err(|e| e.to_string())?;
        }
    }
    let mut decoder = archive.into_inner();
    std::io::copy(&mut decoder, &mut std::io::sink()).map_err(|e| e.to_string())?;
    if decoder.limit() == 0 { return Err("Runtime archive decompressed stream is too large".into()); }
    for (name, target) in &links {
        let mut resolved = target.as_str();
        let mut visited = HashSet::new();
        visited.insert(name.as_str());
        while let Some(next) = links.get(resolved) {
            if !visited.insert(resolved) { return Err("Cyclic runtime symbolic link".into()); }
            resolved = next;
        }
        if !directory.join(resolved).symlink_metadata().is_ok_and(|m| m.is_file()) {
            return Err("Dangling runtime symbolic link".into());
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, directory.join(name)).map_err(|e| e.to_string())?;
        #[cfg(not(unix))]
        return Err("Linux runtime installation requires Unix symbolic links".into());
    }
    Ok(())
}

pub(in crate::asr) fn remove_owned_directory(directory: &Path, asset: &RuntimeAsset) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(directory) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Qwen runtime path is not a managed directory".into());
    }
    let files = std::fs::read_dir(directory).map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    for file in &files {
        let name = file.file_name();
        let name = name.to_string_lossy();
        let owned = asset.runtime_file(&name) || name == "version.txt"
            || name == asset.archive.name || name == format!(".{}.part", asset.archive.name)
            || name == format!("{}.vrcs-verified.json", asset.archive.name)
            || name == ".LICENSE.part" || name == "LICENSE.vrcs-verified.json";
        let kind = file.file_type().map_err(|e| e.to_string())?;
        if !owned || !(kind.is_file() || kind.is_symlink()) {
            return Err("Qwen runtime directory contains unmanaged files".into());
        }
    }
    for file in files { std::fs::remove_file(file.path()).map_err(|e| e.to_string())?; }
    std::fs::remove_dir(directory).map_err(|e| e.to_string())
}

pub(in crate::asr) fn activate(stage: &Path, directory: &Path, asset: &RuntimeAsset) -> Result<(), String> {
    let backup = directory.with_file_name(format!(".{}.previous-{}", asset.version, uuid::Uuid::new_v4()));
    let previous = directory.try_exists().map_err(|e| e.to_string())?;
    if previous {
        // Rename, never delete, the last installation before attempting activation.
        std::fs::rename(directory, &backup).map_err(|e| format!("Could not preserve Qwen runtime: {e}"))?;
    }
    if let Err(error) = std::fs::rename(stage, directory) {
        if previous {
            std::fs::rename(&backup, directory).map_err(|restore| format!(
                "Could not activate Qwen runtime: {error}; previous runtime retained at {} (restore failed: {restore})", backup.display()))?;
        }
        return Err(format!("Could not activate Qwen runtime: {error}"));
    }
    if previous { let _ = remove_owned_directory(&backup, asset); }
    Ok(())
}
