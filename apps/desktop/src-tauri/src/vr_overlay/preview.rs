//! Development-only adapter for the production React surfaces.
use super::control_protocol::{ControlAction, ControlSnapshot, ControlSource};
use super::dashboard::{
    DashboardAction, DashboardChoice, DashboardOption, DashboardSaveState, DashboardState,
    DashboardViewModel, DisplayKind, TargetGroup,
};
use super::interaction::{Hit, Key, Kind, Router, Sample, Surface};
use super::ocr_plane::{OcrScene, OcrSceneBlock};
use super::ocr_wrist::{self, Action, LaserEvent, View, ViewMode};
use super::placement::{Mode, Placement, Pose, Request};
use super::presentation::{MessageSide, WristMessage};
use super::surface::{
    reader_action, SurfaceId, SurfaceKey, SurfaceLayout, SurfaceState, SurfaceView,
};
use super::{dialogue, player_display, transform, wrist_menu};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager as _, State, WebviewWindow};
use vrcs_core::ocr::{BlockTranslation, TextBlock, TranslatedBlock};

const PANELS: [&str; 11] = [
    "Wrist menu",
    "SteamVR Dashboard",
    "Dialogue",
    "Headset subtitles",
    "OCR reader",
    "Vision answer",
    "OCR selection",
    "Spatial OCR result",
    "OCR progress",
    "Vision recording / confirmation",
    "Dashboard thumbnail",
];

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scene {
    Normal,
    Empty,
    Long,
    Partial,
    Expired,
    Busy,
    Unavailable,
    Error,
    Timeout,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Operation {
    Refresh,
    Select { panel: u32 },
    Scene { scene: Scene },
    Pointer { input: Input },
    Cancel,
    Advance,
    Reset,
    Dark { enabled: bool },
    SaveFailure { enabled: bool },
    Tracking { available: bool },
    OriginChanged,
    Place { mode: Mode },
    Text { source: String, translation: String },
    Layout { layout: SurfaceLayout },
}

#[derive(Clone, Copy, Deserialize)]
pub struct Input {
    pub panel: u32,
    pub generation: u32,
    pub revision: u32,
    pub point: Option<[f32; 2]>,
    pub buttons: u8,
    pub depth: f32,
    pub rotation: [f32; 3],
}

#[derive(Serialize)]
pub struct Info {
    session: u32,
    live: bool,
    panels: [&'static str; 11],
    panel: u32,
    scene: Scene,
    generation: u32,
    revision: u32,
    dark: bool,
    fail_save: bool,
    tracking: bool,
    mode: Mode,
    dragging: bool,
    busy: bool,
    pose: Option<Pose>,
    last_action: String,
    surface: Option<SurfaceState>,
    closed: bool,
}

enum Effect {
    Control(ControlAction),
    PlayerDisplay(player_display::Action),
    Dashboard(DashboardAction),
    Placement(Request),
}

pub struct PreviewState(Mutex<Preview>);

struct Preview {
    session: u32,
    live: bool,
    panel: u32,
    scene: Scene,
    generation: u32,
    revision: u32,
    dirty: bool,
    surface: Option<SurfaceState>,
    layout: Option<SurfaceLayout>,
    controls: ControlSnapshot,
    players: player_display::View,
    menu_ui: wrist_menu::Ui,
    view: DashboardViewModel,
    dashboard: DashboardState,
    router: Router,
    pointer: ocr_wrist::Pointer,
    hover: Option<u8>,
    pressed: Option<u8>,
    hold_progress: Option<f32>,
    cursor: Option<[f32; 2]>,
    placement: Arc<Mutex<Placement>>,
    placement_revision: u64,
    reader: ocr_wrist::Reader,
    vision: View,
    page_count: usize,
    closed: bool,
    source: String,
    translation: String,
    advance: usize,
    pending: [Option<(bool, Instant)>; 4],
    player_pending: Option<(player_display::Action, Instant)>,
    fail_save: bool,
    tracking: bool,
    last_action: String,
}

impl Preview {
    fn new(live: bool, placement: Arc<Mutex<Placement>>) -> Self {
        let view: DashboardViewModel = serde_json::from_str(include_str!("preview-fixture.json"))
            .expect("valid preview fixture");
        let mut controls = ControlSnapshot {
            owner_epoch: 1,
            revision: 1,
            ..Default::default()
        };
        for (item, label) in
            controls
                .items
                .iter_mut()
                .zip(["Dialogue", "Subtitles", "OSC", "Transcribe"])
        {
            item.available = !live;
            item.label = label.into();
        }
        let mut dashboard = DashboardState::new();
        dashboard.update_view(&view);
        dashboard.set_interactive(true);
        let mut preview = Self {
            session: 0,
            live,
            panel: 0,
            scene: Scene::Normal,
            generation: 1,
            revision: 1,
            dirty: true,
            surface: None,
            layout: None,
            controls,
            players: Default::default(),
            menu_ui: Default::default(),
            view,
            dashboard,
            router: Default::default(),
            pointer: Default::default(),
            hover: None,
            pressed: None,
            hold_progress: None,
            cursor: None,
            placement,
            placement_revision: 0,
            reader: Default::default(),
            vision: View {
                mode: ViewMode::Vision,
                source: "What does this panel show?".into(),
                translation: String::new(),
                translation_label: String::new(),
                title: "visionTitle".into(),
                status: "complete".into(),
                detail: String::new(),
                block_index: 0,
                block_count: 1,
                page: 0,
                pinned: false,
                source_only: false,
                language_count: 1,
                hovered: None,
            },
            page_count: 1,
            closed: false,
            source: "Welcome to VRCS.\n欢迎使用 VRCS。".into(),
            translation: "You can move while using the overlay.\n操作面板时仍可移动。".into(),
            advance: 0,
            pending: Default::default(),
            player_pending: None,
            fail_save: false,
            tracking: true,
            last_action: "Synthetic content and poses; OpenVR is not initialized.".into(),
        };
        preview.set_scene(Scene::Normal);
        preview
    }

    fn info(&self) -> Info {
        let p = self.placement.lock().unwrap_or_else(|e| e.into_inner());
        Info {
            session: self.session,
            live: self.live,
            panels: PANELS,
            panel: self.panel,
            scene: self.scene,
            generation: self.generation,
            revision: self.revision,
            dark: self.controls.dark,
            fail_save: self.fail_save,
            tracking: self.tracking,
            mode: p.display_mode(),
            dragging: p.dragging().is_some(),
            busy: p.busy(),
            pose: p.absolute_pose().or_else(|| self.tracking.then(wrist_pose)),
            last_action: self.last_action.clone(),
            surface: self.surface.clone(),
            closed: self.closed,
        }
    }

    fn cancel(&mut self) {
        self.router.cancel();
        self.pointer.event(LaserEvent::Leave, None);
        self.dashboard.clear_pointer();
        self.placement
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel_drag();
        self.hover = None;
        self.pressed = None;
        self.hold_progress = None;
        self.cursor = None;
        self.dirty = true;
    }

    fn layout_changed(&mut self) {
        if self.panel == 0 {
            self.cancel();
        }
        self.revision += 1;
        self.layout = None;
        self.cursor = None;
        self.dirty = true;
    }

    fn set_scene(&mut self, scene: Scene) {
        self.cancel();
        self.scene = scene;
        self.closed = false;
        self.advance = 0;
        self.pending = Default::default();
        self.player_pending = None;
        if !self.live {
            self.players = sample_player_display(scene);
            self.menu_ui.clamp(&self.players);
            for (index, item) in self.controls.items.iter_mut().enumerate() {
                item.available = scene != Scene::Unavailable;
                item.busy = matches!(scene, Scene::Busy | Scene::Timeout) && index == 3;
                item.reason = match scene {
                    Scene::Unavailable => Some("Feature unavailable".into()),
                    Scene::Error => Some("Save failed".into()),
                    _ => None,
                };
            }
        }
        self.reader = Default::default();
        let blocks = if matches!(scene, Scene::Empty | Scene::Expired) {
            Vec::new()
        } else {
            sample_blocks(scene == Scene::Long)
        };
        self.reader.sync(1, &blocks);
        self.vision.page = 0;
        self.vision.pinned = false;
        self.vision.status = match scene {
            Scene::Busy => "requesting",
            Scene::Error | Scene::Unavailable | Scene::Timeout => "visionError",
            _ => "complete",
        }
        .into();
        self.vision.detail = if matches!(scene, Scene::Error | Scene::Timeout) {
            "Synthetic request failed".into()
        } else {
            String::new()
        };
        self.vision.translation = if matches!(scene, Scene::Empty | Scene::Expired) {
            String::new()
        } else {
            sample_answer(scene == Scene::Long)
        };
        if scene == Scene::Partial {
            self.vision.translation = "## Streaming answer\n\nThe panel shows".into();
        }
        self.layout_changed();
    }

    fn update(&mut self, operation: Operation) -> Result<Vec<Effect>, String> {
        let mut effects = Vec::new();
        match operation {
            Operation::Refresh => {}
            Operation::Layout { layout } => {
                let surface = self.surface_id().ok_or("Panel has no React surface")?;
                layout.validate(surface)?;
                if layout.key == self.surface_key() {
                    self.page_count = layout.page_count;
                    self.layout = Some(layout);
                }
            }
            Operation::Select { panel } => {
                if panel >= PANELS.len() as u32 {
                    return Err("Unknown preview panel".into());
                }
                self.cancel();
                self.panel = panel;
                self.generation += 1;
                self.layout_changed();
            }
            Operation::Scene { scene } => self.set_scene(scene),
            Operation::Pointer { input } => return self.input(input),
            Operation::Cancel => self.cancel(),
            Operation::Reset => {
                if self.live {
                    return Err("Live preview cannot reset user settings".into());
                }
                let session = self.session;
                let generation = self.generation + 1;
                *self = Self::new(false, Arc::new(Mutex::new(Placement::default())));
                self.session = session;
                self.generation = generation;
            }
            Operation::Advance => {
                self.advance += 1;
                if self.scene == Scene::Partial {
                    self.vision
                        .translation
                        .push_str(" more translated text.\n\n- Keep moving\n- Read the answer");
                }
                if self.scene == Scene::Busy {
                    self.vision.status = match self.advance % 4 {
                        1 => "listening",
                        2 => "finalizing",
                        3 => "requesting",
                        _ => "complete",
                    }
                    .into();
                }
                if self.scene == Scene::Timeout && !self.live {
                    self.controls.items[3].busy = false;
                    self.controls.items[3].reason =
                        Some("Timeout: current capture state checked".into());
                    self.players.busy = false;
                    self.players.error =
                        Some("Timeout: previous personnel settings retained".into());
                }
                // Dialogue toolbar identity is independent of streaming body text.
                if self.panel != 2 {
                    self.layout_changed();
                } else {
                    self.dirty = true;
                }
            }
            Operation::Dark { enabled } => {
                self.controls.dark = enabled;
                self.dirty = true;
            }
            Operation::SaveFailure { enabled } => {
                if self.live {
                    return Err("Failure injection is only available in mock mode".into());
                }
                self.fail_save = enabled;
            }
            Operation::Tracking { available } => {
                self.cancel();
                self.tracking = available;
                if !available {
                    self.menu_ui = Default::default();
                    self.layout_changed();
                }
                self.last_action = "Synthetic controller tracking changed".into();
            }
            Operation::OriginChanged => {
                self.cancel();
                self.placement
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .reset_tracking();
                self.generation += 1;
                self.menu_ui = Default::default();
                self.layout_changed();
                self.last_action =
                    "Synthetic tracking origin reset; previous world pose discarded".into();
            }
            Operation::Place { mode } => {
                if let Some(effect) = self.place(mode) {
                    effects.push(effect);
                }
            }
            Operation::Text {
                source,
                translation,
            } => {
                if source.len() > 32768 || translation.len() > 32768 {
                    return Err("Preview text exceeds the limit".into());
                }
                self.source = source;
                self.translation = translation;
                self.dirty = true;
            }
        }
        Ok(effects)
    }

    fn key(&self) -> Key {
        Key {
            surface: match self.panel {
                0 => Surface::Menu,
                2 => Surface::Dialogue,
                _ => Surface::Reader,
            },
            generation: u64::from(self.generation),
            revision: if self.panel == 2 {
                self.placement
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .revision
            } else {
                u64::from(self.revision)
            },
        }
    }

    fn surface_id(&self) -> Option<SurfaceId> {
        match self.panel {
            0 => Some(SurfaceId::Menu),
            1 => Some(SurfaceId::Dashboard),
            2 => Some(SurfaceId::Dialogue),
            3 => Some(SurfaceId::Headset),
            4 | 5 | 9 => Some(SurfaceId::Reader),
            6 => Some(SurfaceId::OcrSelection),
            7 => Some(SurfaceId::OcrResult),
            8 => Some(SurfaceId::OcrProgress),
            10 => Some(SurfaceId::DashboardThumbnail),
            _ => None,
        }
    }

    fn surface_key(&self) -> SurfaceKey {
        SurfaceKey {
            generation: self.generation,
            revision: self.revision,
        }
    }

    fn input(&mut self, input: Input) -> Result<Vec<Effect>, String> {
        let continuing_drag = self.panel == 2
            && self
                .placement
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .dragging()
                .is_some();
        if input.panel != self.panel
            || input.generation != self.generation
            || input.revision != self.revision && !continuing_drag
        {
            self.cancel();
            return Err("Stale preview layout".into());
        }
        if input
            .point
            .is_some_and(|point| !point.iter().all(|v| v.is_finite() && (0. ..1.).contains(v)))
            || !input.depth.is_finite()
            || !input.rotation.iter().all(|v| v.is_finite())
        {
            self.cancel();
            return Err("Invalid preview input".into());
        }
        if !self.tracking {
            self.cancel();
            return Ok(Vec::new());
        }
        let point = input.point;
        let cursor = point.filter(|point| {
            !self.closed
                && matches!(self.panel, 0 | 1 | 2 | 4 | 5 | 9)
                && self.layout.as_ref().is_some_and(|layout| {
                    layout.key == self.surface_key()
                        && (self.panel == 1 || layout.pointer_contains(*point))
                })
        });
        self.dirty |= self.cursor != cursor;
        self.cursor = cursor;
        let point = cursor;
        let mut effects = Vec::new();
        let key = self.key();
        let hit = point.and_then(|point| {
            if self.closed && matches!(self.panel, 4 | 5 | 9) {
                return None;
            }
            let region = self
                .layout
                .as_ref()
                .filter(|layout| layout.key == self.surface_key())?
                .hit(point)?;
            Some(Hit {
                key,
                region,
                point,
                distance: 1.,
            })
        });
        let sample = Sample {
            device: 1,
            trigger: Some(input.buttons & 1 != 0),
            grip: Some(input.buttons & 2 != 0),
            hit,
        };
        let can_recenter = self.panel == 0
            && self.controls.items[0].requested
            && matches!(
                self.layout
                    .as_ref()
                    .filter(|layout| layout.key == self.surface_key())
                    .and_then(|layout| layout.menu_command(0)),
                Some(wrist_menu::Command::Control(
                    ControlAction::SetWristEnabled(_)
                ))
            )
            && {
                let placement = self.placement.lock().unwrap_or_else(|e| e.into_inner());
                placement.display_mode() == Mode::World && !placement.busy()
            };
        let mut output = self.router.step(&[sample], &[key]);
        let now = Instant::now();
        if can_recenter {
            if let Some(event) = self.router.long_press(now) {
                output.events.push(event);
            }
        }
        let hold_progress = can_recenter
            .then(|| self.router.hold_progress(now))
            .flatten();
        self.dirty |= self.hold_progress != hold_progress;
        self.hold_progress = hold_progress;
        let hover = output.hover.map(|(_, hit)| hit.region);
        self.dirty |= self.hover != hover || !output.events.is_empty();
        self.hover = hover;
        if matches!(self.panel, 4 | 5 | 9) {
            let action = point.and_then(|point| self.reader_hit(point).map(|v| v.1));
            self.pointer.event(
                point
                    .map(|p| LaserEvent::Move {
                        device: 1,
                        point: p.map(|v| v * 1024.),
                    })
                    .unwrap_or(LaserEvent::Leave),
                action,
            );
        }
        let controller = synthetic_controller(input);
        {
            let mut p = self.placement.lock().unwrap_or_else(|e| e.into_inner());
            if self.panel == 2 && p.dragging().is_some() {
                p.drag(1, controller);
            }
        }
        for event in output.events {
            match self.panel {
                0 => match event.kind {
                    Kind::Down => self.pressed = Some(event.hit.region),
                    Kind::LongPress => effects.extend(self.place(Mode::World)),
                    Kind::Up => {
                        self.pressed = None;
                        let command = self
                            .layout
                            .as_ref()
                            .filter(|layout| layout.key == self.surface_key())
                            .and_then(|layout| layout.menu_command(event.hit.region));
                        match command {
                            Some(wrist_menu::Command::Control(action)) => {
                                effects.push(Effect::Control(action))
                            }
                            Some(wrist_menu::Command::Display(action)) => {
                                effects.push(Effect::PlayerDisplay(action))
                            }
                            Some(command) if self.menu_ui.apply(&command, &self.players) => {
                                self.layout_changed()
                            }
                            _ => {}
                        }
                    }
                    _ => self.pressed = None,
                },
                1 => match event.kind {
                    Kind::Down => self.pressed = Some(event.hit.region),
                    Kind::Up => {
                        self.pressed = None;
                        let command = self
                            .layout
                            .as_ref()
                            .filter(|layout| layout.key == self.surface_key())
                            .and_then(|layout| {
                                layout
                                    .regions
                                    .iter()
                                    .find(|region| region.id == event.hit.region)
                            })
                            .and_then(|region| region.dashboard.clone());
                        if let Some(command) = command {
                            if let Some(action) = self.dashboard.activate(command, &self.view) {
                                effects.extend(self.dashboard_action(action));
                            }
                            self.layout_changed();
                        }
                    }
                    _ => self.pressed = None,
                },
                2 => {
                    let mut p = self.placement.lock().unwrap_or_else(|e| e.into_inner());
                    let request = match event.kind {
                        Kind::Grab => {
                            let panel = p.absolute_pose().unwrap_or_else(wrist_pose);
                            p.grab(1, controller, panel);
                            None
                        }
                        Kind::Release => p.release(1),
                        Kind::Cancel => {
                            p.cancel_drag();
                            None
                        }
                        Kind::Up if event.hit.region == 1 => p.set_mode(Mode::Wrist, None),
                        Kind::Up if event.hit.region == 2 => {
                            let pose = p.absolute_pose().unwrap_or_else(wrist_pose);
                            p.set_mode(Mode::World, Some(pose))
                        }
                        Kind::Up if event.hit.region == 3 => {
                            let enabled = !p.follow_view();
                            p.set_follow_view(enabled)
                        }
                        _ => None,
                    };
                    if let Some(request) = request {
                        effects.push(Effect::Placement(request));
                    }
                }
                4 | 5 | 9 => {
                    let action = self.reader_hit(event.hit.point).map(|v| v.1);
                    let point = event.hit.point.map(|v| v * 1024.);
                    let event = match event.kind {
                        Kind::Down => LaserEvent::Down { device: 1, point },
                        Kind::Up => LaserEvent::Up { device: 1, point },
                        _ => LaserEvent::Leave,
                    };
                    if let Some(action) = self.pointer.event(event, action) {
                        self.reader_action(action);
                    }
                }
                _ => {}
            }
        }
        Ok(effects)
    }

    fn reader_hit(&self, point: [f32; 2]) -> Option<(u8, Action)> {
        let id = self.layout.as_ref()?.hit(point)?;
        Some((id, reader_action(id)?))
    }

    fn reader_action(&mut self, action: Action) {
        if self.panel == 4 {
            self.reader.act(action, self.page_count);
        } else {
            match action {
                Action::PreviousPage => self.vision.page = self.vision.page.saturating_sub(1),
                Action::NextPage => {
                    self.vision.page = (self.vision.page + 1).min(self.page_count.saturating_sub(1))
                }
                Action::TogglePin => self.vision.pinned = !self.vision.pinned,
                Action::ToggleQuestion => self.vision.source_only = !self.vision.source_only,
                _ => {}
            }
        }
        if action == Action::Close {
            self.closed = true;
        }
        self.last_action = format!("Synthetic reader: {action:?}");
        self.layout_changed();
    }

    fn place(&mut self, mode: Mode) -> Option<Effect> {
        if !self.tracking {
            self.last_action = "Synthetic tracking is unavailable".into();
            return None;
        }
        self.dirty = true;
        self.placement
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_mode(
                mode,
                (mode == Mode::World).then(|| dialogue::in_front(head_pose())),
            )
            .map(Effect::Placement)
    }

    fn dashboard_action(&mut self, action: DashboardAction) -> Vec<Effect> {
        use DashboardAction::*;
        let control = match &action {
            ToggleWrist => Some(ControlAction::SetWristEnabled(
                !self.controls.items[0].requested,
            )),
            ToggleHeadset => Some(ControlAction::SetHeadsetEnabled(
                !self.controls.items[1].requested,
            )),
            ToggleOsc => Some(ControlAction::SetOscEnabled(
                !self.controls.items[2].requested,
            )),
            ToggleCapture => Some(ControlAction::SetCaptureRunning(
                !self.controls.items[3].requested,
            )),
            _ => None,
        };
        if let Some(control) = control {
            return vec![Effect::Control(control)];
        }
        match action {
            RecenterDialogue => self.place(Mode::World).into_iter().collect(),
            AttachDialogue => self.place(Mode::Wrist).into_iter().collect(),
            OpenOcrBindings => {
                self.last_action = "Controller bindings require a real SteamVR headset".into();
                Vec::new()
            }
            PreviewHeadset => {
                self.view.headset.preview_visible = !self.view.headset.preview_visible;
                Vec::new()
            }
            PreviewWrist => {
                self.view.wrist.preview_visible = !self.view.wrist.preview_visible;
                Vec::new()
            }
            _ => vec![Effect::Dashboard(action)],
        }
    }

    fn apply_mock(&mut self, effect: Effect) {
        match effect {
            Effect::PlayerDisplay(action) => {
                if !self.players.available || self.players.busy || !action.allowed(&self.players) {
                    return;
                }
                self.players.busy = true;
                self.players.error = None;
                self.last_action = format!("Mock personnel: {action:?}");
                self.player_pending = Some((action, Instant::now()));
            }
            Effect::Control(action) => {
                let index = action.index();
                let item = &mut self.controls.items[index];
                if !item.available || item.busy {
                    return;
                }
                item.busy = true;
                item.reason = None;
                self.pending[index] = Some((action.target(), Instant::now()));
                self.last_action = format!("Mock: {action:?}");
            }
            Effect::Placement(request) => {
                self.placement
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .resolve(
                        request.id,
                        !self.fail_save,
                        self.fail_save
                            .then(|| "Synthetic save failed; previous anchor restored".into()),
                    );
                self.last_action = format!(
                    "Mock anchor: {:?}{}",
                    request.mode,
                    if self.fail_save { " (save failed)" } else { "" }
                );
                if self.panel != 0 {
                    self.layout_changed();
                }
            }
            Effect::Dashboard(action) => {
                self.last_action = format!("Mock Dashboard: {action:?}");
                if self.fail_save {
                    self.view.error = Some("Synthetic save failed".into());
                    self.view.save_state = DashboardSaveState::Error;
                } else {
                    apply_dashboard(&mut self.view, action);
                    self.view.error = None;
                    self.view.save_state = DashboardSaveState::Saved;
                }
                self.dashboard.update_view(&self.view);
            }
        }
        self.dirty = true;
    }

    fn sync_control_views(&mut self, controls: ControlSnapshot, players: player_display::View) {
        if controls == self.controls && players == self.players {
            return;
        }
        if controls.owner_epoch != self.controls.owner_epoch
            || self.players.available
                && !players.available
                && controls.items.iter().all(|item| !item.available)
        {
            self.cancel();
            self.generation += 1;
            self.menu_ui = Default::default();
        }
        self.controls = controls;
        self.players = players;
        self.menu_ui.clamp(&self.players);
        if self.panel == 0 {
            self.layout_changed();
        } else {
            self.dirty = true;
        }
    }

    fn sync_dashboard_view(&mut self, view: DashboardViewModel) -> Result<(), String> {
        if view != self.view {
            self.view = view;
            if self.panel == 1 {
                self.layout_changed();
            } else {
                self.dirty = true;
            }
        }
        let mut placement = self.placement.lock().map_err(|e| e.to_string())?;
        placement.sync_mode(Mode::from_config(&self.view.wrist.anchor_mode));
        placement.sync_follow_view(self.view.wrist.follow_view);
        Ok(())
    }

    fn refresh(&mut self) {
        if self
            .player_pending
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= Duration::from_millis(300))
        {
            let (action, _) = self
                .player_pending
                .take()
                .expect("pending mock personnel action");
            self.players.busy = false;
            if self.fail_save {
                self.players.error =
                    Some("Synthetic save failed; previous settings retained".into());
            } else if !action.allowed(&self.players) {
                self.players.error = Some("Personnel state changed; try again".into());
            } else {
                apply_player_display(&mut self.players, action);
                self.players.error = None;
                self.menu_ui.clamp(&self.players);
            }
            self.dirty = true;
        }
        for (index, pending) in self.pending.iter_mut().enumerate() {
            if pending.is_some_and(|(_, at)| at.elapsed() >= Duration::from_millis(300)) {
                let target = pending.take().expect("pending mock action").0;
                let item = &mut self.controls.items[index];
                item.busy = false;
                if self.fail_save {
                    item.reason = Some("Synthetic operation failed".into());
                } else {
                    item.requested = target;
                    item.effective = target;
                }
                self.dirty = true;
            }
        }
        if !self.live {
            self.view.wrist.enabled = self.controls.items[0].requested;
            self.view.headset.enabled = self.controls.items[1].requested;
            self.view.osc.enabled = self.controls.items[2].requested;
            self.view.capture.running = self.controls.items[3].requested;
            let placement = self.placement.lock().unwrap_or_else(|e| e.into_inner());
            self.view.wrist.anchor_mode = match placement.mode() {
                Mode::Wrist => "wrist",
                Mode::World => "world",
            }
            .into();
            self.view.wrist.follow_view = placement.follow_view();
        }
        self.dashboard.update_view(&self.view);
        let mut placement = self.placement.lock().unwrap_or_else(|e| e.into_inner());
        if self.tracking {
            placement.update_head(head_pose());
            placement.initialize_world(dialogue::in_front(head_pose()));
        }
        self.dirty |= placement.revision != self.placement_revision;
    }

    fn surface_view(&self) -> Option<SurfaceView> {
        Some(match self.panel {
            0 => SurfaceView::Menu {
                controls: self.controls.clone(),
                players: self.players.clone(),
                ui: self.menu_ui.clone(),
                hold_progress: self.hold_progress,
            },
            1 => SurfaceView::Dashboard {
                dark: self.controls.dark,
                dashboard: Box::new(self.view.clone()),
                ui: self.dashboard.ui.clone(),
            },
            2 => {
                let p = self.placement.lock().unwrap_or_else(|e| e.into_inner());
                let messages = if matches!(self.scene, Scene::Empty | Scene::Expired) {
                    Vec::new()
                } else {
                    vec![
                        WristMessage {
                            original: None,
                            speaker: None,
                            text: self
                                .source
                                .repeat(if self.scene == Scene::Long { 8 } else { 1 }),
                            side: MessageSide::Left,
                        },
                        WristMessage {
                            original: None,
                            speaker: None,
                            text: self.translation.clone(),
                            side: MessageSide::Right,
                        },
                    ]
                };
                SurfaceView::Dialogue {
                    visibility: super::dashboard::CaptionVisibility::default(),
                    max_visible_players: None,
                    max_entries: 10,
                    controls: self.controls.clone(),
                    messages,
                    notice: None,
                    labels: Default::default(),
                    mode: p.display_mode(),
                    follow_view: p.follow_view(),
                    busy: p.busy(),
                    error: p.error.clone(),
                    font_size: 28,
                    opacity: 0.94,
                }
            }
            3 => SurfaceView::Headset {
                visibility: super::dashboard::CaptionVisibility::default(),
                max_visible_players: None,
                dark: self.controls.dark,
                messages: if matches!(self.scene, Scene::Empty | Scene::Expired) {
                    Vec::new()
                } else {
                    vec![WristMessage {
                        original: None,
                        speaker: Some("Alex".into()),
                        side: MessageSide::Left,
                        text: if matches!(self.scene, Scene::Empty | Scene::Expired) {
                            String::new()
                        } else if self.scene == Scene::Partial {
                            format!("{} …", self.source)
                        } else if self.scene == Scene::Long {
                            format!(
                                "{}\n{}",
                                self.source.repeat(20),
                                self.translation.repeat(20)
                            )
                        } else {
                            format!("{}\n{}", self.source, self.translation)
                        },
                    }]
                },
                notice: None,
                labels: Default::default(),
                lines_per_language: 2,
                font_size: 38,
                opacity: 0.65,
            },
            4 | 5 | 9 => {
                let mut reader = if self.panel == 4 {
                    self.reader.view_with_labels(
                        super::ocr_status::OcrState::Visible,
                        false,
                        false,
                        &self.view.labels.wrist_reader,
                    )
                } else {
                    self.vision.clone()
                };
                if self.panel == 9 {
                    reader.status = ["listening", "finalizing", "requesting", "complete"]
                        [self.advance % 4]
                        .into();
                }
                reader.hovered = None;
                SurfaceView::Reader {
                    dark: self.controls.dark,
                    labels: self.view.labels.wrist_reader.clone(),
                    reader,
                    font_size: 28,
                    opacity: 0.94,
                }
            }
            6 => SurfaceView::OcrSelection {
                dark: self.controls.dark,
            },
            7 => SurfaceView::OcrResult {
                dark: self.controls.dark,
                scene: sample_scene(self.scene),
            },
            8 => {
                use super::ocr_progress::{ProgressView, Stage};
                let step = self.advance % 4;
                let failed = matches!(self.scene, Scene::Error | Scene::Timeout);
                SurfaceView::OcrProgress {
                    dark: self.controls.dark,
                    progress: (!matches!(self.scene, Scene::Empty | Scene::Expired)).then(|| {
                        ProgressView {
                            scan_id: 1,
                            status: if self.scene == Scene::Timeout {
                                "Timed out"
                            } else {
                                "Synthetic OCR"
                            }
                            .into(),
                            detail: if failed {
                                "Recognition failed"
                            } else {
                                "Advance to inspect recognition and translation stages"
                            }
                            .into(),
                            elapsed_seconds: self.advance as u64,
                            stages: std::array::from_fn(|i| {
                                if i < step {
                                    Stage::Complete
                                } else if i == step {
                                    if failed {
                                        Stage::Failed
                                    } else {
                                        Stage::Active
                                    }
                                } else {
                                    Stage::Pending
                                }
                            }),
                            stage_labels: [
                                "Capture".into(),
                                "Recognize".into(),
                                "Translate".into(),
                                "Display".into(),
                            ],
                            translation_fraction: Some((self.advance.min(3), 3)),
                        }
                    }),
                }
            }
            10 => SurfaceView::DashboardThumbnail {
                dark: self.controls.dark,
            },
            _ => return None,
        })
    }

    fn render(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        let placement_revision = self
            .placement
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .revision;
        let mut view = self.surface_view().ok_or("Unknown preview panel")?;
        if self
            .surface
            .as_ref()
            .is_some_and(|state| state.key == self.surface_key() && !state.view.same_layout(&view))
        {
            self.layout_changed();
            view = self.surface_view().ok_or("Unknown preview panel")?;
        }
        self.surface = Some(SurfaceState {
            key: self.surface_key(),
            scroll_key: self.surface_key(),
            view,
            hovered: self.hover,
            pressed: self.pressed,
            pointer: self.cursor,
            activity: super::surface::SurfaceActivity {
                active: true,
                request: 0,
            },
            restore_scroll: None,
        });
        self.dirty = false;
        self.placement_revision = placement_revision;
        Ok(())
    }
}

fn head_pose() -> Pose {
    transform::matrix(0., 0., 0., [0., 1.6, 0.])
}
fn wrist_pose() -> Pose {
    transform::matrix(0., 0., 0., [0., 1.2, -0.6])
}
fn synthetic_controller(input: Input) -> Pose {
    let [u, v] = input.point.unwrap_or([0.5, 0.5]);
    transform::matrix(
        input.rotation[0].clamp(-180., 180.),
        input.rotation[1].clamp(-180., 180.),
        input.rotation[2].clamp(-180., 180.),
        [
            (u - 0.5) * 2.,
            1.6 + (0.5 - v) * 2.,
            -input.depth.clamp(0.2, 3.),
        ],
    )
}
fn sample_answer(long: bool) -> String {
    let answer = "## VR overlay\n\nThe panel keeps recent **transcriptions** and translations visible.\n\n- Trigger: activate a button\n- Grip: move the dialogue panel\n\n| Mode | Anchor |\n| --- | --- |\n| Wrist | Controller |\n| World | Tracking space |\n\n```rust\nlet overlay = \"VRCS\";\n```\n\n> Game movement continues while you use the panel.\n\n";
    answer.repeat(if long { 12 } else { 1 })
}
fn sample_blocks(long: bool) -> Vec<TranslatedBlock> {
    ["Welcome to VRCS", "Recent dialogue and translated text"]
        .into_iter()
        .enumerate()
        .map(|(id, text)| {
            let top = 100. + id as f32 * 160.;
            TranslatedBlock {
                source: TextBlock {
                    id,
                    text: text.repeat(if long { 30 } else { 1 }),
                    confidence: 0.99,
                    polygon: [[80., top], [680., top], [680., top + 90.], [80., top + 90.]],
                },
                fragments: Vec::new(),
                translations: vec![
                    BlockTranslation {
                        target_language: "zh-CN".into(),
                        text: Some(
                            "欢迎使用 VRCS。面板交互期间可以继续在游戏内移动。".repeat(if long {
                                25
                            } else {
                                1
                            }),
                        ),
                        error_code: None,
                    },
                    BlockTranslation {
                        target_language: "ja".into(),
                        text: Some("VRCS へようこそ。".into()),
                        error_code: None,
                    },
                ],
            }
        })
        .collect()
}
fn sample_scene(scene: Scene) -> OcrScene {
    let blocks = if matches!(scene, Scene::Normal | Scene::Long | Scene::Partial) {
        sample_blocks(scene == Scene::Long)
            .into_iter()
            .take(if scene == Scene::Partial { 1 } else { 2 })
            .map(|block| {
                let quad = block.source.polygon.map(|[x, y]| [x / 768., y / 512.]);
                OcrSceneBlock {
                    id: block.source.id,
                    text: block.translations[0].text.clone().unwrap_or_default(),
                    quad,
                    masks: vec![quad],
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    OcrScene {
        blocks,
        excluded: Vec::new(),
        opacity: 0.94,
        aspect: 1.5,
    }
}

fn sample_player_display(scene: Scene) -> player_display::View {
    let mut players = Vec::new();
    if !matches!(scene, Scene::Empty | Scene::Expired) {
        let current = if scene == Scene::Long { 13 } else { 4 };
        let history = if scene == Scene::Long { 8 } else { 3 };
        for index in 0..current + history {
            let user = if index == 3 { 1 } else { index + 1 };
            players.push(player_display::Player {
                track_id: Some(format!("track-{}", index + 1)),
                user_id: (index != 2).then(|| format!("usr_00000000-0000-0000-0000-{user:012}")),
                name: if index == 2 {
                    "Identity pending".into()
                } else if index == 0 && scene == Scene::Long {
                    "Alex with a deliberately long display name for a two-line wrist row".into()
                } else if matches!(index, 0 | 1 | 3) {
                    "Alex".into()
                } else {
                    format!("Player {}", index + 1)
                },
                detail: if index == 2 {
                    "Track 3 - waiting for player identity".into()
                } else {
                    format!("Track {} - account {user:04}", index + 1)
                },
                current: index < current,
                selected: false,
                hidden: false,
            });
        }
        players.push(player_display::Player {
            track_id: None,
            user_id: Some("usr_00000000-0000-0000-0000-000000000999".into()),
            name: "Saved offline player".into(),
            detail: "Saved selection - account 0999".into(),
            current: false,
            selected: true,
            hidden: false,
        });
    }
    player_display::View {
        available: scene != Scene::Unavailable,
        processing_available: scene != Scene::Unavailable,
        player_mode: true,
        selection_mode: "selected".into(),
        selected_count: usize::from(!players.is_empty()),
        players,
        languages: [
            ("en", "English"),
            ("ja", "Japanese"),
            ("zh-CN", "Simplified Chinese"),
            ("zh-TW", "Traditional Chinese"),
            ("ko", "Korean"),
            ("fr", "French"),
            ("de", "German"),
            ("es", "Spanish"),
            ("it", "Italian"),
            ("pt", "Portuguese"),
            ("ru", "Russian"),
            ("ar", "Arabic"),
            ("hi", "Hindi"),
            ("th", "Thai"),
            ("vi", "Vietnamese"),
            ("id", "Indonesian"),
        ]
        .into_iter()
        .map(|(code, label)| player_display::Language {
            code: code.into(),
            label: label.into(),
            hidden: matches!(code, "en" | "ja"),
        })
        .collect(),
        follow_default: true,
        default_languages: vec!["English".into(), "Japanese".into()],
        labels: [
            ("title", "People and translation display"),
            ("playersTab", "People"),
            ("historySaved", "History and saved"),
            (
                "processingUnavailable",
                "Player processing requires per-player audio and a connected Core.",
            ),
            (
                "unavailable",
                "Open the main VRCS window to use these settings.",
            ),
            ("process", "Players to process"),
            ("all", "Translate everyone"),
            ("selected", "Translate selected people only"),
            ("selectCurrent", "Select everyone here"),
            ("selectNone", "Deselect all"),
            (
                "noneSelected",
                "No players selected; other-party audio is not being processed.",
            ),
            ("languages", "Hidden original languages"),
            ("followDefault", "Follow default"),
            ("noHiddenLanguages", "No languages hidden"),
            ("showAll", "Restore people display"),
            ("empty", "Waiting for tracks or loading history."),
            ("translate", "Translate"),
            ("show", "Show"),
            ("hide", "Hide"),
            ("current", "Current world"),
            ("close", "Close"),
            ("previous", "Previous"),
            ("next", "Next"),
            ("busy", "Saving..."),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect(),
        busy: matches!(scene, Scene::Busy | Scene::Timeout),
        error: (scene == Scene::Error)
            .then(|| "Synthetic save failed; previous settings retained".into()),
    }
}

fn apply_player_display(view: &mut player_display::View, action: player_display::Action) {
    use player_display::Action;
    match action {
        Action::SetProcessingMode(mode) => {
            view.selection_mode = mode;
            for player in &mut view.players {
                player.selected = false;
            }
        }
        Action::SetUserSelected { user_id, selected } => {
            for player in &mut view.players {
                if player.user_id.as_ref() == Some(&user_id) {
                    player.selected = selected;
                }
            }
        }
        Action::SelectCurrent(()) => {
            let users: std::collections::HashSet<_> = view
                .players
                .iter()
                .filter(|player| player.current)
                .filter_map(|player| player.user_id.clone())
                .collect();
            for player in &mut view.players {
                player.selected |= player
                    .user_id
                    .as_ref()
                    .is_some_and(|user| users.contains(user));
            }
        }
        Action::ClearSelection(()) => {
            for player in &mut view.players {
                player.selected = false;
            }
        }
        Action::SetTrackHidden { track_id, hidden } => {
            for player in &mut view.players {
                if player.track_id.as_ref() == Some(&track_id) {
                    player.hidden = hidden;
                }
            }
        }
        Action::ShowAllTracks(()) => {
            for player in &mut view.players {
                player.hidden = false;
            }
        }
        Action::SetFollowDefault(follow) => {
            view.follow_default = follow;
            if follow {
                for language in &mut view.languages {
                    language.hidden = view.default_languages.contains(&language.label);
                }
            }
        }
        Action::SetSourceLanguageHidden { language, hidden } => {
            if let Some(candidate) = view
                .languages
                .iter_mut()
                .find(|candidate| candidate.code == language)
            {
                candidate.hidden = hidden;
            }
        }
        Action::ClearHiddenLanguages(()) => {
            view.follow_default = false;
            for language in &mut view.languages {
                language.hidden = false;
            }
        }
    }
    view.selected_count = view
        .players
        .iter()
        .filter(|player| player.selected)
        .filter_map(|player| player.user_id.as_ref())
        .collect::<std::collections::HashSet<_>>()
        .len();
}

fn apply_dashboard(view: &mut DashboardViewModel, action: DashboardAction) {
    use DashboardAction::*;
    fn targets(group: TargetGroup, view: &mut DashboardViewModel) -> &mut Vec<DashboardChoice> {
        match group {
            TargetGroup::Speaker => &mut view.language.speaker_targets,
            TargetGroup::Microphone => &mut view.language.microphone_targets,
        }
    }
    match action {
        ToggleMaster => view.enabled = !view.enabled,
        ToggleOcr => view.ocr.enabled = !view.ocr.enabled,
        ToggleVision => view.vision.enabled = !view.vision.enabled,
        ToggleOcrGesture => view.ocr.gesture = !view.ocr.gesture,
        ToggleOscOriginal => view.osc.original = !view.osc.original,
        ToggleOscMuteSync => view.osc.mute_sync = !view.osc.mute_sync,
        SetDisplayContent(value) => view.content.value = value,
        SetWristHand(value) => view.wrist.hand.value = value,
        SetOscStrategy(value) => view.osc.strategy.value = value,
        SetRecognitionLanguage(value) => view.language.recognition.value = value,
        SetTranslationMode(value) => view.language.mode.value = value,
        SetTargetLanguage {
            group,
            index,
            value,
        } => {
            if let Some(choice) = targets(group, view).get_mut(index) {
                choice.value = value;
            }
        }
        AddTarget(group) => {
            let list = targets(group, view);
            if list.len() < 3 {
                list.push(list.first().cloned().unwrap_or_default());
            }
        }
        DeleteTarget { group, index } => {
            let list = targets(group, view);
            if index < list.len() {
                list.remove(index);
            }
        }
        MoveTarget {
            group,
            index,
            offset,
        } => {
            let list = targets(group, view);
            let next = index as i32 + offset;
            if index < list.len() && next >= 0 && (next as usize) < list.len() {
                list.swap(index, next as usize);
            }
        }
        SaveLanguagePreset(_) => {
            let id = format!("preview-{}", view.language.presets.options.len() + 1);
            view.language.presets.options.push(DashboardOption {
                value: id,
                label: "Preview preset".into(),
            });
        }
        ApplyLanguagePreset(value) => view.language.presets.value = value,
        DeleteLanguagePreset(value) => view.language.presets.options.retain(|v| v.value != value),
        HeadsetWidthDown | HeadsetWidthUp => step_number(
            &mut view.headset.width,
            if action == HeadsetWidthUp {
                0.02
            } else {
                -0.02
            },
            " m",
        ),
        WristWidthDown | WristWidthUp => step_number(
            &mut view.wrist.width,
            if action == WristWidthUp { 0.02 } else { -0.02 },
            " m",
        ),
        HeadsetOpacityDown | HeadsetOpacityUp => step_number(
            &mut view.headset.opacity,
            if action == HeadsetOpacityUp { 5. } else { -5. },
            "%",
        ),
        WristOpacityDown | WristOpacityUp => step_number(
            &mut view.wrist.opacity,
            if action == WristOpacityUp { 5. } else { -5. },
            "%",
        ),
        AdjustDisplayPosition {
            kind,
            field,
            direction,
        } => {
            let list = if kind == DisplayKind::Wrist {
                &mut view.wrist.position
            } else {
                &mut view.headset.position
            };
            let rotation = matches!(
                field,
                super::dashboard::PositionField::Pitch
                    | super::dashboard::PositionField::Yaw
                    | super::dashboard::PositionField::Roll
            );
            if let Some(value) = list.iter_mut().find(|v| v.field == field) {
                step_number(
                    &mut value.value,
                    direction as f32 * if rotation { 5. } else { 0.01 },
                    if rotation { "°" } else { " m" },
                );
            }
        }
        ResetDisplayPosition(kind) => {
            let defaults: DashboardViewModel =
                serde_json::from_str(include_str!("preview-fixture.json")).expect("valid fixture");
            if kind == DisplayKind::Wrist {
                view.wrist.position = defaults.wrist.position;
            } else {
                view.headset.position = defaults.headset.position;
            }
        }
        _ => {}
    }
}
fn step_number(value: &mut String, delta: f32, unit: &str) {
    let number = value
        .trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.')
        .parse::<f32>()
        .unwrap_or(0.);
    *value = format!("{:.2}{unit}", number + delta);
}

pub fn mode() -> Option<bool> {
    match std::env::var("VRCS_VR_PREVIEW").as_deref() {
        Ok("mock") => Some(false),
        Ok("live") => Some(true),
        _ => None,
    }
}
pub fn setup(app: &tauri::AppHandle, live: bool) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("VRCS_VR_SURFACE_SMOKE").is_some() {
        let mut preview = Preview::new(false, Arc::new(Mutex::new(Placement::default())));
        let mut views = Vec::new();
        for panel in [0, 2, 3, 5, 8, 10] {
            preview.panel = panel;
            views.push((
                preview.surface_id().unwrap(),
                preview.surface_view().unwrap(),
            ));
        }
        preview.panel = 0;
        preview.menu_ui.apply(
            &wrist_menu::Command::Page(wrist_menu::Page::Players),
            &preview.players,
        );
        views.push((SurfaceId::Menu, preview.surface_view().unwrap()));
        let dark = preview.controls.dark;
        views.extend([
            (SurfaceId::PlayerAudio, SurfaceView::PlayerAudio {
                dark,
                notice: super::player_audio_ui::Prompt {
                    request_id: 1,
                    title: "Player audio consent".into(),
                    intro: "Read the notice before continuing.".into(),
                    sections: vec![super::player_audio_ui::Section {
                        title: "Audio use".into(),
                        paragraphs: vec!["Only explicitly selected audio is processed.".repeat(30)],
                    }],
                    remaining_seconds: 0,
                    reading_status: "Ready".into(),
                    confirm: "Confirm".into(), cancel: "Cancel".into(),
                    previous: "Previous".into(), next: "Next".into(), scroll_hint: "Scroll to read".into(),
                },
            }),
            (SurfaceId::Dashboard, SurfaceView::Dashboard {
                dark,
                dashboard: Box::new(preview.view.clone()),
                ui: super::dashboard::DashboardUi::default(),
            }),
            (SurfaceId::OcrSelection, SurfaceView::OcrSelection { dark }),
            (SurfaceId::OcrResult, SurfaceView::OcrResult {
                dark,
                scene: super::ocr_plane::OcrScene {
                    aspect: 1.6,
                    opacity: 0.94,
                    blocks: vec![
                        super::ocr_plane::OcrSceneBlock {
                            id: 0,
                            text: "Open settings · 打开设置".into(),
                            quad: [[0.12, 0.18], [0.54, 0.20], [0.52, 0.34], [0.11, 0.32]],
                            masks: vec![[[0.12, 0.18], [0.54, 0.20], [0.52, 0.34], [0.11, 0.32]]],
                        },
                        super::ocr_plane::OcrSceneBlock {
                            id: 1,
                            text: "A longer translation can expand beside the original label without covering nearby text.".into(),
                            quad: [[0.58, 0.58], [0.71, 0.58], [0.71, 0.63], [0.58, 0.63]],
                            masks: vec![[[0.58, 0.58], [0.71, 0.58], [0.71, 0.63], [0.58, 0.63]]],
                        },
                    ],
                    excluded: vec![[[0.78, 0.52], [0.90, 0.52], [0.90, 0.74], [0.78, 0.74]]],
                },
            }),
        ]);
        super::surface::smoke(app.clone(), views);
        return Ok(());
    }
    app.add_capability(include_str!("../../vr-preview.capability.json"))?;
    let manager = app.state::<super::Manager>();
    let placement = if live {
        manager.placement.clone()
    } else {
        Arc::new(Mutex::new(Placement::default()))
    };
    app.manage(PreviewState(Mutex::new(Preview::new(live, placement))));
    tauri::WebviewWindowBuilder::new(
        app,
        "vr-preview",
        tauri::WebviewUrl::App("index.html?window=vr-preview".into()),
    )
    .title(if live {
        "VRCS · VR Preview · LIVE"
    } else {
        "VRCS · VR Preview · MOCK"
    })
    .inner_size(1280., 900.)
    .min_inner_size(900., 680.)
    .build()?;
    Ok(())
}
fn require_window(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "vr-preview" {
        Ok(())
    } else {
        Err("Preview commands require the preview window".into())
    }
}

#[tauri::command]
pub fn vr_preview_open(
    window: WebviewWindow,
    state: State<'_, PreviewState>,
) -> Result<Info, String> {
    require_window(&window)?;
    let mut p = state.0.lock().map_err(|e| e.to_string())?;
    p.cancel();
    p.session += 1;
    p.generation += 1;
    p.render()?;
    Ok(p.info())
}

#[tauri::command]
pub fn vr_preview_update(
    window: WebviewWindow,
    state: State<'_, PreviewState>,
    manager: State<'_, super::Manager>,
    session: u32,
    operation: Operation,
) -> Result<Info, String> {
    require_window(&window)?;
    let mut p = state.0.lock().map_err(|e| e.to_string())?;
    if p.session != session {
        return Err("Stale preview session".into());
    }
    let expected_players = if p.live {
        let (controls, players, revision) = {
            let hub = manager.controls.lock().map_err(|e| e.to_string())?;
            (
                hub.snapshot(),
                hub.player_display().clone(),
                hub.player_display_revision(),
            )
        };
        let expected = (controls.owner_epoch, revision);
        p.sync_control_views(controls, players);
        if let Some(view) = manager.preview_view() {
            p.sync_dashboard_view(view)?;
        }
        expected
    } else {
        (0, 0)
    };
    let effects = p.update(operation)?;
    let mut dispatch = Vec::new();
    for effect in effects {
        if p.live {
            dispatch.push(effect);
        } else {
            p.apply_mock(effect);
        }
    }
    p.refresh();
    p.render()?;
    let info = p.info();
    drop(p);
    for effect in dispatch {
        match effect {
            Effect::PlayerDisplay(action) => super::runtime::dispatch_player_display(
                window.app_handle(),
                &manager.controls,
                ControlSource::DevPreview,
                expected_players,
                action,
            ),
            Effect::Control(action) => super::runtime::dispatch_control(
                window.app_handle(),
                &manager.controls,
                ControlSource::DevPreview,
                action,
            ),
            Effect::Dashboard(action) => window
                .emit_to("main", "vr-dashboard-action", action)
                .map_err(|e| e.to_string())?,
            Effect::Placement(request) => {
                let hub = manager.controls.lock().map_err(|e| e.to_string())?;
                let epoch = if hub.active() {
                    hub.snapshot().owner_epoch
                } else {
                    0
                };
                drop(hub);
                super::ui_runtime::dispatch_placement(
                    window.app_handle(),
                    epoch,
                    &manager.placement,
                    request,
                );
            }
        }
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personnel_entry_navigates_without_capture_and_stale_release_cannot_hide_a_track() {
        let mut p = mock();
        p.render().unwrap();
        let set_layout = |p: &mut Preview, command: serde_json::Value| {
            let layout = serde_json::from_value(serde_json::json!({
                "key": p.surface_key(),
                "regions": [{"id": 4, "bounds": [0.4, 0.84, 0.525, 0.965], "circle": true, "menu": command}],
                "menu_bounds": [0.0125, 0.80625, 0.9875, 0.99375], "page_count": 1
            })).unwrap();
            p.update(Operation::Layout { layout }).unwrap();
        };
        set_layout(&mut p, serde_json::json!({"page": "players"}));
        let point = [0.46, 0.9];
        p.input(input(&p, point, 0)).unwrap();
        p.input(input(&p, point, 1)).unwrap();
        assert!(p.input(input(&p, point, 0)).unwrap().is_empty());
        p.render().unwrap();
        let view = serde_json::to_value(&p.surface.as_ref().unwrap().view).unwrap();
        assert_eq!(view["ui"]["page"], "players");
        assert!(p.pending.iter().all(Option::is_none));
        set_layout(
            &mut p,
            serde_json::json!({"display": {"show_all_tracks": null}}),
        );
        p.input(input(&p, point, 0)).unwrap();
        p.input(input(&p, point, 1)).unwrap();
        let release = input(&p, point, 0);
        p.layout_changed();
        assert!(p.input(release).is_err());
        assert!(p.pressed.is_none());
    }

    #[test]
    fn mock_personnel_processing_visibility_and_language_filters_are_independent() {
        let mut p = mock();
        let first = p.players.players[0].clone();
        let saved = p
            .players
            .players
            .iter()
            .find(|player| player.track_id.is_none())
            .unwrap()
            .user_id
            .clone()
            .unwrap();
        let languages = p.players.languages.clone();
        let select = |p: &mut Preview, action| {
            p.apply_mock(Effect::PlayerDisplay(action));
            assert!(p.players.busy);
            p.player_pending.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(1);
            p.refresh();
            assert!(!p.players.busy);
            assert!(p.players.error.is_none());
        };
        select(&mut p, player_display::Action::SelectCurrent(()));
        assert_eq!(p.players.selected_count, 3);
        assert!(p
            .players
            .players
            .iter()
            .filter(|player| player.current && player.user_id.is_some())
            .all(|player| player.selected));
        assert!(
            p.players
                .players
                .iter()
                .find(|player| player.user_id.as_ref() == Some(&saved))
                .unwrap()
                .selected
        );
        assert!(
            !p.players
                .players
                .iter()
                .find(|player| player.user_id.is_none())
                .unwrap()
                .selected
        );
        select(
            &mut p,
            player_display::Action::SetUserSelected {
                user_id: first.user_id.clone().unwrap(),
                selected: false,
            },
        );
        assert_eq!(p.players.selected_count, 2);
        assert!(p
            .players
            .players
            .iter()
            .filter(|player| player.user_id == first.user_id)
            .all(|player| !player.selected));
        let selected_count = p.players.selected_count;
        select(
            &mut p,
            player_display::Action::SetTrackHidden {
                track_id: first.track_id.unwrap(),
                hidden: true,
            },
        );
        assert_eq!(p.players.selected_count, selected_count);
        assert_eq!(p.players.languages, languages);
        assert!(p.players.players[0].hidden);
        select(&mut p, player_display::Action::ClearSelection(()));
        assert_eq!(p.players.selection_mode, "selected");
        assert_eq!(p.players.selected_count, 0);
        assert!(p.players.players[0].hidden);
        select(&mut p, player_display::Action::SetFollowDefault(false));
        assert_eq!(p.players.languages, languages);
        select(
            &mut p,
            player_display::Action::SetSourceLanguageHidden {
                language: "fr".into(),
                hidden: true,
            },
        );
        select(&mut p, player_display::Action::ShowAllTracks(()));
        assert!(p.players.players.iter().all(|player| !player.hidden));
        assert!(
            p.players
                .languages
                .iter()
                .find(|language| language.code == "fr")
                .unwrap()
                .hidden
        );
        select(&mut p, player_display::Action::ClearHiddenLanguages(()));
        assert!(!p.players.follow_default);
        assert!(p.players.languages.iter().all(|language| !language.hidden));
        select(&mut p, player_display::Action::SetFollowDefault(true));
        assert!(p.players.follow_default);
        assert!(
            p.players
                .languages
                .iter()
                .find(|language| language.code == "en")
                .unwrap()
                .hidden
        );
        select(
            &mut p,
            player_display::Action::SetProcessingMode("all".into()),
        );
        assert_eq!(p.players.selection_mode, "all");
        assert_eq!(p.players.selected_count, 0);
    }

    #[test]
    fn mock_personnel_failure_keeps_confirmed_values_and_does_not_block_quick_controls() {
        let mut p = mock();
        p.fail_save = true;
        let before = p.players.players.clone();
        let action = player_display::Action::SetTrackHidden {
            track_id: before[0].track_id.clone().unwrap(),
            hidden: true,
        };
        p.apply_mock(Effect::PlayerDisplay(action));
        assert!(p.players.busy);
        assert_eq!(p.players.players, before);
        p.apply_mock(Effect::PlayerDisplay(
            player_display::Action::ShowAllTracks(()),
        ));
        p.apply_mock(Effect::Control(ControlAction::SetOscEnabled(true)));
        assert!(p.pending[2].is_some());
        p.pending[2].as_mut().unwrap().1 = Instant::now() + Duration::from_secs(1);
        p.player_pending.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(1);
        p.refresh();
        assert!(!p.players.busy);
        assert!(p.players.error.as_deref().unwrap().contains("failed"));
        assert_eq!(p.players.players, before);
        assert!(p.pending[2].is_some());
    }

    #[test]
    fn live_personnel_sync_invalidates_old_hits_and_owner_or_tracking_reset_closes_settings() {
        let mut p = Preview::new(true, Arc::new(Mutex::new(Placement::default())));
        let players = sample_player_display(Scene::Long);
        p.sync_control_views(p.controls.clone(), players);
        p.menu_ui.apply(
            &wrist_menu::Command::Page(wrist_menu::Page::Players),
            &p.players,
        );
        p.menu_ui.player_page = 2;
        p.render().unwrap();
        measure(&mut p);
        let point = [0.1625, 0.9];
        p.input(input(&p, point, 0)).unwrap();
        p.input(input(&p, point, 1)).unwrap();
        let release = input(&p, point, 0);
        let mut players = p.players.clone();
        players.players.truncate(1);
        p.sync_control_views(p.controls.clone(), players.clone());
        assert_eq!(p.menu_ui.player_page, 0);
        assert!(p.pressed.is_none());
        assert!(p.layout.is_none());
        assert!(p.input(release).is_err());
        let mut controls = p.controls.clone();
        controls.owner_epoch += 1;
        p.sync_control_views(controls, players);
        assert_eq!(p.menu_ui.page, wrist_menu::Page::Home);
        for operation in [
            Operation::Tracking { available: false },
            Operation::OriginChanged,
        ] {
            p.menu_ui.page = wrist_menu::Page::Languages;
            p.update(operation).unwrap();
            assert_eq!(p.menu_ui.page, wrist_menu::Page::Home);
        }
    }

    #[test]
    fn live_personnel_release_emits_a_request_without_mutating_the_snapshot() {
        let mut p = Preview::new(true, Arc::new(Mutex::new(Placement::default())));
        p.sync_control_views(p.controls.clone(), sample_player_display(Scene::Normal));
        p.render().unwrap();
        let action = player_display::Action::SetTrackHidden {
            track_id: p.players.players[0].track_id.clone().unwrap(),
            hidden: true,
        };
        let before = p.players.clone();
        let layout = serde_json::from_value(serde_json::json!({
            "key": p.surface_key(),
            "regions": [{"id": 5, "bounds": [0.1, 0.2, 0.3, 0.3], "menu": {"display": action}}],
            "menu_bounds": [0.0125, 0.80625, 0.9875, 0.99375],
            "panels": [{"bounds": [0.0125, 0.0125, 0.9875, 0.79375], "radius": [0.025, 0.025]}],
            "page_count": 1
        }))
        .unwrap();
        p.update(Operation::Layout { layout }).unwrap();
        let point = [0.2, 0.25];
        for buttons in [0, 1] {
            assert!(p.input(input(&p, point, buttons)).unwrap().is_empty());
        }
        let effects = p.input(input(&p, point, 0)).unwrap();
        assert!(matches!(effects.as_slice(), [Effect::PlayerDisplay(value)] if value == &action));
        assert_eq!(p.players, before);
        assert!(p.player_pending.is_none());
    }

    fn mock() -> Preview {
        Preview::new(false, Arc::new(Mutex::new(Placement::default())))
    }
    fn input(p: &Preview, point: [f32; 2], buttons: u8) -> Input {
        Input {
            panel: p.panel,
            generation: p.generation,
            revision: p.revision,
            point: Some(point),
            buttons,
            depth: 0.8,
            rotation: [0.; 3],
        }
    }
    fn apply(p: &mut Preview, operation: Operation) {
        for effect in p.update(operation).unwrap() {
            p.apply_mock(effect);
        }
        p.refresh();
        p.render().unwrap();
        measure(p);
    }
    fn measure(p: &mut Preview) {
        use super::super::surface::Region;
        let regions = match p.panel {
            0 => vec![Region {
                id: 0,
                bounds: [128., 1072., 288., 1232.].map(|value| value / 1280.),
                circle: true,
                dashboard: None,
                menu: Some(wrist_menu::Command::Control(
                    ControlAction::SetWristEnabled(!p.controls.items[0].requested),
                )),
            }],
            1 => vec![Region {
                id: 17,
                bounds: [0.2, 0.3, 0.4, 0.5],
                circle: false,
                dashboard: Some(super::super::dashboard::DashboardCommand::Action(
                    DashboardAction::ToggleOcr,
                )),
                menu: None,
            }],
            2 => {
                let placement = p.placement.lock().unwrap();
                let mut regions = vec![Region {
                    id: 0,
                    bounds: [0.1, 0.02, 0.6, 0.1],
                    circle: false,
                    dashboard: None,
                    menu: None,
                }];
                if placement.display_mode() == Mode::World && !placement.busy() {
                    regions.push(Region {
                        id: 3,
                        bounds: [0.7, 0.02, 0.8, 0.12],
                        circle: true,
                        dashboard: None,
                        menu: None,
                    });
                }
                regions
            }
            4 | 5 | 9 => vec![Region {
                id: 7,
                bounds: [0.85, 0.88, 0.92, 0.95],
                circle: true,
                dashboard: None,
                menu: None,
            }],
            3 | 6 | 7 | 8 | 10 => vec![],
            _ => return,
        };
        p.update(Operation::Layout {
            layout: SurfaceLayout {
                key: p.surface_key(),
                regions,
                menu_bounds: (p.panel == 0)
                    .then_some([16., 1032., 1264., 1272.].map(|value| value / 1280.)),
                panels: match p.panel {
                    0 if p.menu_ui.page != wrist_menu::Page::Home => {
                        vec![super::super::surface::Panel {
                            bounds: [16., 16., 1264., 1016.].map(|value| value / 1280.),
                            radius: [0.025, 0.025],
                        }]
                    }
                    2 => vec![
                        super::super::surface::Panel {
                            bounds: [0.01, 0.01, 0.99, 0.125],
                            radius: [0.0575, 0.0575],
                        },
                        super::super::surface::Panel {
                            bounds: [0.02, 0.145, 0.98, 0.98],
                            radius: [0.04, 0.04],
                        },
                    ],
                    4 | 5 | 9 => vec![super::super::surface::Panel {
                        bounds: [0.0234375, 0.0234375, 0.9765625, 0.984375],
                        radius: [0.03125, 0.03125],
                    }],
                    _ => Vec::new(),
                },
                page_count: 1,
                unplaced: Vec::new(),
            },
        })
        .unwrap();
    }

    #[test]
    fn passive_panels_use_react_without_pointer_actions_and_hide_expired_content() {
        let mut p = mock();
        for (panel, kind, content) in [(3, "headset", "messages"), (8, "ocr_progress", "progress")]
        {
            apply(&mut p, Operation::Select { panel });
            let surface = p.info().surface.expect("passive panel uses React");
            let view = serde_json::to_value(surface.view).unwrap();
            assert_eq!(view["kind"], kind);
            assert!(!view[content].is_null());
            let pointer = input(&p, [0.5, 0.5], 1);
            assert!(p.input(pointer).unwrap().is_empty());
            p.render().unwrap();
            assert_eq!(p.info().surface.unwrap().pointer, None);
            apply(
                &mut p,
                Operation::Scene {
                    scene: Scene::Expired,
                },
            );
            let view = serde_json::to_value(p.info().surface.unwrap().view).unwrap();
            assert!(view[content].is_null() || view[content].as_array().is_some_and(Vec::is_empty));
            apply(
                &mut p,
                Operation::Scene {
                    scene: Scene::Normal,
                },
            );
        }
    }

    #[test]
    fn every_panel_uses_react_and_reset_invalidates_old_input() {
        let mut p = mock();
        for panel in 0..PANELS.len() as u32 {
            apply(&mut p, Operation::Select { panel });
            assert!(p.surface_id().is_some());
            assert!(p.info().surface.is_some());
        }
        let old = input(&p, [0.5, 0.5], 0);
        apply(&mut p, Operation::Reset);
        assert!(p.input(old).is_err());
    }

    #[test]
    fn react_pointer_tracks_panel_background_and_clears_on_leave_or_tracking_loss() {
        let mut p = mock();
        for panel in [1, 2, 4, 5, 9] {
            apply(&mut p, Operation::Select { panel });
            let key = p.surface_key();
            let idle = input(&p, [0.5, 0.5], 0);
            apply(&mut p, Operation::Pointer { input: idle });
            let surface = p.info().surface.unwrap();
            assert_eq!(surface.pointer, Some([0.5, 0.5]));
            assert_eq!(surface.hovered, None);
            assert_eq!(surface.key, key);
            let moved = input(&p, [0.6, 0.6], 0);
            apply(&mut p, Operation::Pointer { input: moved });
            assert_eq!(p.info().surface.unwrap().pointer, Some([0.6, 0.6]));
            let mut leave = moved;
            leave.point = None;
            apply(&mut p, Operation::Pointer { input: leave });
            assert_eq!(p.info().surface.unwrap().pointer, None);
            apply(&mut p, Operation::Pointer { input: moved });
            apply(&mut p, Operation::Cancel);
            assert_eq!(p.info().surface.unwrap().pointer, None);
            apply(&mut p, Operation::Pointer { input: moved });
            apply(&mut p, Operation::Tracking { available: false });
            assert_eq!(p.info().surface.unwrap().pointer, None);
            apply(&mut p, Operation::Tracking { available: true });
        }
    }

    #[test]
    fn mock_menu_cursor_covers_background_without_expanding_button_hits() {
        let mut p = mock();
        p.render().unwrap();
        measure(&mut p);
        for (pixels, visible, hit) in [
            ([208., 1152.], true, true),
            ([640., 1152.], true, false),
            ([640., 1260.], true, false),
            ([640., 1044.], true, false),
            ([52., 1120.], true, false),
            ([26., 1048.], false, false),
            ([12., 1152.], false, false),
            ([640., 640.], false, false),
        ] {
            let point = pixels.map(|value| value / 1280.);
            let idle = input(&p, point, 0);
            apply(&mut p, Operation::Pointer { input: idle });
            let surface = p.info().surface.unwrap();
            assert_eq!(surface.pointer, visible.then_some(point), "{point:?}");
            assert_eq!(surface.hovered, hit.then_some(0));
            if !hit {
                let down = input(&p, point, 1);
                assert!(p.input(down).unwrap().is_empty());
                let up = input(&p, point, 0);
                assert!(p.input(up).unwrap().is_empty());
                assert!(p.pending.iter().all(Option::is_none));
            }
        }
        let point = [0.1625, 0.9];
        let idle = input(&p, point, 0);
        apply(&mut p, Operation::Pointer { input: idle });
        let down = input(&p, point, 1);
        apply(&mut p, Operation::Pointer { input: down });
        assert!(p.pending.iter().all(Option::is_none));
        let up = input(&p, point, 0);
        apply(&mut p, Operation::Pointer { input: up });
        assert!(p.pending[0].is_some());
        p.pending[0].as_mut().unwrap().1 = Instant::now() - Duration::from_secs(1);
        p.refresh();
        assert!(p.controls.items[0].requested);
    }

    #[test]
    fn menu_hold_recenters_world_dialogue_once_without_toggling_it() {
        let mut p = mock();
        p.controls.items[0].requested = true;
        let original = transform::matrix(0., 90., 0., [3., 1., 2.]);
        {
            let mut placement = p.placement.lock().unwrap();
            placement.sync_mode(Mode::World);
            placement.initialize_world(original);
        }
        p.render().unwrap();
        measure(&mut p);
        let key = p.surface_key();
        let point = [0.1625, 0.9];
        let idle = input(&p, point, 0);
        apply(&mut p, Operation::Pointer { input: idle });
        let down = input(&p, point, 1);
        apply(&mut p, Operation::Pointer { input: down });
        std::thread::sleep(Duration::from_millis(350));
        apply(&mut p, Operation::Pointer { input: down });
        let view = serde_json::to_value(p.info().surface.unwrap().view).unwrap();
        let progress = view["hold_progress"].as_f64().expect("hold progress");
        assert!((0. ..1.).contains(&progress));
        assert_eq!(p.info().pose, Some(original));
        assert_eq!(p.surface_key(), key);
        assert!(p.layout.is_some());

        std::thread::sleep(Duration::from_millis(350));
        apply(&mut p, Operation::Pointer { input: down });
        assert_eq!(p.info().pose, Some(dialogue::in_front(head_pose())));
        assert_eq!(p.info().mode, Mode::World);
        let revision = p.placement.lock().unwrap().revision;
        apply(&mut p, Operation::Pointer { input: down });
        assert_eq!(p.placement.lock().unwrap().revision, revision);
        assert_eq!(p.surface_key(), key);

        let up = input(&p, point, 0);
        apply(&mut p, Operation::Pointer { input: up });
        assert!(p.pending.iter().all(Option::is_none));
        assert!(p.controls.items[0].requested);
        assert!(
            serde_json::to_value(p.info().surface.unwrap().view).unwrap()["hold_progress"]
                .is_null()
        );
        assert_eq!(p.placement.lock().unwrap().revision, revision);
    }

    #[test]
    fn menu_hold_clears_when_cancelled_and_requires_world_dialogue() {
        let mut p = mock();
        p.controls.items[0].requested = true;
        p.placement.lock().unwrap().sync_mode(Mode::World);
        p.refresh();
        p.render().unwrap();
        measure(&mut p);
        let original = p.info().pose;
        let point = [0.1625, 0.9];
        for operation in [Operation::Cancel, Operation::Tracking { available: false }] {
            let idle = input(&p, point, 0);
            apply(&mut p, Operation::Pointer { input: idle });
            let down = input(&p, point, 1);
            apply(&mut p, Operation::Pointer { input: down });
            assert!(p.hold_progress.is_some());
            apply(&mut p, operation);
            assert_eq!(p.hold_progress, None);
            assert_eq!(p.pressed, None);
            assert_eq!(p.info().pose, original);
            assert!(p.pending.iter().all(Option::is_none));
            apply(&mut p, Operation::Tracking { available: true });
        }
        for (mode, enabled) in [(Mode::Wrist, true), (Mode::World, false)] {
            p.placement.lock().unwrap().sync_mode(mode);
            p.controls.items[0].requested = enabled;
            p.dirty = true;
            p.render().unwrap();
            measure(&mut p);
            let idle = input(&p, point, 0);
            apply(&mut p, Operation::Pointer { input: idle });
            let down = input(&p, point, 1);
            apply(&mut p, Operation::Pointer { input: down });
            assert_eq!(p.hold_progress, None);
            assert_eq!(p.pressed, Some(0));
            let up = input(&p, point, 0);
            let effects = p.input(up).unwrap();
            assert!(matches!(effects.as_slice(), [Effect::Control(_)]));
        }
    }

    #[test]
    fn dashboard_activates_current_dom_command_only_after_a_matching_release() {
        let mut p = mock();
        apply(&mut p, Operation::Select { panel: 1 });
        let point = [0.3, 0.4];
        assert!(p.input(input(&p, point, 1)).unwrap().is_empty());
        assert!(p.input(input(&p, point, 0)).unwrap().is_empty());
        assert!(p.input(input(&p, point, 1)).unwrap().is_empty());
        assert_eq!(p.pressed, Some(17));
        let effects = p.input(input(&p, point, 0)).unwrap();
        assert!(effects
            .iter()
            .any(|effect| matches!(effect, Effect::Dashboard(DashboardAction::ToggleOcr))));
        assert!(p.layout.is_none());

        p.render().unwrap();
        measure(&mut p);
        assert!(p.input(input(&p, point, 0)).unwrap().is_empty());
        assert!(p.input(input(&p, point, 1)).unwrap().is_empty());
        let old_release = input(&p, point, 0);
        let old_layout = p.layout.take().unwrap();
        p.layout_changed();
        p.render().unwrap();
        p.update(Operation::Layout { layout: old_layout }).unwrap();
        assert!(p.layout.is_none());
        assert!(p.input(old_release).is_err());
        assert!(p.pressed.is_none());
    }

    #[test]
    fn live_placement_receipt_redraws_without_a_control_or_dashboard_change() {
        let mut p = Preview::new(true, Arc::new(Mutex::new(Placement::default())));
        p.panel = 2;
        let Some(Effect::Placement(request)) = p.place(Mode::World) else {
            panic!("expected placement request");
        };
        p.refresh();
        p.render().unwrap();
        let view = serde_json::to_value(p.info().surface).unwrap();
        p.placement
            .lock()
            .unwrap()
            .resolve(request.id, false, Some("Save failed".into()));
        p.refresh();
        p.render().unwrap();
        assert!(!p.info().busy);
        assert_ne!(serde_json::to_value(p.info().surface).unwrap(), view);
    }

    #[test]
    fn dialogue_drag_survives_streaming_but_cancel_and_save_failure_restore_anchor() {
        let mut p = mock();
        apply(&mut p, Operation::Select { panel: 2 });
        let point = [0.2, 0.05];
        let idle = input(&p, point, 0);
        apply(&mut p, Operation::Pointer { input: idle });
        let down = input(&p, point, 2);
        apply(&mut p, Operation::Pointer { input: down });
        assert_eq!(p.placement.lock().unwrap().dragging(), Some(1));
        apply(
            &mut p,
            Operation::Text {
                source: "A new partial".into(),
                translation: "更新".into(),
            },
        );
        assert_eq!(p.placement.lock().unwrap().dragging(), Some(1));
        let mut moved = input(&p, [0.8, 0.7], 2);
        moved.depth = 1.5;
        apply(&mut p, Operation::Pointer { input: moved });
        assert_ne!(p.info().pose, Some(wrist_pose()));
        apply(&mut p, Operation::SaveFailure { enabled: true });
        moved.buttons = 0;
        apply(&mut p, Operation::Pointer { input: moved });
        assert_eq!(p.placement.lock().unwrap().mode(), Mode::Wrist);
        assert!(p.placement.lock().unwrap().error.is_some());
        let idle = input(&p, point, 0);
        apply(&mut p, Operation::Pointer { input: idle });
        let down = input(&p, point, 2);
        apply(&mut p, Operation::Pointer { input: down });
        apply(&mut p, Operation::Tracking { available: false });
        assert!(p.placement.lock().unwrap().dragging().is_none());
        assert_eq!(p.placement.lock().unwrap().mode(), Mode::Wrist);
    }

    #[test]
    fn dialogue_follow_button_saves_and_rolls_back_through_the_placement_flow() {
        let mut p = mock();
        apply(&mut p, Operation::Select { panel: 2 });
        let point = [0.75, 0.07];
        for buttons in [0, 1, 0] {
            let sample = input(&p, point, buttons);
            apply(&mut p, Operation::Pointer { input: sample });
        }
        assert!(!p.placement.lock().unwrap().follow_view());
        apply(&mut p, Operation::Place { mode: Mode::World });
        let pose = p.info().pose;
        for (fail, expected) in [(false, true), (true, true), (false, false)] {
            apply(&mut p, Operation::SaveFailure { enabled: fail });
            for buttons in [0, 1, 0] {
                let sample = input(&p, point, buttons);
                apply(&mut p, Operation::Pointer { input: sample });
            }
            let placement = p.placement.lock().unwrap();
            assert_eq!(placement.follow_view(), expected);
            assert_eq!(placement.error.is_some(), fail);
            assert!(!placement.busy());
            drop(placement);
            assert_eq!(p.info().pose, pose);
            let view = serde_json::to_value(p.info().surface.unwrap().view).unwrap();
            assert_eq!(view["follow_view"], expected);
        }
    }

    #[test]
    fn live_dialogue_follow_button_dispatches_and_resolves_a_placement_request() {
        let mut p = Preview::new(true, Arc::new(Mutex::new(Placement::default())));
        apply(&mut p, Operation::Select { panel: 2 });
        let Some(Effect::Placement(anchor)) = p.place(Mode::World) else {
            panic!("expected world placement request");
        };
        p.placement.lock().unwrap().resolve(anchor.id, true, None);
        p.refresh();
        p.render().unwrap();
        measure(&mut p);
        let pose = p.info().pose;
        let point = [0.75, 0.07];
        for buttons in [0, 1] {
            assert!(p.input(input(&p, point, buttons)).unwrap().is_empty());
        }
        let effects = p.input(input(&p, point, 0)).unwrap();
        let [Effect::Placement(request)] = effects.as_slice() else {
            panic!("expected follow-view placement request");
        };
        assert_eq!(request.mode, Mode::World);
        assert!(request.follow_view);
        assert!(p.placement.lock().unwrap().busy());
        p.placement.lock().unwrap().resolve(request.id, true, None);
        p.refresh();
        p.render().unwrap();
        assert!(!p.info().busy);
        assert!(p.placement.lock().unwrap().follow_view());
        assert!(p.placement.lock().unwrap().head_relative_pose().is_some());
        assert_eq!(p.info().pose, pose);
        let view = serde_json::to_value(p.info().surface.unwrap().view).unwrap();
        assert_eq!(view["follow_view"], true);
    }

    #[test]
    fn live_preview_loads_saved_follow_preference_and_can_disable_it() {
        let mut p = Preview::new(true, Arc::new(Mutex::new(Placement::default())));
        apply(&mut p, Operation::Select { panel: 2 });
        let mut view = p.view.clone();
        view.wrist.anchor_mode = "world".into();
        view.wrist.follow_view = true;
        p.sync_dashboard_view(view).unwrap();
        p.refresh();
        p.render().unwrap();
        measure(&mut p);
        assert_eq!(p.info().mode, Mode::World);
        assert!(p.placement.lock().unwrap().follow_view());
        assert!(p.placement.lock().unwrap().head_relative_pose().is_some());
        let pose = p.info().pose;
        let surface = serde_json::to_value(p.info().surface.unwrap().view).unwrap();
        assert_eq!(surface["follow_view"], true);

        let point = [0.75, 0.07];
        for buttons in [0, 1] {
            assert!(p.input(input(&p, point, buttons)).unwrap().is_empty());
        }
        let effects = p.input(input(&p, point, 0)).unwrap();
        let [Effect::Placement(request)] = effects.as_slice() else {
            panic!("expected follow-view placement request");
        };
        assert!(!request.follow_view);
        let mut view = p.view.clone();
        view.wrist.follow_view = false;
        p.sync_dashboard_view(view).unwrap();
        assert!(p.placement.lock().unwrap().busy());
        p.placement.lock().unwrap().resolve(request.id, true, None);
        p.refresh();
        p.render().unwrap();
        assert!(!p.info().busy);
        assert_eq!(p.info().pose, pose);
        assert!(!p.placement.lock().unwrap().follow_view());
        let surface = serde_json::to_value(p.info().surface.unwrap().view).unwrap();
        assert_eq!(surface["follow_view"], false);
    }

    #[test]
    fn live_preview_rejects_reset_and_failure_injection() {
        let mut p = Preview::new(true, Arc::new(Mutex::new(Placement::default())));
        assert!(p.update(Operation::Reset).is_err());
        assert!(p.update(Operation::SaveFailure { enabled: true }).is_err());
    }

    #[test]
    fn react_input_requires_current_dom_layout_and_uses_stable_region_ids() {
        let mut p = mock();
        p.render().unwrap();
        let point = [0.1625, 0.9];
        assert!(p.input(input(&p, point, 0)).unwrap().is_empty());
        assert!(p.input(input(&p, point, 1)).unwrap().is_empty());
        assert!(p.input(input(&p, point, 0)).unwrap().is_empty());
        apply(&mut p, Operation::Select { panel: 5 });
        let point = [0.88, 0.91];
        for buttons in [0, 1, 0] {
            let sample = input(&p, point, buttons);
            apply(&mut p, Operation::Pointer { input: sample });
        }
        assert!(p.info().closed);
        let old = p.layout.take().unwrap();
        p.layout_changed();
        p.update(Operation::Layout { layout: old }).unwrap();
        assert!(p.layout.is_none());
    }
}
