use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::Serialize;
use whisper_rs::{
    get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

use crate::config::AsrConfig;

use super::cuda::cuda_capability;
use super::model::{model_spec, verify_model_file};

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

    pub(super) fn set(&self, status: &'static str, last_error: Option<String>) {
        let mut state = self.state.write().expect("ASR runtime state lock");
        state.status = status;
        state.last_error = last_error;
    }
}

pub(super) struct WhisperEngine {
    context: WhisperContext,
}

impl WhisperEngine {
    pub(super) fn load(model_path: &Path, device: &str) -> Result<Self, String> {
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
    pub(super) config: AsrConfig,
    pub(super) model_dir: PathBuf,
    pub(super) engine: Option<Box<dyn AsrEngine>>,
    pub(super) runtime: AsrRuntimeState,
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
            let spec = model_spec(&self.config.local.model)?;
            let path = self.model_dir.join(spec.filename);
            if !verify_model_file(&path, spec, false)? {
                let error = format!("模型文件 {} 完整性校验失败，请重新下载", path.display());
                self.runtime.set("error", Some(error.clone()));
                return Err(error);
            }
            match WhisperEngine::load(&path, &self.config.local.device) {
                Ok(engine) => {
                    self.engine = Some(Box::new(engine));
                    self.runtime.set("ready", None);
                }
                Err(mut error) => {
                    match verify_model_file(&path, spec, true) {
                        Ok(false) => {
                            error =
                                format!("模型文件 {} 完整性校验失败，请重新下载", path.display());
                        }
                        Err(verify_error) => {
                            error = format!("{error}；重新校验模型失败：{verify_error}");
                        }
                        Ok(true) => {}
                    }
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
