use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::watch;

use super::manager::{DownloadJob, ModelManager};
use super::qwen_models::download_file;
use super::qwen_runtime::QwenRuntime;

#[path = "qwen_runtime_assets.rs"]
pub(super) mod assets;
#[path = "qwen_runtime_extract.rs"]
pub(super) mod extract;

const JOB_ID: &str = "qwen-runtime";

pub(super) fn runtime_directory(root: &Path) -> PathBuf {
    match assets::current() {
        Ok(asset) => asset.directory(root),
        // Unsupported combinations never install or execute this path.
        Err(_) => root.join("unsupported"),
    }
}

pub(super) fn installed(directory: &Path) -> bool {
    assets::current().is_ok_and(|asset| extract::installed(directory, asset))
}

#[derive(Serialize)]
pub(crate) struct RuntimeInstallation {
    status: &'static str,
    downloaded_bytes: u64,
    total_bytes: u64,
    progress: f64,
    error: Option<String>,
}

impl ModelManager {
    pub(crate) fn qwen_runtime_installation(&self, runtime: &QwenRuntime) -> RuntimeInstallation {
        let asset = match assets::current() {
            Ok(asset) => asset,
            Err(error) => return RuntimeInstallation {
                status: "unsupported", downloaded_bytes: 0, total_bytes: 0,
                progress: 0.0, error: Some(error),
            },
        };
        let available = runtime.executable_path().is_ok_and(|path| path.is_file());
        let jobs = self.jobs.lock().expect("model jobs lock");
        let (status, bytes, error) = match jobs.get(JOB_ID) {
            Some(job) if job.is_active() || job.status == "error" => {
                (job.status, job.downloaded_bytes, job.error.clone())
            }
            _ if available => ("installed", asset.archive.bytes, None),
            _ => ("not_downloaded", 0, None),
        };
        RuntimeInstallation {
            status, downloaded_bytes: bytes, total_bytes: asset.archive.bytes,
            progress: if status == "installed" { 1.0 } else {
                (bytes as f64 / asset.archive.bytes as f64).min(0.99)
            },
            error,
        }
    }

    pub(crate) fn start_qwen_runtime_download(
        self: &Arc<Self>, runtime: Arc<QwenRuntime>,
    ) -> Result<(), String> {
        let asset = assets::current()?;
        let (cancel, receiver) = watch::channel(false);
        let (done_tx, done_rx) = watch::channel(false);
        {
            let mut jobs = self.jobs.lock().expect("model jobs lock");
            if installed(&runtime.directory) || jobs.get(JOB_ID).is_some_and(DownloadJob::is_active) {
                return Ok(());
            }
            if jobs.values().any(DownloadJob::is_active) {
                return Err("Another model or runtime download is already in progress".into());
            }
            jobs.insert(JOB_ID.into(), DownloadJob {
                status: "downloading", downloaded_bytes: 0, total_bytes: asset.archive.bytes,
                error: None, cancel, done: done_rx,
            });
        }
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let result = manager.download_qwen_runtime(&runtime.directory, receiver.clone()).await;
            let mut jobs = manager.jobs.lock().expect("model jobs lock");
            if *receiver.borrow() || result.is_ok() {
                jobs.remove(JOB_ID);
            } else if let Some(job) = jobs.get_mut(JOB_ID) {
                job.status = "error";
                job.error = result.err();
            }
            done_tx.send_replace(true);
        });
        Ok(())
    }

    pub(crate) async fn cancel_qwen_runtime_download(&self) {
        let job = self.jobs.lock().expect("model jobs lock").get(JOB_ID)
            .filter(|job| job.is_active()).cloned();
        if let Some(job) = job {
            let _ = job.cancel.send(true);
            job.wait_done().await;
        }
    }

    async fn download_qwen_runtime(
        &self, directory: &Path, mut cancel: watch::Receiver<bool>,
    ) -> Result<(), String> {
        let asset = assets::current()?;
        let parent = directory.parent().ok_or("Qwen runtime directory has no parent")?;
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
        let stage = parent.join(format!(".{}.staging", asset.version));
        extract::remove_owned_directory(&stage, asset)?;
        tokio::fs::create_dir(&stage).await.map_err(|e| e.to_string())?;
        let result = async {
            let mut downloads = vec![(asset.archive, asset.url)];
            // The historical Windows ZIP lacks llama.cpp's LICENSE. Linux's
            // pinned tar includes it, checked against the same fixed hash.
            if asset.os == "windows" { downloads.push((assets::LICENSE, assets::LICENSE_URL)); }
            for (file, url) in downloads {
                if *cancel.borrow() { return Err("Qwen runtime download was cancelled".into()); }
                download_file(
                    &self.client, reqwest::Url::parse(url).map_err(|e| e.to_string())?, file,
                    &stage.join(file.name), &stage.join(format!(".{}.part", file.name)),
                    &mut cancel, |bytes| {
                        if file.name == asset.archive.name {
                            if let Some(job) = self.jobs.lock().expect("model jobs lock").get_mut(JOB_ID) {
                                job.downloaded_bytes = bytes;
                                if bytes == asset.archive.bytes { job.status = "verifying"; }
                            }
                        }
                    },
                ).await?;
            }
            let extract_stage = stage.clone();
            tokio::task::spawn_blocking(move || {
                extract::extract_archive(&extract_stage.join(asset.archive.name), &extract_stage, asset)
            }).await.map_err(|e| e.to_string())??;
            if *cancel.borrow() { return Err("Qwen runtime download was cancelled".into()); }
            for name in [asset.archive.name.to_owned(),
                format!("{}.vrcs-verified.json", asset.archive.name),
                "LICENSE.vrcs-verified.json".to_owned()] {
                if let Err(error) = tokio::fs::remove_file(stage.join(name)).await {
                    if error.kind() != std::io::ErrorKind::NotFound { return Err(error.to_string()); }
                }
            }
            extract::activate(&stage, directory, asset)
        }.await;
        if result.is_err() { let _ = extract::remove_owned_directory(&stage, asset); }
        result
    }
}
