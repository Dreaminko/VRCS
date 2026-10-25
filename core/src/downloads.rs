use std::sync::RwLock;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::Mutex;

use crate::config::ModelDownloadSource;

// Revisions belong to each hosting service. Keep these mappings pinned to the
// exact assets verified by the model manifests; never rewrite arbitrary URLs.
const MIRRORS: [(&str, &str); 5] = [
    (
        "https://huggingface.co/ggml-org/Qwen3-ASR-0.6B-GGUF/resolve/928ab958557df9aa2ef1c93e0e83c7ad0933fae2/Qwen3-ASR-0.6B-Q8_0.gguf",
        "https://modelscope.cn/api/v1/models/ggml-org/Qwen3-ASR-0.6B-GGUF/repo?Revision=86222636b790fae434fb24c03c46fc817dfb27b2&FilePath=Qwen3-ASR-0.6B-Q8_0.gguf",
    ),
    (
        "https://huggingface.co/ggml-org/Qwen3-ASR-0.6B-GGUF/resolve/928ab958557df9aa2ef1c93e0e83c7ad0933fae2/mmproj-Qwen3-ASR-0.6B-Q8_0.gguf",
        "https://modelscope.cn/api/v1/models/ggml-org/Qwen3-ASR-0.6B-GGUF/repo?Revision=86222636b790fae434fb24c03c46fc817dfb27b2&FilePath=mmproj-Qwen3-ASR-0.6B-Q8_0.gguf",
    ),
    (
        "https://huggingface.co/PaddlePaddle/PP-OCRv6_small_det_onnx/resolve/28fe5895c24fd108c19eb3e8479f4ab385fbfc62/inference.onnx",
        "https://modelscope.cn/api/v1/models/PaddlePaddle/PP-OCRv6_small_det_onnx/repo?Revision=37b02eded8dbca659f8ee5d51f822ea1ebd9bcba&FilePath=inference.onnx",
    ),
    (
        "https://huggingface.co/PaddlePaddle/PP-OCRv6_small_rec_onnx/resolve/b8f84f0b80c529de40b4fbb3544b84fa7233a513/inference.onnx",
        "https://modelscope.cn/api/v1/models/PaddlePaddle/PP-OCRv6_small_rec_onnx/repo?Revision=ba215b1cc49d9ed4459d161b96778e8643fe0c1f&FilePath=inference.onnx",
    ),
    (
        "https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/f766f81d3cfdf7737ac64aad813d91bbfd56bf93/smart-turn-v3.2-cpu.onnx",
        "https://modelscope.cn/api/v1/models/pipecat-ai/smart-turn-v3/repo?Revision=dd6b4b37fa45d30a8d0d56735a1727c96c11711e&FilePath=smart-turn-v3.2-cpu.onnx",
    ),
];

pub(crate) struct DownloadSources {
    preference: RwLock<ModelDownloadSource>,
    probe: Mutex<Option<ProbeResult>>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProbeResult {
    pub official_latency_ms: Option<u64>,
    pub mirror_latency_ms: Option<u64>,
    pub recommended_source: Option<ModelDownloadSource>,
}

#[derive(Serialize)]
pub(crate) struct DownloadSourceStatus {
    pub configured_source: ModelDownloadSource,
    pub active_source: Option<ModelDownloadSource>,
    pub probe: Option<ProbeResult>,
}

impl DownloadSources {
    pub(crate) fn new(source: ModelDownloadSource) -> Self {
        Self {
            preference: RwLock::new(source),
            probe: Mutex::new(None),
        }
    }

    fn preference(&self) -> ModelDownloadSource {
        *self
            .preference
            .read()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub(crate) fn set_preference(&self, source: ModelDownloadSource) {
        *self
            .preference
            .write()
            .unwrap_or_else(|error| error.into_inner()) = source;
    }

    pub(crate) async fn status(&self) -> DownloadSourceStatus {
        let probe = self.probe.lock().await.clone();
        let configured_source = self.preference();
        DownloadSourceStatus {
            configured_source,
            active_source: match configured_source {
                ModelDownloadSource::Auto => {
                    probe.as_ref().and_then(|result| result.recommended_source)
                }
                source => Some(source),
            },
            probe,
        }
    }

    pub(crate) async fn probe(&self, refresh: bool) -> ProbeResult {
        let mut cached = self.probe.lock().await;
        if !refresh {
            if let Some(result) = cached.as_ref() {
                return result.clone();
            }
        }
        let (official_latency_ms, mirror_latency_ms) = match reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(4))
            .build()
        {
            Ok(client) => tokio::join!(
                probe_url(&client, MIRRORS[0].0),
                probe_url(&client, MIRRORS[0].1),
            ),
            Err(_) => (None, None),
        };
        let result = ProbeResult {
            official_latency_ms,
            mirror_latency_ms,
            recommended_source: recommend_source(official_latency_ms, mirror_latency_ms),
        };
        // An offline first launch must not freeze automatic selection for the
        // rest of the session. Retry when a later download needs a source.
        *cached = result.recommended_source.map(|_| result.clone());
        result
    }

    pub(crate) async fn urls(&self, official: &str) -> Vec<String> {
        let Some((_, mirror)) = MIRRORS.iter().find(|(url, _)| *url == official) else {
            return vec![official.into()];
        };
        let detected = if self.preference() == ModelDownloadSource::Auto {
            self.probe(false).await.recommended_source
        } else {
            None
        };
        // Re-read after the probe so an explicit setting wins over an in-flight
        // network check. A running file transfer keeps its current source.
        match self.preference() {
            ModelDownloadSource::Official => vec![official.into()],
            ModelDownloadSource::Modelscope => vec![(*mirror).into(), official.into()],
            ModelDownloadSource::Auto if detected == Some(ModelDownloadSource::Modelscope) => {
                vec![(*mirror).into(), official.into()]
            }
            ModelDownloadSource::Auto => vec![official.into(), (*mirror).into()],
        }
    }
}

fn recommend_source(official: Option<u64>, mirror: Option<u64>) -> Option<ModelDownloadSource> {
    match (official, mirror) {
        (None, None) => None,
        (None, Some(_)) => Some(ModelDownloadSource::Modelscope),
        (Some(_), None) => Some(ModelDownloadSource::Official),
        (Some(official), Some(mirror)) if mirror.saturating_add(150) < official => {
            Some(ModelDownloadSource::Modelscope)
        }
        _ => Some(ModelDownloadSource::Official),
    }
}

async fn probe_url(client: &reqwest::Client, url: &str) -> Option<u64> {
    let started = Instant::now();
    let mut response = client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
        .ok()?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return None;
    }
    let range = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?;
    let total = range.strip_prefix("bytes 0-0/")?.parse::<u64>().ok()?;
    if total == 0 || response.chunk().await.ok()??.len() != 1 {
        return None;
    }
    Some((started.elapsed().as_millis() as u64).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ModelDownloadSource::{Auto, Modelscope, Official};

    #[test]
    fn recommendation_handles_unreachable_sources_and_prefers_stable_official_ties() {
        assert_eq!(recommend_source(None, Some(80)), Some(Modelscope));
        assert_eq!(recommend_source(Some(80), None), Some(Official));
        assert_eq!(recommend_source(Some(900), Some(100)), Some(Modelscope));
        assert_eq!(recommend_source(Some(100), Some(900)), Some(Official));
        assert_eq!(recommend_source(Some(100), Some(80)), Some(Official));
        assert_eq!(recommend_source(None, None), None);
    }

    #[tokio::test]
    async fn manual_source_is_effective_without_network_detection() {
        let sources = DownloadSources::new(Modelscope);
        let status = sources.status().await;
        assert_eq!(status.configured_source, Modelscope);
        assert_eq!(status.active_source, Some(Modelscope));
        assert!(status.probe.is_none());
        sources.set_preference(Official);
        assert_eq!(sources.status().await.active_source, Some(Official));
        sources.set_preference(Auto);
        assert_eq!(sources.status().await.active_source, None);
    }

    #[tokio::test]
    async fn mirror_uses_its_own_revision_and_preserves_official_fallback() {
        let sources = DownloadSources::new(Modelscope);
        let official = "https://huggingface.co/ggml-org/Qwen3-ASR-0.6B-GGUF/resolve/928ab958557df9aa2ef1c93e0e83c7ad0933fae2/Qwen3-ASR-0.6B-Q8_0.gguf";
        assert_eq!(sources.urls(official).await, vec![
            "https://modelscope.cn/api/v1/models/ggml-org/Qwen3-ASR-0.6B-GGUF/repo?Revision=86222636b790fae434fb24c03c46fc817dfb27b2&FilePath=Qwen3-ASR-0.6B-Q8_0.gguf",
            official,
        ]);
        sources.set_preference(Official);
        assert_eq!(sources.urls(official).await, vec![official]);
    }

    #[tokio::test]
    async fn unmirrored_assets_keep_their_official_url_without_probing() {
        let sources = DownloadSources::new(Auto);
        let url = "https://github.com/ggml-org/llama.cpp/releases/download/b11501/llama-b11501-bin-win-vulkan-x64.zip";
        assert_eq!(sources.urls(url).await, vec![url]);
        assert!(sources.status().await.probe.is_none());
    }

    #[tokio::test]
    async fn automatic_selection_uses_cached_probe_and_keeps_the_other_source_as_fallback() {
        let sources = DownloadSources::new(Auto);
        for (preferred, latencies) in [(Modelscope, (None, Some(50))), (Official, (Some(50), None))]
        {
            *sources.probe.lock().await = Some(ProbeResult {
                official_latency_ms: latencies.0,
                mirror_latency_ms: latencies.1,
                recommended_source: Some(preferred),
            });
            let urls = sources.urls(MIRRORS[0].0).await;
            assert_eq!(urls.len(), 2);
            assert_eq!(
                urls[0],
                if preferred == Modelscope {
                    MIRRORS[0].1
                } else {
                    MIRRORS[0].0
                }
            );
            assert_eq!(sources.status().await.active_source, Some(preferred));
            sources.set_preference(Official);
            assert_eq!(sources.urls(MIRRORS[0].0).await, [MIRRORS[0].0]);
            sources.set_preference(Auto);
        }
    }

    #[tokio::test]
    async fn probe_checks_download_response_and_rejects_error_pages_and_empty_bodies() {
        use axum::{http::StatusCode, routing::get, Router};
        let app = Router::new()
            .route(
                "/file",
                get(|| async {
                    (
                        StatusCode::PARTIAL_CONTENT,
                        [("content-range", "bytes 0-0/100")],
                        "x",
                    )
                }),
            )
            .route(
                "/redirect",
                get(|| async { axum::response::Redirect::temporary("/file") }),
            )
            .route("/html", get(|| async { "<html>Blocked</html>" }))
            .route("/blocked", get(|| async { StatusCode::FORBIDDEN }))
            .route("/empty", get(|| async { "" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        assert!(probe_url(&client, &format!("http://{address}/file"))
            .await
            .is_some());
        assert!(probe_url(&client, &format!("http://{address}/redirect"))
            .await
            .is_some());
        assert!(probe_url(&client, &format!("http://{address}/html"))
            .await
            .is_none());
        assert!(probe_url(&client, &format!("http://{address}/blocked"))
            .await
            .is_none());
        assert!(probe_url(&client, &format!("http://{address}/empty"))
            .await
            .is_none());
        server.abort();
    }
}
