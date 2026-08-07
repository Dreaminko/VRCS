use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{watch, Notify};

use super::model::{
    model_spec, verification_path, verify_model_file, ModelSpec, MODELS, MODEL_REPOSITORY,
};

#[derive(Debug, Clone)]
pub(super) struct DownloadJob {
    pub(super) status: &'static str,
    pub(super) downloaded_bytes: u64,
    pub(super) total_bytes: u64,
    pub(super) error: Option<String>,
    pub(super) cancel: watch::Sender<bool>,
    pub(super) done: Arc<Notify>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelRecord {
    pub id: String,
    pub repository: String,
    pub status: String,
    pub active: bool,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress: f64,
    pub error: Option<String>,
}

pub struct ModelManager {
    model_dir: RwLock<PathBuf>,
    pub(super) client: reqwest::Client,
    pub(super) jobs: Mutex<HashMap<String, DownloadJob>>,
}

impl ModelManager {
    pub fn new(model_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&model_dir)
            .map_err(|error| format!("无法创建 ASR 模型目录 {}：{error}", model_dir.display()))?;
        Ok(Self {
            model_dir: RwLock::new(model_dir),
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(60))
                .build()
                .map_err(|error| format!("无法创建模型下载客户端：{error}"))?,
            jobs: Mutex::new(HashMap::new()),
        })
    }

    pub fn model_dir(&self) -> PathBuf {
        self.model_dir.read().expect("model directory lock").clone()
    }

    pub fn move_model_dir(&self, model_dir: PathBuf) -> Result<(), String> {
        let mut jobs = self.jobs.lock().expect("model jobs lock");
        if jobs.values().any(|job| job.status == "downloading") {
            return Err("模型下载期间不能更改保存位置".into());
        }
        super::migration::move_model_dir(self.model_dir(), model_dir.clone())?;
        *self.model_dir.write().expect("model directory lock") = model_dir;
        jobs.clear();
        Ok(())
    }

    pub fn list(&self, active_model: &str, runtime_status: &str) -> Vec<ModelRecord> {
        MODELS
            .iter()
            .map(|spec| {
                self.describe(spec.id, active_model, runtime_status)
                    .expect("known model")
            })
            .collect()
    }

    pub fn is_downloaded(&self, model: &str) -> Result<bool, String> {
        let spec = model_spec(model)?;
        let path = self.model_dir().join(spec.filename);
        verify_model_file(&path, spec, false)
    }

    pub fn describe(
        &self,
        model: &str,
        active_model: &str,
        runtime_status: &str,
    ) -> Result<ModelRecord, String> {
        let spec = model_spec(model)?;
        let path = self.model_dir().join(spec.filename);
        let downloaded_bytes = path.metadata().map(|meta| meta.len()).unwrap_or(0);
        let downloaded = downloaded_bytes == spec.expected_bytes;
        let job = self
            .jobs
            .lock()
            .expect("model jobs lock")
            .get(model)
            .cloned();
        let active = model == active_model;

        let (status, bytes, total, error) = match job {
            Some(job) if job.status == "downloading" => {
                ("downloading", job.downloaded_bytes, job.total_bytes, None)
            }
            Some(job) if job.status == "error" => {
                ("error", job.downloaded_bytes, job.total_bytes, job.error)
            }
            _ if active && matches!(runtime_status, "loading" | "ready" | "error") => (
                runtime_status,
                downloaded_bytes,
                spec.expected_bytes.max(downloaded_bytes),
                None,
            ),
            _ if downloaded => ("downloaded", downloaded_bytes, downloaded_bytes, None),
            _ => ("not_downloaded", 0, spec.expected_bytes, None),
        };
        let progress = if status == "downloaded" {
            1.0
        } else if status == "downloading" && total > 0 {
            (bytes as f64 / total as f64).min(0.99)
        } else {
            0.0
        };

        Ok(ModelRecord {
            id: model.into(),
            repository: MODEL_REPOSITORY.into(),
            status: status.into(),
            active,
            downloaded_bytes: bytes,
            total_bytes: total,
            progress,
            error,
        })
    }

    pub async fn delete(&self, model: &str, active_model: &str) -> Result<(), String> {
        let spec = model_spec(model)?;
        let job = self
            .jobs
            .lock()
            .expect("model jobs lock")
            .get(model)
            .filter(|job| job.status == "downloading")
            .cloned();
        if let Some(job) = job {
            let _ = job.cancel.send(true);
            job.done.notified().await;
            let _ = tokio::fs::remove_file(self.partial_path(spec)).await;
            self.jobs.lock().expect("model jobs lock").remove(model);
            return Ok(());
        }
        if model == active_model {
            return Err("当前正在使用该模型，请先选择其他模型".into());
        }
        let path = self.model_dir().join(spec.filename);
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|error| error.to_string())?;
        }
        let _ = tokio::fs::remove_file(verification_path(&path)).await;
        self.jobs.lock().expect("model jobs lock").remove(model);
        Ok(())
    }

    pub fn cancel_all(&self) {
        for job in self.jobs.lock().expect("model jobs lock").values() {
            if job.status == "downloading" {
                let _ = job.cancel.send(true);
            }
        }
    }

    pub async fn cancel_all_and_wait(&self) {
        let jobs = self
            .jobs
            .lock()
            .expect("model jobs lock")
            .values()
            .filter(|job| job.status == "downloading")
            .cloned()
            .collect::<Vec<_>>();
        for job in &jobs {
            let _ = job.cancel.send(true);
        }
        for job in jobs {
            job.done.notified().await;
        }
    }

    pub(super) fn partial_path(&self, spec: ModelSpec) -> PathBuf {
        self.model_dir().join(format!("{}.part", spec.filename))
    }
}
