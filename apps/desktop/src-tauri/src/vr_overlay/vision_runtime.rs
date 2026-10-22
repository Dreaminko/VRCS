use super::{
    backend::{OpenVrBackend, OverlayKind},
    ocr_input_state::TriggerUpdate,
    ocr_selection::Selection,
    ocr_wrist::Button,
    ocr_wrist::{Action, LaserEvent, Pointer, View, ViewMode},
    surface::{SurfaceHub, SurfaceId, SurfaceRenderer, SurfaceView},
};
use crate::vision_image::prepare_image;
#[cfg(test)]
use crate::{vision_image::resize, vr_overlay::renderer::Texture};
use std::{sync::mpsc, time::Instant};
use tauri::{AppHandle, Manager as _};
use vrcs_core::{
    vision::{VisionAnswer, VisionProgress, VisionService, VisionSession},
    VisionConfig, VrOcrConfig, VrOcrDisplayMode,
};

enum Update {
    Progress(u64, VisionProgress),
    Finished(u64, Result<VisionAnswer, String>),
}

pub struct VisionRuntime {
    service: Option<VisionService>,
    configuration: Option<VisionConfig>,
    session: Option<VisionSession>,
    image: Option<tauri::async_runtime::JoinHandle<Result<Vec<u8>, String>>>,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    started: Option<Instant>,
    finished: Option<Instant>,
    id: u64,
    sender: mpsc::Sender<Update>,
    receiver: mpsc::Receiver<Update>,
    view: Option<View>,
    rendered: Option<View>,
    buttons: Vec<Button>,
    pages: usize,
    labels: super::wrist_labels::WristLabels,
    pointer: Pointer,
}

impl VisionRuntime {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            service: None,
            configuration: None,
            session: None,
            image: None,
            task: None,
            started: None,
            finished: None,
            id: 0,
            sender,
            receiver,
            view: None,
            rendered: None,
            buttons: Vec::new(),
            pages: 1,
            labels: super::wrist_labels::WristLabels::default(),
            pointer: Pointer::default(),
        }
    }

    pub fn set_service(&mut self, service: VisionService) {
        self.service = Some(service);
    }

    pub fn set_labels(&mut self, labels: super::wrist_labels::WristLabels) {
        if self.labels != labels {
            self.labels = labels;
            self.invalidate_render();
        }
    }

    pub fn enabled(&self) -> bool {
        self.service
            .as_ref()
            .is_some_and(|service| service.configuration().enabled)
    }

    pub fn visible(&self) -> bool {
        self.view.is_some()
    }
    pub fn busy(&self) -> bool {
        self.session.is_some() || self.task.is_some()
    }

    pub fn draft(&self) -> Option<String> {
        self.session.as_ref().map(VisionSession::draft)
    }

    pub fn clear(&mut self) {
        self.id = self.id.wrapping_add(1);
        self.session = None;
        if let Some(task) = self.image.take() {
            task.abort();
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.configuration = None;
        self.started = None;
        self.finished = None;
        self.view = None;
        self.rendered = None;
        self.buttons.clear();
        self.pages = 1;
        self.pointer = Pointer::default();
    }

    fn fail(&mut self, error: String) {
        let config = self.configuration.clone();
        let question = self
            .view
            .as_ref()
            .map(|view| view.source.clone())
            .unwrap_or_default();
        let pinned = self.view.as_ref().is_some_and(|view| view.pinned);
        self.clear();
        self.configuration = config;
        let mut view = Self::view("visionError", &question, &error);
        view.pinned = pinned;
        self.view = Some(view);
        self.finished = Some(Instant::now());
    }

    fn view(status: &str, question: &str, answer: &str) -> View {
        View {
            mode: ViewMode::Vision,
            source: question.into(),
            translation: answer.into(),
            translation_label: "answer".into(),
            title: "visionTitle".into(),
            status: status.into(),
            detail: String::new(),
            block_index: 0,
            block_count: 0,
            page: 0,
            pinned: false,
            source_only: false,
            language_count: 0,
            hovered: None,
        }
    }

    pub fn tick(&mut self) {
        if self.configuration.as_ref().is_some_and(|config| {
            !self
                .service
                .as_ref()
                .is_some_and(|service| service.configuration_matches(config))
        }) {
            self.clear();
        }
        if self.session.is_some()
            && self
                .started
                .zip(self.configuration.as_ref())
                .is_some_and(|(start, config)| {
                    start.elapsed().as_secs_f64() >= f64::from(config.max_capture_seconds)
                })
        {
            self.fail("Recording timed out. Release the trigger and ask again.".into());
        }
        if let Some(error) = self.session.as_ref().and_then(VisionSession::error) {
            self.fail(error);
        }
        if let (Some(session), Some(view)) = (&self.session, &mut self.view) {
            view.source = session.draft();
        }
        while let Ok(update) = self.receiver.try_recv() {
            match update {
                Update::Progress(id, progress) if id == self.id => {
                    if let Some(view) = &mut self.view {
                        match progress {
                            VisionProgress::FinalizingAsr => view.status = "finalizing".into(),
                            VisionProgress::Requesting { question } => {
                                view.status = "requesting".into();
                                view.source = question;
                            }
                        }
                    }
                }
                Update::Finished(id, result) if id == self.id => {
                    self.task = None;
                    match result {
                        Ok(answer) => {
                            let mut view = Self::view("complete", &answer.question, &answer.answer);
                            if let Some(previous) = &self.view {
                                view.pinned = previous.pinned;
                            }
                            self.view = Some(view);
                            self.finished = Some(Instant::now());
                        }
                        Err(error) => self.fail(error),
                    }
                }
                _ => {}
            }
        }
        if let Some(view) = &mut self.view {
            view.detail = if let (Some(started), Some(config)) = (self.started, &self.configuration)
            {
                format!(
                    "{} / {} s",
                    started.elapsed().as_secs(),
                    config.max_capture_seconds
                )
            } else if let (Some(finished), Some(config)) = (self.finished, &self.configuration) {
                if view.pinned {
                    self.labels.text("pinned").into()
                } else {
                    self.labels.text("hideAfter").replace(
                        "{{seconds}}",
                        &((config.display_seconds - finished.elapsed().as_secs_f32())
                            .max(0.)
                            .ceil() as u32)
                            .to_string(),
                    )
                }
            } else {
                String::new()
            };
        }
        if self
            .finished
            .zip(self.configuration.as_ref())
            .is_some_and(|(finished, config)| {
                !self.view.as_ref().is_some_and(|view| view.pinned)
                    && finished.elapsed().as_secs_f32() > config.display_seconds
            })
        {
            self.clear();
        }
    }

    /// Freeze the image on press before opening question audio.
    pub fn begin(&mut self, backend: &mut OpenVrBackend, selection: &Selection) {
        self.clear();
        let Some(service) = &self.service else {
            return;
        };
        let config = service.configuration();
        self.configuration = Some(config.clone());
        // Capture always uses the left eye, independently of the OCR display mode.
        let capture_config = VrOcrConfig {
            display_mode: VrOcrDisplayMode::Wrist,
            ..VrOcrConfig::default()
        };
        let result = backend
            .capture_ocr(&capture_config, Some(selection))
            .and_then(|capture| {
                if capture.scene_pid != selection.scene_pid || capture.origin != selection.origin {
                    return Err("Scene changed before visual capture".into());
                }
                capture
                    .eyes
                    .into_iter()
                    .next()
                    .ok_or_else(|| "Missing left eye capture".into())
            });
        let eye = match result {
            Ok(eye) => eye,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        let session = match service.begin() {
            Ok(session) => session,
            Err(error) => {
                self.fail(error);
                return;
            }
        };
        self.session = Some(session);
        self.started = Some(Instant::now());
        self.view = Some(Self::view("listening", "", ""));
        let selection = selection.clone();
        self.image = Some(tauri::async_runtime::spawn(async move {
            tauri::async_runtime::spawn_blocking(move || {
                let image = selection.crop(&eye)?.image;
                prepare_image(image, &config)
            })
            .await
            .map_err(|error| format!("Image encoding failed: {error}"))?
        }));
    }

    pub fn trigger(&mut self, trigger: &TriggerUpdate) {
        if trigger.lost && self.session.is_some() {
            self.fail("Trigger input was lost. Release the trigger and ask again.".into());
            return;
        }
        if !trigger.released {
            return;
        }
        let Some(mut session) = self.session.take() else {
            return;
        };
        // This must happen on the VR worker, before spawning or waiting for encoding.
        if let Some(view) = &mut self.view {
            view.source = session.draft();
        }
        session.release();
        self.started = None;
        let Some(image) = self.image.take() else {
            self.fail("Visual image unavailable".into());
            return;
        };
        if let Some(view) = &mut self.view {
            view.status = "finalizing".into();
        }
        let id = self.id;
        let sender = self.sender.clone();
        self.task = Some(tauri::async_runtime::spawn(async move {
            let result = async {
                let image = image
                    .await
                    .map_err(|error| format!("Image encoding failed: {error}"))??;
                session
                    .answer(image, |progress| {
                        let _ = sender.send(Update::Progress(id, progress));
                    })
                    .await
            }
            .await;
            let _ = sender.send(Update::Finished(id, result));
        }));
    }

    pub fn action(&mut self, action: Action) {
        if action == Action::Close {
            self.clear();
            return;
        }
        if let Some(view) = &mut self.view {
            if view.busy()
                && matches!(
                    action,
                    Action::PreviousPage | Action::NextPage | Action::ToggleQuestion
                )
            {
                return;
            }
            match action {
                Action::PreviousPage => view.page = view.page.saturating_sub(1),
                Action::NextPage => view.page = (view.page + 1).min(self.pages.saturating_sub(1)),
                Action::TogglePin => view.pinned = !view.pinned,
                Action::ToggleQuestion => {
                    view.source_only = !view.source_only;
                    view.page = 0;
                }
                _ => {}
            }
            if self.finished.is_some() {
                self.finished = Some(Instant::now());
            }
        }
    }

    pub fn pointer(&mut self, events: &[LaserEvent], focused: bool) {
        for event in events.iter().copied() {
            let hovered = event.point().and_then(|[x, y]| {
                self.buttons
                    .iter()
                    .find(|button| {
                        let [left, top, right, bottom] = button.bounds;
                        x >= left && x < right && y >= top && y < bottom
                    })
                    .map(|button| button.action)
            });
            if let Some(view) = &mut self.view {
                view.hovered = hovered;
            }
            if let Some(action) = self.pointer.event(event, hovered) {
                self.action(action);
            }
        }
        if !focused || self.view.is_none() {
            self.pointer = Pointer::default();
            if let Some(view) = &mut self.view {
                view.hovered = None;
            }
        }
    }

    pub fn invalidate_render(&mut self) {
        self.rendered = None;
        self.buttons.clear();
        self.pointer = Pointer::default();
    }

    pub fn input_revision(&self) -> u64 {
        super::ocr_wrist::input_revision(self.rendered.as_ref(), self.id)
    }

    pub fn render(
        &mut self,
        app: &AppHandle,
        backend: &mut OpenVrBackend,
        surface: &mut SurfaceRenderer,
    ) -> Result<bool, String> {
        let (Some(view), Some(config)) = (&mut self.view, &self.configuration) else {
            return Ok(false);
        };
        // Use an independent default, never the subtitle or OCR wrist configuration.
        let wrist = config.wrist.clone().unwrap_or_default();
        // The recording trigger keeps its existing application-local owner.
        if !backend
            .ensure_ocr_wrist(&wrist, self.session.is_none())?
            .available
        {
            return Ok(false);
        }
        let hovered = super::surface::reader_hover(view.hovered);
        let mut reader = view.clone();
        reader.hovered = None;
        let dark = app
            .state::<super::runtime::Manager>()
            .controls
            .lock()
            .map(|hub| hub.snapshot().dark)
            .unwrap_or(false);
        let visible = surface.present(
            app,
            backend,
            OverlayKind::OcrWrist,
            SurfaceView::Reader {
                dark,
                labels: self.labels.clone(),
                reader,
                font_size: wrist.font_size_px,
                opacity: 0.9,
            },
            hovered,
            None,
        )?;
        self.buttons.clear();
        if let Some(layout) = app.state::<SurfaceHub>().layout(SurfaceId::Reader) {
            view.page = view.page.min(layout.page_count.saturating_sub(1));
            self.pages = layout.page_count;
            self.buttons = layout.reader_buttons();
            self.rendered = Some(view.clone());
        }
        if !visible {
            backend.hide(OverlayKind::OcrWrist);
            return Ok(false);
        }
        backend.set_opacity(OverlayKind::OcrWrist, wrist.opacity)?;
        backend.show(OverlayKind::OcrWrist)?;
        Ok(true)
    }
}

impl Drop for VisionRuntime {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resize_preserves_pixels_and_bounds() {
        let image = Texture {
            width: 4,
            height: 2,
            pixels: (0..32).collect(),
        };
        let result = resize(&image, 2);
        assert_eq!((result.width, result.height), (2, 1));
        assert_eq!(result.pixels, vec![0, 1, 2, 3, 8, 9, 10, 11]);
    }
    #[test]
    fn completed_answer_keeps_pin_and_starts_at_the_first_page() {
        let mut runtime = VisionRuntime::new();
        let mut view = VisionRuntime::view("Requesting answer", "question", "");
        view.pinned = true;
        view.page = 2;
        runtime.view = Some(view);
        runtime
            .sender
            .send(Update::Finished(
                runtime.id,
                Ok(VisionAnswer {
                    question: "question".into(),
                    answer: "answer".into(),
                }),
            ))
            .unwrap();
        runtime.tick();
        let view = runtime.view.as_ref().unwrap();
        assert!(view.pinned);
        assert_eq!(view.page, 0);
        assert_eq!(view.translation, "answer");
    }

    #[test]
    fn finalizing_speech_preserves_the_recognized_question() {
        let mut runtime = VisionRuntime::new();
        runtime.view = Some(VisionRuntime::view("Listening", "recognized question", ""));
        runtime
            .sender
            .send(Update::Progress(runtime.id, VisionProgress::FinalizingAsr))
            .unwrap();
        runtime.tick();
        assert_eq!(runtime.view.as_ref().unwrap().source, "recognized question");
    }

    #[test]
    fn question_expansion_resets_paging_and_is_disabled_while_busy() {
        let mut runtime = VisionRuntime::new();
        runtime.view = Some(VisionRuntime::view("complete", "question", "answer"));
        runtime.action(Action::ToggleQuestion);
        assert!(runtime.view.as_ref().unwrap().source_only);
        runtime.action(Action::ToggleQuestion);
        assert!(!runtime.view.as_ref().unwrap().source_only);
        runtime.pages = 3;
        runtime.view.as_mut().unwrap().status = "requesting".into();
        runtime.action(Action::NextPage);
        runtime.action(Action::ToggleQuestion);
        let view = runtime.view.as_ref().unwrap();
        assert_eq!(view.page, 0);
        assert!(!view.source_only);
        runtime.action(Action::Close);
        assert!(!runtime.visible());
    }

    #[test]
    fn render_reset_keeps_the_answer_and_requires_a_fresh_upload() {
        let mut runtime = VisionRuntime::new();
        let view = VisionRuntime::view("Ready", "question", "answer");
        runtime.view = Some(view.clone());
        runtime.rendered = Some(view.clone());
        runtime.buttons.push(Button {
            action: Action::Close,
            bounds: [0.0, 0.0, 1.0, 1.0],
        });
        runtime.invalidate_render();
        assert_eq!(runtime.view.as_ref(), Some(&view));
        assert!(runtime.rendered.is_none());
        assert!(runtime.buttons.is_empty());
        runtime.clear();
        assert!(runtime.view.is_none());
    }

    #[test]
    fn old_results_are_rejected_after_cancel() {
        let mut runtime = VisionRuntime::new();
        let id = runtime.id;
        runtime.clear();
        runtime
            .sender
            .send(Update::Finished(id, Err("late".into())))
            .unwrap();
        runtime.tick();
        assert!(!runtime.visible());
    }
}
