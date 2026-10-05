use super::{
    backend::OpenVrBackend,
    ocr_capture::{center_crop, encode_png, StereoCapture},
    ocr_input::install_manifest,
    ocr_input_state::{HandReleaseWait, HandWait},
    ocr_status::{failure_code, OcrState, OcrStatus},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
use vrcs_core::{
    ocr::{
        source_view, BlockUpdate, OcrImage, OcrImageData, Phase, ScanConfiguration, ScanOutcome,
        ScanResult, ScanSummary, TranslatedBlock, VrOcrService,
    },
    VrOcrConfig,
};

enum Update {
    Phase(u64, Phase),
    Finished(u64, Result<OcrFrame, String>),
}

struct OcrFrame {
    blocks: [Vec<TranslatedBlock>; 2],
    summary: ScanSummary,
}

struct CaptureMetadata {
    scene_pid: u32,
    origin: i32,
    captured_at: std::time::Instant,
}

impl From<&StereoCapture> for CaptureMetadata {
    fn from(capture: &StereoCapture) -> Self {
        Self {
            scene_pid: capture.scene_pid,
            origin: capture.origin,
            captured_at: capture.captured_at,
        }
    }
}

struct ScanProgress {
    scan_id: u64,
    blocks: [Vec<TranslatedBlock>; 2],
    dirty: bool,
}

impl ScanProgress {
    fn new(scan_id: u64) -> Self {
        Self {
            scan_id,
            blocks: Default::default(),
            dirty: false,
        }
    }

    fn apply(&mut self, update: BlockUpdate) {
        if update.scan_id != self.scan_id || update.eye >= 2 {
            return;
        }
        let blocks = &mut self.blocks[update.eye];
        if let Some(block) = blocks
            .iter_mut()
            .find(|block| block.source.id == update.block.source.id)
        {
            *block = update.block;
        } else {
            blocks.push(update.block);
        }
        self.dirty = true;
    }
}

pub struct OcrRuntime {
    service: Option<VrOcrService>,
    directory: Option<PathBuf>,
    manifest: Option<PathBuf>,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    sender: mpsc::Sender<Update>,
    receiver: mpsc::Receiver<Update>,
    capture: Option<CaptureMetadata>,
    progress: Option<Arc<Mutex<ScanProgress>>>,
    configuration: Option<Arc<ScanConfiguration>>,
    capture_after: Option<std::time::Instant>,
    waiting_for_hands: Option<HandReleaseWait>,
    displayed_at: Option<std::time::Instant>,
    encoder: Arc<tokio::sync::Semaphore>,
    pub blocks: [Vec<TranslatedBlock>; 2],
    pub status: OcrStatus,
}

impl OcrRuntime {
    pub fn new(service: Option<VrOcrService>, directory: Option<PathBuf>) -> Self {
        let (sender, receiver) = mpsc::channel(32);
        Self {
            service,
            directory,
            manifest: None,
            task: None,
            sender,
            receiver,
            capture: None,
            progress: None,
            configuration: None,
            capture_after: None,
            waiting_for_hands: None,
            displayed_at: None,
            encoder: Arc::new(tokio::sync::Semaphore::new(1)),
            blocks: Default::default(),
            status: OcrStatus::default(),
        }
    }

    pub fn clear(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.status.scan_id = self.status.scan_id.wrapping_add(1);
        self.capture = None;
        self.progress = None;
        self.configuration = None;
        self.capture_after = None;
        self.waiting_for_hands = None;
        self.displayed_at = None;
        self.blocks = Default::default();
        self.status.block_count = 0;
        self.status.layout_limited = false;
        self.status.completed_translations = 0;
        self.status.failed_translations = 0;
        self.status.timed_out = false;
        self.status.last_error_code = None;
        self.status.last_error = None;
        self.status.state = OcrState::Ready;
    }

    pub fn unavailable(&mut self, enabled: bool) {
        let state = if enabled {
            OcrState::WaitingVr
        } else {
            OcrState::Disabled
        };
        if self.status.state != state {
            self.clear();
        }
        self.status.state = state;
        self.status.controller_bound = false;
        self.status.gesture_available = false;
    }

    pub fn tick(&mut self, backend: &mut OpenVrBackend, config: &VrOcrConfig) {
        if self.capture.is_none() {
            backend.reset_ocr();
        }
        if !config.enabled {
            backend.reset_ocr();
            backend.reset_ocr_input();
            self.unavailable(false);
            return;
        }
        if let Err(error) = self.update(backend, config) {
            self.clear();
            backend.reset_ocr();
            let code = failure_code(&error);
            self.status.state = if code == "timeout" {
                OcrState::TimedOut
            } else {
                OcrState::Error
            };
            self.status.last_error = Some(error);
            self.status.last_error_code = Some(code.into());
            self.status.timed_out = code == "timeout";
        }
    }

    fn update(&mut self, backend: &mut OpenVrBackend, config: &VrOcrConfig) -> Result<(), String> {
        if self.manifest.is_none() {
            self.manifest = Some(install_manifest(
                self.directory
                    .as_deref()
                    .ok_or("SteamVR input directory unavailable")?,
            )?);
        }
        let input = backend.ocr_input(
            self.manifest.as_deref().ok_or("OCR manifest unavailable")?,
            config.hand_gesture_enabled,
        )?;
        self.status.controller_bound = input.available;
        self.status.gesture_available = input.gesture_available;
        if input.clear {
            self.clear();
            backend.reset_ocr();
            return Ok(());
        }
        if let Some(capture) = &self.capture {
            let configuration_valid = self.configuration.as_deref().is_some_and(|snapshot| {
                self.service
                    .as_ref()
                    .is_some_and(|service| service.configuration_matches(snapshot))
            });
            let valid = backend
                .ocr_tracking()
                .is_ok_and(|(pose, pid, origin)| tracking_valid(capture, pose, pid, origin));
            let expired = self.task.is_none()
                && self.displayed_at.is_some_and(|started| {
                    started.elapsed().as_secs_f32() > config.display_seconds
                });
            if !configuration_valid || expired {
                self.clear();
                backend.reset_ocr();
            } else if !valid {
                self.clear();
                backend.reset_ocr();
                self.status.state = OcrState::Invalid;
            }
        }
        if input.scan || input.gesture_scan {
            self.clear();
            backend.reset_ocr();
            if input.scan {
                self.status.state = OcrState::Capturing;
                // Allow the compositor to remove the previous translation before acquiring a new frame.
                self.capture_after =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(100));
            } else {
                self.status.state = OcrState::WaitingHands;
                self.waiting_for_hands = Some(HandReleaseWait::new(std::time::Instant::now()));
            }
        }
        if let Some(wait) = self.waiting_for_hands.as_mut() {
            match wait.poll(
                input.gesture_available,
                input.hands_in_view,
                std::time::Instant::now(),
            ) {
                HandWait::Waiting => {}
                HandWait::Invalid => {
                    self.clear();
                    self.status.state = OcrState::Invalid;
                }
                HandWait::Ready => {
                    self.waiting_for_hands = None;
                    self.status.state = OcrState::Capturing;
                    self.capture_after = Some(std::time::Instant::now());
                }
            }
        }
        if self
            .capture_after
            .is_some_and(|at| std::time::Instant::now() >= at)
        {
            self.capture_after = None;
            let started = std::time::Instant::now();
            let capture = Arc::new(backend.capture_ocr()?);
            tracing::debug!(
                scan_id = self.status.scan_id,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "OCR captured"
            );
            self.start(capture, config)?;
        }
        while let Some(update) = self.next_current_update() {
            match update {
                Update::Phase(id, phase) if id == self.status.scan_id => {
                    self.status.state = match phase {
                        Phase::Submitting => OcrState::Submitting,
                        Phase::Pending => OcrState::Pending,
                        Phase::Running => OcrState::Running,
                        Phase::Downloading => OcrState::Downloading,
                        Phase::LoadingModel => OcrState::LoadingModel,
                        Phase::Recognizing => OcrState::Recognizing,
                        Phase::Translating => OcrState::Translating,
                    }
                }
                Update::Finished(id, result) if id == self.status.scan_id => {
                    self.finish_frame(result?);
                }
                _ => {}
            }
        }
        self.sync_progress();
        if self.capture.is_none()
            && self.capture_after.is_none()
            && self.waiting_for_hands.is_none()
            && self.status.last_error.is_none()
            && self.status.last_error_code.is_none()
            && matches!(
                self.status.state,
                OcrState::Ready | OcrState::Disabled | OcrState::WaitingVr | OcrState::Unbound
            )
        {
            self.status.state = if input.available {
                OcrState::Ready
            } else {
                OcrState::Unbound
            };
        }
        Ok(())
    }

    fn finish_frame(&mut self, frame: OcrFrame) {
        self.task = None;
        self.progress = None;
        self.blocks = frame.blocks;
        self.status.layout_limited = false;
        self.status.block_count = self.blocks.iter().map(Vec::len).max().unwrap_or(0);
        let visible = !self.wrist_texts().is_empty();
        self.complete_status(&frame.summary, visible);
        if visible {
            self.displayed_at = Some(std::time::Instant::now());
        } else {
            self.capture = None;
            self.configuration = None;
        }
    }

    fn sync_progress(&mut self) {
        let Some(progress) = &self.progress else {
            return;
        };
        let mut progress = progress.lock().unwrap_or_else(|error| error.into_inner());
        if self.capture.is_some() && progress.scan_id == self.status.scan_id && progress.dirty {
            self.blocks = progress.blocks.clone();
            progress.dirty = false;
            self.status.block_count = self.blocks.iter().map(Vec::len).max().unwrap_or(0);
        }
    }

    fn next_current_update(&mut self) -> Option<Update> {
        while let Ok(update) = self.receiver.try_recv() {
            let id = match &update {
                Update::Phase(id, _) | Update::Finished(id, _) => *id,
            };
            if id == self.status.scan_id && self.capture.is_some() {
                return Some(update);
            }
        }
        None
    }

    pub fn wrist_texts(&self) -> Vec<String> {
        self.blocks
            .iter()
            .map(|blocks| {
                let mut translated = false;
                let mut texts = Vec::new();
                for block in blocks {
                    let translations: Vec<_> = block
                        .translations
                        .iter()
                        .filter_map(|translation| translation.text.as_deref())
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .collect();
                    if translations.is_empty() {
                        let source = block.source.text.trim();
                        if !source.is_empty() {
                            texts.push(source.to_owned());
                        }
                    } else {
                        translated = true;
                        texts.extend(translations.into_iter().map(str::to_owned));
                    }
                }
                (translated, texts)
            })
            .max_by_key(|(translated, texts)| {
                (*translated, texts.iter().map(String::len).sum::<usize>())
            })
            .map(|(_, texts)| texts)
            .unwrap_or_default()
    }

    fn complete_status(&mut self, summary: &ScanSummary, visible: bool) {
        self.status.completed_translations = summary.success_count;
        self.status.failed_translations = summary.failure_count;
        self.status.timed_out = summary.timed_out;
        self.status.state = match summary.outcome {
            ScanOutcome::NoText => OcrState::NoText,
            ScanOutcome::LowConfidence => OcrState::LowConfidence,
            ScanOutcome::SourceOnly if visible => OcrState::SourceVisible,
            ScanOutcome::PartialFailure if visible => OcrState::PartialVisible,
            ScanOutcome::TotalFailure => OcrState::TranslationFailed,
            ScanOutcome::TimedOut => OcrState::TimedOut,
            _ if visible => OcrState::Visible,
            _ => OcrState::Recognized,
        };
        self.status.last_error_code = match summary.outcome {
            ScanOutcome::TotalFailure => Some("translation_failed".into()),
            ScanOutcome::TimedOut => Some("timeout".into()),
            ScanOutcome::PartialFailure if summary.timed_out => Some("partial_timeout".into()),
            ScanOutcome::PartialFailure => Some("partial_failure".into()),
            _ => None,
        };
    }

    fn start(&mut self, capture: Arc<StereoCapture>, config: &VrOcrConfig) -> Result<(), String> {
        let service = self.service.clone().ok_or("OCR service unavailable")?;
        let settings = Arc::new(service.configuration()?);
        if settings.ocr() != config {
            return Err("OCR configuration changed".into());
        }
        self.configuration = Some(settings.clone());
        let metadata = CaptureMetadata::from(capture.as_ref());
        let scan_started = metadata.captured_at;
        self.capture = Some(metadata);
        let id = self.status.scan_id;
        let fraction = config.region_fraction;
        let local = config.backend == vrcs_core::VrOcrBackend::Local;
        let sender = self.sender.clone();
        let encoder = self.encoder.clone();
        let completed_blocks = Arc::new(Mutex::new(ScanProgress::new(id)));
        self.progress = Some(completed_blocks.clone());
        let deadline = tokio::time::Instant::from_std(
            scan_started + std::time::Duration::from_secs(config.timeout_seconds as u64),
        );
        self.task = Some(tauri::async_runtime::spawn(async move {
            let saved_blocks = completed_blocks.clone();
            let work = async {
                let permit = encoder
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| "OCR encoder unavailable")?;
                let encoding_capture = capture;
                let (images, crops) = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    let started = std::time::Instant::now();
                    let mut images = Vec::with_capacity(2);
                    let mut crops = Vec::with_capacity(2);
                    for (eye_index, eye) in encoding_capture.eyes.iter().enumerate() {
                        let eye_started = std::time::Instant::now();
                        let mut crop = center_crop(&eye.image, fraction)?;
                        images.push(OcrImage {
                            data: if local {
                                OcrImageData::Rgba(std::mem::take(&mut crop.image.pixels))
                            } else {
                                OcrImageData::Encoded(encode_png(&crop.image)?)
                            },
                            width: crop.image.width,
                            height: crop.image.height,
                        });
                        let upload_bytes = match &images.last().unwrap().data {
                            OcrImageData::Encoded(bytes) => bytes.len(),
                            OcrImageData::Rgba(_) => 0,
                        };
                        tracing::debug!(
                            scan_id = id,
                            eye = eye_index,
                            elapsed_ms = eye_started.elapsed().as_millis() as u64,
                            upload_bytes,
                            "OCR eye prepared"
                        );
                        crops.push(crop.transform());
                    }
                    let images: [OcrImage; 2] =
                        images.try_into().map_err(|_| "Missing OCR eye image")?;
                    tracing::debug!(
                        scan_id = id,
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        "OCR cropped and encoded"
                    );
                    Ok::<_, String>((images, crops))
                })
                .await
                .map_err(|_| "OCR encoding worker failed")??;
                let progress_sender = sender.clone();
                let completed_crops = &crops;
                let mut result = service
                    .process_scan(
                        images,
                        &settings,
                        id,
                        deadline,
                        move |phase| {
                            let _ = progress_sender.try_send(Update::Phase(id, phase));
                        },
                        move |_| async move { Ok(()) },
                        move |mut update| {
                            if update.scan_id != id || update.eye >= 2 {
                                return;
                            }
                            completed_crops[update.eye].restore(&mut update.block.source.polygon);
                            let mut saved = saved_blocks
                                .lock()
                                .unwrap_or_else(|error| error.into_inner());
                            saved.apply(update);
                        },
                    )
                    .await?;
                for (eye_blocks, crop) in result.blocks.iter_mut().zip(crops) {
                    for block in eye_blocks {
                        crop.restore(&mut block.source.polygon);
                    }
                }
                Ok::<_, String>(result)
            };
            let result = match tokio::time::timeout_at(deadline, work).await {
                Ok(result) => result,
                Err(_) => {
                    let blocks = completed_blocks
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .blocks
                        .clone();
                    let summary =
                        ScanSummary::from_blocks(&blocks, source_view(settings.ocr()), true);
                    Ok(ScanResult { blocks, summary })
                }
            };
            let result = result.map(|result| OcrFrame {
                blocks: result.blocks,
                summary: result.summary,
            });
            tracing::info!(
                scan_id = id,
                elapsed_ms = scan_started.elapsed().as_millis() as u64,
                success = result.is_ok(),
                "OCR worker finished"
            );
            let _ = sender.send(Update::Finished(id, result)).await;
        }));
        Ok(())
    }
}

fn tracking_valid(
    capture: &CaptureMetadata,
    pose: [[f32; 4]; 3],
    scene_pid: u32,
    origin: i32,
) -> bool {
    scene_pid == capture.scene_pid
        && origin == capture.origin
        && pose.iter().flatten().all(|value| value.is_finite())
}

impl Drop for OcrRuntime {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ocr_capture::EyeCapture, renderer::Texture, transform};
    use super::*;
    use vrcs_core::ocr::{BlockTranslation, TextBlock};

    fn capture() -> Arc<StereoCapture> {
        let pose = transform::matrix(0., 0., 0., [0.; 3]);
        let eye = EyeCapture {
            image: Texture {
                width: 2,
                height: 2,
                pixels: vec![0; 16],
            },
            projection: [-1., 1., -1., 1.],
            eye_to_head: pose,
            head_pose: pose,
        };
        Arc::new(StereoCapture {
            eyes: [eye.clone(), eye],
            pose,
            scene_pid: 1,
            origin: 1,
            captured_at: std::time::Instant::now(),
        })
    }

    fn block(id: usize, source: &str, translation: Option<&str>) -> TranslatedBlock {
        TranslatedBlock {
            source: TextBlock {
                id,
                text: source.into(),
                confidence: 0.95,
                polygon: [[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
            },
            translations: vec![BlockTranslation {
                target_language: "zh-Hans".into(),
                text: translation.map(str::to_string),
                error_code: translation.is_none().then(|| "translation_failed".into()),
            }],
        }
    }

    #[test]
    fn ocr_wrist_keeps_untranslated_blocks_between_translations() {
        let mut runtime = OcrRuntime::new(None, None);
        runtime.blocks = [
            vec![
                block(0, "first", Some("translated")),
                block(1, "pending", None),
            ],
            vec![],
        ];
        assert_eq!(runtime.wrist_texts(), ["translated", "pending"]);
    }

    #[test]
    fn ocr_progress_shows_source_then_translation_and_rejects_old_scans() {
        let mut runtime = OcrRuntime::new(None, None);
        runtime.capture = Some(capture().as_ref().into());
        runtime.status.state = OcrState::Translating;
        let progress = Arc::new(Mutex::new(ScanProgress::new(runtime.status.scan_id)));
        runtime.progress = Some(progress.clone());
        let update = |scan_id, text| BlockUpdate {
            scan_id,
            eye: 0,
            target_language: None,
            block: block(0, "source", text),
        };
        progress
            .lock()
            .unwrap()
            .apply(update(runtime.status.scan_id, None));
        runtime.sync_progress();
        assert_eq!(runtime.wrist_texts(), ["source"]);
        assert!(!progress.lock().unwrap().dirty);
        assert!(runtime.displayed_at.is_none());
        progress
            .lock()
            .unwrap()
            .apply(update(runtime.status.scan_id, Some("translated")));
        runtime.sync_progress();
        assert_eq!(runtime.wrist_texts(), ["translated"]);
        assert_eq!(runtime.status.state, OcrState::Translating);
        progress.lock().unwrap().apply(update(
            runtime.status.scan_id.wrapping_add(1),
            Some("stale"),
        ));
        runtime.sync_progress();
        assert_eq!(runtime.wrist_texts(), ["translated"]);
        runtime.clear();
        progress.lock().unwrap().apply(update(0, Some("late")));
        runtime.sync_progress();
        assert!(runtime.wrist_texts().is_empty());
    }

    #[test]
    fn ocr_timeout_keeps_recognized_sources_and_starts_display_time_on_finish() {
        let mut runtime = OcrRuntime::new(None, None);
        let mut metadata: CaptureMetadata = capture().as_ref().into();
        metadata.captured_at -= std::time::Duration::from_secs(60);
        runtime.capture = Some(metadata);
        let blocks = [vec![block(0, "source", None)], vec![]];
        let summary = ScanSummary::from_blocks(&blocks, false, true);
        let finishing = std::time::Instant::now();
        runtime.finish_frame(OcrFrame { blocks, summary });
        assert_eq!(runtime.wrist_texts(), ["source"]);
        assert_eq!(runtime.status.state, OcrState::TimedOut);
        assert!(runtime.status.timed_out);
        assert!(runtime.displayed_at.unwrap() >= finishing);
        let blocks = Default::default();
        let summary = ScanSummary::from_blocks(&blocks, false, true);
        runtime.finish_frame(OcrFrame { blocks, summary });
        assert!(runtime.wrist_texts().is_empty());
        assert!(runtime.capture.is_none());
        assert_eq!(runtime.status.state, OcrState::TimedOut);
    }

    #[test]
    fn wrist_texts_use_the_more_complete_eye_without_stereo_duplicates() {
        let mut runtime = OcrRuntime::new(None, None);
        runtime.blocks = [
            vec![block(1, "hello", Some("你好"))],
            vec![
                block(1, "hello", Some("你好")),
                block(2, "world", Some("世界")),
            ],
        ];

        assert_eq!(runtime.wrist_texts(), vec!["你好", "世界"]);
    }

    #[test]
    fn wrist_texts_fall_back_to_recognized_source_when_translation_is_missing() {
        let mut runtime = OcrRuntime::new(None, None);
        runtime.blocks = [
            vec![block(1, "recognized text", None)],
            vec![block(1, "recognized", None)],
        ];

        assert_eq!(runtime.wrist_texts(), vec!["recognized text"]);
    }

    #[test]
    fn wrist_texts_prefer_a_translation_over_a_longer_source_fallback() {
        let mut runtime = OcrRuntime::new(None, None);
        runtime.blocks = [
            vec![block(1, "source", Some("译文"))],
            vec![block(1, "a much longer recognized source line", None)],
        ];

        assert_eq!(runtime.wrist_texts(), vec!["译文"]);
    }

    #[test]
    fn ocr_runtime_distinguishes_translation_failure_partial_timeout_and_source_view() {
        let mut runtime = OcrRuntime::new(None, None);
        runtime.complete_status(
            &ScanSummary {
                outcome: ScanOutcome::TotalFailure,
                success_count: 0,
                failure_count: 2,
                timed_out: false,
            },
            false,
        );
        assert_eq!(runtime.status.state, OcrState::TranslationFailed);
        assert_eq!(
            runtime.status.last_error_code.as_deref(),
            Some("translation_failed")
        );
        runtime.complete_status(
            &ScanSummary {
                outcome: ScanOutcome::PartialFailure,
                success_count: 2,
                failure_count: 1,
                timed_out: true,
            },
            true,
        );
        assert_eq!(runtime.status.state, OcrState::PartialVisible);
        assert_eq!(
            (
                runtime.status.completed_translations,
                runtime.status.failed_translations
            ),
            (2, 1)
        );
        assert_eq!(
            runtime.status.last_error_code.as_deref(),
            Some("partial_timeout")
        );
        assert!(runtime.status.timed_out);
        runtime.complete_status(
            &ScanSummary {
                outcome: ScanOutcome::SourceOnly,
                success_count: 0,
                failure_count: 0,
                timed_out: false,
            },
            true,
        );
        assert_eq!(runtime.status.state, OcrState::SourceVisible);
        assert!(runtime.status.last_error_code.is_none());
        runtime.complete_status(
            &ScanSummary {
                outcome: ScanOutcome::PartialFailure,
                success_count: 2,
                failure_count: 1,
                timed_out: false,
            },
            false,
        );
        assert_ne!(
            runtime.status.state,
            OcrState::PartialVisible,
            "Unplaceable text must not report visible"
        );
    }

    #[test]
    fn ocr_clearing_rejects_late_phases() {
        let mut runtime = OcrRuntime::new(None, None);
        runtime.capture = Some(capture().as_ref().into());
        let old = runtime.status.scan_id;
        runtime
            .sender
            .try_send(Update::Phase(old, Phase::Translating))
            .unwrap();
        runtime.status.failed_translations = 2;
        runtime.clear();
        assert!(runtime.capture.is_none());
        assert!(runtime.blocks.iter().all(|eye| eye.is_empty()));
        assert_eq!(runtime.status.failed_translations, 0);
        runtime.capture = Some(capture().as_ref().into());
        runtime
            .sender
            .try_send(Update::Phase(runtime.status.scan_id, Phase::Recognizing))
            .unwrap();
        assert!(matches!(
            runtime.next_current_update(),
            Some(Update::Phase(_, Phase::Recognizing))
        ));
        assert!(runtime.next_current_update().is_none());
        runtime.clear();
        runtime
            .sender
            .try_send(Update::Phase(runtime.status.scan_id, Phase::Translating))
            .unwrap();
        assert!(
            runtime.next_current_update().is_none(),
            "An idle scan must not revive processing"
        );
    }

    #[test]
    fn scan_result_is_not_cancelled_by_head_movement() {
        let reference: CaptureMetadata = capture().as_ref().into();
        let moved = transform::matrix(0., 15., 0., [0.08, 0., 0.]);
        assert!(tracking_valid(&reference, moved, 1, 1));
        let walked = transform::matrix(0., 15., 0., [0.15, 0., 0.]);
        assert!(tracking_valid(&reference, walked, 1, 1));
        assert!(!tracking_valid(&reference, moved, 2, 1));
        let mut invalid = moved;
        invalid[0][0] = f32::NAN;
        assert!(!tracking_valid(&reference, invalid, 1, 1));
    }
}
