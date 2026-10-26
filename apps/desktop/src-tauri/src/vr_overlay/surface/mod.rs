//! React owns pixels and layout; the VR worker owns input and texture submission.
mod metrics;
pub mod windows_wgc;

use serde::{Deserialize, Serialize};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use super::backend::{OpenVrBackend, OverlayKind};
use super::control_protocol::ControlSnapshot;
use super::dashboard::{DashboardCommand, DashboardUi, DashboardViewModel};
use super::ocr_plane::OcrScene;
use super::ocr_progress::ProgressView;
use super::ocr_wrist::{Action, Button, View};
use super::placement::Mode;
use super::presentation::WristMessage;
use super::wrist_labels::WristLabels;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceId {
    Menu,
    Dialogue,
    Reader,
    Headset,
    OcrProgress,
    Dashboard,
    DashboardThumbnail,
    OcrSelection,
    OcrResult,
    PlayerAudio,
}

impl SurfaceId {
    fn index(self) -> usize {
        self as usize
    }
    fn name(self) -> &'static str {
        match self {
            Self::Menu => "menu",
            Self::Dialogue => "dialogue",
            Self::Reader => "reader",
            Self::Headset => "headset",
            Self::OcrProgress => "ocr_progress",
            Self::Dashboard => "dashboard",
            Self::DashboardThumbnail => "dashboard_thumbnail",
            Self::OcrSelection => "ocr_selection",
            Self::OcrResult => "ocr_result",
            Self::PlayerAudio => "player_audio",
        }
    }
    fn label(self) -> String {
        format!("vr-surface-{}", self.name())
    }
    pub fn size(self) -> (u32, u32) {
        match self {
            Self::Menu => (1024, 256),
            Self::Dialogue => (768, 768),
            Self::Reader => (1024, 1024),
            Self::PlayerAudio => (1024, 1024),
            // A centered card can grow to four languages with four lines each.
            Self::Headset => (1024, 1360),
            Self::OcrProgress => (768, 224),
            Self::Dashboard => (1440, 900),
            Self::DashboardThumbnail | Self::OcrSelection => (256, 256),
            Self::OcrResult => (2048, 2048),
        }
    }
    fn region_count(self) -> usize {
        match self {
            Self::Menu => 4,
            Self::Dialogue => 4,
            Self::Reader => 8,
            Self::PlayerAudio => 4,
            Self::Dashboard => 192,
            Self::Headset
            | Self::OcrProgress
            | Self::DashboardThumbnail
            | Self::OcrSelection
            | Self::OcrResult => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceKey {
    pub generation: u32,
    pub revision: u32,
}

#[derive(Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SurfaceView {
    PlayerAudio {
        dark: bool,
        notice: super::player_audio_ui::Prompt,
    },
    Menu {
        controls: ControlSnapshot,
        hold_progress: Option<f32>,
    },
    Dialogue {
        visibility: super::dashboard::CaptionVisibility,
        max_visible_players: Option<usize>,
        max_entries: usize,
        controls: ControlSnapshot,
        messages: Vec<WristMessage>,
        parallel: bool,
        notice: Option<String>,
        labels: super::caption_status::CaptionLabels,
        mode: Mode,
        follow_view: bool,
        busy: bool,
        error: Option<String>,
        font_size: u32,
        opacity: f32,
    },
    Reader {
        dark: bool,
        labels: WristLabels,
        reader: View,
        font_size: u32,
        opacity: f32,
    },
    Headset {
        visibility: super::dashboard::CaptionVisibility,
        max_visible_players: Option<usize>,
        dark: bool,
        messages: Vec<WristMessage>,
        notice: Option<String>,
        labels: super::caption_status::CaptionLabels,
        lines_per_language: u32,
        font_size: u32,
        opacity: f32,
    },
    OcrProgress {
        dark: bool,
        progress: Option<ProgressView>,
    },
    Dashboard {
        dark: bool,
        dashboard: Box<DashboardViewModel>,
        ui: DashboardUi,
    },
    #[cfg(all(debug_assertions, feature = "vr-preview"))]
    DashboardThumbnail {
        dark: bool,
    },
    OcrSelection {
        dark: bool,
    },
    OcrResult {
        dark: bool,
        scene: OcrScene,
    },
}

impl SurfaceView {
    pub(super) fn same_layout(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Menu { controls: a, .. }, Self::Menu { controls: b, .. }) => a == b,
            _ => self == other,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct SurfaceState {
    pub key: SurfaceKey,
    pub view: SurfaceView,
    pub hovered: Option<u8>,
    pub pressed: Option<u8>,
    pub pointer: Option<[f32; 2]>,
    pub activity: SurfaceActivity,
    pub restore_scroll: Option<f64>,
}

#[derive(Clone, Copy, Serialize)]
pub struct SurfaceActivity {
    pub active: bool,
    pub request: u32,
}

// Matches the fields that reset the reader viewport in VrSurface.tsx.
#[derive(Clone, PartialEq)]
struct ReaderIdentity(
    super::ocr_wrist::ViewMode,
    usize,
    String,
    String,
    bool,
    usize,
);

fn reader_identity(view: &SurfaceView) -> Option<ReaderIdentity> {
    if let SurfaceView::Reader { reader, .. } = view {
        Some(ReaderIdentity(
            reader.mode,
            reader.block_index,
            reader.source.clone(),
            reader.translation_label.clone(),
            reader.source_only,
            reader.page,
        ))
    } else {
        None
    }
}

fn serial_at_least(value: u32, previous: u32) -> bool {
    value == previous || value.wrapping_sub(previous) < 0x8000_0000
}

#[derive(Clone, Serialize)]
struct SurfacePointer {
    key: SurfaceKey,
    point: Option<[f32; 2]>,
}

#[derive(Clone, Serialize)]
struct SurfaceScroll {
    key: SurfaceKey,
    delta: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Region {
    pub id: u8,
    pub bounds: [f32; 4],
    #[serde(default)]
    pub circle: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard: Option<DashboardCommand>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Panel {
    pub bounds: [f32; 4],
    pub radius: [f32; 2],
}

impl Panel {
    fn contains(&self, [x, y]: [f32; 2]) -> bool {
        let [l, t, r, b] = self.bounds;
        if !(l..r).contains(&x) || !(t..b).contains(&y) {
            return false;
        }
        let rx = self.radius[0].min((r - l) / 2.);
        let ry = self.radius[1].min((b - t) / 2.);
        if rx == 0. || ry == 0. {
            return true;
        }
        let x = x - l;
        let y = y - t;
        ((x - x.clamp(rx, r - l - rx)) / rx).powi(2) + ((y - y.clamp(ry, b - t - ry)) / ry).powi(2)
            <= 1.
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SurfaceLayout {
    pub key: SurfaceKey,
    pub regions: Vec<Region>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu_bounds: Option<[f32; 4]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<Panel>,
    pub page_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unplaced: Vec<usize>,
}

impl SurfaceLayout {
    pub fn validate(&self, surface: SurfaceId) -> Result<(), String> {
        if self.regions.len() > surface.region_count()
            || !(1..=10000).contains(&self.page_count)
            || surface.region_count() == 0 && self.page_count != 1
        {
            return Err("Invalid surface layout size".into());
        }
        if let Some([l, t, r, b]) = self.menu_bounds {
            let (width, height) = SurfaceId::Menu.size();
            if surface != SurfaceId::Menu
                || ![l, t, r, b]
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                || l >= r
                || t >= b
                || (r - l) * (width as f32) < (b - t) * height as f32
            {
                return Err("Invalid menu background bounds".into());
            }
        }
        let panel_limit = match surface {
            SurfaceId::Dialogue => 2,
            SurfaceId::Reader | SurfaceId::PlayerAudio => 1,
            _ => 0,
        };
        if self.panels.len() > panel_limit {
            return Err("Invalid surface panel count".into());
        }
        for panel in &self.panels {
            let [l, t, r, b] = panel.bounds;
            let [rx, ry] = panel.radius;
            if ![l, t, r, b]
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                || l >= r
                || t >= b
                || !rx.is_finite()
                || !ry.is_finite()
                || rx < 0.
                || ry < 0.
                || rx > (r - l) / 2. + f32::EPSILON
                || ry > (b - t) / 2. + f32::EPSILON
            {
                return Err("Invalid surface panel contour".into());
            }
        }
        if self.unplaced.len() > 256
            || surface != SurfaceId::OcrResult && !self.unplaced.is_empty()
            || self
                .unplaced
                .iter()
                .enumerate()
                .any(|(i, id)| self.unplaced[..i].contains(id))
        {
            return Err("Invalid OCR overflow layout".into());
        }
        for (i, region) in self.regions.iter().enumerate() {
            let [l, t, r, b] = region.bounds;
            if usize::from(region.id) >= surface.region_count()
                || ![l, t, r, b]
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                || l >= r
                || t >= b
                || (surface == SurfaceId::Dashboard) != region.dashboard.is_some()
            {
                return Err("Invalid surface hit region".into());
            }
            if region.circle
                && ((r - l) * surface.size().0 as f32 - (b - t) * surface.size().1 as f32).abs()
                    > 2.0
            {
                return Err("Invalid circular hit region".into());
            }
            if self.regions[..i].iter().any(|other| {
                let [ol, ot, or, ob] = other.bounds;
                other.id == region.id || l < or && r > ol && t < ob && b > ot
            }) {
                return Err("Duplicate or overlapping surface hit region".into());
            }
        }
        Ok(())
    }

    pub fn menu_contains(&self, [x, y]: [f32; 2]) -> bool {
        // The capsule background determines cursor visibility; buttons determine clicks.
        self.menu_bounds.is_some_and(|[l, t, r, b]| {
            if !(l..r).contains(&x) || !(t..b).contains(&y) {
                return false;
            }
            let (width, height) = SurfaceId::Menu.size();
            let radius = (b - t) * height as f32 / 2.;
            let span = (r - l) * width as f32;
            let x = (x - l) * width as f32;
            let y = (y - t) * height as f32 - radius;
            (x - x.clamp(radius, span - radius)).powi(2) + y.powi(2) <= radius.powi(2)
        })
    }

    pub fn pointer_contains(&self, point: [f32; 2]) -> bool {
        self.menu_contains(point) || self.panels.iter().any(|panel| panel.contains(point))
    }

    pub fn hit(&self, [x, y]: [f32; 2]) -> Option<u8> {
        self.regions
            .iter()
            .find(|region| {
                let [l, t, r, b] = region.bounds;
                x >= l
                    && x < r
                    && y >= t
                    && y < b
                    && (!region.circle
                        || ((x - (l + r) / 2.) / ((r - l) / 2.)).powi(2)
                            + ((y - (t + b) / 2.) / ((b - t) / 2.)).powi(2)
                            <= 1.)
            })
            .map(|region| region.id)
    }

    pub fn reader_buttons(&self) -> Vec<Button> {
        self.regions
            .iter()
            .filter_map(|region| {
                Some(Button {
                    action: reader_action(region.id)?,
                    bounds: region.bounds.map(|v| v * SurfaceId::Reader.size().0 as f32),
                })
            })
            .collect()
    }
}

pub fn reader_action(id: u8) -> Option<Action> {
    Some(match id {
        0 => Action::PreviousBlock,
        1 => Action::NextBlock,
        2 => Action::PreviousPage,
        3 => Action::NextPage,
        4 => Action::NextLanguage,
        5 => Action::ToggleQuestion,
        6 => Action::TogglePin,
        7 => Action::Close,
        _ => return None,
    })
}

pub fn reader_hover(action: Option<Action>) -> Option<u8> {
    (0..8).find(|&id| reader_action(id) == action)
}

struct Entry {
    generation: u32,
    host_epoch: u32,
    state: Option<SurfaceState>,
    layout: Option<SurfaceLayout>,
    displayed_layout: Option<SurfaceLayout>,
    submitted: Option<SurfaceKey>,
    hwnd: Option<isize>,
    creating: bool,
    ready: bool,
    error: Option<String>,
    pending_since: Option<Instant>,
    has_frame: bool,
    active: bool,
    activity_request: u32,
    host_awake: bool,
    hidden_since: Option<Instant>,
    wake_since: Option<Instant>,
    paint: Option<u32>,
    submitted_paint: Option<u32>,
    layout_paint: Option<u32>,
    snapshot_pending: Option<(SurfaceKey, ReaderIdentity)>,
    snapshot_done: bool,
    saved_scroll: Option<(ReaderIdentity, f64)>,
    last_publish: Option<Instant>,
    metrics: metrics::Metrics,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            generation: 1,
            host_epoch: 1,
            state: None,
            layout: None,
            displayed_layout: None,
            submitted: None,
            hwnd: None,
            creating: false,
            ready: false,
            error: None,
            pending_since: None,
            has_frame: false,
            active: false,
            activity_request: 0,
            host_awake: false,
            hidden_since: None,
            wake_since: None,
            paint: None,
            submitted_paint: None,
            layout_paint: None,
            snapshot_pending: None,
            snapshot_done: true,
            saved_scroll: None,
            last_publish: None,
            metrics: metrics::Metrics::default(),
        }
    }
}

impl Entry {
    fn activity(&self) -> SurfaceActivity {
        SurfaceActivity {
            active: self.active,
            request: self.activity_request,
        }
    }

    fn set_active(&mut self, active: bool, now: Instant) -> bool {
        if self.active == active {
            return false;
        }
        self.active = active;
        self.activity_request = self.activity_request.wrapping_add(1).max(1);
        self.host_awake = false;
        self.layout = None;
        self.displayed_layout = None;
        self.submitted = None;
        self.paint = None;
        self.submitted_paint = None;
        self.layout_paint = None;
        self.has_frame = false;
        self.pending_since = active.then_some(now);
        self.hidden_since = (!active).then_some(now);
        self.wake_since = active.then_some(now);
        let activity = self.activity();
        if let Some(state) = &mut self.state {
            state.activity = activity;
            state.hovered = None;
            state.pressed = None;
            state.pointer = None;
            state.restore_scroll = None;
            if active {
                state.key.revision = state.key.revision.wrapping_add(1).max(1);
            } else {
                self.snapshot_pending =
                    reader_identity(&state.view).map(|identity| (state.key, identity));
                self.snapshot_done = self.snapshot_pending.is_none();
            }
        }
        if metrics::enabled() {
            if active {
                self.metrics.resumes += 1;
            } else {
                self.metrics.pauses += 1;
            }
        }
        true
    }

    fn publish(&mut self, view: SurfaceView, hovered: Option<u8>, pressed: Option<u8>) -> bool {
        // Hold progress changes pixels without invalidating the menu's hit regions.
        let changed = self
            .state
            .as_ref()
            .is_none_or(|state| !state.view.same_layout(&view));
        let feedback = self.state.as_ref().is_none_or(|state| {
            state.view != view || state.hovered != hovered || state.pressed != pressed
        });
        if changed {
            let revision = self
                .state
                .as_ref()
                .map_or(1, |state| state.key.revision.wrapping_add(1).max(1));
            self.state = Some(SurfaceState {
                key: SurfaceKey {
                    generation: self.generation,
                    revision,
                },
                view,
                hovered,
                pressed,
                pointer: None,
                activity: self.activity(),
                restore_scroll: None,
            });
            self.layout = None;
            self.submitted = None;
            self.paint = None;
            self.submitted_paint = None;
            self.layout_paint = None;
        } else if let Some(state) = &mut self.state {
            state.view = view;
            state.hovered = hovered;
            state.pressed = pressed;
        }
        if self.active && self.submitted.is_none() {
            self.pending_since.get_or_insert_with(Instant::now);
        }
        changed || feedback
    }

    fn point(&mut self, point: Option<[f32; 2]>) -> bool {
        let point = point.filter(|point| {
            self.active_layout().is_some()
                && point
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v))
        });
        let Some(state) = &mut self.state else {
            return false;
        };
        if state.pointer == point {
            return false;
        }
        state.pointer = point;
        true
    }

    fn reload(&mut self) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.layout = None;
        self.displayed_layout = None;
        self.submitted = None;
        self.ready = false;
        self.has_frame = false;
        self.pending_since = None;
        self.paint = None;
        self.submitted_paint = None;
        self.layout_paint = None;
        if let Some(state) = &mut self.state {
            state.key = SurfaceKey {
                generation: self.generation,
                revision: 1,
            };
            state.hovered = None;
            state.pressed = None;
            state.pointer = None;
            state.restore_scroll = self.saved_scroll.as_ref().and_then(|(identity, scroll)| {
                (reader_identity(&state.view).as_ref() == Some(identity)).then_some(*scroll)
            });
        }
    }

    fn invalidate_host(&mut self) {
        self.reload();
        self.host_epoch = self.host_epoch.wrapping_add(1).max(1);
        self.hwnd = None;
        self.creating = false;
        self.host_awake = false;
        self.error = None;
    }

    fn timed_out(&self, now: Instant) -> bool {
        if !self.active {
            return false;
        }
        let budget = if self.has_frame {
            Duration::from_millis(500)
        } else {
            Duration::from_secs(3)
        };
        self.pending_since
            .is_some_and(|since| now.saturating_duration_since(since) >= budget)
    }

    fn accept_layout(&mut self, surface: SurfaceId, layout: SurfaceLayout) -> Result<bool, String> {
        layout.validate(surface)?;
        if !self.active
            || self
                .state
                .as_ref()
                .is_none_or(|state| state.key != layout.key)
        {
            return Ok(false);
        }
        if let Some(SurfaceState {
            view: SurfaceView::OcrResult { scene, .. },
            ..
        }) = &self.state
        {
            if layout
                .unplaced
                .iter()
                .any(|id| !scene.blocks.iter().any(|block| block.id == *id))
            {
                return Err("Unknown OCR overflow block".into());
            }
        }
        self.layout = Some(layout);
        Ok(true)
    }

    fn accept_layout_paint(
        &mut self,
        surface: SurfaceId,
        layout: SurfaceLayout,
        paint_serial: u32,
    ) -> Result<bool, String> {
        if paint_serial == 0
            || self
                .layout_paint
                .is_some_and(|previous| !serial_at_least(paint_serial, previous))
        {
            return Ok(false);
        }
        let key = layout.key;
        if !self.accept_layout(surface, layout)? {
            return Ok(false);
        }
        self.layout_paint = Some(paint_serial);
        if self
            .submitted_paint
            .is_none_or(|serial| !serial_at_least(serial, paint_serial))
        {
            self.submitted = None;
            self.pending_since.get_or_insert_with(Instant::now);
        }
        self.accept_paint(key, paint_serial);
        if metrics::enabled() {
            self.metrics.layouts += 1;
        }
        Ok(true)
    }

    fn accept_snapshot(
        &mut self,
        key: Option<SurfaceKey>,
        request: u32,
        scroll: Option<f64>,
    ) -> bool {
        if self.active || self.activity_request != request {
            return false;
        }
        if let Some((expected, identity)) = &self.snapshot_pending {
            if key != Some(*expected) {
                return false;
            }
            let Some(scroll) = scroll.filter(|v| v.is_finite() && (0.0..=1.0e8).contains(v)) else {
                return false;
            };
            self.saved_scroll = Some((identity.clone(), scroll));
        }
        self.snapshot_done = true;
        true
    }

    fn cold(&self, now: Instant) -> bool {
        !self.active
            && self
                .hidden_since
                .is_some_and(|at| now.saturating_duration_since(at) >= Duration::from_secs(60))
    }

    #[cfg(test)]
    fn commit(&mut self, key: SurfaceKey) -> bool {
        if self.active
            && self.state.as_ref().is_some_and(|state| state.key == key)
            && self.layout.as_ref().is_some_and(|layout| layout.key == key)
        {
            self.submitted = Some(key);
            self.displayed_layout = self.layout.clone();
            self.pending_since = None;
            self.has_frame = true;
            true
        } else {
            false
        }
    }

    fn accept_paint(&mut self, key: SurfaceKey, serial: u32) -> bool {
        if !self.active
            || serial == 0
            || self.state.as_ref().is_none_or(|state| state.key != key)
            || self
                .paint
                .is_some_and(|previous| !serial_at_least(serial, previous))
        {
            return false;
        }
        self.paint = Some(serial);
        if metrics::enabled() {
            self.metrics.paints += 1;
        }
        true
    }

    fn needs_paint(&self) -> bool {
        self.active
            && self.paint.is_some()
            && (self.paint != self.submitted_paint || self.submitted.is_none())
    }

    fn capture_target(&self) -> Option<(isize, u32)> {
        // Layout gates new pixels and input, not the last submitted texture.
        self.hwnd
            .filter(|_| self.ready && self.host_awake)
            .map(|hwnd| (hwnd, self.generation))
    }

    fn accepts_frame(&self, key: SurfaceKey, serial: u32, requested: u32) -> bool {
        self.active
            && serial != 0
            && requested != 0
            && serial_at_least(serial, requested)
            && self
                .paint
                .is_some_and(|latest| serial_at_least(latest, serial))
            && self
                .layout_paint
                .is_none_or(|layout| serial_at_least(serial, layout))
            && self
                .submitted_paint
                .is_none_or(|previous| serial_at_least(serial, previous))
            && self.state.as_ref().is_some_and(|state| state.key == key)
            && self.layout.as_ref().is_some_and(|layout| layout.key == key)
    }

    fn commit_paint(&mut self, key: SurfaceKey, serial: u32) -> bool {
        if !self.accepts_frame(key, serial, serial) {
            return false;
        }
        self.submitted = Some(key);
        self.submitted_paint = Some(serial);
        self.displayed_layout = self.layout.clone();
        self.pending_since = None;
        self.has_frame = true;
        if let Some(started) = self.wake_since.take() {
            self.metrics.wake(started.elapsed());
        }
        true
    }

    fn active_layout(&self) -> Option<&SurfaceLayout> {
        self.layout.as_ref().filter(|layout| {
            self.active
                && Some(layout.key) == self.submitted
                && self
                    .state
                    .as_ref()
                    .is_some_and(|state| state.key == layout.key)
        })
    }

    fn pointer_layout(&self) -> Option<&SurfaceLayout> {
        // Keep the contour of the visible texture while new pixels and hit regions are pending.
        self.displayed_layout.as_ref().filter(|layout| {
            self.active && self.has_frame && layout.key.generation == self.generation
        })
    }
}

#[derive(Default)]
pub struct SurfaceHub(Mutex<[Entry; 10]>);

impl SurfaceHub {
    fn set_active(&self, app: &AppHandle, surface: SurfaceId, active: bool) -> Result<(), String> {
        let request = {
            let mut entries = self.0.lock().map_err(|e| e.to_string())?;
            let entry = &mut entries[surface.index()];
            entry
                .set_active(active, Instant::now())
                .then_some((entry.host_epoch, entry.activity()))
        };
        if let Some((epoch, activity)) = request {
            set_host_activity(app, surface, epoch, activity)?;
        }
        Ok(())
    }

    pub fn generation(&self, surface: SurfaceId) -> u32 {
        self.0.lock().unwrap_or_else(|e| e.into_inner())[surface.index()].generation
    }

    pub fn layout(&self, surface: SurfaceId) -> Option<SurfaceLayout> {
        self.0.lock().ok()?[surface.index()]
            .active_layout()
            .cloned()
    }

    pub fn pointer_layout(&self, surface: SurfaceId) -> Option<SurfaceLayout> {
        self.0.lock().ok()?[surface.index()]
            .pointer_layout()
            .cloned()
    }

    pub fn point(
        &self,
        app: &AppHandle,
        surface: SurfaceId,
        point: Option<[f32; 2]>,
    ) -> Result<(), String> {
        let update = {
            let mut entries = self.0.lock().map_err(|e| e.to_string())?;
            let entry = &mut entries[surface.index()];
            if !entry.point(point) || !entry.ready {
                return Ok(());
            }
            entry.state.as_ref().map(|state| SurfacePointer {
                key: state.key,
                point: state.pointer,
            })
        };
        if let Some(update) = update {
            app.emit_to(surface.label(), "vr-surface-pointer", update)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn scroll_reader(&self, app: &AppHandle, delta: f32) -> Result<(), String> {
        self.scroll(app, SurfaceId::Reader, delta)
    }

    pub fn scroll(&self, app: &AppHandle, surface: SurfaceId, delta: f32) -> Result<(), String> {
        if !delta.is_finite() || delta == 0.0 {
            return Ok(());
        }
        let update = {
            let entries = self.0.lock().map_err(|e| e.to_string())?;
            let entry = &entries[surface.index()];
            entry
                .ready
                .then(|| entry.active_layout())
                .flatten()
                .map(|layout| SurfaceScroll {
                    key: layout.key,
                    delta,
                })
        };
        if let Some(update) = update {
            app.emit_to(surface.label(), "vr-surface-scroll", update)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub(super) fn invalidate_host(&self, surface: SurfaceId) {
        self.0.lock().unwrap_or_else(|e| e.into_inner())[surface.index()].invalidate_host();
    }

    pub(super) fn publish(
        &self,
        app: &AppHandle,
        surface: SurfaceId,
        view: SurfaceView,
        hovered: Option<u8>,
        pressed: Option<u8>,
    ) -> Result<(), String> {
        let (create, update) = {
            let mut entries = self.0.lock().map_err(|e| e.to_string())?;
            let entry = &mut entries[surface.index()];
            if let Some(error) = &entry.error {
                return Err(error.clone());
            }
            let content_changed = entry
                .state
                .as_ref()
                .is_none_or(|state| !state.view.same_layout(&view));
            if entry.active
                && content_changed
                && matches!(surface, SurfaceId::Headset | SurfaceId::Dialogue)
                && entry
                    .last_publish
                    .is_some_and(|at| at.elapsed() < Duration::from_millis(50))
            {
                // present runs on every business tick with the latest view, including the final result.
                return Ok(());
            }
            let changed = entry.publish(view, hovered, pressed);
            if changed && entry.active {
                entry.last_publish = Some(Instant::now());
                if metrics::enabled() {
                    entry.metrics.publications += 1;
                }
            }
            let create = entry.active && !entry.creating && entry.hwnd.is_none();
            if create {
                entry.creating = true;
            }
            (
                create.then_some(entry.host_epoch),
                (changed && entry.ready && entry.active)
                    .then(|| entry.state.clone())
                    .flatten(),
            )
        };
        if let Some(epoch) = create {
            create_host(app, surface, epoch)?;
        }
        if let Some(state) = update {
            app.emit_to(surface.label(), "vr-surface-view", state)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Exercises the real offscreen hosts and shared GPU textures without initializing OpenVR.
#[cfg(all(debug_assertions, feature = "vr-preview"))]
pub fn smoke(app: AppHandle, views: Vec<(SurfaceId, SurfaceView)>) {
    use windows_sys::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONULL};

    fn wait_for_layout(
        app: &AppHandle,
        surface: SurfaceId,
        view: &SurfaceView,
        previous: Option<SurfaceKey>,
    ) -> Result<(isize, SurfaceKey), String> {
        let hub = app.state::<SurfaceHub>();
        let started = Instant::now();
        loop {
            hub.set_active(app, surface, true)?;
            hub.publish(app, surface, view.clone(), None, None)?;
            {
                let entries = hub.0.lock().map_err(|e| e.to_string())?;
                let entry = &entries[surface.index()];
                if let Some(error) = &entry.error {
                    return Err(error.clone());
                }
                if entry.ready {
                    if let (Some(hwnd), Some(layout)) = (entry.hwnd, &entry.layout) {
                        if previous.is_none_or(|key| key.generation != layout.key.generation) {
                            if !unsafe { MonitorFromWindow(hwnd as _, MONITOR_DEFAULTTONULL) }
                                .is_null()
                            {
                                return Err(format!(
                                    "{} host overlaps the desktop",
                                    surface.name()
                                ));
                            }
                            return Ok((hwnd, layout.key));
                        }
                    }
                }
            }
            if started.elapsed() >= Duration::from_secs(20) {
                return Err(format!(
                    "{} host did not report a new layout",
                    surface.name()
                ));
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn wait_for_frame(
        app: &AppHandle,
        capture: &mut windows_wgc::Capture,
        surface: SurfaceId,
        key: SurfaceKey,
    ) -> Result<windows_wgc::Captured, String> {
        let started = Instant::now();
        loop {
            let requested = {
                let hub = app.state::<SurfaceHub>();
                let entries = hub.0.lock().map_err(|e| e.to_string())?;
                let entry = &entries[surface.index()];
                entry.needs_paint().then_some(entry.paint).flatten()
            };
            if let Some(frame) = capture.poll(requested)? {
                let hub = app.state::<SurfaceHub>();
                let entries = hub.0.lock().map_err(|e| e.to_string())?;
                if (frame.generation, frame.revision) == (key.generation, key.revision)
                    && entries[surface.index()].accepts_frame(
                        key,
                        frame.paint_serial,
                        frame.requested_serial,
                    )
                {
                    return Ok(frame);
                }
            }
            if started.elapsed() >= Duration::from_secs(10) {
                return Err(format!(
                    "{} capture did not contain its marker",
                    surface.name()
                ));
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    std::thread::spawn(move || {
        let result = (|| -> Result<Vec<serde_json::Value>, String> {
            let directory = std::path::Path::new("target/vr-surface-smoke");
            std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
            let hub = app.state::<SurfaceHub>();
            let mut results = Vec::new();
            // Keep sibling hosts alive when a recovery check destroys one window.
            for (surface, view) in &views {
                wait_for_layout(&app, *surface, view, None)?;
            }
            for (surface, view) in views {
                let (hwnd, mut key) = wait_for_layout(&app, surface, &view, None)?;
                let (width, height) = surface.size();
                let counters = hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                    .metrics
                    .capture
                    .clone();
                let mut capture =
                    windows_wgc::Capture::with_counters(hwnd, 0, width, height, counters.clone())?;
                let first_frame = wait_for_frame(&app, &mut capture, surface, key)?;
                let first = capture.snapshot(first_frame)?;
                let transparent = first
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| pixel[3] == 0)
                    .count();
                let visible = first
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| pixel[3] > 0)
                    .count();
                if transparent == 0 || visible < 100 {
                    return Err(format!(
                        "{} capture lost transparency or content",
                        surface.name()
                    ));
                }
                std::fs::write(directory.join(format!("{}.bgra", surface.name())), &first)
                    .map_err(|e| e.to_string())?;
                capture.submitted(first_frame)?;
                hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                    .commit_paint(key, first_frame.paint_serial);
                // Ignore late font receipts before measuring the settled static interval.
                let settle_until = Instant::now() + Duration::from_millis(350);
                while Instant::now() < settle_until {
                    let requested = {
                        let entries = hub.0.lock().map_err(|e| e.to_string())?;
                        let entry = &entries[surface.index()];
                        entry.needs_paint().then_some(entry.paint).flatten()
                    };
                    if let Some(frame) = capture.poll(requested)? {
                        let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
                        if entries[surface.index()].accepts_frame(
                            key,
                            frame.paint_serial,
                            frame.requested_serial,
                        ) {
                            entries[surface.index()].commit_paint(key, frame.paint_serial);
                            // Protect first_frame for the existing buffer-preservation check below.
                        }
                    }
                    std::thread::sleep(Duration::from_millis(15));
                }
                let static_surface = surface != SurfaceId::OcrProgress;
                let copies_before_idle = counters.copies.load(std::sync::atomic::Ordering::Relaxed);
                if static_surface {
                    let until = Instant::now() + Duration::from_secs(2);
                    while Instant::now() < until {
                        let requested = {
                            let entries = hub.0.lock().map_err(|e| e.to_string())?;
                            let entry = &entries[surface.index()];
                            entry.needs_paint().then_some(entry.paint).flatten()
                        };
                        if capture.poll(requested)?.is_some() {
                            return Err("Static surface requested another copy".into());
                        }
                        std::thread::sleep(Duration::from_millis(15));
                    }
                    if counters.copies.load(std::sync::atomic::Ordering::Relaxed)
                        != copies_before_idle
                    {
                        return Err(format!("{} made idle texture copies", surface.name()));
                    }
                }
                let hovered = hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                    .layout
                    .as_ref()
                    .and_then(|layout| layout.regions.last())
                    .map(|region| region.id);
                let mut previous = first.clone();
                for hover in hovered
                    .into_iter()
                    .flat_map(|id| [Some(id), None, Some(id)])
                {
                    hub.publish(&app, surface, view.clone(), hover, None)?;
                    let changed_at = std::time::Instant::now();
                    let mut last_frame = None;
                    let paint_before_hover =
                        hub.0.lock().map_err(|e| e.to_string())?[surface.index()].paint;
                    let copies_before_hover =
                        counters.copies.load(std::sync::atomic::Ordering::Relaxed);
                    let frames_before_hover = counters
                        .source_frames
                        .load(std::sync::atomic::Ordering::Relaxed);
                    loop {
                        let requested =
                            hub.0.lock().map_err(|e| e.to_string())?[surface.index()].paint;
                        if let Some(frame) = capture.poll(requested)? {
                            last_frame = Some(frame);
                            if capture.snapshot(first_frame)? != first {
                                return Err(format!(
                                    "{} capture overwrote its submitted texture",
                                    surface.name()
                                ));
                            }
                            let pixels = capture.snapshot(frame)?;
                            if (frame.generation, frame.revision) == (key.generation, key.revision)
                                && pixels != previous
                            {
                                previous = pixels;
                                break;
                            }
                        }
                        if changed_at.elapsed().as_secs() >= 5 {
                            let entries = hub.0.lock().map_err(|e| e.to_string())?;
                            let entry = &entries[surface.index()];
                            return Err(format!(
                                "{} hover {hover:?} did not update its captured texture: paint={paint_before_hover:?}->{:?}, feedback={:?}, last_frame={last_frame:?}, copies={}, frames={}",
                                surface.name(), entry.paint, entry.state.as_ref().and_then(|state| state.hovered),
                                counters.copies.load(std::sync::atomic::Ordering::Relaxed) - copies_before_hover,
                                counters.source_frames.load(std::sync::atomic::Ordering::Relaxed) - frames_before_hover
                            ));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                }
                if hovered.is_none() {
                    let mut updated = view.clone();
                    match &mut updated {
                        SurfaceView::Headset { messages, .. } => {
                            *messages = vec![WristMessage {
                                original: None,
                                text: "Updated caption / 字幕内容已更新".into(),
                                side: super::presentation::MessageSide::Left,
                                speaker: None,
                            }];
                        }
                        SurfaceView::OcrProgress {
                            progress: Some(progress),
                            ..
                        } => {
                            progress.status = "Updated OCR progress".into();
                            progress.translation_fraction = Some((2, 3));
                        }
                        SurfaceView::OcrResult { scene, .. } => {
                            for block in &mut scene.blocks {
                                block.text = "Updated OCR translation / 已更新".into();
                            }
                        }
                        SurfaceView::OcrSelection { dark } => {
                            *dark = !*dark;
                        }
                        _ => return Err("Smoke surface has no content to update".into()),
                    }
                    hub.publish(&app, surface, updated, None, None)?;
                    key = hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                        .state
                        .as_ref()
                        .ok_or("Smoke surface has no state")?
                        .key;
                    let frame = wait_for_frame(&app, &mut capture, surface, key)?;
                    if capture.snapshot(frame)? == first {
                        return Err(format!("{} content did not update", surface.name()));
                    }
                    if capture.snapshot(first_frame)? != first {
                        return Err(format!(
                            "{} capture overwrote its submitted texture",
                            surface.name()
                        ));
                    }
                }
                // The same source shutdown and host request used by the worker must be reversible.
                hub.set_active(&app, surface, false)?;
                capture.suspend();
                if capture.active() {
                    return Err("Hidden source remained active".into());
                }
                let hidden_copies = counters.copies.load(std::sync::atomic::Ordering::Relaxed);
                let until = Instant::now() + Duration::from_secs(2);
                while Instant::now() < until {
                    if capture.poll(Some(1))?.is_some() {
                        return Err("Hidden capture produced a frame".into());
                    }
                    std::thread::sleep(Duration::from_millis(15));
                }
                if counters.copies.load(std::sync::atomic::Ordering::Relaxed) != hidden_copies {
                    return Err("Hidden capture copied pixels".into());
                }
                let (_, warm_key) = wait_for_layout(&app, surface, &view, None)?;
                if warm_key == key {
                    return Err("Wake reused an old input key".into());
                }
                key = warm_key;
                capture.resume()?;
                let warm_frame = wait_for_frame(&app, &mut capture, surface, key)?;
                capture.submitted(warm_frame)?;
                hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                    .commit_paint(key, warm_frame.paint_serial);
                for recreate in [false, true] {
                    drop(capture);
                    let generations = hub
                        .0
                        .lock()
                        .map_err(|e| e.to_string())?
                        .each_ref()
                        .map(|entry| entry.generation);
                    if recreate {
                        let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
                        entries[surface.index()].invalidate_host();
                        if entries[surface.index()].active_layout().is_some() {
                            return Err("Invalidated host retained its input layout".into());
                        }
                    } else {
                        app.get_webview_window(&surface.label())
                            .ok_or("Smoke surface window is missing")?
                            .reload()
                            .map_err(|e| e.to_string())?;
                    }
                    let (hwnd, recovered) = wait_for_layout(&app, surface, &view, Some(key))?;
                    capture = windows_wgc::Capture::with_counters(
                        hwnd,
                        0,
                        width,
                        height,
                        counters.clone(),
                    )?;
                    let frame = wait_for_frame(&app, &mut capture, surface, recovered)?;
                    let pixels = capture.snapshot(frame)?;
                    if !pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] == 0)
                        || pixels
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .filter(|pixel| pixel[3] > 0)
                            .count()
                            < 100
                    {
                        return Err(format!("{} recovery lost its content", surface.name()));
                    }
                    capture.submitted(frame)?;
                    let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
                    if entries.iter().enumerate().any(|(index, entry)| {
                        index != surface.index() && entry.generation != generations[index]
                    }) {
                        return Err("Surface recovery changed another host".into());
                    }
                    if !entries[surface.index()].commit_paint(recovered, frame.paint_serial) {
                        return Err("Recovered frame did not match the current layout".into());
                    }
                    key = recovered;
                }
                hub.set_active(&app, surface, false)?;
                capture.suspend();
                results.push(serde_json::json!({"surface":surface,"width":width,"height":height,"key":key,"desktop_hidden":true,"transparent_pixels":transparent,"visible_pixels":visible,"hover_updated":hovered.map(|_| true),"content_updated":hovered.is_none(),"submitted_texture_preserved":true,"reload_recovered":true,"host_recreated":true,"idle_no_copies":static_surface.then_some(true),"hidden_source_closed":true,"hidden_no_copies":true,"warm_recovered":true}));
            }
            if app.get_webview_window("vr-surface-smoke-desktop").is_none() {
                return Err("Default desktop WebView was closed during capture smoke".into());
            }
            Ok(results)
        })();
        let report = match &result {
            Ok(surfaces) => {
                serde_json::json!({"ok":true,"desktop_webview_coexisted":true,"surfaces":surfaces})
            }
            Err(error) => serde_json::json!({"ok":false,"error":error}),
        };
        eprintln!("VR surface capture smoke: {report}");
        let _ = std::fs::write(
            "target/vr-surface-smoke/result.json",
            serde_json::to_vec_pretty(&report).unwrap_or_default(),
        );
        app.exit(if result.is_ok() { 0 } else { 1 });
    });
}

fn local_url(url: &tauri::Url) -> bool {
    url.scheme() == "tauri"
        || matches!(url.host_str(), Some("tauri.localhost"))
        || cfg!(debug_assertions)
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

fn require_host(window: &WebviewWindow, surface: SurfaceId) -> Result<u32, String> {
    let url = window.url().map_err(|e| e.to_string())?;
    if window.label() != surface.label() || !local_url(&url) {
        return Err("Invalid VR surface window".into());
    }
    url.query_pairs()
        .find_map(|(name, value)| (name == "host_epoch").then(|| value.parse().ok()).flatten())
        .ok_or_else(|| "Missing VR surface host identity".into())
}

#[tauri::command]
pub fn vr_surface_ready(
    window: WebviewWindow,
    surface: SurfaceId,
    hub: State<'_, SurfaceHub>,
) -> Result<Option<SurfaceState>, String> {
    let epoch = require_host(&window, surface)?;
    let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
    let entry = &mut entries[surface.index()];
    if entry.host_epoch != epoch
        || entry.error.is_some()
        || (!entry.creating && entry.hwnd.is_none())
    {
        return Ok(None);
    }
    entry.reload();
    entry.ready = true;
    Ok(entry.state.clone())
}

#[tauri::command]
pub fn vr_surface_layout(
    window: WebviewWindow,
    surface: SurfaceId,
    layout: SurfaceLayout,
    paint_serial: u32,
    hub: State<'_, SurfaceHub>,
) -> Result<bool, String> {
    let epoch = require_host(&window, surface)?;
    let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
    let entry = &mut entries[surface.index()];
    if entry.host_epoch != epoch {
        return Ok(false);
    }
    entry.accept_layout_paint(surface, layout, paint_serial)
}

#[tauri::command]
pub fn vr_surface_paint(
    window: WebviewWindow,
    surface: SurfaceId,
    key: SurfaceKey,
    paint_serial: u32,
    hub: State<'_, SurfaceHub>,
) -> Result<bool, String> {
    let epoch = require_host(&window, surface)?;
    let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
    let entry = &mut entries[surface.index()];
    Ok(entry.host_epoch == epoch && entry.accept_paint(key, paint_serial))
}

#[tauri::command]
pub fn vr_surface_snapshot(
    window: WebviewWindow,
    surface: SurfaceId,
    key: Option<SurfaceKey>,
    activity_request: u32,
    scroll_top: Option<f64>,
    hub: State<'_, SurfaceHub>,
) -> Result<bool, String> {
    let epoch = require_host(&window, surface)?;
    let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
    let entry = &mut entries[surface.index()];
    Ok(entry.host_epoch == epoch && entry.accept_snapshot(key, activity_request, scroll_top))
}

fn set_host_activity(
    app: &AppHandle,
    surface: SurfaceId,
    epoch: u32,
    activity: SurfaceActivity,
) -> Result<(), String> {
    let host_app = app.clone();
    app.run_on_main_thread(move || {
        let hub = host_app.state::<SurfaceHub>();
        {
            let entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
            let entry = &entries[surface.index()];
            if entry.host_epoch != epoch
                || entry.activity_request != activity.request
                || entry.active != activity.active
            {
                return;
            }
        }
        let Some(window) = host_app.get_webview_window(&surface.label()) else {
            return;
        };
        // No new WebView2 dependency is required: WGC is stopped separately and the page pauses its tasks.
        let result = if activity.active {
            window.show()
        } else {
            window.hide()
        };
        let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
        let entry = &mut entries[surface.index()];
        if entry.host_epoch != epoch || entry.activity_request != activity.request {
            return;
        }
        match result {
            Ok(()) => entry.host_awake = activity.active,
            Err(error) => {
                entry.error = Some(error.to_string());
                return;
            }
        }
        let state = (activity.active && entry.ready)
            .then(|| entry.state.clone())
            .flatten();
        drop(entries);
        let _ = host_app.emit_to(surface.label(), "vr-surface-activity", activity);
        if let Some(state) = state {
            let _ = host_app.emit_to(surface.label(), "vr-surface-view", state);
        }
    })
    .map_err(|e| e.to_string())
}

fn create_host(app: &AppHandle, surface: SurfaceId, epoch: u32) -> Result<(), String> {
    let host_app = app.clone();
    app.run_on_main_thread(move || {
        {
            let hub = host_app.state::<SurfaceHub>();
            let entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
            if entries[surface.index()].host_epoch != epoch { return; }
            if !entries[surface.index()].active {
                drop(entries);
                hub.0.lock().unwrap_or_else(|e| e.into_inner())[surface.index()].creating = false;
                return;
            }
        }
        let result = (|| -> Result<Option<isize>, String> {
            let (width, height) = surface.size();
            if let Some(window) = host_app.get_webview_window(&surface.label()) {
                window.destroy().map_err(|e| e.to_string())?;
                // Tauri destroys windows asynchronously. Create on a later worker tick.
                return Ok(None);
            }
            let url = format!(
                "index.html?window=vr-surface&surface={}&host_epoch={epoch}",
                surface.name()
            );
            // Different browser arguments must not share the desktop WebView2 profile.
            let data_directory = host_app.path().app_local_data_dir()
                .map_err(|e| e.to_string())?
                .join("vr-surfaces");
            let window = WebviewWindowBuilder::new(&host_app, surface.label(), WebviewUrl::App(url.into()))
                .title("VRCS VR surface")
                .decorations(false)
                .shadow(false)
                .transparent(true)
                .resizable(false)
                .skip_taskbar(true)
                .focused(false)
                .focusable(false)
                .visible(false)
                .inner_size(width as f64, (height + windows_wgc::MARKER_HEIGHT) as f64)
                .data_directory(data_directory)
                .additional_browser_args("--disable-features=CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows --disable-renderer-backgrounding")
                .on_navigation(local_url)
                .on_page_load(move |window, payload| {
                    if matches!(payload.event(), tauri::webview::PageLoadEvent::Started) {
                        let hub = window.state::<SurfaceHub>();
                        let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                        let entry = &mut entries[surface.index()];
                        if entry.host_epoch == epoch {
                            entry.reload();
                        }
                    }
                })
                .build().map_err(|e| e.to_string())?;
            let event_app = host_app.clone();
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    let hub = event_app.state::<SurfaceHub>();
                    let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                    let entry = &mut entries[surface.index()];
                    if entry.host_epoch == epoch {
                        entry.reload();
                        entry.error = Some("The VR surface window was closed".into());
                    }
                }
            });
            // Initial offscreen coordinates are clamped by tao. Move before showing.
            let left = window.available_monitors().map_err(|e| e.to_string())?
                .iter()
                .map(|monitor| monitor.position().x)
                .min()
                .unwrap_or(0);
            window.set_position(tauri::PhysicalPosition::new(left - width as i32 - 8, 0))
                .map_err(|e| e.to_string())?;
            let scale = window.scale_factor().map_err(|e| e.to_string())?;
            window.set_zoom(1. / scale).map_err(|e| e.to_string())?;
            window.set_size(tauri::PhysicalSize::new(width, height + windows_wgc::MARKER_HEIGHT)).map_err(|e| e.to_string())?;
            window.set_ignore_cursor_events(true).map_err(|e| e.to_string())?;
            // WGC needs a shown window to keep producing frames.
            let active = host_app.state::<SurfaceHub>().0.lock().unwrap_or_else(|e| e.into_inner())[surface.index()].active;
            if active { window.show().map_err(|e| e.to_string())?; }
            window.hwnd().map(|handle| Some(handle.0 as isize)).map_err(|e| e.to_string())
        })();
        let hub = host_app.state::<SurfaceHub>();
        let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
        let entry = &mut entries[surface.index()];
        if entry.host_epoch != epoch {
            return;
        }
        entry.creating = false;
        match result {
            Ok(hwnd) => { entry.hwnd = hwnd; entry.host_awake = entry.active && hwnd.is_some(); }
            Err(error) => entry.error = Some(error),
        }
    }).map_err(|e| e.to_string())
}

/// The worker keeps the GPU resources alive for as long as OpenVR uses them.
pub struct SurfaceRenderer {
    surface: SurfaceId,
    capture: Option<windows_wgc::Capture>,
    identity: Option<(isize, u32, u32)>,
    visible_frame: bool,
    retry_at: Option<Instant>,
    requested: bool,
    last_copy: Option<Instant>,
}

impl SurfaceRenderer {
    pub fn new(surface: SurfaceId) -> Self {
        Self {
            surface,
            capture: None,
            identity: None,
            visible_frame: false,
            retry_at: None,
            requested: false,
            last_copy: None,
        }
    }
    pub fn reset(&mut self) {
        self.capture = None;
        self.identity = None;
        self.visible_frame = false;
        self.retry_at = None;
        self.last_copy = None;
    }

    /// Every business tick ends here, including early returns and disconnected states.
    pub fn finish_tick(
        &mut self,
        app: &AppHandle,
        mut backend: Option<&mut OpenVrBackend>,
        kind: OverlayKind,
    ) {
        let requested = std::mem::take(&mut self.requested) && backend.is_some();
        let hub = app.state::<SurfaceHub>();
        if !requested {
            if let Some(backend) = backend.as_deref_mut() {
                backend.hide(kind);
            }
            if let Some(capture) = &mut self.capture {
                capture.suspend();
            }
            self.visible_frame = false;
            if let Err(error) = hub.set_active(app, self.surface, false) {
                tracing::warn!(surface = self.surface.name(), %error, "Surface sleep failed");
            }
            let (cold, release_host) = {
                let entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                let entry = &entries[self.surface.index()];
                let cold = entry.cold(Instant::now());
                (cold, cold && entry.snapshot_done && entry.hwnd.is_some())
            };
            if cold && self.capture.is_some() {
                // Keep textures alive if OpenVR refuses to detach its reference.
                let detached =
                    backend.is_none_or(|backend| backend.clear_surface_texture(kind).is_ok());
                if detached {
                    self.reset();
                }
            }
            if release_host {
                let epoch = {
                    let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                    let entry = &mut entries[self.surface.index()];
                    entry.invalidate_host();
                    if metrics::enabled() {
                        entry.metrics.cold_releases += 1;
                    }
                    entry.host_epoch
                };
                let host_app = app.clone();
                let surface = self.surface;
                let _ = app.run_on_main_thread(move || {
                    let hub = host_app.state::<SurfaceHub>();
                    let entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                    let entry = &entries[surface.index()];
                    if entry.host_epoch != epoch || entry.active {
                        return;
                    }
                    drop(entries);
                    if let Some(window) = host_app.get_webview_window(&surface.label()) {
                        let _ = window.destroy();
                    }
                });
            }
        }
        let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
        let entry = &mut entries[self.surface.index()];
        let state = if !entry.active {
            if entry.hwnd.is_some() {
                "warm_hidden"
            } else {
                "dormant"
            }
        } else if !self.visible_frame {
            "waking"
        } else if entry.needs_paint() {
            "visible_active"
        } else {
            "visible_idle"
        };
        entry.metrics.report(
            self.surface.name(),
            state,
            entry.hwnd.is_some(),
            self.capture
                .as_ref()
                .is_some_and(|capture| capture.active()),
        );
    }

    pub fn stop(
        &mut self,
        app: &AppHandle,
        backend: Option<&mut OpenVrBackend>,
        kind: OverlayKind,
    ) {
        self.requested = false;
        self.finish_tick(app, backend, kind);
    }

    pub fn present(
        &mut self,
        app: &AppHandle,
        backend: &mut OpenVrBackend,
        kind: OverlayKind,
        view: SurfaceView,
        hovered: Option<u8>,
        pressed: Option<u8>,
    ) -> Result<bool, String> {
        self.requested = true;
        if self.retry_at.is_some_and(|at| Instant::now() < at) {
            return Ok(false);
        }
        self.retry_at = None;
        let result = self.present_frame(app, backend, kind, view, hovered, pressed);
        if result.is_err() {
            app.state::<SurfaceHub>().invalidate_host(self.surface);
            // Detach the submitted texture before releasing its capture resources.
            backend.reset(kind);
            self.reset();
            self.retry_at = Some(Instant::now() + Duration::from_secs(1));
        }
        result
    }

    fn present_frame(
        &mut self,
        app: &AppHandle,
        backend: &mut OpenVrBackend,
        kind: OverlayKind,
        view: SurfaceView,
        hovered: Option<u8>,
        pressed: Option<u8>,
    ) -> Result<bool, String> {
        let hub = app.state::<SurfaceHub>();
        hub.set_active(app, self.surface, true)?;
        hub.publish(app, self.surface, view, hovered, pressed)?;
        let (hwnd, generation) = {
            let entries = hub.0.lock().map_err(|e| e.to_string())?;
            let entry = &entries[self.surface.index()];
            if let Some(error) = &entry.error {
                return Err(error.clone());
            }
            if entry.timed_out(Instant::now()) {
                return Err(format!(
                    "{} surface {} timed out",
                    self.surface.name(),
                    if entry.has_frame {
                        "layout"
                    } else {
                        "first frame"
                    }
                ));
            }
            let Some(target) = entry.capture_target() else {
                return Ok(false);
            };
            target
        };
        let adapter = backend.surface_adapter_index()?;
        let identity = (hwnd, adapter, generation);
        if self.identity != Some(identity) {
            if self.identity.is_some() {
                backend.reset(kind);
                self.reset();
                let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
                entries[self.surface.index()].submitted = None;
                entries[self.surface.index()].displayed_layout = None;
                return Ok(false);
            }
            self.reset();
            let (width, height) = self.surface.size();
            let counters = hub.0.lock().map_err(|e| e.to_string())?[self.surface.index()]
                .metrics
                .capture
                .clone();
            self.capture = Some(windows_wgc::Capture::with_counters(
                hwnd, adapter, width, height, counters,
            )?);
            self.identity = Some(identity);
        }
        let capture = self.capture.as_mut().expect("capture exists");
        capture.resume()?;
        let requested = {
            let entries = hub.0.lock().map_err(|e| e.to_string())?;
            let entry = &entries[self.surface.index()];
            let interval = if self.surface == SurfaceId::OcrProgress {
                Duration::from_millis(34)
            } else {
                Duration::from_micros(16_667)
            };
            (entry.needs_paint() && self.last_copy.is_none_or(|at| at.elapsed() >= interval))
                .then_some(entry.paint)
                .flatten()
        };
        if requested.is_some() {
            self.last_copy = Some(Instant::now());
        }
        if let Some(frame) = capture.poll(requested)? {
            let key = SurfaceKey {
                generation: frame.generation,
                revision: frame.revision,
            };
            let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
            let entry = &mut entries[self.surface.index()];
            if entry.accepts_frame(key, frame.paint_serial, frame.requested_serial) {
                backend.upload_surface(kind, frame.shared_handle)?;
                entry.commit_paint(key, frame.paint_serial);
                self.visible_frame = true;
                drop(entries);
                capture.submitted(frame)?;
            }
        }
        Ok(self.visible_frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_entry() -> Entry {
        let mut entry = Entry::default();
        entry.set_active(true, Instant::now());
        entry
    }

    #[test]
    fn dormant_content_has_no_deadline_and_cannot_accept_input_or_pixels() {
        let mut entry = Entry::default();
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            None,
            None,
        );
        let key = entry.state.as_ref().unwrap().key;
        assert!(!entry.timed_out(Instant::now() + Duration::from_secs(120)));
        assert!(!entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap());
        assert!(!entry.accept_paint(key, 1));
        assert!(!entry.needs_paint());
        assert!(!entry.commit(key));
    }

    #[test]
    fn sleep_revokes_hits_pauses_deadlines_and_coalesces_hidden_content() {
        let mut entry = active_entry();
        entry.set_active(true, Instant::now());
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            Some(0),
            Some(0),
        );
        let old = entry.state.as_ref().unwrap().key;
        entry.accept_layout(SurfaceId::Menu, layout(old)).unwrap();
        entry.commit(old);
        assert!(entry.active_layout().is_some());
        let now = Instant::now();
        assert!(entry.set_active(false, now));
        let sleep_request = entry.activity_request;
        assert!(entry.active_layout().is_none());
        assert!(!entry.commit(old));
        assert!(!entry.set_active(false, now));
        assert_eq!(entry.activity_request, sleep_request);
        for revision in 2..100 {
            let controls = ControlSnapshot {
                revision,
                ..Default::default()
            };
            entry.publish(
                SurfaceView::Menu {
                    controls,
                    hold_progress: None,
                },
                None,
                None,
            );
        }
        assert!(!entry.timed_out(now + Duration::from_secs(120)));
        assert!(entry.set_active(true, now + Duration::from_secs(120)));
        assert!(entry.activity_request > sleep_request);
        assert_ne!(entry.state.as_ref().unwrap().key, old);
        assert!(!entry.accept_layout(SurfaceId::Menu, layout(old)).unwrap());
        assert!(!entry.timed_out(now + Duration::from_secs(122)));
        assert!(
            matches!(&entry.state.as_ref().unwrap().view, SurfaceView::Menu { controls, .. } if controls.revision == 99)
        );
    }

    #[test]
    fn visual_work_remains_pending_until_a_matching_gpu_marker_is_submitted() {
        let mut entry = active_entry();
        entry.set_active(true, Instant::now());
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            None,
            None,
        );
        let key = entry.state.as_ref().unwrap().key;
        entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap();
        assert!(entry.accept_paint(key, 10));
        assert!(entry.needs_paint());
        assert!(!entry.accepts_frame(key, 9, 10));
        assert!(!entry.accept_paint(key, 8));
        assert!(entry.accepts_frame(key, 10, 10));
        assert!(entry.commit_paint(key, 10));
        assert!(!entry.needs_paint());
        assert!(entry.accept_paint(key, 11));
        assert!(entry.needs_paint());
        assert!(entry.commit_paint(key, 10));
        assert!(
            entry.needs_paint(),
            "an older in-flight frame cannot acknowledge newer pixels"
        );
        assert!(entry.commit_paint(key, 11));
        assert!(!entry.needs_paint());
        entry.reload();
        assert!(!entry.accept_paint(key, 12));
    }

    #[test]
    fn paint_serials_cross_the_u32_boundary_without_accepting_old_receipts() {
        let mut entry = active_entry();
        entry.set_active(true, Instant::now());
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            None,
            None,
        );
        let key = entry.state.as_ref().unwrap().key;
        assert!(!entry.accept_paint(key, 0));
        assert!(entry.accept_paint(key, u32::MAX));
        assert!(entry.accept_paint(key, 1));
        assert!(!entry.accept_paint(key, u32::MAX));
        entry.set_active(false, Instant::now());
        assert!(!entry.accept_paint(key, 2));
    }

    #[test]
    fn new_geometry_under_the_same_key_waits_for_its_own_pixels() {
        let mut entry = active_entry();
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            None,
            None,
        );
        let key = entry.state.as_ref().unwrap().key;
        assert!(entry
            .accept_layout_paint(SurfaceId::Menu, layout(key), 10)
            .unwrap());
        assert!(entry.commit_paint(key, 10));
        assert!(entry.active_layout().is_some());
        let mut moved = layout(key);
        moved.regions[0].bounds = [0.2, 0., 0.4, 0.8];
        assert!(entry
            .accept_layout_paint(SurfaceId::Menu, moved.clone(), 11)
            .unwrap());
        assert!(entry.active_layout().is_none());
        assert!(!entry.accepts_frame(key, 10, 10));
        assert!(!entry.commit_paint(key, 10));
        assert!(!entry
            .accept_layout_paint(SurfaceId::Menu, layout(key), 10)
            .unwrap());
        assert!(entry.commit_paint(key, 11));
        assert_eq!(
            entry.active_layout().unwrap().regions[0].bounds,
            moved.regions[0].bounds
        );
    }

    #[test]
    fn cold_deadline_resets_after_wake_and_reader_snapshots_follow_content_identity() {
        use super::super::{ocr_status::OcrState, ocr_wrist::Reader};
        let now = Instant::now();
        let mut entry = active_entry();
        entry.publish(
            SurfaceView::Reader {
                dark: true,
                labels: WristLabels::default(),
                reader: Reader::default().view(OcrState::Visible, false, false),
                font_size: 28,
                opacity: 1.0,
            },
            None,
            None,
        );
        let key = entry.state.as_ref().unwrap().key;
        entry.set_active(false, now);
        let request = entry.activity_request;
        assert!(!entry.cold(now + Duration::from_secs(59)));
        assert!(entry.cold(now + Duration::from_secs(60)));
        assert!(!entry.snapshot_done);
        assert!(!entry.accept_snapshot(Some(key), request - 1, Some(180.0)));
        assert!(!entry.accept_snapshot(None, request, Some(180.0)));
        assert!(!entry.accept_snapshot(Some(key), request, Some(f64::NAN)));
        assert!(entry.accept_snapshot(Some(key), request, Some(180.0)));
        entry.invalidate_host();
        assert_eq!(entry.state.as_ref().unwrap().restore_scroll, Some(180.0));
        entry.set_active(true, now + Duration::from_secs(61));
        assert!(!entry.cold(now + Duration::from_secs(120)));
        assert!(!entry.accept_snapshot(Some(key), request, Some(999.0)));
        if let SurfaceView::Reader { reader, .. } = &mut entry.state.as_mut().unwrap().view {
            reader.source = "A different question".into();
        }
        entry.reload();
        assert_eq!(entry.state.as_ref().unwrap().restore_scroll, None);
        entry.set_active(false, now + Duration::from_secs(121));
        assert!(!entry.cold(now + Duration::from_secs(122)));
    }

    fn layout(key: SurfaceKey) -> SurfaceLayout {
        SurfaceLayout {
            key,
            regions: vec![Region {
                id: 0,
                bounds: [0., 0., 0.2, 0.8],
                circle: true,
                dashboard: None,
            }],
            menu_bounds: None,
            panels: vec![],
            page_count: 1,
            unplaced: vec![],
        }
    }
    #[test]
    fn pointer_updates_keep_the_submitted_layout_and_reject_inactive_or_invalid_hits() {
        let mut entry = active_entry();
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            None,
            None,
        );
        let key = entry.state.as_ref().unwrap().key;
        assert!(!entry.point(Some([0.5, 0.5])));
        entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap();
        entry.commit(key);
        assert!(entry.point(Some([0.5, 0.5])));
        assert_eq!(entry.state.as_ref().unwrap().pointer, Some([0.5, 0.5]));
        assert_eq!(entry.state.as_ref().unwrap().key, key);
        assert!(entry.active_layout().is_some());
        assert!(!entry.point(Some([0.5, 0.5])));
        assert!(entry.point(Some([0.6, 0.4])));
        assert!(entry.point(None));
        for point in [[f32::NAN, 0.5], [-0.1, 0.5], [0.5, 1.1]] {
            assert!(!entry.point(Some(point)));
            assert_eq!(entry.state.as_ref().unwrap().pointer, None);
        }
        entry.point(Some([0.5, 0.5]));
        entry.reload();
        assert_eq!(entry.state.as_ref().unwrap().pointer, None);
        assert!(!entry.point(Some([0.5, 0.5])));
    }

    #[test]
    fn input_requires_matching_layout_capture_and_current_view() {
        let mut entry = active_entry();
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            None,
            None,
        );
        let key = entry.state.as_ref().unwrap().key;
        assert!(!entry.commit(key));
        assert!(entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap());
        assert!(entry.active_layout().is_none());
        assert!(entry.commit(key));
        assert!(entry.active_layout().is_some());
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            Some(0),
            Some(0),
        );
        assert_eq!(entry.state.as_ref().unwrap().key, key);
        assert!(entry.active_layout().is_some());
        let mut controls = ControlSnapshot::default();
        controls.revision += 1;
        entry.publish(
            SurfaceView::Menu {
                controls,
                hold_progress: None,
            },
            None,
            None,
        );
        assert!(!entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap());
        assert!(!entry.commit(key));
        assert!(entry.active_layout().is_none());
    }

    #[test]
    fn content_updates_keep_the_capture_target_while_waiting_for_new_pixels() {
        let mut entry = active_entry();
        entry.hwnd = Some(123);
        entry.ready = true;
        entry.host_awake = true;
        let mut controls = ControlSnapshot::default();
        entry.publish(
            SurfaceView::Menu {
                controls: controls.clone(),
                hold_progress: None,
            },
            None,
            None,
        );
        let old = entry.state.as_ref().unwrap().key;
        assert!(entry.pointer_layout().is_none());
        assert!(entry
            .accept_layout_paint(SurfaceId::Menu, layout(old), 1)
            .unwrap());
        assert!(entry.commit_paint(old, 1));
        assert_eq!(entry.pointer_layout().unwrap().key, old);
        let target = entry.capture_target().unwrap();

        controls.revision += 1;
        entry.publish(
            SurfaceView::Menu {
                controls,
                hold_progress: None,
            },
            None,
            None,
        );
        let current = entry.state.as_ref().unwrap().key;
        assert_ne!(current, old);
        assert_eq!(entry.capture_target(), Some(target));
        assert!(entry.has_frame);
        assert!(entry.active_layout().is_none());
        assert_eq!(entry.pointer_layout().unwrap().key, old);
        assert!(!entry.accepts_frame(old, 1, 1));

        assert!(entry
            .accept_layout_paint(SurfaceId::Menu, layout(current), 2)
            .unwrap());
        assert_eq!(entry.capture_target(), Some(target));
        assert!(entry.active_layout().is_none());
        assert_eq!(entry.pointer_layout().unwrap().key, old);
        assert!(entry.commit_paint(current, 2));
        assert_eq!(entry.active_layout().unwrap().key, current);
        assert_eq!(entry.pointer_layout().unwrap().key, current);

        entry.set_active(false, Instant::now());
        assert!(entry.capture_target().is_none());
        assert!(entry.pointer_layout().is_none());
        entry.set_active(true, Instant::now());
        assert!(entry.capture_target().is_none());
        assert!(entry.pointer_layout().is_none());
        entry.host_awake = true;
        entry.reload();
        assert!(entry.capture_target().is_none());
    }

    #[test]
    fn menu_hold_progress_keeps_the_submitted_hit_layout() {
        let mut entry = Entry::default();
        entry.set_active(true, Instant::now());
        let mut controls = ControlSnapshot::default();
        entry.publish(
            SurfaceView::Menu {
                controls: controls.clone(),
                hold_progress: None,
            },
            None,
            None,
        );
        let key = entry.state.as_ref().unwrap().key;
        entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap();
        assert!(entry.commit(key));
        for progress in [Some(0.), Some(0.5), Some(1.), None] {
            assert!(entry.publish(
                SurfaceView::Menu {
                    controls: controls.clone(),
                    hold_progress: progress
                },
                Some(0),
                progress.map(|_| 0)
            ));
            assert_eq!(entry.state.as_ref().unwrap().key, key);
            assert_eq!(entry.submitted, Some(key));
            assert!(entry.active_layout().is_some());
            assert!(matches!(&entry.state.as_ref().unwrap().view,
                SurfaceView::Menu { hold_progress, .. } if *hold_progress == progress));
        }
        controls.revision += 1;
        entry.publish(
            SurfaceView::Menu {
                controls,
                hold_progress: None,
            },
            None,
            None,
        );
        assert_ne!(entry.state.as_ref().unwrap().key, key);
        assert!(entry.active_layout().is_none());
    }
    #[test]
    fn reload_revokes_old_frames_and_layouts() {
        let mut entry = active_entry();
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            None,
            None,
        );
        let key = entry.state.as_ref().unwrap().key;
        entry.ready = true;
        entry.state.as_mut().unwrap().hovered = Some(0);
        entry.state.as_mut().unwrap().pressed = Some(0);
        entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap();
        assert!(entry.commit(key));
        entry.reload();
        assert!(!entry.ready);
        assert_eq!(entry.state.as_ref().unwrap().hovered, None);
        assert_eq!(entry.state.as_ref().unwrap().pressed, None);
        assert!(entry.active_layout().is_none());
        assert_ne!(entry.state.as_ref().unwrap().key.generation, key.generation);
        assert!(!entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap());
        assert!(!entry.commit(key));
    }
    #[test]
    fn malformed_and_overlapping_regions_are_rejected() {
        let key = SurfaceKey {
            generation: 1,
            revision: 1,
        };
        let mut value = layout(key);
        assert!(value.validate(SurfaceId::Menu).is_ok());
        value.regions.push(value.regions[0].clone());
        assert!(value.validate(SurfaceId::Menu).is_err());
        value.regions.pop();
        value.regions[0].bounds[0] = f32::NAN;
        assert!(value.validate(SurfaceId::Menu).is_err());
    }

    #[test]
    fn dialogue_toolbar_accepts_the_follow_button_with_the_existing_controls() {
        let mut value = layout(SurfaceKey {
            generation: 1,
            revision: 1,
        });
        value.regions = [
            (0, 0.05, 0.55),
            (1, 0.6, 0.7),
            (3, 0.72, 0.82),
            (2, 0.84, 0.94),
        ]
        .into_iter()
        .map(|(id, left, right)| Region {
            id,
            bounds: [left, 0.02, right, 0.12],
            circle: id != 0,
            dashboard: None,
        })
        .collect();
        assert!(value.validate(SurfaceId::Dialogue).is_ok());
        assert_eq!(value.hit([0.77, 0.07]), Some(3));
    }

    #[test]
    fn menu_background_bounds_require_a_valid_capsule() {
        let mut value = layout(SurfaceKey {
            generation: 1,
            revision: 1,
        });
        value.regions.clear();
        value.menu_bounds = Some([0.015625, 0.03125, 0.984375, 0.96875]);
        assert!(value.validate(SurfaceId::Menu).is_ok());
        assert!(value.validate(SurfaceId::Dialogue).is_err());
        for bounds in [
            [-0.1, 0., 1., 1.],
            [f32::NAN, 0., 1., 1.],
            [0.9, 0., 0.1, 1.],
            [0., 0.5, 1., 0.5],
            [0.4, 0., 0.6, 1.],
        ] {
            value.menu_bounds = Some(bounds);
            assert!(value.validate(SurfaceId::Menu).is_err());
        }
    }

    #[test]
    fn passive_surfaces_cannot_publish_input_regions_or_pages() {
        let key = SurfaceKey {
            generation: 1,
            revision: 1,
        };
        for name in [
            "headset",
            "ocr_progress",
            "dashboard_thumbnail",
            "ocr_selection",
            "ocr_result",
        ] {
            let surface: SurfaceId = serde_json::from_value(serde_json::json!(name)).unwrap();
            let mut value = SurfaceLayout {
                key,
                regions: vec![],
                menu_bounds: None,
                panels: vec![],
                page_count: 1,
                unplaced: vec![],
            };
            assert!(value.validate(surface).is_ok());
            assert_eq!(value.hit([0.5, 0.5]), None);
            value.page_count = 2;
            assert!(value.validate(surface).is_err());
            value.page_count = 1;
            value.regions = layout(key).regions;
            assert!(value.validate(surface).is_err());
        }
    }

    #[test]
    fn dashboard_regions_keep_their_dom_ids_and_require_dashboard_commands() {
        let mut value = layout(SurfaceKey {
            generation: 1,
            revision: 1,
        });
        value.regions[0].id = 42;
        value.regions[0].circle = false;
        assert!(value.validate(SurfaceId::Dashboard).is_err());
        value.regions[0].dashboard = Some(DashboardCommand::Page(
            super::super::dashboard::DashboardPage::Language,
        ));
        assert!(value.validate(SurfaceId::Dashboard).is_ok());
        assert_eq!(value.hit([0.1, 0.4]), Some(42));
        assert!(value.validate(SurfaceId::Menu).is_err());
    }

    #[test]
    fn overflow_requires_current_scene_ids_and_a_committed_frame() {
        let mut entry = active_entry();
        entry.publish(
            SurfaceView::OcrResult {
                dark: false,
                scene: OcrScene {
                    blocks: vec![super::super::ocr_plane::OcrSceneBlock {
                        id: 7,
                        text: "translation".into(),
                        quad: [[0.1, 0.1], [0.9, 0.1], [0.9, 0.8], [0.1, 0.8]],
                        masks: vec![],
                    }],
                    excluded: vec![],
                    opacity: 0.5,
                    aspect: 1.,
                },
            },
            None,
            None,
        );
        let key = entry.state.as_ref().unwrap().key;
        let mut value = SurfaceLayout {
            key,
            regions: vec![],
            menu_bounds: None,
            panels: vec![],
            page_count: 1,
            unplaced: vec![9],
        };
        assert!(entry
            .accept_layout(SurfaceId::OcrResult, value.clone())
            .is_err());
        value.unplaced = vec![7, 7];
        assert!(entry
            .accept_layout(SurfaceId::OcrResult, value.clone())
            .is_err());
        value.unplaced = vec![7];
        assert!(entry
            .accept_layout(SurfaceId::OcrResult, value.clone())
            .unwrap());
        assert!(entry.active_layout().is_none());
        assert!(entry.commit(key));
        assert_eq!(entry.active_layout().unwrap().unplaced, vec![7]);
        entry.reload();
        assert!(!entry.accept_layout(SurfaceId::OcrResult, value).unwrap());
        assert!(entry.active_layout().is_none());
    }

    #[test]
    fn first_frame_and_layout_deadlines_do_not_slide_with_streaming_updates() {
        let mut entry = active_entry();
        let now = Instant::now();
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            None,
            None,
        );
        entry.pending_since = Some(now);
        assert!(!entry.timed_out(now + Duration::from_millis(2999)));
        assert!(entry.timed_out(now + Duration::from_secs(3)));
        let key = entry.state.as_ref().unwrap().key;
        entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap();
        assert!(entry.commit(key));
        assert!(!entry.timed_out(now + Duration::from_secs(60)));
        let mut controls = ControlSnapshot {
            revision: 2,
            ..Default::default()
        };
        entry.publish(
            SurfaceView::Menu {
                controls: controls.clone(),
                hold_progress: None,
            },
            None,
            None,
        );
        entry.pending_since = Some(now);
        controls.revision = 3;
        entry.publish(
            SurfaceView::Menu {
                controls,
                hold_progress: None,
            },
            None,
            None,
        );
        assert!(!entry.timed_out(now + Duration::from_millis(499)));
        assert!(entry.timed_out(now + Duration::from_millis(500)));
    }

    #[test]
    fn host_failure_revokes_input_and_allows_a_fresh_host() {
        let mut entry = active_entry();
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            Some(0),
            Some(0),
        );
        let key = entry.state.as_ref().unwrap().key;
        let host_epoch = entry.host_epoch;
        entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap();
        entry.commit(key);
        entry.hwnd = Some(1);
        entry.creating = true;
        entry.ready = true;
        entry.error = Some("capture failed".into());
        entry.invalidate_host();
        assert_ne!(entry.host_epoch, host_epoch);
        assert!(entry.hwnd.is_none() && !entry.creating && !entry.ready);
        assert!(entry.error.is_none());
        assert!(entry.active_layout().is_none());
        assert!(!entry.commit(key));
        assert!(!entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap());
        assert!(entry.pending_since.is_none());
    }
}
