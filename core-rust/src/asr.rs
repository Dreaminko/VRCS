//! whisper.cpp 本地识别、GGML 模型状态与下载管理。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::{watch, Notify};
use whisper_rs::{
    get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

use crate::config::AsrConfig;

const MODEL_REPOSITORY: &str = "ggerganov/whisper.cpp";
const MODEL_REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve";

#[derive(Clone, Copy)]
struct ModelSpec {
    id: &'static str,
    filename: &'static str,
    expected_bytes: u64,
    sha256: &'static str,
}

const MODELS: [ModelSpec; 5] = [
    ModelSpec {
        id: "tiny",
        filename: "ggml-tiny.bin",
        expected_bytes: 77_691_713,
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
    },
    ModelSpec {
        id: "base",
        filename: "ggml-base.bin",
        expected_bytes: 147_951_465,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
    },
    ModelSpec {
        id: "small",
        filename: "ggml-small.bin",
        expected_bytes: 487_601_967,
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
    },
    ModelSpec {
        id: "medium",
        filename: "ggml-medium.bin",
        expected_bytes: 1_533_763_059,
        sha256: "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
    },
    ModelSpec {
        id: "large-v3",
        filename: "ggml-large-v3.bin",
        expected_bytes: 3_095_033_483,
        sha256: "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
    },
];

fn model_spec(model: &str) -> Result<ModelSpec, String> {
    MODELS
        .iter()
        .copied()
        .find(|spec| spec.id == model)
        .ok_or_else(|| format!("不支持的识别模型：{model}"))
}

pub fn validate_config(config: &AsrConfig) -> Result<(), String> {
    model_spec(&config.model)?;
    if config.device == "cuda" && !cfg!(feature = "cuda") {
        return Err("当前构建未包含 CUDA 后端".into());
    }
    if config.compute_type != "int8" {
        return Err("Rust Core 的 whisper.cpp 后端当前仅接受 int8 兼容配置".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CudaCapability {
    pub available: bool,
    pub device_count: u32,
    pub error: Option<String>,
}

pub fn cuda_capability() -> CudaCapability {
    match cuda_device_count() {
        Ok(device_count) if device_count > 0 => CudaCapability {
            available: true,
            device_count,
            error: None,
        },
        Ok(_) => CudaCapability {
            available: false,
            device_count: 0,
            error: Some("未发现 CUDA 设备".into()),
        },
        Err(error) => CudaCapability {
            available: false,
            device_count: 0,
            error: Some(error),
        },
    }
}

#[cfg(all(feature = "cuda", windows))]
fn cuda_device_count() -> Result<u32, String> {
    use windows::core::s;
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

    type CuInit = unsafe extern "system" fn(u32) -> i32;
    type CuDeviceGetCount = unsafe extern "system" fn(*mut i32) -> i32;

    unsafe {
        let library = LoadLibraryA(s!("nvcuda.dll"))
            .map_err(|error| format!("无法加载 CUDA 驱动：{error}"))?;
        let result = (|| {
            let init = GetProcAddress(library, s!("cuInit"))
                .ok_or_else(|| "CUDA 驱动缺少 cuInit".to_string())?;
            let get_count = GetProcAddress(library, s!("cuDeviceGetCount"))
                .ok_or_else(|| "CUDA 驱动缺少 cuDeviceGetCount".to_string())?;
            let init: CuInit = std::mem::transmute(init);
            let get_count: CuDeviceGetCount = std::mem::transmute(get_count);

            let status = init(0);
            if status != 0 {
                return Err(format!("CUDA 驱动初始化失败（错误码 {status}）"));
            }
            let mut count = 0;
            let status = get_count(&mut count);
            if status != 0 {
                return Err(format!("无法枚举 CUDA 设备（错误码 {status}）"));
            }
            Ok(count.max(0) as u32)
        })();
        let _ = FreeLibrary(library);
        result
    }
}

#[cfg(not(all(feature = "cuda", windows)))]
fn cuda_device_count() -> Result<u32, String> {
    Err("当前构建未包含 CUDA 后端".into())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Transcription {
    pub text: String,
    pub language: Option<String>,
}

pub trait AsrEngine: Send {
    fn transcribe(
        &mut self,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<Transcription, String>;
}

#[derive(Clone)]
pub struct AsrRuntimeState {
    state: Arc<RwLock<AsrRuntimeSnapshot>>,
}

struct AsrRuntimeSnapshot {
    status: &'static str,
    last_error: Option<String>,
}

impl AsrRuntimeState {
    fn new(status: &'static str) -> Self {
        Self {
            state: Arc::new(RwLock::new(AsrRuntimeSnapshot {
                status,
                last_error: None,
            })),
        }
    }

    pub fn snapshot(&self) -> (&'static str, Option<String>) {
        let state = self.state.read().expect("ASR runtime state lock");
        (state.status, state.last_error.clone())
    }

    fn set(&self, status: &'static str, last_error: Option<String>) {
        let mut state = self.state.write().expect("ASR runtime state lock");
        state.status = status;
        state.last_error = last_error;
    }
}

struct WhisperEngine {
    context: WhisperContext,
}

impl WhisperEngine {
    fn load(model_path: &Path, device: &str) -> Result<Self, String> {
        match device {
            "cpu" => Self::load_with_backend(model_path, false),
            "cuda" => {
                let capability = cuda_capability();
                if !capability.available {
                    return Err(capability.error.unwrap_or_else(|| "CUDA 当前不可用".into()));
                }
                Self::load_with_backend(model_path, true)
            }
            "auto" => {
                let capability = cuda_capability();
                if capability.available {
                    match Self::load_with_backend(model_path, true) {
                        Ok(engine) => return Ok(engine),
                        Err(error) => tracing::warn!(
                            %error,
                            "CUDA 模型装载失败，自动回退到 CPU"
                        ),
                    }
                }
                Self::load_with_backend(model_path, false)
            }
            value => Err(format!("不支持的识别设备：{value}")),
        }
    }

    fn load_with_backend(model_path: &Path, use_gpu: bool) -> Result<Self, String> {
        let path = model_path
            .to_str()
            .ok_or_else(|| format!("模型路径不是有效 UTF-8：{}", model_path.display()))?;
        let mut parameters = WhisperContextParameters::default();
        parameters.use_gpu(use_gpu);
        let context = WhisperContext::new_with_params(path, parameters)
            .map_err(|error| format!("无法加载 Whisper 模型：{error}"))?;
        tracing::info!(
            backend = if use_gpu { "cuda" } else { "cpu" },
            model = %model_path.display(),
            "Whisper 模型已装载"
        );
        Ok(Self { context })
    }
}

impl AsrEngine for WhisperEngine {
    fn transcribe(
        &mut self,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<Transcription, String> {
        let mut state = self
            .context
            .create_state()
            .map_err(|error| error.to_string())?;
        let mut parameters = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let threads = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .min(8) as i32;
        parameters.set_n_threads(threads);
        parameters.set_translate(false);
        parameters.set_no_context(true);
        parameters.set_no_timestamps(true);
        parameters.set_print_progress(false);
        parameters.set_print_realtime(false);
        parameters.set_print_timestamps(false);
        parameters.set_language(language);
        // language=None 已会自动检测；detect_language=true 只检测语言并提前返回。
        parameters.set_detect_language(false);

        state
            .full(parameters, samples)
            .map_err(|error| format!("Whisper 识别失败：{error}"))?;

        let mut text = Vec::new();
        for segment in state.as_iter() {
            let value = segment.to_str_lossy().map_err(|error| error.to_string())?;
            let value = value.trim();
            if !value.is_empty() {
                text.push(value.to_owned());
            }
        }
        let language = get_lang_str(state.full_lang_id_from_state()).map(str::to_owned);
        Ok(Transcription {
            text: text.join(" "),
            language,
        })
    }
}

pub struct AsrService {
    config: AsrConfig,
    model_dir: PathBuf,
    engine: Option<Box<dyn AsrEngine>>,
    runtime: AsrRuntimeState,
}

impl AsrService {
    pub fn new(config: AsrConfig, model_dir: PathBuf) -> Self {
        Self {
            config,
            model_dir,
            engine: None,
            runtime: AsrRuntimeState::new("not_loaded"),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_engine(config: AsrConfig, engine: Box<dyn AsrEngine>) -> Self {
        Self {
            config,
            model_dir: PathBuf::new(),
            engine: Some(engine),
            runtime: AsrRuntimeState::new("ready"),
        }
    }

    pub fn runtime_state(&self) -> AsrRuntimeState {
        self.runtime.clone()
    }

    pub fn update(&mut self, config: AsrConfig, model_dir: PathBuf) {
        if config != self.config || model_dir != self.model_dir {
            self.config = config;
            self.model_dir = model_dir;
            self.engine = None;
            self.runtime.set("not_loaded", None);
        }
    }

    #[allow(dead_code)]
    pub fn transcribe(&mut self, samples: &[f32]) -> Result<Transcription, String> {
        if self.engine.is_none() {
            self.runtime.set("loading", None);
            let path = model_path(&self.model_dir, &self.config.model)?;
            match WhisperEngine::load(&path, &self.config.device) {
                Ok(engine) => {
                    self.engine = Some(Box::new(engine));
                    self.runtime.set("ready", None);
                }
                Err(error) => {
                    self.runtime.set("error", Some(error.clone()));
                    return Err(error);
                }
            }
        }

        let language = (self.config.language != "auto").then_some(self.config.language.as_str());
        let result = self
            .engine
            .as_mut()
            .expect("engine initialized")
            .transcribe(samples, language);
        match &result {
            Ok(_) => {
                self.runtime.set("ready", None);
            }
            Err(error) => {
                self.runtime.set("error", Some(error.clone()));
            }
        }
        result
    }
}

fn model_path(model_dir: &Path, model: &str) -> Result<PathBuf, String> {
    Ok(model_dir.join(model_spec(model)?.filename))
}

#[derive(Debug, Clone)]
struct DownloadJob {
    status: &'static str,
    downloaded_bytes: u64,
    total_bytes: u64,
    error: Option<String>,
    cancel: watch::Sender<bool>,
    done: Arc<Notify>,
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
    client: reqwest::Client,
    jobs: Mutex<HashMap<String, DownloadJob>>,
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
        #[derive(Clone, Copy)]
        enum TransferKind {
            Renamed,
            Copied,
            AlreadyPresent,
        }

        struct Transfer {
            source: PathBuf,
            destination: PathBuf,
            kind: TransferKind,
        }

        fn rollback(transfers: &[Transfer]) {
            for transfer in transfers.iter().rev() {
                match transfer.kind {
                    TransferKind::Renamed => {
                        let _ = std::fs::rename(&transfer.destination, &transfer.source);
                    }
                    TransferKind::Copied => {
                        let _ = std::fs::remove_file(&transfer.destination);
                    }
                    TransferKind::AlreadyPresent => {}
                }
            }
        }

        fn file_sha256(path: &Path) -> Result<String, String> {
            use std::io::Read;

            let mut file = std::fs::File::open(path)
                .map_err(|error| format!("无法打开模型文件 {}：{error}", path.display()))?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0u8; 1024 * 1024];
            loop {
                let read = file
                    .read(&mut buffer)
                    .map_err(|error| format!("无法读取模型文件 {}：{error}", path.display()))?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok(format!("{:x}", hasher.finalize()))
        }

        let mut jobs = self.jobs.lock().expect("model jobs lock");
        if jobs.values().any(|job| job.status == "downloading") {
            return Err("模型下载期间不能更改保存位置".into());
        }
        let source_dir = self.model_dir();
        std::fs::create_dir_all(&model_dir)
            .map_err(|error| format!("无法创建 ASR 模型目录 {}：{error}", model_dir.display()))?;
        let same_directory = source_dir == model_dir
            || matches!(
                (
                    std::fs::canonicalize(&source_dir),
                    std::fs::canonicalize(&model_dir)
                ),
                (Ok(source), Ok(destination)) if source == destination
            );
        if same_directory {
            *self.model_dir.write().expect("model directory lock") = model_dir;
            jobs.clear();
            return Ok(());
        }

        let models = MODELS
            .iter()
            .filter_map(|spec| {
                let source = source_dir.join(spec.filename);
                source
                    .metadata()
                    .ok()
                    .filter(|metadata| metadata.is_file() && metadata.len() == spec.expected_bytes)
                    .map(|_| (*spec, source, model_dir.join(spec.filename)))
            })
            .collect::<Vec<_>>();
        for (spec, _, destination) in &models {
            if let Ok(metadata) = destination.metadata() {
                if !metadata.is_file() || metadata.len() != spec.expected_bytes {
                    return Err(format!(
                        "目标目录中已存在不完整的模型文件 {}，请移走后重试",
                        destination.display()
                    ));
                }
                let digest = file_sha256(destination)?;
                if digest != spec.sha256 {
                    return Err(format!(
                        "目标目录中的模型文件 {} 校验失败，请移走后重试",
                        destination.display()
                    ));
                }
            }
        }

        let mut transfers = Vec::with_capacity(models.len());
        for (spec, source, destination) in models {
            if destination.exists() {
                transfers.push(Transfer {
                    source,
                    destination,
                    kind: TransferKind::AlreadyPresent,
                });
                continue;
            }
            match std::fs::rename(&source, &destination) {
                Ok(()) => transfers.push(Transfer {
                    source,
                    destination,
                    kind: TransferKind::Renamed,
                }),
                Err(_) => {
                    let temporary =
                        destination.with_extension(format!("moving-{}", std::process::id()));
                    if temporary.exists() {
                        rollback(&transfers);
                        return Err(format!(
                            "目标目录中存在未完成的迁移文件 {}，请移走后重试",
                            temporary.display()
                        ));
                    }
                    let copied = match std::fs::copy(&source, &temporary) {
                        Ok(copied) => copied,
                        Err(error) => {
                            let _ = std::fs::remove_file(&temporary);
                            rollback(&transfers);
                            return Err(format!("无法移动模型文件 {}：{error}", source.display()));
                        }
                    };
                    if copied != spec.expected_bytes {
                        let _ = std::fs::remove_file(&temporary);
                        rollback(&transfers);
                        return Err(format!(
                            "模型文件复制不完整：应为 {} 字节，实际为 {copied} 字节",
                            spec.expected_bytes
                        ));
                    }
                    let copied_digest = match file_sha256(&temporary) {
                        Ok(digest) => digest,
                        Err(error) => {
                            let _ = std::fs::remove_file(&temporary);
                            rollback(&transfers);
                            return Err(error);
                        }
                    };
                    if copied_digest != spec.sha256 {
                        let _ = std::fs::remove_file(&temporary);
                        rollback(&transfers);
                        return Err(format!("模型文件复制校验失败：{}", source.display()));
                    }
                    if let Err(error) = std::fs::rename(&temporary, &destination) {
                        let _ = std::fs::remove_file(&temporary);
                        rollback(&transfers);
                        return Err(format!(
                            "无法完成模型文件迁移 {}：{error}",
                            destination.display()
                        ));
                    }
                    transfers.push(Transfer {
                        source,
                        destination,
                        kind: TransferKind::Copied,
                    });
                }
            }
        }

        for transfer in &transfers {
            if matches!(
                transfer.kind,
                TransferKind::Copied | TransferKind::AlreadyPresent
            ) {
                if let Err(error) = std::fs::remove_file(&transfer.source) {
                    tracing::warn!(
                        path = %transfer.source.display(),
                        %error,
                        "模型已迁移，但无法删除旧目录中的副本"
                    );
                }
            }
        }
        let _ = std::fs::remove_dir(&source_dir);
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
        Ok(path
            .metadata()
            .is_ok_and(|metadata| metadata.len() == spec.expected_bytes))
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
            return Err("模型下载已取消".into());
        }
        let url = format!("{MODEL_BASE_URL}/{MODEL_REVISION}/{}", spec.filename);
        let request = self.client.get(url).send();
        let response = tokio::select! {
            _ = cancel.changed() => return Err("模型下载已取消".into()),
            response = request => response,
        }
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| error.to_string())?;
        if response
            .content_length()
            .is_some_and(|length| length != spec.expected_bytes)
        {
            return Err(format!(
                "模型文件大小不符：应为 {} 字节",
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
                _ = cancel.changed() => return Err("模型下载已取消".into()),
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk.map_err(|error| error.to_string())?;
            downloaded = downloaded
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| "模型文件大小溢出".to_string())?;
            if downloaded > spec.expected_bytes {
                return Err("模型文件超过预期大小".into());
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
                "模型下载不完整：应为 {} 字节，实际为 {downloaded} 字节",
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
        tokio::fs::rename(partial_path, final_path)
            .await
            .map_err(|error| error.to_string())
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
            tokio::fs::remove_file(path)
                .await
                .map_err(|error| error.to_string())?;
        }
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

    fn partial_path(&self, spec: ModelSpec) -> PathBuf {
        self.model_dir().join(format!("{}.part", spec.filename))
    }
}

fn validate_download(spec: ModelSpec, downloaded: u64, sha256: &str) -> Result<(), String> {
    if downloaded != spec.expected_bytes {
        return Err(format!(
            "模型文件大小不符：应为 {} 字节，实际为 {downloaded} 字节",
            spec.expected_bytes
        ));
    }
    if sha256 != spec.sha256 {
        return Err("模型文件 SHA-256 校验失败".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_supported_configuration() {
        let config = AsrConfig {
            model: "small".into(),
            language: "auto".into(),
            device: "cpu".into(),
            compute_type: "int8".into(),
        };
        assert!(validate_config(&config).is_ok());

        let mut cuda = config.clone();
        cuda.device = "cuda".into();
        if cfg!(feature = "cuda") {
            assert!(validate_config(&cuda).is_ok());
        } else {
            assert!(validate_config(&cuda).unwrap_err().contains("未包含 CUDA"));
        }
    }

    #[test]
    fn model_manager_reports_local_models() {
        let directory = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(directory.path().to_path_buf()).unwrap();
        let tiny_spec = model_spec("tiny").unwrap();
        let file = std::fs::File::create(directory.path().join(tiny_spec.filename)).unwrap();
        file.set_len(tiny_spec.expected_bytes).unwrap();

        let tiny = manager.describe("tiny", "small", "not_loaded").unwrap();
        assert_eq!(tiny.status, "downloaded");
        assert_eq!(tiny.progress, 1.0);
        assert_eq!(tiny.downloaded_bytes, tiny_spec.expected_bytes);

        let small = manager.describe("small", "small", "not_loaded").unwrap();
        assert_eq!(small.status, "not_downloaded");
        assert!(small.active);
    }

    struct FakeEngine;

    impl AsrEngine for FakeEngine {
        fn transcribe(
            &mut self,
            _samples: &[f32],
            language: Option<&str>,
        ) -> Result<Transcription, String> {
            Ok(Transcription {
                text: "hello".into(),
                language: language.map(str::to_owned),
            })
        }
    }

    #[test]
    fn service_uses_engine_abstraction_and_resets_on_update() {
        let config = AsrConfig {
            model: "small".into(),
            language: "ja".into(),
            device: "cpu".into(),
            compute_type: "int8".into(),
        };
        let mut service = AsrService::new(config.clone(), PathBuf::new());
        service.engine = Some(Box::new(FakeEngine));
        service.runtime.set("error", Some("old error".into()));

        let result = service.transcribe(&[0.0; 512]).unwrap();
        assert_eq!(result.language.as_deref(), Some("ja"));
        assert_eq!(service.runtime.snapshot(), ("ready", None));

        let mut changed = config;
        changed.model = "tiny".into();
        service.update(changed, PathBuf::new());
        assert_eq!(service.runtime.snapshot(), ("not_loaded", None));
        assert!(service.engine.is_none());
    }

    #[test]
    fn service_resets_when_model_directory_changes() {
        let config = AsrConfig::default();
        let mut service = AsrService::with_engine(config.clone(), Box::new(FakeEngine));

        service.update(config, PathBuf::from("custom-models"));

        assert_eq!(service.runtime.snapshot(), ("not_loaded", None));
        assert!(service.engine.is_none());
    }

    #[test]
    fn model_manager_switches_directories() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(first.path().to_path_buf()).unwrap();
        let tiny_spec = model_spec("tiny").unwrap();
        let source = first.path().join(tiny_spec.filename);
        let file = std::fs::File::create(&source).unwrap();
        file.set_len(tiny_spec.expected_bytes).unwrap();
        let destination_dir = second.path().join("custom-models");

        manager.move_model_dir(destination_dir.clone()).unwrap();

        assert_eq!(manager.model_dir(), destination_dir);
        assert!(!source.exists());
        assert!(manager.model_dir().join(tiny_spec.filename).is_file());
        assert!(manager.is_downloaded("tiny").unwrap());
    }

    #[test]
    fn model_manager_keeps_the_old_directory_when_the_target_conflicts() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(first.path().to_path_buf()).unwrap();
        let tiny_spec = model_spec("tiny").unwrap();
        let source = first.path().join(tiny_spec.filename);
        let source_file = std::fs::File::create(&source).unwrap();
        source_file.set_len(tiny_spec.expected_bytes).unwrap();
        let destination = second.path().join(tiny_spec.filename);
        std::fs::write(&destination, b"incomplete").unwrap();

        let error = manager
            .move_model_dir(second.path().to_path_buf())
            .unwrap_err();

        assert!(error.contains("不完整"));
        assert_eq!(manager.model_dir(), first.path());
        assert!(source.exists());
        assert_eq!(std::fs::read(destination).unwrap(), b"incomplete");
    }

    #[test]
    fn runtime_status_does_not_require_the_inference_lock() {
        let service = AsrService::with_engine(AsrConfig::default(), Box::new(FakeEngine));
        let runtime = service.runtime_state();
        let service = Mutex::new(service);
        let _inference_guard = service.lock().unwrap();

        assert_eq!(runtime.snapshot(), ("ready", None));
    }

    #[tokio::test]
    async fn model_manager_protects_active_model() {
        let directory = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(directory.path().to_path_buf()).unwrap();
        let error = manager.delete("small", "small").await.unwrap_err();
        assert!(error.contains("当前正在使用"));
    }

    #[tokio::test]
    async fn deleting_an_active_download_cancels_it() {
        let directory = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(directory.path().to_path_buf()).unwrap();
        let (cancel, mut cancelled) = watch::channel(false);
        let done = Arc::new(Notify::new());
        manager.jobs.lock().unwrap().insert(
            "small".into(),
            DownloadJob {
                status: "downloading",
                downloaded_bytes: 10,
                total_bytes: model_spec("small").unwrap().expected_bytes,
                error: None,
                cancel,
                done: Arc::clone(&done),
            },
        );
        tokio::spawn(async move {
            cancelled.changed().await.unwrap();
            done.notify_one();
        });

        manager.delete("small", "small").await.unwrap();

        assert!(!manager.jobs.lock().unwrap().contains_key("small"));
    }

    #[test]
    fn download_validation_rejects_wrong_size_or_hash() {
        let spec = model_spec("tiny").unwrap();
        assert!(validate_download(spec, spec.expected_bytes, spec.sha256).is_ok());
        assert!(validate_download(spec, spec.expected_bytes - 1, spec.sha256).is_err());
        assert!(validate_download(spec, spec.expected_bytes, &"0".repeat(64)).is_err());
        assert!(
            format!("{MODEL_BASE_URL}/{MODEL_REVISION}/{}", spec.filename).contains(MODEL_REVISION)
        );
    }

    #[test]
    #[ignore = "需要通过 VRCS_WHISPER_MODEL 指定真实 GGML 模型"]
    fn whisper_model_transcribes_when_provided() {
        let path = std::env::var("VRCS_WHISPER_MODEL").expect("VRCS_WHISPER_MODEL");
        let mut engine = WhisperEngine::load(Path::new(&path), "cpu").unwrap();
        let result = engine.transcribe(&vec![0.0; 16_000], None).unwrap();
        assert!(result.text.is_empty());
    }

    #[test]
    #[ignore = "需要通过 VRCS_WHISPER_MODEL 和 VRCS_WHISPER_WAV 指定真实文件"]
    fn whisper_model_transcribes_wav_when_provided() {
        let model = std::env::var("VRCS_WHISPER_MODEL").expect("VRCS_WHISPER_MODEL");
        let wav = std::env::var("VRCS_WHISPER_WAV").expect("VRCS_WHISPER_WAV");
        let device = std::env::var("VRCS_WHISPER_DEVICE").unwrap_or_else(|_| "cpu".into());
        let mut reader = hound::WavReader::open(wav).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16_000);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.bits_per_sample, 16);
        let samples = reader
            .samples::<i16>()
            .map(|sample| f32::from(sample.unwrap()) / 32_768.0)
            .collect::<Vec<_>>();
        let mut engine = WhisperEngine::load(Path::new(&model), &device).unwrap();

        let result = engine.transcribe(&samples, None).unwrap();

        assert!(
            !result.text.is_empty(),
            "Whisper auto-language mode returned no text"
        );
    }
}
