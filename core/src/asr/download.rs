use std::sync::Arc;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::{watch, Notify};

use super::manager::{DownloadJob, ModelManager};
use super::model::{
    model_spec, record_verification, validate_download, ModelSpec, MODEL_BASE_URL, MODEL_REVISION,
};

impl ModelManager {
    pub fn start_download(self: &Arc<Self>, model: &str) -> Result<(), String> {
        let spec = model_spec(model)?;
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let done = Arc::new(Notify::new());
        {
            let mut jobs = self.jobs.lock().expect("model jobs lock");
            if self.is_downloaded(model)? {
                return Ok(());
            }
            if jobs
                .get(model)
                .is_some_and(|job| job.status == "downloading")
            {
                return Ok(());
            }
            if jobs.values().any(|job| job.status == "downloading") {
                return Err("Another model download is already in progress".into());
            }
            jobs.insert(
                model.into(),
                DownloadJob {
                    status: "downloading",
                    downloaded_bytes: 0,
                    total_bytes: spec.expected_bytes,
                    error: None,
                    cancel: cancel_tx,
                    done: Arc::clone(&done),
                },
            );
        }

        let manager = Arc::clone(self);
        tokio::spawn(async move {
            manager.download(spec, cancel_rx, done).await;
        });
        Ok(())
    }

    async fn download(&self, spec: ModelSpec, cancel: watch::Receiver<bool>, done: Arc<Notify>) {
        if let Err(error) = self.download_inner(spec, cancel.clone()).await {
            let _ = tokio::fs::remove_file(self.partial_path(spec)).await;
            let mut jobs = self.jobs.lock().expect("model jobs lock");
            if *cancel.borrow() {
                jobs.remove(spec.id);
                tracing::info!(model = spec.id, "ASR model download cancelled");
            } else if let Some(job) = jobs.get_mut(spec.id) {
                job.status = "error";
                job.downloaded_bytes = 0;
                job.error = Some(error.clone());
                tracing::warn!(model = spec.id, %error, "ASR model download failed");
            }
        } else {
            self.jobs.lock().expect("model jobs lock").remove(spec.id);
        }
        done.notify_one();
    }

    async fn download_inner(
        &self,
        spec: ModelSpec,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<(), String> {
        if *cancel.borrow() {
            return Err("Model download was cancelled".into());
        }
        let url = format!("{MODEL_BASE_URL}/{MODEL_REVISION}/{}", spec.filename);
        let request = self.client.get(url).send();
        let response = tokio::select! {
            _ = cancel.changed() => return Err("Model download was cancelled".into()),
            response = request => response,
        }
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| error.to_string())?;
        if response
            .content_length()
            .is_some_and(|length| length != spec.expected_bytes)
        {
            return Err(format!(
                "Model file size mismatch: expected {} bytes",
                spec.expected_bytes
            ));
        }
        if let Some(job) = self.jobs.lock().expect("model jobs lock").get_mut(spec.id) {
            job.total_bytes = spec.expected_bytes;
        }

        let partial_path = self.partial_path(spec);
        let mut file = tokio::fs::File::create(&partial_path)
            .await
            .map_err(|error| error.to_string())?;
        let mut downloaded = 0u64;
        let mut hasher = Sha256::new();
        let mut stream = response.bytes_stream();
        loop {
            let next = tokio::select! {
                _ = cancel.changed() => return Err("Model download was cancelled".into()),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk.map_err(|error| error.to_string())?;
            downloaded = downloaded
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| "Model file size overflow".to_string())?;
            if downloaded > spec.expected_bytes {
                return Err("Model file exceeds the expected size".into());
            }
            file.write_all(&chunk)
                .await
                .map_err(|error| error.to_string())?;
            hasher.update(&chunk);
            if let Some(job) = self.jobs.lock().expect("model jobs lock").get_mut(spec.id) {
                job.downloaded_bytes = downloaded;
            }
        }
        file.flush().await.map_err(|error| error.to_string())?;
        drop(file);
        if downloaded != spec.expected_bytes {
            return Err(format!(
                "Incomplete model download: expected {} bytes, received {downloaded} bytes",
                spec.expected_bytes
            ));
        }
        let digest = format!("{:x}", hasher.finalize());
        validate_download(spec, downloaded, &digest)?;
        let final_path = self.model_dir().join(spec.filename);
        if final_path.exists() {
            tokio::fs::remove_file(&final_path)
                .await
                .map_err(|error| error.to_string())?;
        }
        tokio::fs::rename(partial_path, &final_path)
            .await
            .map_err(|error| error.to_string())?;
        if let Err(error) = record_verification(&final_path, spec, &digest) {
            tracing::warn!(model = spec.id, %error, "unable to cache downloaded model verification");
        }
        Ok(())
    }
}
