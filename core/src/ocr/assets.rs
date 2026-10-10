use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use tokio::io::AsyncWriteExt;

struct Asset {
    name: &'static str,
    url: &'static str,
    bytes: u64,
    sha256: &'static str,
}

const ASSETS: [Asset; 3] = [
    Asset {
        name: "det.onnx",
        url: "https://huggingface.co/PaddlePaddle/PP-OCRv6_small_det_onnx/resolve/28fe5895c24fd108c19eb3e8479f4ab385fbfc62/inference.onnx",
        bytes: 9_880_512,
        sha256: "d73e0058b7a8086bbd57f3d10b8bcd4ff95363f67e06e2762b5e814fe9c9410e",
    },
    Asset {
        name: "rec.onnx",
        url: "https://huggingface.co/PaddlePaddle/PP-OCRv6_small_rec_onnx/resolve/b8f84f0b80c529de40b4fbb3544b84fa7233a513/inference.onnx",
        bytes: 21_159_378,
        sha256: "5435fd747c9e0efe15a96d0b378d5bd157e9492ed8fd80edf08f30d02fa24634",
    },
    Asset {
        name: "dict.txt",
        url: "https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/b03f46425e8ff4442b268ce449e3eef758146cd4/ppocr/utils/dict/ppocrv6_dict.txt",
        bytes: 74_947,
        sha256: "b5f2bfe2bdd9448429e3e82b51c789775d9b42f2403d082b00662eb77e401c5d",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelState {
    Missing,
    Downloading,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub state: ModelState,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
}

#[derive(Clone)]
pub(super) struct ModelAssets {
    pub directory: PathBuf,
    status: Arc<RwLock<ModelStatus>>,
    prepare_lock: Arc<tokio::sync::Mutex<()>>,
    download_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl ModelAssets {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            status: Arc::new(RwLock::new(ModelStatus {
                state: ModelState::Missing,
                downloaded_bytes: 0,
                total_bytes: ASSETS.iter().map(|asset| asset.bytes).sum(),
                error: None,
            })),
            prepare_lock: Arc::new(tokio::sync::Mutex::new(())),
            download_task: Default::default(),
        }
    }

    pub fn snapshot(&self) -> ModelStatus {
        self.status
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub async fn status(&self) -> Result<ModelStatus, String> {
        let Ok(_guard) = self.prepare_lock.try_lock() else {
            return Ok(self.snapshot());
        };
        if self.snapshot().state == ModelState::Missing {
            let assets = self.clone();
            let bytes = tokio::task::spawn_blocking(move || assets.verified_bytes())
                .await
                .map_err(|_| "OCR model verification worker failed")??;
            let mut status = self
                .status
                .write()
                .unwrap_or_else(|error| error.into_inner());
            status.downloaded_bytes = bytes;
            if bytes == status.total_bytes {
                status.state = ModelState::Ready;
            }
        }
        Ok(self.snapshot())
    }

    pub fn start_download(&self) -> Result<ModelStatus, String> {
        let Ok(guard) = self.prepare_lock.clone().try_lock_owned() else {
            return Ok(self.snapshot());
        };
        {
            let mut status = self
                .status
                .write()
                .unwrap_or_else(|error| error.into_inner());
            status.state = ModelState::Downloading;
            status.downloaded_bytes = 0;
            status.error = None;
        }
        let assets = self.clone();
        let task = tokio::spawn(async move {
            let _guard = guard;
            let result = assets.download().await;
            let mut status = assets
                .status
                .write()
                .unwrap_or_else(|error| error.into_inner());
            match result {
                Ok(()) => {
                    status.state = ModelState::Ready;
                    status.downloaded_bytes = status.total_bytes;
                }
                Err(error) => {
                    status.state = ModelState::Error;
                    status.error = Some(error);
                }
            }
        });
        *self.download_task.lock().expect("OCR download lock") = Some(task);
        Ok(self.snapshot())
    }

    pub async fn cancel_download(&self) {
        let task = self.download_task.lock().expect("OCR download lock").take();
        if let Some(task) = task.filter(|task| !task.is_finished()) {
            task.abort();
            let _ = task.await;
            let mut status = self
                .status
                .write()
                .unwrap_or_else(|error| error.into_inner());
            status.state = ModelState::Missing;
            status.error = None;
        }
    }

    pub fn delete(&self) -> Result<ModelStatus, String> {
        let _guard = self
            .prepare_lock
            .try_lock()
            .map_err(|_| "OCR models are being downloaded or verified")?;
        {
            let mut status = self
                .status
                .write()
                .unwrap_or_else(|error| error.into_inner());
            status.state = ModelState::Missing;
            status.downloaded_bytes = 0;
            status.error = None;
        }
        for asset in &ASSETS {
            let path = self.directory.join(asset.name);
            for file in [&path, &path.with_extension("download.tmp")] {
                match std::fs::remove_file(file) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err(format!("Could not delete OCR asset {}", asset.name)),
                }
            }
        }
        Ok(self.snapshot())
    }

    fn verified_bytes(&self) -> Result<u64, String> {
        let mut bytes = 0;
        for asset in &ASSETS {
            if verify_file(&self.directory.join(asset.name), asset)? {
                bytes += asset.bytes;
            }
        }
        Ok(bytes)
    }

    pub fn verify(&self) -> Result<(), String> {
        if self.verified_bytes()? != ASSETS.iter().map(|asset| asset.bytes).sum::<u64>() {
            return Err(
                "OCR models are missing or invalid; download the local models in settings".into(),
            );
        }
        Ok(())
    }

    async fn download(&self) -> Result<(), String> {
        tokio::fs::create_dir_all(&self.directory)
            .await
            .map_err(|_| "Could not create the OCR model directory")?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .map_err(|_| "Could not create the OCR model download client")?;
        for asset in &ASSETS {
            let path = self.directory.join(asset.name);
            let check_path = path.clone();
            if tokio::task::spawn_blocking(move || verify_file(&check_path, asset))
                .await
                .map_err(|_| "OCR model verification worker failed")??
            {
                self.add_progress(asset.bytes);
                continue;
            }
            let temporary = path.with_extension("download.tmp");
            let result = self.download_file(&client, asset, &temporary, &path).await;
            if result.is_err() {
                let _ = tokio::fs::remove_file(&temporary).await;
            }
            result?;
        }
        Ok(())
    }

    async fn download_file(
        &self,
        client: &reqwest::Client,
        asset: &Asset,
        temporary: &Path,
        path: &Path,
    ) -> Result<(), String> {
        let response = client
            .get(asset.url)
            .send()
            .await
            .map_err(|_| format!("Could not download OCR asset {}", asset.name))?
            .error_for_status()
            .map_err(|_| format!("OCR asset {} download was rejected", asset.name))?;
        if response
            .content_length()
            .is_some_and(|bytes| bytes != asset.bytes)
        {
            return Err(format!("OCR asset {} has an unexpected size", asset.name));
        }
        let mut file = tokio::fs::File::create(temporary)
            .await
            .map_err(|_| "Could not create the OCR download file")?;
        let mut stream = response.bytes_stream();
        let mut bytes = 0;
        let mut digest = Sha256::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "OCR model download was interrupted")?;
            bytes += chunk.len() as u64;
            if bytes > asset.bytes {
                return Err("OCR model download exceeds the expected size".into());
            }
            digest.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|_| "Could not write the OCR model download")?;
            self.add_progress(chunk.len() as u64);
        }
        if bytes != asset.bytes || format!("{:x}", digest.finalize()) != asset.sha256 {
            return Err(format!(
                "OCR asset {} failed integrity verification",
                asset.name
            ));
        }
        file.sync_all()
            .await
            .map_err(|_| "Could not save the OCR model download")?;
        drop(file);
        tokio::fs::rename(temporary, path)
            .await
            .map_err(|_| "Could not install the OCR model download".to_string())
    }

    fn add_progress(&self, bytes: u64) {
        let mut status = self
            .status
            .write()
            .unwrap_or_else(|error| error.into_inner());
        status.downloaded_bytes += bytes;
    }
}

fn verify_file(path: &Path, asset: &Asset) -> Result<bool, String> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(format!("Could not inspect OCR asset {}", asset.name)),
    };
    if !metadata.is_file() || metadata.len() != asset.bytes {
        return Ok(false);
    }
    let mut file = std::fs::File::open(path)
        .map_err(|_| format!("Could not read OCR asset {}", asset.name))?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "Could not verify the OCR model file")?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()) == asset.sha256)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn feature_switches_cancel_model_download_and_release_the_prepare_lock() {
        let directory = tempfile::tempdir().unwrap();
        let assets = ModelAssets::new(directory.path().into());
        let guard = assets.prepare_lock.clone().try_lock_owned().unwrap();
        let task = tokio::spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        *assets.download_task.lock().unwrap() = Some(task);
        assets.status.write().unwrap().state = ModelState::Downloading;
        assets.cancel_download().await;
        assert_eq!(assets.snapshot().state, ModelState::Missing);
        assert!(assets.prepare_lock.try_lock().is_ok());
        assets.cancel_download().await;
        assets.status.write().unwrap().state = ModelState::Ready;
        assets.cancel_download().await;
        assert_eq!(assets.snapshot().state, ModelState::Ready);
    }

    #[tokio::test]
    async fn ocr_model_delete_clears_assets_and_status_but_preserves_other_files() {
        let directory = tempfile::tempdir().unwrap();
        let assets = ModelAssets::new(directory.path().into());
        for asset in &ASSETS {
            let path = directory.path().join(asset.name);
            std::fs::write(&path, b"model").unwrap();
            std::fs::write(path.with_extension("download.tmp"), b"partial").unwrap();
        }
        let unrelated = directory.path().join("keep.txt");
        std::fs::write(&unrelated, b"keep").unwrap();
        {
            let mut status = assets.status.write().unwrap();
            status.state = ModelState::Ready;
            status.downloaded_bytes = status.total_bytes;
            status.error = Some("previous failure".into());
        }
        let deleted = assets.delete().unwrap();
        assert_eq!(deleted.state, ModelState::Missing);
        assert_eq!(deleted.downloaded_bytes, 0);
        assert!(deleted.error.is_none());
        for asset in &ASSETS {
            let path = directory.path().join(asset.name);
            assert!(!path.exists());
            assert!(!path.with_extension("download.tmp").exists());
        }
        assert_eq!(std::fs::read(unrelated).unwrap(), b"keep");
        assert_eq!(assets.status().await.unwrap().state, ModelState::Missing);
        assert!(assets.delete().is_ok());
    }

    #[test]
    fn ocr_model_delete_rejects_an_active_download() {
        let directory = tempfile::tempdir().unwrap();
        let assets = ModelAssets::new(directory.path().into());
        let path = directory.path().join(ASSETS[0].name);
        std::fs::write(&path, b"model").unwrap();
        let _download = assets.prepare_lock.try_lock().unwrap();
        assert!(assets.delete().is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"model");
    }

    #[test]
    fn ocr_model_verification_checks_digest_not_only_size() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("model.onnx");
        let asset = Asset {
            name: "model.onnx",
            url: "unused",
            bytes: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        };
        std::fs::write(&path, b"abc").unwrap();
        assert!(verify_file(&path, &asset).unwrap());
        std::fs::write(&path, b"abd").unwrap();
        assert!(!verify_file(&path, &asset).unwrap());
        std::fs::write(&path, b"ab").unwrap();
        assert!(!verify_file(&path, &asset).unwrap());
        assert!(!verify_file(directory.path(), &asset).unwrap());
    }

    #[tokio::test]
    async fn ocr_model_download_verifies_before_replacing_an_existing_file() {
        use axum::{routing::get, Router};
        use std::sync::atomic::{AtomicBool, Ordering};
        let corrupt = Arc::new(AtomicBool::new(true));
        let handler_corrupt = corrupt.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/asset", listener.local_addr().unwrap());
        let router = Router::new().route(
            "/asset",
            get(move || {
                let corrupt = handler_corrupt.clone();
                async move {
                    if corrupt.load(Ordering::Acquire) {
                        "abd"
                    } else {
                        "abc"
                    }
                }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let assets = ModelAssets::new(directory.path().into());
        let path = directory.path().join("model.onnx");
        let temporary = path.with_extension("download.tmp");
        std::fs::write(&path, b"previous").unwrap();
        let asset = Asset {
            name: "model.onnx",
            url: Box::leak(url.into_boxed_str()),
            bytes: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        };
        let client = reqwest::Client::new();
        assert!(assets
            .download_file(&client, &asset, &temporary, &path)
            .await
            .unwrap_err()
            .contains("integrity"));
        assert_eq!(std::fs::read(&path).unwrap(), b"previous");
        corrupt.store(false, Ordering::Release);
        assets
            .download_file(&client, &asset, &temporary, &path)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        assert!(!temporary.exists());
        server.abort();
    }
}
