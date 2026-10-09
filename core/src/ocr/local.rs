use super::assets::{ModelAssets, ModelStatus};
use super::{processors, OcrImage, OcrImageData, Phase, TextBlock};
use ort::session::{RunOptions, Session};
use ort::value::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub(crate) struct LocalOcrRuntime {
    assets: ModelAssets,
    engine: Arc<Mutex<Option<Engine>>>,
    inference_gate: Arc<tokio::sync::Semaphore>,
}

enum InferenceUpdate {
    Phase(Phase),
    Eye(usize, Vec<TextBlock>),
}

impl LocalOcrRuntime {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            assets: ModelAssets::new(directory),
            engine: Arc::new(Mutex::new(None)),
            inference_gate: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    pub async fn model_status(&self) -> Result<ModelStatus, String> {
        self.assets.status().await
    }

    pub fn download_models(&self) -> Result<ModelStatus, String> {
        self.assets.start_download()
    }

    pub async fn delete_models(&self) -> Result<ModelStatus, String> {
        let permit = self
            .inference_gate
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Local OCR is recognizing; try deleting the models again later")?;
        let engine = self.engine.clone();
        let assets = self.assets.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut engine = engine.lock().map_err(|_| "Local OCR engine lock failed")?;
            *engine = None;
            assets.delete()
        })
        .await
        .map_err(|_| "OCR model deletion worker failed")?
    }

    pub async fn recognize<const N: usize>(
        &self,
        images: [OcrImage; N],
        progress: impl FnMut(Phase),
    ) -> Result<[Vec<TextBlock>; N], String> {
        let mut results = std::array::from_fn(|_| Vec::new());
        self.recognize_each(images.into_iter().collect(), progress, |eye, blocks| {
            results[eye] = blocks
        })
        .await?;
        Ok(results)
    }

    pub(super) async fn recognize_each(
        &self,
        images: Vec<OcrImage>,
        mut progress: impl FnMut(Phase),
        mut recognized: impl FnMut(usize, Vec<TextBlock>),
    ) -> Result<(), String> {
        for image in &images {
            if image.width == 0 || image.height == 0 || image.width > 16384 || image.height > 16384
            {
                return Err("Local OCR capture exceeds image limits".into());
            }
            let OcrImageData::Rgba(pixels) = &image.data else {
                return Err("Local OCR requires RGBA pixels".into());
            };
            if pixels.len() > 128 * 1024 * 1024
                || pixels.len() != image.width as usize * image.height as usize * 4
            {
                return Err("Invalid local OCR pixel buffer".into());
            }
        }
        let permit = self
            .inference_gate
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "Local OCR worker is unavailable")?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let options =
            Arc::new(RunOptions::new().map_err(|_| "Could not create OCR inference options")?);
        let _cancel = CancelOnDrop {
            cancelled: cancelled.clone(),
            options: options.clone(),
        };
        let engine = self.engine.clone();
        let assets = self.assets.clone();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let span = tracing::Span::current();
        let mut worker = tokio::task::spawn_blocking(move || {
            let _span = span.enter();
            let _permit = permit;
            let mut engine = engine.lock().map_err(|_| "Local OCR engine lock failed")?;
            check_cancelled(&cancelled)?;
            if engine.is_none() {
                let started = std::time::Instant::now();
                let _ = sender.try_send(InferenceUpdate::Phase(Phase::LoadingModel));
                assets.verify()?;
                *engine = Some(Engine::load(&assets.directory)?);
                tracing::info!(
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "OCR local model loaded"
                );
            }
            check_cancelled(&cancelled)?;
            let _ = sender.try_send(InferenceUpdate::Phase(Phase::Recognizing));
            let engine = engine.as_mut().ok_or("Local OCR engine is unavailable")?;
            for (index, image) in images.into_iter().enumerate() {
                let _eye = tracing::info_span!("ocr_image", index).entered();
                let blocks = engine.recognize(image, &options, &cancelled)?;
                sender
                    .blocking_send(InferenceUpdate::Eye(index, blocks))
                    .map_err(|_| "Local OCR was cancelled")?;
            }
            Ok(())
        });
        loop {
            tokio::select! {
                biased;
                Some(update) = receiver.recv() => match update {
                    InferenceUpdate::Phase(phase) => progress(phase),
                    InferenceUpdate::Eye(eye, blocks) => recognized(eye, blocks),
                },
                result = &mut worker => return result.map_err(|_| "Local OCR inference worker failed")?,
            }
        }
    }
}

struct CancelOnDrop {
    cancelled: Arc<AtomicBool>,
    options: Arc<RunOptions>,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.options.terminate();
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        Err("Local OCR was cancelled".into())
    } else {
        Ok(())
    }
}

struct Engine {
    detector: Session,
    recognizer: Session,
    characters: Vec<String>,
}

impl Engine {
    fn load(directory: &std::path::Path) -> Result<Self, String> {
        let load = |filename: &str| {
            let session = || -> ort::Result<Session> {
                Session::builder()?
                    .with_intra_threads(2)?
                    .with_inter_threads(1)?
                    .with_parallel_execution(false)?
                    .with_intra_op_spinning(false)?
                    .with_inter_op_spinning(false)?
                    .commit_from_file(directory.join(filename))
            };
            session().map_err(|error| format!("Could not load OCR model {filename}: {error}"))
        };
        Ok(Self {
            detector: load("det.onnx")?,
            recognizer: load("rec.onnx")?,
            characters: characters(
                &std::fs::read_to_string(directory.join("dict.txt"))
                    .map_err(|_| "Could not read the OCR dictionary")?,
            )?,
        })
    }

    fn recognize(
        &mut self,
        image: OcrImage,
        options: &RunOptions,
        cancelled: &AtomicBool,
    ) -> Result<Vec<TextBlock>, String> {
        check_cancelled(cancelled)?;
        let OcrImageData::Rgba(pixels) = image.data else {
            return Err("Local OCR requires RGBA pixels".into());
        };
        let detecting = std::time::Instant::now();
        let input = Value::from_array(processors::det_input(&pixels, image.width, image.height)?)
            .map_err(|_| "Could not create OCR detector input")?;
        let preparation_ms = detecting.elapsed().as_millis() as u64;
        check_cancelled(cancelled)?;
        let inference = std::time::Instant::now();
        let output = self
            .detector
            .run_with_options(ort::inputs![input], options)
            .map_err(|error| format!("OCR detection failed: {error}"))?;
        let inference_ms = inference.elapsed().as_millis() as u64;
        let (shape, probabilities) = output[0]
            .try_extract_tensor::<f32>()
            .map_err(|_| "Invalid OCR detector tensor")?;
        if shape.len() != 4 || shape[0] != 1 || shape[1] != 1 || shape[2] <= 0 || shape[3] <= 0 {
            return Err("Unexpected OCR detector output shape".into());
        }
        check_cancelled(cancelled)?;
        let polygons = processors::det_boxes(
            probabilities,
            shape[3] as usize,
            shape[2] as usize,
            image.width,
            image.height,
        )?;
        drop(output);
        tracing::info!(
            elapsed_ms = detecting.elapsed().as_millis() as u64,
            preparation_ms,
            inference_ms,
            regions = polygons.len(),
            "OCR local detection processed"
        );
        let recognizing = std::time::Instant::now();
        let mut skipped = 0usize;
        let mut blocks = Vec::with_capacity(polygons.len());
        let mut preparation_time = std::time::Duration::ZERO;
        let mut inference_time = std::time::Duration::ZERO;
        for polygon in polygons {
            check_cancelled(cancelled)?;
            let preparation = std::time::Instant::now();
            let Some(input) = rec_input(&pixels, image.width, image.height, polygon)? else {
                skipped += 1;
                continue;
            };
            let input =
                Value::from_array(input).map_err(|_| "Could not create OCR recognition input")?;
            preparation_time += preparation.elapsed();
            check_cancelled(cancelled)?;
            let inference = std::time::Instant::now();
            let output = self
                .recognizer
                .run_with_options(ort::inputs![input], options)
                .map_err(|error| format!("OCR recognition failed: {error}"))?;
            inference_time += inference.elapsed();
            let (shape, probabilities) = output[0]
                .try_extract_tensor::<f32>()
                .map_err(|_| "Invalid OCR recognizer tensor")?;
            if shape.len() != 3 || shape[0] != 1 || shape[1] <= 0 || shape[2] <= 0 {
                return Err("Unexpected OCR recognizer output shape".into());
            }
            let (text, confidence) = processors::ctc_decode(
                probabilities,
                shape[1] as usize,
                shape[2] as usize,
                &self.characters,
            )?;
            let text = text.trim().to_owned();
            if !text.is_empty() && confidence.is_finite() && (0.0..=1.0).contains(&confidence) {
                blocks.push(TextBlock {
                    id: blocks.len(),
                    text,
                    confidence,
                    polygon,
                });
            }
        }
        check_cancelled(cancelled)?;
        tracing::info!(
            elapsed_ms = recognizing.elapsed().as_millis() as u64,
            preparation_ms = preparation_time.as_millis() as u64,
            inference_ms = inference_time.as_millis() as u64,
            blocks = blocks.len(),
            skipped,
            "OCR local recognition processed"
        );
        Ok(blocks)
    }
}

fn rec_input(
    pixels: &[u8],
    width: u32,
    height: u32,
    polygon: [[f32; 2]; 4],
) -> Result<Option<ndarray::Array4<f32>>, String> {
    processors::validate_image(pixels, width, height)?;
    // Once pixels are valid, crop errors describe only invalid polygon geometry.
    Ok(processors::crop_and_rec_input(pixels, width, height, polygon).ok())
}

fn characters(text: &str) -> Result<Vec<String>, String> {
    let mut characters = vec![String::new()];
    for character in text.lines() {
        if character.is_empty() || character.chars().count() != 1 {
            return Err("Invalid OCR character dictionary".into());
        }
        characters.push(character.into());
    }
    if characters.len() < 2 {
        return Err("OCR character dictionary is empty".into());
    }
    characters.push(" ".into());
    Ok(characters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ocr_model_delete_rejects_active_recognition_and_allows_retry() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = LocalOcrRuntime::new(directory.path().into());
        let path = directory.path().join("det.onnx");
        std::fs::write(&path, b"model").unwrap();
        let inference = runtime.inference_gate.acquire().await.unwrap();
        assert!(runtime.delete_models().await.is_err());
        assert!(path.exists());
        drop(inference);
        assert_eq!(
            runtime.delete_models().await.unwrap().state,
            super::super::ModelState::Missing
        );
        assert!(!path.exists());
        assert_eq!(runtime.inference_gate.available_permits(), 1);
    }

    #[test]
    fn ocr_local_skips_geometry_errors_but_rejects_invalid_images() {
        assert!(rec_input(&[255; 16], 2, 2, [[0.; 2]; 4]).unwrap().is_none());
        assert!(rec_input(&[], 2, 2, [[0.; 2]; 4]).is_err());
        assert!(
            rec_input(&[255; 16], 2, 2, [[0., 0.], [2., 0.], [2., 2.], [0., 2.]])
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn ocr_dictionary_adds_ctc_blank_and_space_without_reordering() {
        let dictionary = characters("!\nあ\n字\n").unwrap();
        assert_eq!(dictionary, ["", "!", "あ", "字", " "]);
        assert!(characters("").is_err());
        assert!(characters("a\n\na\n").is_err());
    }

    fn sample() -> OcrImage {
        sample_offset(0)
    }

    fn sample_offset(offset: usize) -> OcrImage {
        let fixture = include_bytes!("../../tests/fixtures/ocr-multilingual.pgm");
        let mut gray = vec![255; 520 * 180];
        for y in 0..180 {
            gray[y * 520 + offset..(y + 1) * 520]
                .copy_from_slice(&fixture[15 + y * 520..15 + (y + 1) * 520 - offset]);
        }
        let pixels = gray
            .into_iter()
            .flat_map(|gray| [gray, gray, gray, 255])
            .collect();
        OcrImage {
            data: OcrImageData::Rgba(pixels),
            width: 520,
            height: 180,
        }
    }

    #[tokio::test]
    async fn ocr_local_missing_models_reports_preparation_not_cloud_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = LocalOcrRuntime::new(directory.path().into());
        let error = runtime
            .recognize([sample(), sample()], |_| {})
            .await
            .unwrap_err();
        assert!(error.contains("models are missing"), "{error}");
    }

    #[tokio::test]
    async fn desktop_scan_uses_local_recognition_with_vr_ocr_disabled() {
        let directory = tempfile::tempdir().unwrap();
        let mut config = crate::config::AppConfig::default();
        config.ocr.desktop_enabled = true;
        config.ocr.backend = crate::config::VrOcrBackend::Local;
        let service = super::super::VrOcrService::new(
            Arc::new(std::sync::RwLock::new(config)),
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            Arc::new(LocalOcrRuntime::new(directory.path().into())),
        )
        .unwrap();
        let snapshot = service.configuration().unwrap();
        assert!(!snapshot.ocr().enabled);
        let error = service
            .process_scan(
                [sample()],
                &snapshot,
                1,
                tokio::time::Instant::now() + std::time::Duration::from_secs(2),
                |_| {},
                |_| async { Ok(()) },
                |_| {},
            )
            .await
            .err()
            .expect("Missing local models must fail recognition");
        assert!(error.contains("models are missing"), "{error}");
    }

    #[tokio::test]
    async fn vr_single_eye_uses_vr_enablement_independently_of_desktop() {
        let directory = tempfile::tempdir().unwrap();
        let mut config = crate::config::AppConfig::default();
        config.ocr.enabled = true;
        config.ocr.desktop_enabled = false;
        config.ocr.backend = crate::config::VrOcrBackend::Local;
        let service = super::super::VrOcrService::new(
            Arc::new(std::sync::RwLock::new(config)),
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            Arc::new(LocalOcrRuntime::new(directory.path().into())),
        )
        .unwrap();
        let snapshot = service.configuration().unwrap();
        let error = service
            .process_vr_scan(
                vec![sample()],
                &snapshot,
                1,
                tokio::time::Instant::now() + std::time::Duration::from_secs(2),
                |_| {},
                |_| {},
            )
            .await
            .err()
            .unwrap();
        assert!(error.contains("models are missing"), "{error}");
        for images in [vec![], vec![sample(), sample(), sample()]] {
            let error = service
                .process_vr_scan(
                    images,
                    &snapshot,
                    1,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(2),
                    |_| {},
                    |_| {},
                )
                .await
                .err()
                .unwrap();
            assert!(error.contains("one or two images"), "{error}");
        }
        service.config.write().unwrap().ocr.enabled = false;
        service.config.write().unwrap().ocr.desktop_enabled = true;
        let snapshot = service.configuration().unwrap();
        let error = service
            .process_vr_scan(
                vec![sample()],
                &snapshot,
                1,
                tokio::time::Instant::now() + std::time::Duration::from_secs(2),
                |_| {},
                |_| {},
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error, "OCR is disabled");
    }

    #[tokio::test]
    async fn local_ocr_accepts_original_desktop_resolution_before_loading_models() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = LocalOcrRuntime::new(directory.path().into());
        let image = OcrImage {
            width: 5120,
            height: 2,
            data: OcrImageData::Rgba(vec![255; 5120 * 2 * 4]),
        };
        let error = runtime.recognize([image], |_| {}).await.err().unwrap();
        assert!(error.contains("models are missing"), "{error}");
    }

    #[tokio::test]
    #[ignore = "requires downloaded official PP-OCRv6 small weights"]
    async fn ocr_official_onnx_models_recognize_multilingual_sample() {
        let directory = std::env::var_os("VRCS_TEST_OCR_MODELS")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models/ocr/ppocrv6-small")
            });
        let runtime = LocalOcrRuntime::new(directory);
        assert_eq!(
            runtime.model_status().await.unwrap().state,
            super::super::ModelState::Ready
        );
        let started = std::time::Instant::now();
        let result = runtime
            .recognize([sample(), sample_offset(30)], |_| {})
            .await
            .unwrap();
        println!("Cold stereo OCR: {:?}", started.elapsed());
        for blocks in &result {
            println!("Recognized: {blocks:?}");
            for expected in [
                "LOCAL OCR TEST 123",
                "こんにちは",
                "本地文字识别",
                "本地文字識別",
            ] {
                assert!(
                    blocks.iter().any(|block| block.text == expected),
                    "Missing {expected}"
                );
            }
            assert!(blocks.iter().all(|block| block.confidence >= 0.6
                && block
                    .polygon
                    .iter()
                    .flatten()
                    .all(|coordinate| coordinate.is_finite())));
        }
        for (left, right) in result[0].iter().zip(&result[1]) {
            assert_eq!(left.text, right.text);
            assert!((right.polygon[0][0] - left.polygon[0][0] - 30.0).abs() <= 5.0);
        }
        let started = std::time::Instant::now();
        let warm = runtime
            .recognize([sample(), sample_offset(30)], |_| {})
            .await
            .unwrap();
        assert_eq!(warm, result);
        println!("Warm stereo OCR: {:?}", started.elapsed());
    }

    #[tokio::test]
    #[ignore = "requires downloaded official PP-OCRv6 small weights"]
    async fn ocr_local_service_verifies_both_eyes_before_translating_without_cloud_ocr() {
        use axum::{routing::post, Json, Router};
        use std::sync::atomic::AtomicUsize;
        let verified = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let handler_verified = verified.clone();
        let handler_requests = requests.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().route("/chat/completions", post(move || {
            let verified = handler_verified.clone();
            let requests = handler_requests.clone();
            async move {
                assert!(verified.load(Ordering::Acquire));
                requests.fetch_add(1, Ordering::Relaxed);
                Json(serde_json::json!({"choices":[{"message":{"content":"translated sample"}}]}))
            }
        }));
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let mut config = crate::config::AppConfig::default();
        config.ocr.enabled = true;
        config.ocr.backend = crate::config::VrOcrBackend::Local;
        config.ocr.targets[0].profile_id = Some("ocr-test".into());
        config.asr.api_profiles.push(crate::config::ApiProfile {
            id: "ocr-test".into(),
            provider: crate::providers::OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some(origin),
            auth_mode: crate::config::ApiAuthMode::None,
            is_local: true,
            enabled_capabilities: vec![crate::providers::CAPABILITY_TEXT_TRANSLATION.into()],
            ..crate::config::ApiProfile::default()
        });
        let directory = std::env::var_os("VRCS_TEST_OCR_MODELS")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models/ocr/ppocrv6-small")
            });
        let service = super::super::VrOcrService::new(
            Arc::new(std::sync::RwLock::new(config)),
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            Arc::new(LocalOcrRuntime::new(directory)),
        )
        .unwrap();
        let mut phases = Vec::new();
        let result = service
            .process(
                [sample(), sample_offset(30)],
                |phase| phases.push(phase),
                move |regions| async move {
                    assert_eq!((regions[0].len(), regions[1].len()), (4, 4));
                    assert!((regions[1][0][0][0] - regions[0][0][0][0] - 30.0).abs() <= 5.0);
                    verified.store(true, Ordering::Release);
                    Ok(())
                },
            )
            .await
            .unwrap();
        server.abort();
        assert_eq!(requests.load(Ordering::Relaxed), 1);
        assert!(phases.contains(&Phase::Recognizing));
        assert!(phases.contains(&Phase::Translating));
        assert!(!phases.contains(&Phase::Submitting));
        for eye in result {
            assert_eq!(eye.len(), 1);
            assert!(eye
                .iter()
                .any(|block| block.source.text.contains("LOCAL OCR TEST 123")));
            assert!(eye
                .iter()
                .all(|block| block.translations[0].text.as_deref() == Some("translated sample")));
        }
        service.config.write().unwrap().ocr.targets[0].profile_id = None;
        phases.clear();
        let original = service
            .process(
                [sample(), sample_offset(30)],
                |phase| phases.push(phase),
                |regions| async move {
                    assert_eq!((regions[0].len(), regions[1].len()), (4, 4));
                    Ok(())
                },
            )
            .await
            .unwrap();
        assert!(original
            .iter()
            .all(|eye| eye.len() == 1 && eye.iter().all(|block| block.translations.is_empty())));
        assert_eq!(requests.load(Ordering::Relaxed), 1);
        assert!(!phases.contains(&Phase::Translating));
    }

    #[tokio::test]
    #[ignore = "requires downloaded official PP-OCRv6 small weights"]
    async fn ocr_local_vr_streams_translations_while_the_second_eye_is_recognizing() {
        use axum::{routing::post, Json, Router};
        use std::sync::atomic::AtomicUsize;
        let directory = std::env::var_os("VRCS_TEST_OCR_MODELS")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models/ocr/ppocrv6-small")
            });
        let runtime = Arc::new(LocalOcrRuntime::new(directory));
        let overlapped = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let (worker, overlap, count) = (runtime.clone(), overlapped.clone(), requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/chat/completions", post(move || {
                let (worker, overlap, count) = (worker.clone(), overlap.clone(), count.clone());
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    if worker.inference_gate.available_permits() == 0 {
                        overlap.store(true, Ordering::SeqCst);
                    }
                    Json(serde_json::json!({"choices":[{"message":{"content":"translated"}}]}))
                }
            }))).await.unwrap();
        });
        let mut config = crate::config::AppConfig::default();
        config.ocr.enabled = true;
        config.ocr.desktop_enabled = false;
        config.ocr.backend = crate::config::VrOcrBackend::Local;
        config.ocr.targets[0].profile_id = Some("ocr-test".into());
        config.asr.api_profiles.push(crate::config::ApiProfile {
            id: "ocr-test".into(),
            provider: crate::providers::OPENAI_COMPATIBLE_PROVIDER.into(),
            base_url: Some(origin),
            auth_mode: crate::config::ApiAuthMode::None,
            is_local: true,
            enabled_capabilities: vec![crate::providers::CAPABILITY_TEXT_TRANSLATION.into()],
            ..crate::config::ApiProfile::default()
        });
        let service = super::super::VrOcrService::new(
            Arc::new(std::sync::RwLock::new(config)),
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            runtime,
        )
        .unwrap();
        let snapshot = service.configuration().unwrap();
        let mut updates = Vec::new();
        let stereo = service
            .process_vr_scan(
                vec![sample(), sample_offset(30)],
                &snapshot,
                4,
                tokio::time::Instant::now() + std::time::Duration::from_secs(30),
                |_| {},
                |update| updates.push(update),
            )
            .await
            .unwrap();
        assert!(
            overlapped.load(Ordering::SeqCst),
            "Translation must begin while sequential OCR still holds its worker permit"
        );
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(stereo.summary.success_count, 2);
        assert_eq!((stereo.blocks[0].len(), stereo.blocks[1].len()), (1, 1));
        assert!(updates
            .iter()
            .any(|update| update.eye == 0 && update.target_language.is_some()));
        assert!(updates
            .iter()
            .any(|update| update.eye == 1 && update.target_language.is_some()));
        let mono = service
            .process_vr_scan(
                vec![sample()],
                &snapshot,
                5,
                tokio::time::Instant::now() + std::time::Duration::from_secs(30),
                |_| {},
                |_| {},
            )
            .await
            .unwrap();
        server.abort();
        assert_eq!(mono.blocks[0].len(), 1);
        assert!(mono.blocks[1].is_empty());
        assert_eq!(mono.summary.success_count, 1);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    #[ignore = "requires downloaded official PP-OCRv6 small weights"]
    async fn ocr_local_cancel_releases_worker_and_allows_a_new_scan() {
        let directory = std::env::var_os("VRCS_TEST_OCR_MODELS")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models/ocr/ppocrv6-small")
            });
        let runtime = Arc::new(LocalOcrRuntime::new(directory));
        let recognizing = Arc::new(tokio::sync::Notify::new());
        let worker_runtime = runtime.clone();
        let worker_recognizing = recognizing.clone();
        let task = tokio::spawn(async move {
            worker_runtime
                .recognize([sample(), sample()], move |phase| {
                    if phase == Phase::Recognizing {
                        worker_recognizing.notify_one();
                    }
                })
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), recognizing.notified())
            .await
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let permit = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            runtime.inference_gate.acquire(),
        )
        .await
        .unwrap()
        .unwrap();
        drop(permit);
        let blocks = runtime
            .recognize([sample(), sample()], |_| {})
            .await
            .unwrap();
        assert_eq!(blocks[0][0].text, "LOCAL OCR TEST 123");
    }
}
