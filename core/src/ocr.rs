//! OCR for user-triggered VR captures.

mod assets;
mod local;
mod processors;
mod tasks;

pub use assets::{ModelState, ModelStatus};
pub(crate) use local::LocalOcrRuntime;
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
    pub translations: Vec<BlockTranslation>,
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
    client: Arc<PaddleOcrClient>,
    local: Arc<LocalOcrRuntime>,
}

impl VrOcrService {
    pub(crate) fn new(
        config: Arc<RwLock<crate::config::AppConfig>>,
        translation: Arc<crate::translation::TranslationService>,
        local: Arc<LocalOcrRuntime>,
    ) -> Result<Self, String> {
        Ok(Self {
            config,
            translation,
            client: Arc::new(PaddleOcrClient::new()?),
            local,
        })
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

    pub async fn process_scan<F: std::future::Future<Output = Result<(), String>>>(
        &self,
        images: [OcrImage; 2],
        config: &ScanConfiguration,
        scan_id: u64,
        deadline: tokio::time::Instant,
        mut progress: impl FnMut(Phase),
        verify: impl FnOnce(TextRegions) -> F,
        completed: impl FnMut(BlockUpdate),
    ) -> Result<ScanResult, String> {
        let config = &config.0;
        let ocr = &config.vr_overlay.ocr;
        if !ocr.enabled {
            return Err("VR OCR is disabled".into());
        }
        if !self.matches_config(config) {
            return Err("OCR configuration changed".into());
        }
        let blocks = tokio::time::timeout_at(deadline, async {
            let blocks = if ocr.backend == crate::config::VrOcrBackend::Local {
                self.local
                    .recognize(images, &mut progress)
                    .await?
                    .into_iter()
                    .collect()
            } else {
                let token = crate::credentials::read_ocr_token()?
                    .ok_or("OCR access token is not configured")?;
                let mut blocks = Vec::with_capacity(2);
                // Each eye has its own image coordinates. Submit one bounded image per cloud job.
                for image in images {
                    let OcrImageData::Encoded(bytes) = image.data else {
                        return Err("Cloud OCR requires an encoded image".into());
                    };
                    blocks.push(
                        self.client
                            .recognize(
                                &token,
                                bytes,
                                image.width,
                                image.height,
                                deadline.saturating_duration_since(tokio::time::Instant::now()),
                                &mut progress,
                            )
                            .await?,
                    );
                }
                blocks
            };
            Ok::<_, String>(blocks)
        })
        .await
        .map_err(|_| "OCR task timed out".to_string())??;
        self.translate_scan(
            config, blocks, scan_id, deadline, verify, progress, completed,
        )
        .await
    }

    #[cfg(test)]
    async fn translate_blocks<F: std::future::Future<Output = Result<(), String>>>(
        &self,
        config: &crate::config::AppConfig,
        images: Vec<Vec<TextBlock>>,
        verify: impl FnOnce(TextRegions) -> F,
        progress: impl FnMut(Phase),
    ) -> Result<Vec<Vec<TranslatedBlock>>, String> {
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(config.vr_overlay.ocr.timeout_seconds as u64);
        self.translate_scan(config, images, 0, deadline, verify, progress, |_| {})
            .await
            .map(|result| result.blocks.into_iter().collect())
    }
}

const JOB_URL: &str = "https://paddleocr.aistudio-app.com/api/v2/ocr/jobs";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    LoadingModel,
    Recognizing,
    Submitting,
    Pending,
    Running,
    Downloading,
    Translating,
}

pub struct PaddleOcrClient {
    http: reqwest::Client,
    jobs: reqwest::Url,
    poll_interval: Duration,
}

impl PaddleOcrClient {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|_| "Could not create OCR HTTP client")?,
            jobs: JOB_URL.parse().expect("static OCR URL"),
            poll_interval: Duration::from_secs(5),
        })
    }

    /// Dropping this future stops local requests and polling; it does not cancel the cloud job.
    pub async fn recognize(
        &self,
        token: &str,
        image: Vec<u8>,
        width: u32,
        height: u32,
        timeout: Duration,
        mut progress: impl FnMut(Phase),
    ) -> Result<Vec<TextBlock>, String> {
        if token.trim().is_empty() || token.len() > 4096 {
            return Err("OCR token is missing or invalid".into());
        }
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || image.len() > 8 * 1024 * 1024
        {
            return Err("OCR capture exceeds image limits".into());
        }
        let mime = if image.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if image.starts_with(&[0xff, 0xd8, 0xff]) {
            "image/jpeg"
        } else {
            return Err("OCR capture must be PNG or JPEG".into());
        };
        tokio::time::timeout(timeout, async {
            progress(Phase::Submitting);
            let file = reqwest::multipart::Part::bytes(image).file_name("vr-capture").mime_str(mime)
                .map_err(|_| "Invalid OCR image type")?;
            let form = reqwest::multipart::Form::new().part("file", file).text("model","PP-OCRv6")
                .text("optionalPayload",r#"{"useDocOrientationClassify":false,"useDocUnwarping":false,"useTextlineOrientation":false}"#);
            // A failed submission is not retried: the server may already have created the job.
            let response = self.http.post(self.jobs.clone()).bearer_auth(token.trim()).multipart(form)
                .send().await.map_err(|_| "OCR submission failed; retry manually")?;
            let submitted: serde_json::Value = serde_json::from_slice(&bounded_body(response, 64*1024).await?)
                .map_err(|_| "Invalid OCR submission response")?;
            let id = submitted.pointer("/data/jobId").and_then(|v|v.as_str()).filter(|id|
                !id.is_empty() && id.len() <= 256 && id.bytes().all(|c|c.is_ascii_alphanumeric() || c == b'-' || c == b'_'))
                .ok_or("Missing or invalid OCR job ID")?;
            let mut job_url = self.jobs.clone();
            job_url.path_segments_mut().map_err(|_| "Invalid OCR API URL")?.push(id);
            let result_url = loop {
                let response = self.get(job_url.clone(),Some(token.trim())).await?;
                let job: serde_json::Value = serde_json::from_slice(&bounded_body(response,64*1024).await?)
                    .map_err(|_| "Invalid OCR job response")?;
                match job.pointer("/data/state").and_then(|v|v.as_str()) {
                    Some("pending") => progress(Phase::Pending),
                    Some("running") => progress(Phase::Running),
                    Some("done") => break job.pointer("/data/resultUrl/jsonUrl").and_then(|v|v.as_str())
                        .ok_or("Missing OCR result URL")?.to_owned(),
                    Some("failed") => return Err("Cloud OCR job failed".into()),
                    _ => return Err("Unknown OCR job state".into()),
                }
                tokio::time::sleep(self.poll_interval).await;
            };
            progress(Phase::Downloading);
            let url: reqwest::Url = result_url.parse().map_err(|_| "Invalid OCR result URL")?;
            if !valid_result_url(&url) { return Err("Unsupported OCR result URL".into()); }
            // No Authorization header is attached to result downloads, including same-origin URLs.
            let result = bounded_body(self.get(url,None).await?,4*1024*1024).await?;
            let result = std::str::from_utf8(&result).map_err(|_| "OCR result is not UTF-8")?;
            parse_jsonl(result,width,height)
        }).await.map_err(|_| "OCR task timed out")?
    }

    async fn get(
        &self,
        url: reqwest::Url,
        token: Option<&str>,
    ) -> Result<reqwest::Response, String> {
        for attempt in 0..3 {
            let mut request = self.http.get(url.clone());
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
            match request.send().await {
                Ok(response)
                    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
                        || response.status().is_server_error() =>
                {
                    if attempt == 2 {
                        return Err(http_error(response.status()));
                    }
                    let delay = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(1 << attempt);
                    tokio::time::sleep(Duration::from_secs(delay.min(300))).await;
                }
                Ok(response) => return Ok(response),
                Err(_) if attempt < 2 => {
                    tokio::time::sleep(Duration::from_secs(1 << attempt)).await
                }
                Err(_) => return Err("OCR network request failed".into()),
            }
        }
        unreachable!()
    }
}

fn valid_result_url(url: &reqwest::Url) -> bool {
    let secure = url.scheme() == "https";
    #[cfg(test)]
    let secure = secure || (url.scheme() == "http" && url.host_str() == Some("127.0.0.1"));
    secure && url.host_str().is_some() && url.username().is_empty() && url.password().is_none()
}

fn http_error(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 | 403 => "OCR authentication failed; check the access token".into(),
        429 => "OCR service rate limit reached".into(),
        code => format!("OCR service returned HTTP {code}"),
    }
}

async fn bounded_body(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, String> {
    if !response.status().is_success() {
        return Err(http_error(response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err("OCR response exceeds size limit".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "OCR response download failed")?
    {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err("OCR response exceeds size limit".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TextBlock {
    pub id: usize,
    pub text: String,
    pub confidence: f32,
    pub polygon: [[f32; 2]; 4],
}

fn parse_jsonl(input: &str, width: u32, height: u32) -> Result<Vec<TextBlock>, String> {
    #[derive(serde::Deserialize)]
    struct Recognition {
        rec_texts: Vec<String>,
        rec_scores: Vec<f32>,
        rec_polys: Vec<[[f32; 2]; 4]>,
    }
    let mut lines = input.lines().filter(|line| !line.trim().is_empty());
    let line = lines.next().ok_or("OCR result is empty")?;
    if lines.next().is_some() {
        return Err("A VR capture must produce exactly one OCR page".into());
    }
    let page: serde_json::Value = serde_json::from_str(line).map_err(|_| "Invalid OCR JSONL")?;
    let results = page
        .pointer("/result/ocrResults")
        .and_then(|v| v.as_array())
        .ok_or("Missing OCR results")?;
    if results.len() != 1 {
        return Err("A VR capture must produce exactly one OCR image".into());
    }
    let result: Recognition = serde_json::from_value(results[0]["prunedResult"].clone())
        .map_err(|_| "Missing or invalid OCR text coordinates")?;
    if result.rec_texts.len() != result.rec_scores.len()
        || result.rec_texts.len() != result.rec_polys.len()
        || result.rec_texts.len() > 256
    {
        return Err("OCR text and coordinate arrays do not match or exceed the block limit".into());
    }
    let mut blocks = Vec::new();
    for ((text, confidence), polygon) in result
        .rec_texts
        .into_iter()
        .zip(result.rec_scores)
        .zip(result.rec_polys)
    {
        if !confidence.is_finite()
            || !(0.0..=1.0).contains(&confidence)
            || polygon.iter().any(|[x, y]| {
                !x.is_finite()
                    || !y.is_finite()
                    || *x < 0.0
                    || *y < 0.0
                    || *x > width as f32
                    || *y > height as f32
            })
        {
            return Err("OCR coordinates or confidence are outside the capture bounds".into());
        }
        let area = (0..4)
            .map(|i| {
                let next = (i + 1) % 4;
                polygon[i][0] * polygon[next][1] - polygon[next][0] * polygon[i][1]
            })
            .sum::<f32>()
            .abs();
        if area < 1.0 || text.chars().count() > 5000 {
            return Err("Invalid OCR text block".into());
        }
        let text = text.trim().to_owned();
        if !text.is_empty() {
            blocks.push(TextBlock {
                id: blocks.len(),
                text,
                confidence,
                polygon,
            });
        }
    }
    Ok(blocks)
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
                    let input = body["messages"].to_string();
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
        config.vr_overlay.ocr.targets[0].profile_id = Some("ocr-test".into());
        let service = VrOcrService::new(
            Arc::new(RwLock::new(config.clone())),
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            Arc::new(LocalOcrRuntime::new(std::path::PathBuf::from(
                "models/ocr/ppocrv6-small",
            ))),
        )
        .unwrap();
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
        config.vr_overlay.ocr.targets[0].profile_id = Some("ocr-test".into());
        let service = VrOcrService::new(
            Arc::new(RwLock::new(config.clone())),
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            Arc::new(LocalOcrRuntime::new(std::path::PathBuf::from(
                "models/ocr/ppocrv6-small",
            ))),
        )
        .unwrap();
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

    #[tokio::test]
    async fn replacing_a_scan_and_task_deadline_stop_local_polling() {
        use axum::{
            routing::{get, post},
            Json, Router,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};
        let polls = Arc::new(AtomicUsize::new(0));
        let counter = polls.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route(
                "/jobs",
                post(|| async { Json(json!({"data":{"jobId":"cancel-test"}})) }),
            )
            .route(
                "/jobs/cancel-test",
                get(move || {
                    let counter = counter.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        Json(json!({"data":{"state":"running"}}))
                    }
                }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut client = PaddleOcrClient::new().unwrap();
        client.jobs = format!("{origin}/jobs").parse().unwrap();
        client.poll_interval = Duration::from_millis(5);
        let client = Arc::new(client);
        let task_client = client.clone();
        let task = tokio::spawn(async move {
            task_client
                .recognize(
                    "test-token",
                    b"\x89PNG\r\n\x1a\nfixture".to_vec(),
                    100,
                    100,
                    Duration::from_secs(30),
                    |_| {},
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            while polls.load(Ordering::SeqCst) < 2 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let count = polls.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        // One request sent before cancellation may still arrive at the cloud service.
        assert!(polls.load(Ordering::SeqCst) <= count + 1);
        let settled = polls.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(polls.load(Ordering::SeqCst), settled);
        let error = client
            .recognize(
                "test-token",
                b"\x89PNG\r\n\x1a\nfixture".to_vec(),
                100,
                100,
                Duration::from_millis(30),
                |_| {},
            )
            .await
            .unwrap_err();
        assert!(error.contains("timed out"));
        let count = polls.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(polls.load(Ordering::SeqCst) <= count + 1);
        let settled = polls.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(polls.load(Ordering::SeqCst), settled);
        server.abort();
    }

    #[tokio::test]
    async fn submission_redirects_are_not_followed_or_retried_and_error_bodies_are_not_exposed() {
        use axum::{
            http::{header, StatusCode},
            routing::post,
            Router,
        };
        use std::sync::atomic::{AtomicUsize, Ordering};
        let submits = Arc::new(AtomicUsize::new(0));
        let counter = submits.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route(
                "/jobs",
                post(move || {
                    let counter = counter.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        (
                            StatusCode::TEMPORARY_REDIRECT,
                            [(header::LOCATION, "/unexpected")],
                            "private signed URL and token",
                        )
                    }
                }),
            )
            .route(
                "/unexpected",
                post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut client = PaddleOcrClient::new().unwrap();
        client.jobs = format!("{origin}/jobs").parse().unwrap();
        let error = client
            .recognize(
                "test-token",
                b"\x89PNG\r\n\x1a\nfixture".to_vec(),
                100,
                100,
                Duration::from_secs(2),
                |_| {},
            )
            .await
            .unwrap_err();
        server.abort();
        assert_eq!(submits.load(Ordering::SeqCst), 1);
        assert!(error.contains("307"));
        assert!(!error.contains("private"));
        assert!(!error.contains("test-token"));
    }

    fn page(
        texts: serde_json::Value,
        scores: serde_json::Value,
        polygons: serde_json::Value,
    ) -> String {
        json!({"result":{"ocrResults":[{"prunedResult":{
            "rec_texts":texts,"rec_scores":scores,"rec_polys":polygons,
            "dt_polys":[[[90,90],[99,90],[99,99],[90,99]]]
        }}]}})
        .to_string()
    }

    #[test]
    fn filtered_polygons_keep_text_alignment_and_multiple_jsonl_pages_are_rejected() {
        let input = page(
            json!(["看板", " "]),
            json!([0.95, 0.2]),
            json!([
                [[10, 20], [80, 20], [80, 40], [10, 40]],
                [[0, 0], [2, 0], [2, 2], [0, 2]]
            ]),
        );
        let blocks = parse_jsonl(&input, 100, 100).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "看板");
        assert_eq!(
            blocks[0].polygon,
            [[10., 20.], [80., 20.], [80., 40.], [10., 40.]]
        );
        assert!(parse_jsonl(&format!("{input}\n{input}"), 100, 100).is_err());
    }

    #[test]
    fn mismatched_arrays_and_out_of_image_polygons_are_rejected() {
        assert!(parse_jsonl(&page(json!(["text"]), json!([]), json!([])), 100, 100).is_err());
        assert!(parse_jsonl(
            &page(
                json!(["text"]),
                json!([0.9]),
                json!([[[0, 0], [101, 0], [101, 20], [0, 20]]])
            ),
            100,
            100
        )
        .is_err());
    }

    #[test]
    fn empty_valid_recognition_is_distinct_from_missing_structured_output() {
        assert_eq!(
            parse_jsonl(&page(json!([]), json!([]), json!([])), 100, 100).unwrap(),
            vec![]
        );
        assert!(parse_jsonl(
            r#"{"result":{"ocrResults":[{"ocrImage":"https://example.test/image"}]}}"#,
            100,
            100
        )
        .is_err());
    }

    #[tokio::test]
    async fn cloud_job_upload_poll_and_download_preserve_coordinates_without_forwarding_token() {
        use axum::{
            body::Bytes,
            extract::State,
            http::HeaderMap,
            routing::{get, post},
            Json, Router,
        };
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let polls = Arc::new(AtomicUsize::new(0));
        let result_url = format!("{origin}/result");
        let router = Router::new().route("/jobs",post(|headers:HeaderMap, body:Bytes| async move {
            assert_eq!(headers["authorization"],"Bearer test-only-token");
            assert!(headers["content-type"].to_str().unwrap().starts_with("multipart/form-data"));
            let body = String::from_utf8_lossy(&body);
            for field in ["name=\"file\"", "PP-OCRv6", "useDocUnwarping", "false"] { assert!(body.contains(field)); }
            Json(json!({"data":{"jobId":"job-1"}}))
        })).route("/jobs/job-1",get(move |State(polls):State<Arc<AtomicUsize>>,headers:HeaderMap| {
            let result_url = result_url.clone();
            async move {
                assert_eq!(headers["authorization"],"Bearer test-only-token");
                Json(match polls.fetch_add(1,Ordering::SeqCst) {
                    0 => json!({"data":{"state":"pending"}}),
                    1 => json!({"data":{"state":"running"}}),
                    _ => json!({"data":{"state":"done","resultUrl":{"jsonUrl":result_url}}})
                })
            }
        })).route("/result",get(|headers:HeaderMap| async move {
            assert!(!headers.contains_key("authorization"));
            page(json!(["VR text"]),json!([0.9]),json!([[[2,3],[40,3],[40,12],[2,12]]]))
        })).with_state(polls.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut client = PaddleOcrClient::new().unwrap();
        client.jobs = format!("{origin}/jobs").parse().unwrap();
        client.poll_interval = Duration::from_millis(1);
        let mut phases = Vec::new();
        let blocks = client
            .recognize(
                "test-only-token",
                b"\x89PNG\r\n\x1a\nfixture".to_vec(),
                100,
                100,
                Duration::from_secs(3),
                |phase| phases.push(phase),
            )
            .await
            .unwrap();
        server.abort();
        assert_eq!(blocks[0].text, "VR text");
        assert_eq!(
            phases,
            vec![
                Phase::Submitting,
                Phase::Pending,
                Phase::Running,
                Phase::Downloading
            ]
        );
        assert_eq!(polls.load(Ordering::SeqCst), 3);
    }
}
