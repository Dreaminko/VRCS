use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tokio::sync::{watch, Notify};

#[derive(Debug, Clone)]
pub(super) struct DownloadJob {
    pub(super) status: &'static str,
    pub(super) downloaded_bytes: u64,
    pub(super) total_bytes: u64,
    pub(super) error: Option<String>,
    pub(super) cancel: watch::Sender<bool>,
    pub(super) done: Arc<Notify>,
}

impl DownloadJob {
    pub(super) fn is_active(&self) -> bool {
        matches!(self.status, "downloading" | "verifying")
    }
}

pub struct ModelManager {
    model_dir: RwLock<PathBuf>,
    pub(super) client: reqwest::Client,
    pub(super) jobs: Mutex<HashMap<String, DownloadJob>>,
}

impl ModelManager {
    pub fn new(model_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&model_dir).map_err(|error| {
            format!(
                "Failed to create ASR model directory {}: {error}",
                model_dir.display()
            )
        })?;
        Ok(Self {
            model_dir: RwLock::new(model_dir),
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(60))
                .build()
                .map_err(|error| format!("Failed to create model download client: {error}"))?,
            jobs: Mutex::new(HashMap::new()),
        })
    }

    pub fn model_dir(&self) -> PathBuf {
        self.model_dir.read().expect("model directory lock").clone()
    }

    pub fn move_model_dir(&self, model_dir: PathBuf) -> Result<(), String> {
        let mut jobs = self.jobs.lock().expect("model jobs lock");
        if jobs.values().any(DownloadJob::is_active) {
            return Err("The model storage path cannot be changed during a download".into());
        }
        let current = self.model_dir();
        super::migration::move_model_dir(current, model_dir.clone())?;
        *self.model_dir.write().expect("model directory lock") = model_dir;
        jobs.clear();
        Ok(())
    }

    pub fn cancel_all(&self) {
        for job in self.jobs.lock().expect("model jobs lock").values() {
            if job.is_active() {
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
            .filter(|job| job.is_active())
            .cloned()
            .collect::<Vec<_>>();
        for job in &jobs {
            let _ = job.cancel.send(true);
        }
        for job in jobs {
            job.done.notified().await;
        }
    }
}
