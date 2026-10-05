use std::sync::Arc;

use serde::Serialize;
use tokio::sync::{watch, Notify};

use super::download::download_into_staging;
use super::manifest::{package_spec, PACKAGES};
use super::package::{
    check_owned_directory, is_installed, managed_parent, package_dir, staging_dir,
};
use crate::asr::manager::{DownloadJob, ModelManager};

#[derive(Serialize)]
pub struct QwenModelRecord {
    pub id: String,
    pub engine: &'static str,
    pub repository: &'static str,
    pub revision: &'static str,
    pub status: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress: f64,
    pub error: Option<String>,
}

impl ModelManager {
    pub fn list_qwen(&self) -> Result<Vec<QwenModelRecord>, String> {
        PACKAGES
            .iter()
            .map(|spec| self.describe_qwen(spec.id))
            .collect()
    }

    pub fn describe_qwen(&self, id: &str) -> Result<QwenModelRecord, String> {
        let spec = package_spec(id)?;
        let root = self.model_dir();
        let installed = is_installed(&root, spec, false)?;
        let final_exists = package_dir(&root, spec).exists();
        let job = self.jobs.lock().expect("model jobs lock").get(id).cloned();
        let total = spec.files.iter().map(|file| file.bytes).sum::<u64>();
        let (status, bytes, error) = match job {
            Some(job) if job.is_active() => (job.status, job.downloaded_bytes, None),
            Some(job) if job.status == "error" => ("error", job.downloaded_bytes, job.error),
            _ if installed => ("installed", total, None),
            _ if final_exists => ("corrupt", 0, None),
            _ => ("not_downloaded", 0, None),
        };
        Ok(QwenModelRecord {
            id: spec.id.into(),
            engine: "qwen3_asr_llama_cpp",
            repository: spec.repository,
            revision: spec.revision,
            status: status.into(),
            downloaded_bytes: bytes,
            total_bytes: total,
            progress: if status == "installed" {
                1.0
            } else if matches!(status, "downloading" | "verifying") && total > 0 {
                (bytes as f64 / total as f64).min(0.99)
            } else {
                0.0
            },
            error,
        })
    }

    pub fn start_qwen_download(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let spec = package_spec(id)?;
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let done = Arc::new(Notify::new());
        let root = {
            let mut jobs = self.jobs.lock().expect("model jobs lock");
            let root = self.model_dir();
            if is_installed(&root, spec, false)? {
                return Ok(());
            }
            if package_dir(&root, spec).exists() {
                return Err(
                    "An invalid Qwen ASR package already exists; remove it before retrying".into(),
                );
            }
            if jobs.get(id).is_some_and(DownloadJob::is_active) {
                return Ok(());
            }
            if jobs.values().any(DownloadJob::is_active) {
                return Err("Another model download is already in progress".into());
            }
            jobs.insert(
                id.into(),
                DownloadJob {
                    status: "downloading",
                    downloaded_bytes: 0,
                    total_bytes: spec.files.iter().map(|file| file.bytes).sum(),
                    error: None,
                    cancel: cancel_tx,
                    done: Arc::clone(&done),
                },
            );
            root
        };
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let base_url = format!(
                "https://huggingface.co/{}/resolve/{}",
                spec.repository, spec.revision
            );
            let result = download_into_staging(
                &root,
                spec,
                &base_url,
                &manager.client,
                cancel_rx.clone(),
                |bytes, total| {
                    if let Some(job) = manager
                        .jobs
                        .lock()
                        .expect("model jobs lock")
                        .get_mut(spec.id)
                    {
                        job.downloaded_bytes = bytes;
                        job.total_bytes = total;
                        job.status = if bytes == total {
                            "verifying"
                        } else {
                            "downloading"
                        };
                    }
                },
            )
            .await;
            let mut jobs = manager.jobs.lock().expect("model jobs lock");
            if *cancel_rx.borrow() || result.is_ok() {
                jobs.remove(spec.id);
            } else if let Some(job) = jobs.get_mut(spec.id) {
                job.status = "error";
                job.error = result.err();
            }
            done.notify_one();
        });
        Ok(())
    }

    pub async fn cancel_qwen_download(&self, id: &str) -> Result<(), String> {
        package_spec(id)?;
        let job = self
            .jobs
            .lock()
            .expect("model jobs lock")
            .get(id)
            .filter(|job| job.is_active())
            .cloned();
        if let Some(job) = job {
            let _ = job.cancel.send(true);
            job.done.notified().await;
        }
        Ok(())
    }

    pub fn verify_qwen(&self, id: &str) -> Result<QwenModelRecord, String> {
        let spec = package_spec(id)?;
        if self
            .jobs
            .lock()
            .expect("model jobs lock")
            .values()
            .any(DownloadJob::is_active)
        {
            return Err("Model verification is unavailable during a download".into());
        }
        let _ = is_installed(&self.model_dir(), spec, true)?;
        self.describe_qwen(id)
    }

    pub fn delete_qwen(&self, id: &str) -> Result<(), String> {
        let spec = package_spec(id)?;
        let mut jobs = self.jobs.lock().expect("model jobs lock");
        if jobs.values().any(DownloadJob::is_active) {
            return Err("A model download is in progress".into());
        }
        let root = self.model_dir();
        if root.join("qwen-asr").exists() {
            managed_parent(&root, spec, false)?;
        }
        for directory in [package_dir(&root, spec), staging_dir(&root, spec)] {
            remove_owned_directory(&directory, spec)?;
        }
        jobs.remove(id);
        Ok(())
    }
}

fn remove_owned_directory(
    directory: &std::path::Path,
    spec: super::PackageSpec,
) -> Result<(), String> {
    if check_owned_directory(directory, spec, true)? {
        std::fs::remove_dir_all(directory).map_err(|error| error.to_string())?;
    }
    Ok(())
}
