use super::{
    backend::{OpenVrBackend, OverlayKind},
    ocr_capture::{center_crop, encode_png, CropTransform, StereoCapture},
    ocr_input::install_manifest,
    ocr_progress::{Feedback, ProgressLabels, ProgressView},
    ocr_progress_renderer,
    ocr_selection::Selection,
    ocr_status::{failure_code, OcrState, OcrStatus, OcrWristState},
    ocr_wrist::{Action, LaserEvent, Pointer, Reader, View},
    ocr_wrist_renderer::{self, Button},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, watch};
use vrcs_core::{
    ocr::{
        source_view, BlockUpdate, OcrImage, OcrImageData, Phase, PipelineProgress,
        ScanConfiguration, ScanOutcome, ScanResult, ScanSummary, TranslatedBlock, VrOcrService,
    },
    VrOcrConfig, VrOcrDisplayMode,
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
    source_capture: Option<Arc<StereoCapture>>,
    fallback_blocks: Vec<TranslatedBlock>,
    result_dirty: bool,
    summary: Option<ScanSummary>,
    progress: Option<Arc<Mutex<ScanProgress>>>,
    pipeline: Option<watch::Receiver<Option<PipelineProgress>>>,
    feedback: Option<Feedback>,
    pub progress_labels: ProgressLabels,
    feedback_view: Option<(ProgressView, u8)>,
    configuration: Option<Arc<ScanConfiguration>>,
    selecting: bool,
    selection: Option<Selection>,
    displayed_at: Option<std::time::Instant>,
    reader: Reader,
    wrist_eye: Option<usize>,
    wrist_view: Option<View>,
    wrist_buttons: Vec<Button>,
    wrist_pages: usize,
    wrist_pointer: Pointer,
    wrist_hovered: Option<Action>,
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
            source_capture: None,
            fallback_blocks: Vec::new(),
            result_dirty: false,
            summary: None,
            progress: None,
            pipeline: None,
            feedback: None,
            progress_labels: ProgressLabels::default(),
            feedback_view: None,
            configuration: None,
            selecting: false,
            selection: None,
            displayed_at: None,
            reader: Reader::default(),
            wrist_eye: None,
            wrist_view: None,
            wrist_buttons: Vec::new(),
            wrist_pages: 1,
            wrist_pointer: Pointer::default(),
            wrist_hovered: None,
            encoder: Arc::new(tokio::sync::Semaphore::new(1)),
            blocks: Default::default(),
            status: OcrStatus::default(),
        }
    }

    fn reading_blocks(&mut self, mode: VrOcrDisplayMode) -> Vec<TranslatedBlock> {
        if mode == VrOcrDisplayMode::Stereo {
            self.fallback_blocks.clone()
        } else {
            if self.wrist_eye.is_none() {
                self.wrist_eye = self
                    .blocks
                    .iter()
                    .enumerate()
                    .filter(|(_, blocks)| !blocks.is_empty())
                    .max_by_key(|(_, blocks)| {
                        blocks
                            .iter()
                            .map(|block| block.source.text.len())
                            .sum::<usize>()
                    })
                    .map(|(eye, _)| eye);
            }
            self.wrist_eye
                .map(|eye| self.blocks[eye].clone())
                .unwrap_or_default()
        }
    }

    fn expired(&self, now: std::time::Instant, seconds: f32) -> bool {
        !self.reader.pinned
            && self.task.is_none()
            && self.displayed_at.is_some_and(|started| {
                now.saturating_duration_since(started).as_secs_f32() > seconds
            })
    }

    pub fn clear(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.status.scan_id = self.status.scan_id.wrapping_add(1);
        self.capture = None;
        self.source_capture = None;
        self.fallback_blocks.clear();
        self.result_dirty = false;
        self.summary = None;
        self.progress = None;
        self.pipeline = None;
        self.feedback = None;
        self.feedback_view = None;
        self.status.progress = None;
        self.status.progress_error = None;
        self.configuration = None;
        self.selecting = false;
        self.selection = None;
        self.displayed_at = None;
        self.reader = Reader::default();
        self.reader.sync(self.status.scan_id, &[]);
        self.wrist_eye = None;
        self.wrist_view = None;
        self.wrist_buttons.clear();
        self.wrist_pages = 1;
        self.wrist_pointer = Pointer::default();
        self.wrist_hovered = None;
        self.blocks = Default::default();
        self.status.block_count = 0;
        self.status.layout_limited = false;
        self.status.completed_translations = 0;
        self.status.failed_translations = 0;
        self.status.timed_out = false;
        self.status.last_error_code = None;
        self.status.last_error = None;
        self.status.state = OcrState::Ready;
        self.status.wrist_state = OcrWristState::Hidden;
        self.status.wrist_error = None;
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

    fn invalidate(&mut self, config: &VrOcrConfig) {
        let feedback = self.feedback.take();
        self.clear();
        self.status.state = OcrState::Invalid;
        let now = std::time::Instant::now();
        let mut feedback = feedback
            .unwrap_or_else(|| Feedback::new(self.status.scan_id, source_view(config), now));
        feedback.results_changed();
        feedback.fail(OcrState::Invalid, "view_changed", now);
        self.feedback = Some(feedback);
        self.displayed_at = Some(now);
    }

    pub fn tick(
        &mut self,
        backend: &mut OpenVrBackend,
        config: &VrOcrConfig,
        wrist: &vrcs_core::VrOverlayWristConfig,
    ) {
        if self.capture.is_none() && !self.selecting {
            backend.reset_ocr();
        }
        if !config.enabled {
            backend.reset_ocr();
            backend.reset(OverlayKind::OcrWrist);
            backend.reset(OverlayKind::OcrProgress);
            backend.reset_ocr_input();
            self.unavailable(false);
            return;
        }
        let result = self.update(backend, config).and_then(|_| {
            let changed = self.result_dirty;
            self.update_result(backend, config)?;
            if changed {
                let blocks = self.reading_blocks(config.display_mode);
                self.reader.sync(self.status.scan_id, &blocks);
            }
            Ok(())
        });
        if let Err(error) = result {
            let feedback = self.feedback.take();
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
            self.displayed_at = Some(std::time::Instant::now());
            let now = std::time::Instant::now();
            let mut feedback = feedback
                .unwrap_or_else(|| Feedback::new(self.status.scan_id, source_view(config), now));
            feedback.results_changed();
            feedback.fail(self.status.state, code, now);
            self.feedback = Some(feedback);
        }
        if let Err(error) = self.update_wrist(backend, config, wrist) {
            backend.reset(OverlayKind::OcrWrist);
            self.wrist_view = None;
            self.status.wrist_state = OcrWristState::Error;
            self.status.wrist_error = Some(error);
        }
        if self.task.is_none() && self.summary.is_some() {
            if let Some(feedback) = &mut self.feedback {
                let state = if self.status.timed_out {
                    OcrState::TimedOut
                } else {
                    self.status.state
                };
                feedback.finish(state, std::time::Instant::now());
            }
        }
        if let Err(error) = self.update_feedback(backend) {
            tracing::warn!(error, "OCR progress overlay unavailable");
            backend.reset(OverlayKind::OcrProgress);
            self.feedback_view = None;
            self.status.progress_error = Some(error);
        }
    }

    fn sync_pipeline(&mut self) {
        if let Some(receiver) = &mut self.pipeline {
            // The worker can close the channel before this tick; its final value is still valid.
            if let (Some(snapshot), Some(feedback)) =
                (receiver.borrow_and_update().clone(), &mut self.feedback)
            {
                feedback.apply(snapshot, std::time::Instant::now());
            }
        }
    }

    fn update_feedback(&mut self, backend: &mut OpenVrBackend) -> Result<(), String> {
        let now = std::time::Instant::now();
        self.status.progress = self
            .feedback
            .as_ref()
            .and_then(|feedback| feedback.view_with_labels(now, &self.progress_labels));
        let Some(view) = &self.status.progress else {
            backend.hide(OverlayKind::OcrProgress);
            self.feedback_view = None;
            return Ok(());
        };
        if self.status.progress_error.is_some() {
            return Ok(());
        }
        let animation = self
            .feedback
            .as_ref()
            .map(|feedback| feedback.animation(now))
            .unwrap_or(0);
        let next = (view.clone(), animation);
        backend.ensure_ocr_progress()?;
        if self.feedback_view.as_ref() != Some(&next) {
            let texture = ocr_progress_renderer::render(view, animation)?;
            backend.upload(OverlayKind::OcrProgress, &texture)?;
            self.feedback_view = Some(next);
        }
        backend.set_opacity(OverlayKind::OcrProgress, 0.94)?;
        backend.show(OverlayKind::OcrProgress)
    }

    fn update_wrist_pointer(&mut self, events: &[LaserEvent], focused: bool) -> bool {
        // Suppress scan/clear even if a complete click and focus loss arrive in the same tick.
        let reading_focus = focused || events.iter().any(|event| event.point().is_some());
        if self.status.wrist_state == OcrWristState::Visible {
            for event in events.iter().copied() {
                self.wrist_hovered = event.point().and_then(|[x, y]| {
                    self.wrist_buttons
                        .iter()
                        .find(|button| {
                            let [left, top, right, bottom] = button.bounds;
                            x >= left && x < right && y >= top && y < bottom
                        })
                        .map(|button| button.action)
                });
                if let Some(action) = self.wrist_pointer.event(event, self.wrist_hovered) {
                    tracing::info!(?action, "SteamVR OCR wrist button clicked");
                    self.reading_action(action);
                    if action == Action::Close {
                        break;
                    }
                }
            }
        }
        if !focused || self.status.wrist_state != OcrWristState::Visible {
            self.wrist_pointer = Pointer::default();
            self.wrist_hovered = None;
        }
        reading_focus
    }

    fn update(&mut self, backend: &mut OpenVrBackend, config: &VrOcrConfig) -> Result<(), String> {
        let (events, focused) = backend.poll_ocr_wrist_events()?;
        let reading_focus = self.update_wrist_pointer(&events, focused);
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
            reading_focus,
        )?;
        for action in input.navigation.iter().copied() {
            self.reading_action(action);
        }
        self.status.controller_bound = input.available;
        self.status.gesture_available = input.gesture_available;
        if input.clear {
            self.clear();
            backend.reset_ocr();
            return Ok(());
        }
        if self.expired(std::time::Instant::now(), config.display_seconds) {
            self.clear();
            backend.reset_ocr();
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
            if !configuration_valid {
                self.clear();
                backend.reset_ocr();
            } else if !valid {
                self.invalidate(config);
                backend.reset_ocr();
            }
        }
        if input.frame_cancelled && self.selecting {
            self.clear();
            backend.reset_ocr();
            return Ok(());
        }
        let mut capture_requested = false;
        if let Some(frame) = &input.frame {
            if !self.selecting {
                self.clear();
                backend.reset_ocr();
                self.selecting = true;
            }
            let (eyes, pid, origin) = backend.ocr_view()?;
            if frame.origin != origin
                || self.selection.as_ref().is_some_and(|selection| {
                    selection.scene_pid != pid || selection.origin != origin
                })
            {
                self.invalidate(config);
                backend.reset_ocr();
                return Ok(());
            }
            let selection = if let Some(selection) = &self.selection {
                selection.with_corners(frame.corners)
            } else {
                Selection::from_corners(frame.corners, frame.head_pose, pid, origin)
            };
            let Some(selection) = selection else {
                // The hands can pass through the same row while the user adjusts the diagonal.
                // Keep the initial axes while the current frame is degenerate.
                backend.reset_ocr();
                self.status.state = OcrState::Selecting;
                if input.frame_confirmed {
                    self.invalidate(config);
                }
                return Ok(());
            };
            let visible = eyes.iter().all(|eye| selection.crop_bounds(eye).is_some());
            self.selection = Some(selection);
            // Hide an off-screen preview without ending the frame session.
            if !visible {
                backend.reset_ocr();
                self.status.state = OcrState::Selecting;
                if input.frame_confirmed {
                    self.invalidate(config);
                }
                return Ok(());
            }
            if input.frame_confirmed {
                self.selecting = false;
                backend.reset_ocr();
                self.status.state = OcrState::Capturing;
                capture_requested = true;
            } else {
                self.status.state = OcrState::Selecting;
                let plane = self
                    .selection
                    .as_ref()
                    .ok_or("Invalid OCR frame position")?
                    .preview();
                backend.ensure_ocr_plane(OverlayKind::OcrFrame, &plane, origin)?;
                backend.upload(OverlayKind::OcrFrame, &plane.texture)?;
                backend.set_opacity(OverlayKind::OcrFrame, 1.)?;
                backend.show(OverlayKind::OcrFrame)?;
            }
        } else if input.scan && !self.selecting {
            self.clear();
            backend.reset_ocr();
            self.status.state = OcrState::Capturing;
            capture_requested = true;
        }
        if capture_requested {
            backend.hide(OverlayKind::OcrProgress);
            let started = std::time::Instant::now();
            let capture = Arc::new(backend.capture_ocr(config, self.selection.as_ref())?);
            if self.selection.as_ref().is_some_and(|selection| {
                selection.scene_pid != capture.scene_pid || selection.origin != capture.origin
            }) {
                return Err("OCR scene changed before capture".into());
            }
            tracing::info!(
                scan_id = self.status.scan_id,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "OCR captured"
            );
            self.start(capture, config)?;
        }
        self.sync_pipeline();
        while let Some(update) = self.next_current_update() {
            match update {
                Update::Phase(id, phase) if id == self.status.scan_id => {
                    self.status.state = match phase {
                        Phase::WaitingWorker => OcrState::Recognizing,
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
                    self.sync_pipeline();
                    self.finish_frame(result?);
                }
                _ => {}
            }
        }
        self.sync_progress();
        if self.capture.is_none()
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
        self.pipeline = None;
        if let Some(feedback) = &mut self.feedback {
            feedback.results_changed();
        }
        self.blocks = frame.blocks;
        self.summary = Some(frame.summary.clone());
        self.result_dirty = true;
        self.status.layout_limited = false;
        self.status.block_count = self.blocks.iter().map(Vec::len).max().unwrap_or(0);
        let visible = !self.wrist_texts().is_empty();
        self.complete_status(&frame.summary, visible);
        self.displayed_at = Some(std::time::Instant::now());
        if !visible {
            self.capture = None;
            self.configuration = None;
        }
    }

    fn reading_action(&mut self, action: Action) {
        if action == Action::Close {
            self.clear();
            return;
        }
        self.reader.act(action, self.wrist_pages);
        if self.displayed_at.is_some() {
            self.displayed_at = Some(std::time::Instant::now());
        }
    }

    fn update_wrist(
        &mut self,
        backend: &mut OpenVrBackend,
        config: &VrOcrConfig,
        subtitle_wrist: &vrcs_core::VrOverlayWristConfig,
    ) -> Result<(), String> {
        let mut view = self.reader.view(
            self.status.state,
            source_view(config),
            config.display_mode == VrOcrDisplayMode::Stereo,
        );
        let visible = match config.display_mode {
            VrOcrDisplayMode::Stereo => view.block_count > 0,
            VrOcrDisplayMode::Wrist => {
                view.block_count > 0
                    || matches!(
                        self.status.state,
                        OcrState::Submitting
                            | OcrState::Pending
                            | OcrState::Running
                            | OcrState::Downloading
                            | OcrState::LoadingModel
                            | OcrState::Recognizing
                            | OcrState::Translating
                            | OcrState::NoText
                            | OcrState::LowConfidence
                            | OcrState::Error
                            | OcrState::TimedOut
                    )
            }
        };
        if !visible {
            backend.reset(OverlayKind::OcrWrist);
            self.wrist_view = None;
            self.status.wrist_state = OcrWristState::Hidden;
            self.status.wrist_error = None;
            return Ok(());
        }
        let config_wrist = config
            .wrist
            .clone()
            .unwrap_or_else(|| vrcs_core::VrOcrWristConfig::from(subtitle_wrist));
        if !backend.ensure_ocr_wrist(&config_wrist)?.available {
            self.status.wrist_state = OcrWristState::DeviceUnavailable;
            self.status.wrist_error = None;
            return Ok(());
        }
        view.hovered = self.wrist_hovered;
        let has_blocks = view.block_count > 0;
        if self.wrist_view.as_ref() != Some(&view) {
            let rendered = ocr_wrist_renderer::render(
                &view,
                config_wrist.font_size_px,
                config.background_opacity,
            )?;
            self.reader.clamp_page(rendered.page_count);
            self.wrist_pages = rendered.page_count;
            self.wrist_buttons = rendered.buttons;
            backend.upload(OverlayKind::OcrWrist, &rendered.texture)?;
            self.wrist_view = Some(view);
        }
        backend.set_opacity(OverlayKind::OcrWrist, config_wrist.opacity)?;
        backend.show(OverlayKind::OcrWrist)?;
        if has_blocks {
            if let Some(feedback) = &mut self.feedback {
                feedback.displayed();
            }
        }
        self.status.wrist_state = OcrWristState::Visible;
        self.status.wrist_error = None;
        Ok(())
    }

    fn sync_progress(&mut self) {
        let Some(progress) = &self.progress else {
            return;
        };
        let mut progress = progress.lock().unwrap_or_else(|error| error.into_inner());
        if self.capture.is_some() && progress.scan_id == self.status.scan_id && progress.dirty {
            self.blocks = progress.blocks.clone();
            self.result_dirty = true;
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

    fn result_frame(
        &mut self,
        config: &VrOcrConfig,
    ) -> Result<Option<(super::ocr_plane::PlaneOverlay, bool)>, String> {
        self.fallback_blocks.clear();
        if config.display_mode != VrOcrDisplayMode::Stereo || self.blocks.iter().all(Vec::is_empty)
        {
            return Ok(None);
        }
        let result = if let Some(capture) = &self.source_capture {
            Some(super::ocr_plane::render(
                capture
                    .eyes
                    .as_slice()
                    .try_into()
                    .map_err(|_| "Missing stereo capture")?,
                &self.blocks,
                config.background_opacity,
                source_view(config),
            )?)
        } else {
            None
        };
        if let Some((plane, fallback)) = result {
            self.fallback_blocks = fallback;
            Ok(plane.map(|plane| (plane, !self.fallback_blocks.is_empty())))
        } else {
            self.fallback_blocks = self.preferred_blocks().to_vec();
            Ok(None)
        }
    }

    fn update_result(
        &mut self,
        backend: &mut OpenVrBackend,
        config: &VrOcrConfig,
    ) -> Result<(), String> {
        if self.selecting {
            return Ok(());
        }
        if config.display_mode != VrOcrDisplayMode::Stereo || self.capture.is_none() {
            backend.reset_ocr();
            self.result_dirty = false;
            return Ok(());
        }
        if !self.result_dirty {
            return Ok(());
        }
        // A partial recognition update can contain only one eye. Wait before choosing a fallback.
        if self.task.is_some() && self.blocks.iter().any(Vec::is_empty) {
            return Ok(());
        }
        backend.reset(OverlayKind::OcrFrame);
        let visible;
        if let Some((plane, limited)) = self.result_frame(config)? {
            let origin = self
                .capture
                .as_ref()
                .ok_or("OCR capture is missing")?
                .origin;
            backend.ensure_ocr_plane(OverlayKind::OcrResult, &plane, origin)?;
            backend.upload(OverlayKind::OcrResult, &plane.texture)?;
            backend.set_opacity(OverlayKind::OcrResult, 1.0)?;
            backend.show(OverlayKind::OcrResult)?;
            if let Some(feedback) = &mut self.feedback {
                feedback.plane_displayed(limited);
            }
            visible = true;
            self.status.layout_limited = limited;
        } else {
            backend.reset_ocr();
            self.status.layout_limited = !self.blocks.iter().all(Vec::is_empty);
            visible = !self.fallback_texts().is_empty();
        }
        if let Some(summary) = self.summary.clone() {
            self.complete_status(&summary, visible);
        }
        self.result_dirty = false;
        Ok(())
    }

    pub fn fallback_texts(&self) -> Vec<String> {
        block_texts(&self.fallback_blocks).1
    }

    pub fn wrist_texts(&self) -> Vec<String> {
        block_texts(self.preferred_blocks()).1
    }

    fn preferred_blocks(&self) -> &[TranslatedBlock] {
        self.blocks
            .iter()
            .max_by_key(|blocks| {
                let (translated, texts) = block_texts(blocks);
                (translated, texts.iter().map(String::len).sum::<usize>())
            })
            .map(Vec::as_slice)
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
        self.feedback = Some(Feedback::new(
            self.status.scan_id,
            source_view(config),
            scan_started,
        ));
        self.capture = Some(metadata);
        // Keep source pixels while visible so each translated patch can match its background.
        self.source_capture =
            (config.display_mode == VrOcrDisplayMode::Stereo).then(|| capture.clone());
        let id = self.status.scan_id;
        let selection = self.selection.take();
        let sender = self.sender.clone();
        let encoder = self.encoder.clone();
        let completed_blocks = Arc::new(Mutex::new(ScanProgress::new(id)));
        let (pipeline_sender, pipeline_receiver) = watch::channel(None);
        self.pipeline = Some(pipeline_receiver);
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
                let preparing_settings = settings.clone();
                let (images, crops) = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    prepare_images(
                        &encoding_capture,
                        preparing_settings.ocr(),
                        selection.as_ref(),
                        id,
                    )
                })
                .await
                .map_err(|_| "OCR encoding worker failed")??;
                let progress_sender = sender.clone();
                let completed_crops = &crops;
                let mut result = service
                    .process_vr_scan_with_progress(
                        images,
                        &settings,
                        id,
                        deadline,
                        move |phase| {
                            let _ = progress_sender.try_send(Update::Phase(id, phase));
                        },
                        move |mut update| {
                            if update.scan_id != id || update.eye >= 2 {
                                return;
                            }
                            completed_crops[update.eye].restore_group(&mut update.block);
                            let mut saved = saved_blocks
                                .lock()
                                .unwrap_or_else(|error| error.into_inner());
                            saved.apply(update);
                        },
                        move |snapshot| {
                            pipeline_sender.send_replace(Some(snapshot));
                        },
                    )
                    .await?;
                for (eye_blocks, crop) in result.blocks.iter_mut().zip(crops) {
                    for block in eye_blocks {
                        crop.restore_group(block);
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

fn prepare_images(
    capture: &StereoCapture,
    config: &VrOcrConfig,
    selection: Option<&Selection>,
    scan_id: u64,
) -> Result<(Vec<OcrImage>, Vec<CropTransform>), String> {
    let started = std::time::Instant::now();
    let eye_count = if config.display_mode == VrOcrDisplayMode::Stereo {
        2
    } else {
        1
    };
    if capture.eyes.len() < eye_count {
        return Err("Missing OCR eye capture".into());
    }
    let prepare = |index: usize| {
        let eye_started = std::time::Instant::now();
        let eye = &capture.eyes[index];
        let crop = if let Some(selection) = selection {
            selection.crop(eye)?
        } else {
            // The compositor readback has already selected the requested region.
            center_crop(&eye.image, 1.0)?
        };
        let transform = crop.transform();
        let (width, height) = (crop.image.width, crop.image.height);
        let data = if config.backend == vrcs_core::VrOcrBackend::Local {
            OcrImageData::Rgba(crop.image.pixels)
        } else {
            OcrImageData::Encoded(encode_png(crop.image)?)
        };
        let upload_bytes = match &data {
            OcrImageData::Encoded(bytes) => bytes.len(),
            OcrImageData::Rgba(_) => 0,
        };
        tracing::debug!(
            scan_id,
            eye = index,
            elapsed_ms = eye_started.elapsed().as_millis() as u64,
            upload_bytes,
            "OCR eye prepared"
        );
        Ok::<_, String>((
            OcrImage {
                data,
                width,
                height,
            },
            transform,
        ))
    };
    let prepared = std::thread::scope(|scope| {
        let right = (eye_count == 2).then(|| scope.spawn(|| prepare(1)));
        let left = prepare(0);
        let right = right
            .map(|worker| {
                worker
                    .join()
                    .map_err(|_| "OCR eye preparation worker failed".to_string())?
            })
            .transpose()?;
        let mut images = vec![left?];
        if let Some(right) = right {
            images.push(right);
        }
        Ok::<_, String>(images)
    })?;
    tracing::info!(
        scan_id,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "OCR cropped and encoded"
    );
    Ok(prepared.into_iter().unzip())
}

fn block_texts(blocks: &[TranslatedBlock]) -> (bool, Vec<String>) {
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
    #[test]
    fn invalid_view_ends_feedback_and_clear_removes_it() {
        let now = std::time::Instant::now();
        let mut runtime = super::OcrRuntime::new(None, None);
        let id = runtime.status.scan_id;
        runtime.feedback = Some(super::Feedback::new(id, false, now));
        runtime.invalidate(&vrcs_core::VrOcrConfig::default());
        let view = runtime.feedback.as_ref().unwrap().view(now).unwrap();
        assert!(!view
            .stages
            .contains(&super::super::ocr_progress::Stage::Active));
        assert!(view.status.contains("View changed"));
        runtime.clear();
        assert!(runtime.feedback.is_none());
        assert!(runtime.pipeline.is_none());
        assert!(runtime.status.progress.is_none());
    }

    #[test]
    fn latest_pipeline_snapshot_survives_sender_drop() {
        let mut runtime = super::OcrRuntime::new(None, None);
        let id = runtime.status.scan_id;
        runtime.feedback = Some(super::Feedback::new(id, false, std::time::Instant::now()));
        let (sender, receiver) = tokio::sync::watch::channel(None);
        runtime.pipeline = Some(receiver);
        sender.send_replace(Some(super::PipelineProgress {
            scan_id: id,
            images_total: 1,
            images_done: 1,
            recognition_done: true,
            translation_started: true,
            translation_total: 3,
            translation_completed: 2,
            translation_total_final: true,
            ..Default::default()
        }));
        drop(sender);
        runtime.sync_pipeline();
        assert_eq!(
            runtime
                .feedback
                .as_ref()
                .unwrap()
                .view(std::time::Instant::now())
                .unwrap()
                .translation_fraction,
            Some((2, 3))
        );
    }

    use super::super::{ocr_capture::EyeCapture, renderer::Texture, transform};
    use super::*;
    use vrcs_core::ocr::{BlockTranslation, TextBlock};

    #[test]
    fn wrist_preparation_skips_the_other_eye() {
        let mut capture = capture().as_ref().clone();
        capture.eyes[1].image.pixels.clear();
        let config = VrOcrConfig {
            display_mode: VrOcrDisplayMode::Wrist,
            backend: vrcs_core::VrOcrBackend::Local,
            ..Default::default()
        };
        let (images, crops) = prepare_images(&capture, &config, None, 1).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(crops.len(), 1);
        assert_eq!((images[0].width, images[0].height), (2, 2));
        let OcrImageData::Rgba(pixels) = &images[0].data else {
            panic!("Local recognition requires pixels");
        };
        assert_eq!(pixels, &[0; 16]);
    }

    #[test]
    fn stereo_preparation_keeps_eye_order_and_restores_coordinates() {
        let mut capture = capture().as_ref().clone();
        capture.eyes[0].image.pixels = [10, 20, 30, 255].repeat(4);
        capture.eyes[1].image.pixels = [40, 50, 60, 255].repeat(4);
        let config = VrOcrConfig {
            display_mode: VrOcrDisplayMode::Stereo,
            backend: vrcs_core::VrOcrBackend::Local,
            ..Default::default()
        };
        let (images, crops) = prepare_images(&capture, &config, None, 1).unwrap();
        assert_eq!(images.len(), 2);
        for (index, image) in images.iter().enumerate() {
            let OcrImageData::Rgba(pixels) = &image.data else {
                panic!("Expected pixels");
            };
            assert_eq!(pixels, &capture.eyes[index].image.pixels);
            let mut polygon = [[0., 0.], [2., 0.], [2., 2.], [0., 2.]];
            crops[index].restore(&mut polygon);
            assert_eq!(polygon, [[0., 0.], [2., 0.], [2., 2.], [0., 2.]]);
        }
        capture.eyes.truncate(1);
        assert!(prepare_images(&capture, &config, None, 1).is_err());
    }

    #[test]
    fn native_laser_events_operate_reader_controls_and_keep_scan_suppressed_for_the_batch() {
        use super::super::ocr_wrist::LaserEvent;
        for action in [Action::NextPage, Action::TogglePin, Action::Close] {
            let mut runtime = OcrRuntime::new(None, None);
            runtime.status.wrist_state = OcrWristState::Visible;
            runtime.wrist_pages = 2;
            runtime.wrist_buttons = vec![Button {
                action,
                bounds: [0.75, 0.9, 0.95, 0.99],
            }];
            let events = [
                LaserEvent::Down {
                    device: 2,
                    point: [0.9, 0.95],
                },
                LaserEvent::Up {
                    device: 2,
                    point: [0.9, 0.95],
                },
                LaserEvent::Leave,
            ];
            assert!(runtime.update_wrist_pointer(&events, false));
            let view = runtime.reader.view(OcrState::Visible, false, false);
            match action {
                Action::NextPage => assert_eq!(view.page, 1),
                Action::TogglePin => assert!(view.pinned),
                Action::Close => assert_eq!(runtime.status.wrist_state, OcrWristState::Hidden),
                _ => unreachable!(),
            }
            assert!(runtime.wrist_hovered.is_none());
            assert!(!runtime.update_wrist_pointer(&[], false));
        }
    }

    #[test]
    fn native_laser_focus_loss_clears_hover_and_prevents_an_orphan_release() {
        use super::super::ocr_wrist::LaserEvent;
        let mut runtime = OcrRuntime::new(None, None);
        runtime.status.wrist_state = OcrWristState::Visible;
        runtime.wrist_buttons = vec![Button {
            action: Action::TogglePin,
            bounds: [0., 0., 1., 1.],
        }];
        runtime.update_wrist_pointer(
            &[LaserEvent::Down {
                device: 2,
                point: [0.5, 0.5],
            }],
            true,
        );
        assert_eq!(runtime.wrist_hovered, Some(Action::TogglePin));
        runtime.update_wrist_pointer(&[LaserEvent::Leave], false);
        assert!(runtime.wrist_hovered.is_none());
        runtime.update_wrist_pointer(
            &[LaserEvent::Up {
                device: 2,
                point: [0.5, 0.5],
            }],
            true,
        );
        assert!(!runtime.reader.pinned);
    }

    #[test]
    fn wrist_reading_source_does_not_switch_eyes_when_translation_arrives() {
        let mut runtime = OcrRuntime::new(None, None);
        runtime.blocks = [
            vec![block(1, "complete original", None)],
            vec![block(1, "short", None)],
        ];
        assert_eq!(
            runtime.reading_blocks(VrOcrDisplayMode::Wrist)[0]
                .source
                .text,
            "complete original"
        );
        runtime.blocks[1][0].translations[0].text = Some("translated".into());
        assert_eq!(
            runtime.reading_blocks(VrOcrDisplayMode::Wrist)[0]
                .source
                .text,
            "complete original"
        );
    }

    #[test]
    fn pinned_reader_survives_expiry_but_new_scan_resets_pin() {
        let mut runtime = OcrRuntime::new(None, None);
        let now = std::time::Instant::now();
        runtime.displayed_at = Some(now - std::time::Duration::from_secs(20));
        assert!(runtime.expired(now, 15.));
        runtime.reader.pinned = true;
        assert!(!runtime.expired(now, 15.));
        runtime.clear();
        assert!(!runtime.reader.pinned);
    }

    #[test]
    fn wrist_fallback_keeps_only_unplaced_paired_blocks() {
        let mut runtime = OcrRuntime::new(None, None);
        runtime.blocks = [vec![block(1, "placed", Some("已放置"))], vec![]];
        runtime.fallback_blocks = vec![block(2, "unplaced", Some("未放置"))];
        let result = runtime.reading_blocks(VrOcrDisplayMode::Stereo);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source.text, "unplaced");
        assert_eq!(result[0].translations[0].text.as_deref(), Some("未放置"));
    }

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
            eyes: vec![eye.clone(), eye],
            pose,
            scene_pid: 1,
            origin: 1,
            captured_at: std::time::Instant::now(),
        })
    }

    fn block(id: usize, source: &str, translation: Option<&str>) -> TranslatedBlock {
        TranslatedBlock {
            fragments: vec![],
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

    fn result_fixture() -> (OcrRuntime, VrOcrConfig) {
        let mut runtime = OcrRuntime::new(None, None);
        let mut eyes = capture().eyes.clone();
        for (index, eye) in eyes.iter_mut().enumerate() {
            eye.image = Texture {
                width: 320,
                height: 240,
                pixels: vec![],
            };
            eye.eye_to_head =
                transform::matrix(0., 0., 0., [if index == 0 { -0.03 } else { 0.03 }, 0., 0.]);
        }
        runtime.source_capture = Some(Arc::new(StereoCapture {
            eyes,
            ..capture().as_ref().clone()
        }));
        let mut left = block(0, "source", Some("VR"));
        left.source.polygon = [[92.4, 80.], [232.4, 80.], [232.4, 150.], [92.4, 150.]];
        let mut right = block(7, "source", None);
        right.source.polygon = left.source.polygon.map(|[x, y]| [x - 4.8, y]);
        runtime.blocks = [vec![left], vec![right]];
        let mut config = VrOcrConfig {
            display_mode: VrOcrDisplayMode::Stereo,
            background_opacity: 0.4,
            ..Default::default()
        };
        config.targets[0].profile_id = Some("translation-profile".into());
        (runtime, config)
    }

    #[test]
    fn result_display_uses_one_plane_and_ocr_background() {
        let (mut runtime, config) = result_fixture();
        let (plane, limited) = runtime.result_frame(&config).unwrap().unwrap();
        assert!(!limited);
        assert!(plane.texture.width < 320);
        assert!((plane.pose[2][3] + 2.).abs() < 0.01);
        assert!((plane.texel_aspect - 0.75).abs() < 0.01);
        assert!(plane
            .texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] == 102));
        assert!(plane
            .texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] > 0));
        assert!(runtime
            .result_frame(&VrOcrConfig {
                display_mode: VrOcrDisplayMode::Wrist,
                ..config.clone()
            })
            .unwrap()
            .is_none());
        runtime.clear();
        assert!(runtime.result_frame(&config).unwrap().is_none());
    }

    #[test]
    fn cropped_capture_preserves_stereo_overlay_position() {
        let (mut runtime, config) = result_fixture();
        let original = runtime.result_frame(&config).unwrap().unwrap().0;
        let capture = Arc::make_mut(runtime.source_capture.as_mut().unwrap());
        for (index, eye) in capture.eyes.iter_mut().enumerate() {
            let bounds @ [x, y, right, bottom] = [60, 40, 260, 200];
            eye.projection = super::super::ocr_capture::crop_projection(
                eye.projection,
                [eye.image.width, eye.image.height],
                bounds,
            );
            eye.image.width = right - x;
            eye.image.height = bottom - y;
            for block in &mut runtime.blocks[index] {
                for point in &mut block.source.polygon {
                    point[0] -= x as f32;
                    point[1] -= y as f32;
                }
            }
        }
        let cropped = runtime.result_frame(&config).unwrap().unwrap().0;
        for (actual, expected) in cropped
            .pose
            .iter()
            .flatten()
            .zip(original.pose.iter().flatten())
        {
            assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
        }
        assert!((cropped.width_m - original.width_m).abs() < 0.0001);
        assert!((cropped.texel_aspect - original.texel_aspect).abs() < 0.0001);
    }

    #[test]
    fn result_display_uses_ready_translation_from_either_eye() {
        let (mut runtime, config) = result_fixture();
        let (left_ready, _) = runtime.result_frame(&config).unwrap().unwrap();
        runtime.blocks[0][0].translations[0].text = None;
        runtime.blocks[1][0].translations[0].text = Some("VR".into());
        let (right_ready, _) = runtime.result_frame(&config).unwrap().unwrap();
        assert_eq!(left_ready.texture.pixels, right_ready.texture.pixels);
        assert_eq!(left_ready.pose, right_ready.pose);
    }

    #[test]
    fn whole_group_fallback_keeps_unique_text_from_both_eyes_without_a_plane() {
        for unmatched in [false, true] {
            let (mut runtime, config) = result_fixture();
            for (eye, groups) in runtime.blocks.iter_mut().enumerate() {
                let group = &mut groups[0];
                let mut first = group.source.clone();
                first.text = if eye == 1 && unmatched {
                    "unmatched"
                } else {
                    "shared fragment"
                }
                .into();
                let mut second = first.clone();
                second.id = 1;
                second.text = format!("eye {eye} fragment");
                group.fragments = vec![first, second];
                group.source.id = 0;
                group.source.text = format!("shared fragment eye {eye} fragment");
                groups.push(block(
                    1,
                    &format!("eye {eye} only"),
                    Some(&format!("eye {eye} translation")),
                ));
            }
            assert!(runtime.result_frame(&config).unwrap().is_none());
            let fallback = runtime.reading_blocks(VrOcrDisplayMode::Stereo);
            assert_eq!(
                fallback.len(),
                4,
                "both complete groups and both single-eye results survive"
            );
            assert!(fallback
                .iter()
                .any(|block| block.source.text == "eye 0 only"));
            assert!(fallback
                .iter()
                .any(|block| block.source.text == "eye 1 only"));
            let ids: std::collections::HashSet<_> =
                fallback.iter().map(|block| block.source.id).collect();
            assert_eq!(ids.len(), 4);
        }
    }

    #[test]
    fn result_display_rejects_unmatched_geometry_and_keeps_wrist_translation() {
        let (mut runtime, config) = result_fixture();
        runtime.blocks[1][0].source.text = "unmatched".into();
        assert!(runtime.result_frame(&config).unwrap().is_none());
        assert_eq!(runtime.wrist_texts(), ["VR"]);
    }

    #[test]
    fn placed_result_does_not_duplicate_text_on_the_wrist() {
        let (mut runtime, config) = result_fixture();
        assert!(runtime.result_frame(&config).unwrap().is_some());
        assert!(runtime.fallback_texts().is_empty());
    }

    #[test]
    fn unplaceable_translation_keeps_other_results_on_the_source_plane() {
        let (mut runtime, config) = result_fixture();
        let long = "More text ".repeat(2000);
        let mut extra = block(1, "extra", Some(&long));
        extra.source.polygon = [[92.4, 160.], [232.4, 160.], [232.4, 190.], [92.4, 190.]];
        let mut other = extra.clone();
        other.source.id = 8;
        other.source.polygon = extra.source.polygon.map(|[x, y]| [x - 4.8, y]);
        runtime.blocks[0].push(extra);
        runtime.blocks[1].push(other);
        assert!(runtime.result_frame(&config).unwrap().is_some());
        assert_eq!(runtime.fallback_texts().len(), 1);
        assert_eq!(runtime.fallback_texts()[0], long.trim());
        runtime.clear();
        assert!(runtime.fallback_texts().is_empty());
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
    fn new_scan_rejects_previous_finished_frame() {
        let mut runtime = OcrRuntime::new(None, None);
        let previous = runtime.status.scan_id;
        runtime.clear();
        runtime.capture = Some(capture().as_ref().into());
        runtime
            .sender
            .try_send(Update::Finished(
                previous,
                Ok(OcrFrame {
                    blocks: [vec![block(0, "previous image", Some("old result"))], vec![]],
                    summary: ScanSummary {
                        outcome: ScanOutcome::Complete,
                        success_count: 1,
                        failure_count: 0,
                        timed_out: false,
                    },
                }),
            ))
            .unwrap();
        runtime
            .sender
            .try_send(Update::Phase(runtime.status.scan_id, Phase::Recognizing))
            .unwrap();
        assert!(matches!(
            runtime.next_current_update(),
            Some(Update::Phase(_, Phase::Recognizing))
        ));
        assert!(runtime.next_current_update().is_none());
        assert!(runtime.wrist_texts().is_empty());
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
