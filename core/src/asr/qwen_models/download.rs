use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

use super::package::{cache_verified_asset, managed_parent, verify_asset};
use super::{activate_staging, staging_dir, AssetSpec, PackageSpec};

pub(crate) async fn download_into_staging(
    root: &Path,
    spec: PackageSpec,
    base_url: &str,
    client: &reqwest::Client,
    mut cancel: watch::Receiver<bool>,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let stage = staging_dir(root, spec);
    managed_parent(root, spec, true)?;
    match tokio::fs::create_dir(&stage).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = tokio::fs::symlink_metadata(&stage)
                .await
                .map_err(|error| error.to_string())?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err("Qwen ASR staging path is not a managed directory".into());
            }
        }
        Err(error) => {
            return Err(format!(
                "Could not create Qwen ASR staging directory: {error}"
            ))
        }
    }
    let total = spec.files.iter().map(|file| file.bytes).sum();
    let mut completed = 0u64;
    on_progress(completed, total);
    for asset in spec.files {
        if *cancel.borrow() {
            return Err("Qwen ASR download was cancelled".into());
        }
        let path = stage.join(asset.name);
        let check = path.clone();
        if tokio::task::spawn_blocking(move || verify_asset(&check, *asset, false))
            .await
            .map_err(|error| format!("Qwen ASR verification task failed: {error}"))??
        {
            completed += asset.bytes;
            on_progress(completed, total);
            continue;
        }
        let temporary = stage.join(format!(".{}.part", asset.name));
        let result = download_file(
            client,
            base_url,
            *asset,
            &path,
            &temporary,
            &mut cancel,
            |bytes| on_progress(completed + bytes, total),
        )
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        result?;
        completed += asset.bytes;
    }
    if *cancel.borrow() {
        return Err("Qwen ASR download was cancelled".into());
    }
    activate_staging(root, spec)
}

async fn download_file(
    client: &reqwest::Client,
    base_url: &str,
    asset: AssetSpec,
    path: &Path,
    temporary: &PathBuf,
    cancel: &mut watch::Receiver<bool>,
    mut on_progress: impl FnMut(u64),
) -> Result<(), String> {
    let mut url = reqwest::Url::parse(base_url)
        .map_err(|error| format!("Invalid Qwen ASR download URL: {error}"))?;
    url.path_segments_mut()
        .map_err(|_| "Invalid Qwen ASR download URL".to_string())?
        .push(asset.name);
    let request = client.get(url).send();
    let response = tokio::select! {
        _ = cancel.changed() => return Err("Qwen ASR download was cancelled".into()),
        response = request => response,
    }
    .and_then(reqwest::Response::error_for_status)
    .map_err(|error| format!("Could not download Qwen ASR asset {}: {error}", asset.name))?;
    if response
        .content_length()
        .is_some_and(|size| size != asset.bytes)
    {
        return Err(format!(
            "Qwen ASR asset {} has an unexpected size",
            asset.name
        ));
    }
    let mut file = tokio::fs::File::create(temporary)
        .await
        .map_err(|error| format!("Could not create Qwen ASR download file: {error}"))?;
    let mut stream = response.bytes_stream();
    let mut downloaded = 0u64;
    let mut digest = Sha256::new();
    loop {
        let chunk = tokio::select! {
            _ = cancel.changed() => return Err("Qwen ASR download was cancelled".into()),
            chunk = stream.next() => chunk,
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk.map_err(|error| format!("Qwen ASR download was interrupted: {error}"))?;
        downloaded = downloaded
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| "Qwen ASR download size overflow".to_string())?;
        if downloaded > asset.bytes {
            return Err(format!(
                "Qwen ASR asset {} exceeds its expected size",
                asset.name
            ));
        }
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("Could not write Qwen ASR download: {error}"))?;
        digest.update(&chunk);
        on_progress(downloaded);
    }
    if downloaded != asset.bytes {
        return Err(format!("Qwen ASR asset {} is incomplete", asset.name));
    }
    let digest = format!("{:x}", digest.finalize());
    if digest != asset.sha256 {
        return Err(format!(
            "Qwen ASR asset {} failed SHA-256 verification",
            asset.name
        ));
    }
    file.sync_all()
        .await
        .map_err(|error| format!("Could not save Qwen ASR download: {error}"))?;
    drop(file);
    if path.exists() {
        tokio::fs::remove_file(path)
            .await
            .map_err(|error| format!("Could not replace invalid Qwen ASR asset: {error}"))?;
    }
    tokio::fs::rename(temporary, path)
        .await
        .map_err(|error| format!("Could not stage Qwen ASR asset: {error}"))?;
    cache_verified_asset(path, asset, digest)
}
