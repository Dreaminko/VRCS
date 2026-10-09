use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::{watch, Notify};

use super::manager::{DownloadJob, ModelManager};
use super::qwen_models::{download_file, AssetSpec};
use super::qwen_runtime::QwenRuntime;

pub(super) const VERSION: &str = "b11501";
pub(super) const SHA256: &str = "6bf677025ae9ccdf5ca5e1ee1961ecaf25759f8cf63e875925a83bfaf00b7475";
const JOB_ID: &str = "qwen-runtime";
const ARCHIVE: AssetSpec = AssetSpec {
    name: "runtime.zip",
    bytes: 33_425_118,
    sha256: SHA256,
};
const LICENSE: AssetSpec = AssetSpec {
    name: "LICENSE",
    bytes: 1_078,
    sha256: "94f29bbed6a22c35b992c5c6ebf0e7c92f13b836b90f36f461c9cf2f0f1d010d",
};
const ARCHIVE_URL: &str = "https://github.com/ggml-org/llama.cpp/releases/download/b11501/llama-b11501-bin-win-vulkan-x64.zip";
const LICENSE_URL: &str = "https://raw.githubusercontent.com/ggml-org/llama.cpp/46baf1f1fec5a06d1e52122a9207ca978b720f95/LICENSE";
pub(super) const REQUIRED_FILES: &[&str] = &[
    "llama-server.exe",
    "llama-server-impl.dll",
    "llama.dll",
    "llama-common.dll",
    "mtmd.dll",
    "ggml.dll",
    "ggml-base.dll",
    "ggml-vulkan.dll",
    "ggml-cpu-x64.dll",
    "libomp.dll",
    "LICENSE",
    "LICENSE-LLVM-OpenMP",
];

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
        let available = runtime.executable_path().is_ok_and(|path| path.is_file());
        let jobs = self.jobs.lock().expect("model jobs lock");
        let (status, bytes, error) = match jobs.get(JOB_ID) {
            Some(job) if job.is_active() || job.status == "error" => {
                (job.status, job.downloaded_bytes, job.error.clone())
            }
            _ if available => ("installed", ARCHIVE.bytes, None),
            _ => ("not_downloaded", 0, None),
        };
        RuntimeInstallation {
            status,
            downloaded_bytes: bytes,
            total_bytes: ARCHIVE.bytes,
            progress: if status == "installed" {
                1.0
            } else {
                (bytes as f64 / ARCHIVE.bytes as f64).min(0.99)
            },
            error,
        }
    }

    pub(crate) fn start_qwen_runtime_download(
        self: &Arc<Self>,
        runtime: Arc<QwenRuntime>,
    ) -> Result<(), String> {
        if !cfg!(all(windows, target_arch = "x86_64")) {
            return Err("Managed Qwen runtime download supports Windows x64 only".into());
        }
        let (cancel, receiver) = watch::channel(false);
        let done = Arc::new(Notify::new());
        {
            let mut jobs = self.jobs.lock().expect("model jobs lock");
            if installed(&runtime.directory) || jobs.get(JOB_ID).is_some_and(DownloadJob::is_active)
            {
                return Ok(());
            }
            if jobs.values().any(DownloadJob::is_active) {
                return Err("Another model or runtime download is already in progress".into());
            }
            jobs.insert(
                JOB_ID.into(),
                DownloadJob {
                    status: "downloading",
                    downloaded_bytes: 0,
                    total_bytes: ARCHIVE.bytes,
                    error: None,
                    cancel,
                    done: Arc::clone(&done),
                },
            );
        }
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let result = manager
                .download_qwen_runtime(&runtime.directory, receiver.clone())
                .await;
            let mut jobs = manager.jobs.lock().expect("model jobs lock");
            if *receiver.borrow() || result.is_ok() {
                jobs.remove(JOB_ID);
            } else if let Some(job) = jobs.get_mut(JOB_ID) {
                job.status = "error";
                job.error = result.err();
            }
            done.notify_one();
        });
        Ok(())
    }

    pub(crate) async fn cancel_qwen_runtime_download(&self) {
        let job = self
            .jobs
            .lock()
            .expect("model jobs lock")
            .get(JOB_ID)
            .filter(|job| job.is_active())
            .cloned();
        if let Some(job) = job {
            let _ = job.cancel.send(true);
            job.done.notified().await;
        }
    }

    async fn download_qwen_runtime(
        &self,
        directory: &Path,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<(), String> {
        let parent = directory
            .parent()
            .ok_or("Qwen runtime directory has no parent")?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| error.to_string())?;
        let stage = parent.join(format!(".{VERSION}.staging"));
        remove_owned_directory(&stage)?;
        tokio::fs::create_dir(&stage)
            .await
            .map_err(|error| error.to_string())?;
        let result = async {
            for (asset, url) in [(ARCHIVE, ARCHIVE_URL), (LICENSE, LICENSE_URL)] {
                if *cancel.borrow() {
                    return Err("Qwen runtime download was cancelled".into());
                }
                download_file(
                    &self.client,
                    reqwest::Url::parse(url).map_err(|error| error.to_string())?,
                    asset,
                    &stage.join(asset.name),
                    &stage.join(format!(".{}.part", asset.name)),
                    &mut cancel,
                    |bytes| {
                        if asset.name == ARCHIVE.name {
                            if let Some(job) =
                                self.jobs.lock().expect("model jobs lock").get_mut(JOB_ID)
                            {
                                job.downloaded_bytes = bytes;
                                if bytes == ARCHIVE.bytes {
                                    job.status = "verifying";
                                }
                            }
                        }
                    },
                )
                .await?;
            }
            let extract_stage = stage.clone();
            tokio::task::spawn_blocking(move || {
                extract_archive(&extract_stage.join(ARCHIVE.name), &extract_stage)
            })
            .await
            .map_err(|error| error.to_string())??;
            if *cancel.borrow() {
                return Err("Qwen runtime download was cancelled".into());
            }
            for name in [
                ARCHIVE.name,
                "runtime.zip.vrcs-verified.json",
                "LICENSE.vrcs-verified.json",
            ] {
                if let Err(error) = tokio::fs::remove_file(stage.join(name)).await {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        return Err(error.to_string());
                    }
                }
            }
            remove_owned_directory(directory)?;
            tokio::fs::rename(&stage, directory)
                .await
                .map_err(|error| format!("Could not activate Qwen runtime: {error}"))
        }
        .await;
        if result.is_err() {
            let _ = remove_owned_directory(&stage);
        }
        result
    }
}

pub(super) fn installed(directory: &Path) -> bool {
    std::fs::read_to_string(directory.join("version.txt")).is_ok_and(|value| value.trim() == SHA256)
        && REQUIRED_FILES.iter().all(|name| {
            std::fs::symlink_metadata(directory.join(name))
                .is_ok_and(|file| file.is_file() && file.len() > 0)
        })
}

fn runtime_file(name: &str) -> bool {
    REQUIRED_FILES.contains(&name)
        || (name.starts_with("ggml-cpu-") && name.ends_with(".dll") && !name.contains(['/', '\\']))
        || name == "ggml-rpc.dll"
}

pub(super) fn extract_archive(archive: &Path, directory: &Path) -> Result<(), String> {
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(archive).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        if !runtime_file(entry.name()) || entry.name() == "LICENSE" {
            continue;
        }
        let mut file = std::fs::File::create(directory.join(entry.name()))
            .map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut file).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    if !REQUIRED_FILES.iter().all(|name| {
        directory
            .join(name)
            .metadata()
            .is_ok_and(|file| file.is_file() && file.len() > 0)
    }) {
        return Err("Qwen runtime archive is missing required files".into());
    }
    std::fs::write(directory.join("version.txt"), SHA256).map_err(|error| error.to_string())
}

fn remove_owned_directory(directory: &Path) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("Qwen runtime path is not a managed directory".into());
    }
    let files = std::fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for file in &files {
        let name = file.file_name();
        let name = name.to_string_lossy();
        if !(runtime_file(&name)
            || [
                "version.txt",
                "runtime.zip",
                ".runtime.zip.part",
                ".LICENSE.part",
                "runtime.zip.vrcs-verified.json",
                "LICENSE.vrcs-verified.json",
            ]
            .contains(&name.as_ref()))
            || !file
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
        {
            return Err("Qwen runtime directory contains unmanaged files".into());
        }
    }
    for file in files {
        std::fs::remove_file(file.path()).map_err(|error| error.to_string())?;
    }
    std::fs::remove_dir(directory).map_err(|error| error.to_string())
}
