//! OCR for user-triggered desktop and VR captures.

#[cfg(windows)]
pub mod desktop_capture;

mod assets;
mod cache;
mod layout;
mod local;
mod processors;
mod progress;
mod tasks;

pub use assets::{ModelState, ModelStatus};
pub(crate) use local::{LocalOcrRuntime, OcrExecutionStatus};
pub use progress::PipelineProgress;
pub use tasks::{
    source_view, BlockUpdate, ScanConfiguration, ScanOutcome, ScanResult, ScanSummary,
};

use serde::Serialize;
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct BlockTranslation {
    pub target_language: String,
    pub text: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranslatedBlock {
    pub source: TextBlock,
    /// Original OCR fragments. The source polygon is only a layout envelope.
    #[serde(skip)]
    pub fragments: Vec<TextBlock>,
    pub translations: Vec<BlockTranslation>,
}

impl TranslatedBlock {
    pub fn fragments(&self) -> &[TextBlock] {
        if self.fragments.is_empty() {
            std::slice::from_ref(&self.source)
        } else {
            &self.fragments
        }
    }
}

pub struct OcrImage {
    pub data: OcrImageData,
    pub width: u32,
    pub height: u32,
}

pub enum OcrImageData {
    Encoded(Vec<u8>),
    Rgba(Vec<u8>),
}

pub type TextRegions = [Vec<[[f32; 2]; 4]>; 2];

#[derive(Clone)]
pub struct VrOcrService {
    config: Arc<RwLock<crate::config::AppConfig>>,
    translation: Arc<crate::translation::TranslationService>,
    local: Arc<LocalOcrRuntime>,
    cache: Arc<std::sync::Mutex<cache::TranslationCache>>,
}

impl VrOcrService {
    pub(crate) fn new(
        config: Arc<RwLock<crate::config::AppConfig>>,
        translation: Arc<crate::translation::TranslationService>,
        local: Arc<LocalOcrRuntime>,
    ) -> Self {
        Self {
            config,
            translation,
            local,
            cache: Default::default(),
        }
    }

    /// The SteamVR owner must drop this future when a capture becomes invalid or is replaced.
    pub async fn process<F: std::future::Future<Output = Result<(), String>>>(
        &self,
        images: [OcrImage; 2],
        progress: impl FnMut(Phase),
        verify: impl FnOnce(TextRegions) -> F,
    ) -> Result<[Vec<TranslatedBlock>; 2], String> {
        let config = self.configuration()?;
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(config.ocr().timeout_seconds as u64);
        self.process_scan(images, &config, 0, deadline, progress, verify, |_| {})
            .await
            .map(|result| result.blocks)
    }

    // Keep the scan callbacks separate so callers can stream progress and completed blocks.
    #[allow(clippy::too_many_arguments)]
    pub async fn process_scan<
        const N: usize,
        F: std::future::Future<Output = Result<(), String>>,
    >(
        &self,
        images: [OcrImage; N],
        config: &ScanConfiguration,
        scan_id: u64,
        deadline: tokio::time::Instant,
        mut progress: impl FnMut(Phase),
        verify: impl FnOnce(TextRegions) -> F,
        completed: impl FnMut(BlockUpdate),
    ) -> Result<ScanResult, String> {
        use tracing::Instrument;
        let span = tracing::info_span!("ocr_scan", scan_id);
        let started = std::time::Instant::now();
        let result = async {
            let config = &config.0;
            let ocr = &config.ocr;
            if !matches!(N, 1 | 2) {
                return Err("OCR requires one or two images".into());
            }
            if (N == 1 && !ocr.desktop_enabled) || (N == 2 && !ocr.enabled) {
                return Err("OCR is disabled".into());
            }
            if !self.matches_config(config) {
                return Err("OCR configuration changed".into());
            }
            let blocks = tokio::time::timeout_at(deadline, async {
                let blocks = self
                    .local
                    .recognize(images, ocr.device, &mut progress)
                    .await?
                    .into_iter()
                    .collect();
                Ok::<_, String>(blocks)
            })
            .await
            .map_err(|_| "OCR task timed out".to_string())??;
            self.translate_scan(
                config, blocks, scan_id, deadline, verify, progress, completed,
            )
            .await
        }
        .instrument(span.clone())
        .await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        tracing::info!(parent: &span, elapsed_ms, success = result.is_ok(), "OCR scan processed");
        if let Err(error) = &result {
            tracing::warn!(parent: &span, elapsed_ms, %error, "OCR scan failed");
        }
        result
    }

    /// Dropping this future cancels recognition and all scan-owned translation requests.
    pub async fn process_vr_scan(
        &self,
        images: Vec<OcrImage>,
        config: &ScanConfiguration,
        scan_id: u64,
        deadline: tokio::time::Instant,
        progress: impl FnMut(Phase),
        completed: impl FnMut(BlockUpdate),
    ) -> Result<ScanResult, String> {
        self.process_vr_scan_with_progress(
            images,
            config,
            scan_id,
            deadline,
            progress,
            completed,
            |_| {},
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn process_vr_scan_with_progress(
        &self,
        images: Vec<OcrImage>,
        config: &ScanConfiguration,
        scan_id: u64,
        deadline: tokio::time::Instant,
        progress: impl FnMut(Phase),
        completed: impl FnMut(BlockUpdate),
        pipeline_progress: impl FnMut(PipelineProgress),
    ) -> Result<ScanResult, String> {
        use tracing::Instrument;
        let span = tracing::info_span!("ocr_scan", scan_id);
        let started = std::time::Instant::now();
        let result = async {
            if !(1..=2).contains(&images.len()) {
                return Err("OCR requires one or two images".into());
            }
            if !config.ocr().enabled {
                return Err("OCR is disabled".into());
            }
            if !self.configuration_matches(config) {
                return Err("OCR configuration changed".into());
            }
            let state = std::sync::Mutex::new((progress, 0usize));
            let pipeline = std::sync::Mutex::new(progress::Reporter::new(
                scan_id,
                images.len(),
                pipeline_progress,
            ));
            let report = |phase| {
                let rank = match phase {
                    Phase::WaitingWorker => 0,
                    Phase::LoadingModel => 1,
                    Phase::Recognizing => 2,
                    Phase::Translating => 3,
                };
                let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
                if rank > state.1 {
                    state.1 = rank;
                    (state.0)(phase);
                }
            };
            let report_eye = |eye, phase| {
                report(phase);
                pipeline
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .phase(eye, phase);
            };
            // At most two results are queued. Both futures remain owned by this scan.
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let recognize = async {
                let recognized = |eye, blocks| {
                    let _ = sender.send(Ok((eye, blocks)));
                };
                self.local
                    .recognize_each(
                        images,
                        config.ocr().device,
                        |phase| report_eye(None, phase),
                        recognized,
                    )
                    .await?;
                drop(sender);
                Ok::<(), String>(())
            };
            let results = futures_util::stream::unfold(receiver, |mut receiver| async {
                receiver.recv().await.map(|result| (result, receiver))
            });
            let translate = self.translate_stream_with_progress(
                &config.0,
                results,
                scan_id,
                deadline,
                &report,
                completed,
                |event| {
                    pipeline
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .translation(event)
                },
            );
            tokio::pin!(recognize, translate);
            tokio::select! {
                biased;
                result = &mut translate => result,
                result = &mut recognize => {
                    result?;
                    translate.await
                }
            }
        }
        .instrument(span.clone())
        .await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        tracing::info!(parent: &span, elapsed_ms, success = result.is_ok(), "OCR scan processed");
        if let Err(error) = &result {
            tracing::warn!(parent: &span, elapsed_ms, %error, "OCR scan failed");
        }
        result
    }

    #[cfg(test)]
    async fn translate_blocks<F: std::future::Future<Output = Result<(), String>>>(
        &self,
        config: &crate::config::AppConfig,
        images: Vec<Vec<TextBlock>>,
        verify: impl FnOnce(TextRegions) -> F,
        progress: impl FnMut(Phase),
    ) -> Result<Vec<Vec<TranslatedBlock>>, String> {
        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(config.ocr.timeout_seconds as u64);
        self.translate_scan(config, images, 0, deadline, verify, progress, |_| {})
            .await
            .map(|result| result.blocks.into_iter().collect())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    WaitingWorker,
    LoadingModel,
    Recognizing,
    Translating,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TextBlock {
    pub id: usize,
    pub text: String,
    pub confidence: f32,
    pub polygon: [[f32; 2]; 4],
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn ocr_translation_queue_releases_slots_before_the_first_request_finishes() {
        use axum::{routing::post, Json, Router};
        let release = Arc::new(tokio::sync::Notify::new());
        let fifth = Arc::new(tokio::sync::Notify::new());
        let release_handler = release.clone();
        let fifth_handler = fifth.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().route(
            "/chat/completions",
            post(move |Json(body): Json<serde_json::Value>| {
                let release = release_handler.clone();
                let fifth = fifth_handler.clone();
                async move {
                    let input = body["messages"].as_array().unwrap().last().unwrap()["content"]
                        .as_str()
                        .unwrap();
                    if input.contains("slow-first") {
                        release.notified().await;
                    }
                    if input.contains("fifth-last") {
                        fifth.notify_one();
                    }
                    Json(json!({"choices":[{"message":{"content":"translated"}}]}))
                }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut config = crate::config::AppConfig::default();
        config.asr.api_profiles.push(crate::config::ApiProfile {
            id: "ocr-test".into(),
            provider: crate::providers::OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some(origin),
            auth_mode: crate::config::ApiAuthMode::None,
            is_local: true,
            enabled_capabilities: vec![crate::providers::CAPABILITY_TEXT_TRANSLATION.into()],
            ..crate::config::ApiProfile::default()
        });
        config.ocr.targets[0].profile_id = Some("ocr-test".into());
        let service = VrOcrService::new(
            Arc::new(RwLock::new(config.clone())),
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            Arc::new(LocalOcrRuntime::new(std::path::PathBuf::from(
                "models/ocr/ppocrv6-small",
            ))),
        );
        let blocks = ["slow-first", "second", "third", "fourth", "fifth-last"]
            .into_iter()
            .enumerate()
            .map(|(id, text)| TextBlock {
                id,
                text: text.into(),
                confidence: 0.95,
                polygon: [[10., 10.], [80., 10.], [80., 30.], [10., 30.]],
            })
            .collect();
        let work = tokio::spawn(async move {
            service
                .translate_blocks(&config, vec![blocks, vec![]], |_| async { Ok(()) }, |_| {})
                .await
        });
        let fifth_started = tokio::time::timeout(Duration::from_millis(800), fifth.notified())
            .await
            .is_ok();
        release.notify_one();
        let result = work.await.unwrap().unwrap();
        server.abort();
        assert_eq!(result[0].len(), 5);
        assert!(
            fifth_started,
            "A completed request must release its slot while the first request is pending"
        );
    }

    #[tokio::test]
    async fn both_eye_blocks_share_translation_but_keep_their_own_coordinates() {
        use axum::{routing::post, Json, Router};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().route(
            "/chat/completions",
            post(move |Json(body): Json<serde_json::Value>| {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    assert!(body["messages"].to_string().contains("hello"));
                    Json(json!({"choices":[{"message":{"content":"你好"}}]}))
                }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut config = crate::config::AppConfig::default();
        config.asr.api_profiles.push(crate::config::ApiProfile {
            id: "ocr-test".into(),
            provider: crate::providers::OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some(origin),
            auth_mode: crate::config::ApiAuthMode::None,
            is_local: true,
            enabled_capabilities: vec![crate::providers::CAPABILITY_TEXT_TRANSLATION.into()],
            ..crate::config::ApiProfile::default()
        });
        config.ocr.targets[0].profile_id = Some("ocr-test".into());
        let service = VrOcrService::new(
            Arc::new(RwLock::new(config.clone())),
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            Arc::new(LocalOcrRuntime::new(std::path::PathBuf::from(
                "models/ocr/ppocrv6-small",
            ))),
        );
        let left = TextBlock {
            id: 0,
            text: "hello".into(),
            confidence: 0.95,
            polygon: [[20., 10.], [80., 10.], [80., 30.], [20., 30.]],
        };
        let right = TextBlock {
            polygon: [[10., 10.], [70., 10.], [70., 30.], [10., 30.]],
            ..left.clone()
        };
        let rejected = service
            .translate_blocks(
                &config,
                vec![vec![left.clone()], vec![right.clone()]],
                |regions| async move {
                    assert_eq!(
                        regions[0],
                        vec![[[20., 10.], [80., 10.], [80., 30.], [20., 30.]]]
                    );
                    assert_eq!(
                        regions[1],
                        vec![[[10., 10.], [70., 10.], [70., 30.], [10., 30.]]]
                    );
                    Err("capture expired".into())
                },
                |_| panic!("Rejected captures must not start translation"),
            )
            .await
            .unwrap_err();
        assert_eq!(rejected, "capture expired");
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        let translated = service
            .translate_blocks(
                &config,
                vec![vec![left.clone()], vec![right.clone()]],
                |_| async { Ok(()) },
                |_| {},
            )
            .await
            .unwrap();
        server.abort();
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(translated[0][0].source.polygon, left.polygon);
        assert_eq!(translated[1][0].source.polygon, right.polygon);
        for eye in translated {
            assert_eq!(eye[0].translations[0].text.as_deref(), Some("你好"));
        }
    }
}
