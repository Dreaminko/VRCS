//! React owns pixels and layout; the VR worker owns input and texture submission.
mod bridge;
mod native_host;
pub(crate) use native_host::NativeHostManager;
mod metrics;
mod recovery;
pub mod windows_wgc;

use bridge::{HostId, SurfaceEvent};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

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
    const ALL: [Self; 10] = [
        Self::Menu,
        Self::Dialogue,
        Self::Reader,
        Self::Headset,
        Self::OcrProgress,
        Self::Dashboard,
        Self::DashboardThumbnail,
        Self::OcrSelection,
        Self::OcrResult,
        Self::PlayerAudio,
    ];
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
    pub fn size(self) -> (u32, u32) {
        match self {
            Self::Menu => (1280, 1280),
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
            Self::Menu => 64,
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
#[serde(deny_unknown_fields)]
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
        players: super::player_display::View,
        ui: super::wrist_menu::Ui,
        hold_progress: Option<f32>,
    },
    Dialogue {
        visibility: super::dashboard::CaptionVisibility,
        max_visible_players: Option<usize>,
        max_entries: usize,
        controls: ControlSnapshot,
        messages: Vec<WristMessage>,
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
    fn same_scroll_view(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Reader { .. }, Self::Reader { .. }) => {
                reader_identity(self) == reader_identity(other)
            }
            (
                Self::Dialogue {
                    controls: a,
                    mode: am,
                    visibility: av,
                    max_entries: ae,
                    max_visible_players: ap,
                    ..
                },
                Self::Dialogue {
                    controls: b,
                    mode: bm,
                    visibility: bv,
                    max_entries: be,
                    max_visible_players: bp,
                    ..
                },
            ) => a.owner_epoch == b.owner_epoch && am == bm && av == bv && ae == be && ap == bp,
            _ => false,
        }
    }

    pub(super) fn same_layout(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Menu {
                    controls: a,
                    players: ap,
                    ui: au,
                    ..
                },
                Self::Menu {
                    controls: b,
                    players: bp,
                    ui: bu,
                    ..
                },
            ) => a == b && ap == bp && au == bu,
            _ => self == other,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct SurfaceState {
    pub key: SurfaceKey,
    pub scroll_key: SurfaceKey,
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
#[serde(deny_unknown_fields)]
pub struct Region {
    pub id: u8,
    pub bounds: [f32; 4],
    #[serde(default)]
    pub circle: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard: Option<DashboardCommand>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menu: Option<super::wrist_menu::Command>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
            SurfaceId::Menu | SurfaceId::Reader | SurfaceId::PlayerAudio => 1,
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
                || (surface == SurfaceId::Menu) != region.menu.is_some()
            {
                return Err("Invalid surface hit region".into());
            }
            if let Some(command) = &region.menu {
                use super::wrist_menu::Command;
                let valid = match command {
                    Command::Control(action) => usize::from(region.id) == action.index(),
                    _ => region.id >= 4,
                };
                if !valid {
                    return Err("Invalid menu command region".into());
                }
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

    pub fn menu_command(&self, id: u8) -> Option<super::wrist_menu::Command> {
        self.regions
            .iter()
            .find(|region| region.id == id)?
            .menu
            .clone()
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
    navigation_id: u64,
    document: Option<bridge::DocumentSession>,
    state: Option<SurfaceState>,
    layout: Option<SurfaceLayout>,
    displayed_layout: Option<SurfaceLayout>,
    displayed_scroll_key: Option<SurfaceKey>,
    submitted: Option<SurfaceKey>,
    endpoint: Option<native_host::CaptureEndpoint>,
    creating: bool,
    ready: bool,
    error: Option<String>,
    pending_since: Option<Instant>,
    pending_paints: recovery::PendingPaints,
    recovery: recovery::Recovery,
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
            navigation_id: 0,
            document: None,
            state: None,
            layout: None,
            displayed_layout: None,
            displayed_scroll_key: None,
            submitted: None,
            endpoint: None,
            creating: false,
            ready: false,
            error: None,
            pending_since: None,
            pending_paints: recovery::PendingPaints::default(),
            recovery: recovery::Recovery::default(),
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
        if self.active == active || self.recovery.phase == recovery::Phase::Closing {
            return false;
        }
        self.active = active;
        self.activity_request = self.activity_request.wrapping_add(1).max(1);
        self.host_awake = false;
        self.layout = None;
        self.displayed_layout = None;
        self.displayed_scroll_key = None;
        self.submitted = None;
        self.paint = None;
        self.submitted_paint = None;
        self.layout_paint = None;
        self.has_frame = false;
        self.pending_since = active.then_some(now);
        self.pending_paints.clear();
        self.recovery.set_active(active, now);
        if active {
            self.error = None;
        }
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
                state.scroll_key = state.key;
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
        if matches!(
            self.recovery.phase,
            recovery::Phase::Failed | recovery::Phase::Closing
        ) {
            return false;
        }
        // Hold progress changes pixels without invalidating the menu's hit regions.
        let changed = self
            .state
            .as_ref()
            .is_none_or(|state| !state.view.same_layout(&view));
        let feedback = self.state.as_ref().is_none_or(|state| {
            state.view != view || state.hovered != hovered || state.pressed != pressed
        });
        if changed {
            self.pending_since = self.recovery_observation(None).pending_since;
            let revision = self
                .state
                .as_ref()
                .map_or(1, |state| state.key.revision.wrapping_add(1).max(1));
            let key = SurfaceKey {
                generation: self.generation,
                revision,
            };
            let scroll_key = self
                .state
                .as_ref()
                .filter(|state| state.view.same_scroll_view(&view))
                .map_or(key, |state| state.scroll_key);
            self.state = Some(SurfaceState {
                key,
                scroll_key,
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
            self.pending_paints.clear();
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
        self.document = None;
        self.generation = self.generation.wrapping_add(1).max(1);
        self.layout = None;
        self.displayed_layout = None;
        self.displayed_scroll_key = None;
        self.submitted = None;
        self.ready = false;
        self.has_frame = false;
        self.pending_since = None;
        self.pending_paints.clear();
        self.recovery.navigating(Instant::now());
        self.paint = None;
        self.submitted_paint = None;
        self.layout_paint = None;
        if let Some(state) = &mut self.state {
            state.key = SurfaceKey {
                generation: self.generation,
                revision: 1,
            };
            state.scroll_key = state.key;
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
        self.navigation_id = 0;
        self.endpoint = None;
        self.creating = false;
        self.host_awake = false;
        self.error = None;
    }

    fn page_ready(&mut self) -> Option<SurfaceState> {
        if !self.ready {
            self.ready = true;
            self.pending_since = self.active.then(Instant::now);
            if self.active {
                self.recovery.page_ready(Instant::now());
            }
        }
        self.state.clone()
    }

    fn recovery_observation(&self, fault: Option<recovery::Fault>) -> recovery::Observation {
        let pending_since = self
            .pending_paints
            .oldest()
            .into_iter()
            .chain(self.pending_since)
            .min();
        recovery::Observation {
            ready: self.ready,
            layout: self.layout.is_some(),
            has_frame: self.has_frame,
            pending_since,
            fault,
        }
    }

    fn clear_frame(&mut self) {
        self.submitted = None;
        self.submitted_paint = None;
        self.displayed_layout = None;
        self.displayed_scroll_key = None;
        self.has_frame = false;
        self.pending_since.get_or_insert_with(Instant::now);
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
            self.displayed_scroll_key = self.state.as_ref().map(|state| state.scroll_key);
            self.pending_since = None;
            self.pending_paints.clear();
            self.has_frame = true;
            self.recovery.submitted(Instant::now());
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
        if self
            .submitted_paint
            .is_none_or(|submitted| !serial_at_least(submitted, serial))
            && !self.pending_paints.accept(serial, Instant::now())
        {
            self.error = Some("surface_paint_overload".into());
            return false;
        }
        if metrics::enabled() {
            self.metrics.paints += 1;
            self.metrics.last_paint = Some(Instant::now());
        }
        true
    }

    fn needs_paint(&self) -> bool {
        self.active
            && self.paint.is_some()
            && (self.paint != self.submitted_paint || self.submitted.is_none())
    }

    fn capture_generation(&self) -> Option<u32> {
        // Layout gates new pixels and input, not the last submitted texture.
        (self.ready
            && self.host_awake
            && self.error.is_none()
            && !matches!(
                self.recovery.phase,
                recovery::Phase::Failed | recovery::Phase::Closing
            ))
        .then_some(self.generation)
    }

    fn accepts_frame(&self, key: SurfaceKey, serial: u32, requested: u32) -> bool {
        self.active
            && self.error.is_none()
            && !matches!(
                self.recovery.phase,
                recovery::Phase::Failed | recovery::Phase::Closing
            )
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
        self.displayed_scroll_key = self.state.as_ref().map(|state| state.scroll_key);
        self.pending_paints.committed(serial);
        self.pending_since = self.pending_paints.oldest();
        self.has_frame = true;
        self.recovery.submitted(Instant::now());
        if let Some(started) = self.wake_since.take() {
            self.metrics.wake(started.elapsed());
        }
        true
    }

    fn active_layout(&self) -> Option<&SurfaceLayout> {
        self.layout.as_ref().filter(|layout| {
            self.active
                && self.error.is_none()
                && self.recovery.input_allowed()
                && Some(layout.key) == self.submitted
                && self
                    .state
                    .as_ref()
                    .is_some_and(|state| state.key == layout.key)
        })
    }

    fn scroll_key(&self) -> Option<SurfaceKey> {
        let state = self.state.as_ref()?;
        // Text can change while the same viewport stays on screen. Clicks still use active_layout.
        (self.active
            && self.error.is_none()
            && self.recovery.input_allowed()
            && self.ready
            && self.has_frame
            && self.displayed_scroll_key == Some(state.scroll_key))
        .then_some(state.scroll_key)
    }

    fn pointer_layout(&self) -> Option<&SurfaceLayout> {
        // Keep the contour of the visible texture while new pixels and hit regions are pending.
        self.displayed_layout.as_ref().filter(|layout| {
            self.active
                && self.error.is_none()
                && self.recovery.input_allowed()
                && self.has_frame
                && layout.key.generation == self.generation
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
            app.state::<NativeHostManager>().set_activity(
                HostId {
                    surface,
                    host_epoch: epoch,
                },
                activity,
            )?;
        }
        Ok(())
    }

    fn send(&self, app: &AppHandle, surface: SurfaceId, event: SurfaceEvent) -> Result<(), String> {
        let host_epoch =
            self.0.lock().unwrap_or_else(|e| e.into_inner())[surface.index()].host_epoch;
        app.state::<NativeHostManager>().send(
            HostId {
                surface,
                host_epoch,
            },
            event,
        )
    }

    pub fn generation(&self, surface: SurfaceId) -> u32 {
        self.0.lock().unwrap_or_else(|e| e.into_inner())[surface.index()].generation
    }

    pub(super) fn recovery_failed(&self, surface: SurfaceId) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner())[surface.index()]
            .recovery
            .phase
            == recovery::Phase::Failed
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
            self.send(app, surface, SurfaceEvent::Pointer(update))?;
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
            entry.scroll_key().map(|key| SurfaceScroll { key, delta })
        };
        if let Some(update) = update {
            self.send(app, surface, SurfaceEvent::Scroll(update))?;
        }
        Ok(())
    }

    // The caller has already released the OpenVR backend and all worker captures.
    pub(super) fn retry(&self, app: &AppHandle) {
        let mut hosts = Vec::new();
        {
            let mut entries = self.0.lock().unwrap_or_else(|e| e.into_inner());
            for (index, entry) in entries.iter_mut().enumerate() {
                hosts.push(HostId {
                    surface: SurfaceId::ALL[index],
                    host_epoch: entry.host_epoch,
                });
                entry.invalidate_host();
                entry.recovery = recovery::Recovery::default();
                if entry.active {
                    entry.recovery.retry(Instant::now());
                }
            }
        }
        for id in hosts {
            let _ = app.state::<NativeHostManager>().close(id);
        }
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
            if matches!(
                entry.recovery.phase,
                recovery::Phase::Failed | recovery::Phase::Closing
            ) {
                return Err(entry
                    .error
                    .clone()
                    .unwrap_or_else(|| "Surface recovery exhausted".into()));
            }
            if entry.error.is_some() {
                return Ok(());
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
            let create = entry.active && !entry.creating && entry.endpoint.is_none();
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
            app.state::<NativeHostManager>().ensure(HostId {
                surface,
                host_epoch: epoch,
            })?;
        }
        if let Some(state) = update {
            self.send(app, surface, SurfaceEvent::View(state))?;
        }
        Ok(())
    }
}

#[cfg(all(debug_assertions, feature = "vr-preview"))]
mod smoke;
#[cfg(all(debug_assertions, feature = "vr-preview"))]
pub use smoke::smoke;

/// The worker keeps the GPU resources alive for as long as OpenVR uses them.
pub struct SurfaceRenderer {
    surface: SurfaceId,
    capture: Option<Rc<RefCell<windows_wgc::Capture>>>,
    identity: Option<(u32, u32, u32)>,
    visible_frame: bool,
    retained_key: Option<SurfaceKey>,
    retained_activity: u32,
    capture_fault: Option<windows_wgc::CaptureError>,
    pending_action: Option<(recovery::Action, Instant)>,
    requested: bool,
    next_copy: Option<Instant>,
}

impl SurfaceRenderer {
    pub fn new(surface: SurfaceId) -> Self {
        Self {
            surface,
            capture: None,
            identity: None,
            visible_frame: false,
            retained_key: None,
            retained_activity: 0,
            capture_fault: None,
            pending_action: None,
            requested: false,
            next_copy: None,
        }
    }
    pub fn reset(&mut self) {
        if let Some(capture) = &self.capture {
            capture.borrow_mut().suspend();
        }
        self.capture = None;
        self.identity = None;
        self.visible_frame = false;
        self.retained_key = None;
        self.retained_activity = 0;
        self.capture_fault = None;
        self.pending_action = None;
        self.next_copy = None;
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
                capture.borrow_mut().suspend();
            }
            self.visible_frame = false;
            self.capture_fault = None;
            self.pending_action = None;
            if let Err(error) = hub.set_active(app, self.surface, false) {
                tracing::warn!(surface = self.surface.name(), %error, "Surface sleep failed");
            }
            let (cold, release_host) = {
                let entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                let entry = &entries[self.surface.index()];
                let cold = entry.cold(Instant::now());
                (cold, cold && entry.endpoint.is_some())
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
                let old = {
                    let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                    let entry = &mut entries[self.surface.index()];
                    let old = HostId {
                        surface: self.surface,
                        host_epoch: entry.host_epoch,
                    };
                    entry.invalidate_host();
                    if metrics::enabled() {
                        entry.metrics.cold_releases += 1;
                    }
                    old
                };
                let _ = app.state::<NativeHostManager>().close(old);
            }
        }
        let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
        let entry = &mut entries[self.surface.index()];
        let state = if !entry.active {
            if entry.endpoint.is_some() {
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
            entry.endpoint.is_some(),
            self.capture
                .as_ref()
                .is_some_and(|capture| capture.borrow().active()),
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

    pub(super) fn request(&mut self, app: &AppHandle) -> Result<(), String> {
        self.requested = true;
        app.state::<SurfaceHub>()
            .set_active(app, self.surface, true)
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
        self.request(app)?;
        let hub = app.state::<SurfaceHub>();
        let fault = windows_wgc::ensure_capture_supported().err().or_else(|| {
            hub.publish(app, self.surface, view, hovered, pressed)
                .err()
                .map(|error| windows_wgc::CaptureError::new(recovery::Fault::Host, error))
        });
        if let Some(error) = fault {
            self.capture_fault = Some(error);
        }
        let now = Instant::now();
        self.advance_recovery(&hub, now);
        if self.pending_action.is_some() {
            if matches!(
                self.pending_action,
                Some((
                    recovery::Action::RecreateDevice | recovery::Action::RecreateHost,
                    _
                ))
            ) {
                backend.hide(kind);
            }
            if let Err(error) =
                self.apply_recovery(app, || backend.clear_surface_texture(kind), now)
            {
                self.capture_fault = Some(error);
                self.advance_recovery(&hub, now);
            }
        }
        let failed = hub.0.lock().unwrap_or_else(|e| e.into_inner())[self.surface.index()]
            .recovery
            .phase
            == recovery::Phase::Failed;
        if failed {
            backend.hide(kind);
            if let Some(capture) = &self.capture {
                capture.borrow_mut().suspend();
            }
            if backend.clear_surface_texture(kind).is_ok() {
                self.reset();
            }
            let (id, error) = {
                let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                let entry = &mut entries[self.surface.index()];
                entry.endpoint = None;
                entry.document = None;
                entry.ready = false;
                entry.creating = false;
                (
                    HostId {
                        surface: self.surface,
                        host_epoch: entry.host_epoch,
                    },
                    entry
                        .error
                        .clone()
                        .unwrap_or_else(|| "Surface recovery exhausted".into()),
                )
            };
            let _ = app.state::<NativeHostManager>().close(id);
            return Err(error);
        }
        if self.pending_action.is_none() && self.capture_fault.is_none() {
            if let Err(error) = self.present_frame(app, backend, kind) {
                self.capture_fault = Some(error);
                self.advance_recovery(&hub, Instant::now());
            }
        }
        let entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
        let entry = &entries[self.surface.index()];
        let visible = self.frame_visible(entry, Instant::now());
        if !visible {
            backend.hide(kind);
        }
        Ok(visible)
    }

    fn advance_recovery(&mut self, hub: &SurfaceHub, now: Instant) {
        if self.pending_action.is_some() {
            return;
        }
        let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
        let entry = &mut entries[self.surface.index()];
        let fault = self
            .capture_fault
            .as_ref()
            .map(|error| error.kind)
            .or_else(|| entry.error.as_ref().map(|_| recovery::Fault::Host));
        let observation = entry.recovery_observation(fault);
        let previous = entry.recovery.phase;
        let action = entry.recovery.advance(observation, now);
        if action != recovery::Action::None {
            entry.error = Some(
                self.capture_fault
                    .as_ref()
                    .map(ToString::to_string)
                    .or_else(|| entry.error.clone())
                    .unwrap_or_else(|| "Surface frame deadline exceeded".into()),
            );
            // Capture errors do not revoke the document or stop its paint receipts.
            if action != recovery::Action::Fail && fault != Some(recovery::Fault::Host) {
                entry.error = None;
            }
            if action != recovery::Action::Fail {
                self.pending_action = Some((action, now));
            }
        }
        if previous != entry.recovery.phase
            || !matches!(action, recovery::Action::None | recovery::Action::Fail)
        {
            tracing::info!(surface = self.surface.name(), host_epoch = entry.host_epoch,
                generation = entry.generation, ?previous, state = ?entry.recovery.phase, ?fault, ?action,
                capture_attempts = entry.recovery.capture_attempts, host_attempts = entry.recovery.host_attempts,
                stage = self.capture_fault.as_ref().map(|error| error.stage.as_str()),
                hresult = self.capture_fault.as_ref().and_then(|error| error.hresult),
                "Surface recovery transition");
        }
    }

    fn frame_visible(&self, entry: &Entry, now: Instant) -> bool {
        let same_host = self.identity.is_some_and(|(epoch, _, generation)| {
            epoch == entry.host_epoch && generation == entry.generation
        });
        if !self.visible_frame
            || !entry.active
            || !same_host
            || self.retained_activity != entry.activity_request
        {
            return false;
        }
        if entry.recovery.input_allowed() && self.capture_fault.is_none() && entry.error.is_none() {
            return true;
        }
        entry.recovery.retains_frame(
            self.retained_key == entry.state.as_ref().map(|state| state.key),
            !matches!(self.surface, SurfaceId::OcrSelection | SurfaceId::OcrResult),
            now,
        )
    }

    fn apply_recovery(
        &mut self,
        app: &AppHandle,
        mut detach: impl FnMut() -> Result<(), String>,
        now: Instant,
    ) -> Result<(), windows_wgc::CaptureError> {
        use recovery::{Action, Fault};
        let Some((action, started)) = self.pending_action else {
            return Ok(());
        };
        let hub = app.state::<SurfaceHub>();
        if matches!(action, Action::RecreateDevice | Action::RecreateHost) {
            self.visible_frame = false;
            if let Some(capture) = &self.capture {
                capture.borrow_mut().suspend();
            }
            if let Err(error) = detach() {
                // Retry detaching on worker ticks, retaining both owners until it succeeds.
                if now.saturating_duration_since(started) < Duration::from_secs(3) {
                    return Ok(());
                }
                self.pending_action = None;
                return Err(windows_wgc::CaptureError::new(Fault::Submission, error));
            }
            self.reset();
        }
        self.pending_action = None;
        self.capture_fault = None;
        match action {
            Action::RecreateHost => {
                let id = {
                    let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                    let entry = &mut entries[self.surface.index()];
                    let id = HostId {
                        surface: self.surface,
                        host_epoch: entry.host_epoch,
                    };
                    entry.invalidate_host();
                    id
                };
                app.state::<NativeHostManager>()
                    .close(id)
                    .map_err(|error| windows_wgc::CaptureError::new(Fault::Host, error))?;
            }
            Action::RestartCapture => {
                let endpoint = hub.0.lock().unwrap_or_else(|e| e.into_inner())
                    [self.surface.index()]
                .endpoint
                .clone();
                if let (Some(capture), Some(endpoint)) = (&self.capture, endpoint) {
                    capture.borrow_mut().restart_source(&endpoint.item)?;
                }
                hub.send(app, self.surface, SurfaceEvent::Repaint)
                    .map_err(|error| windows_wgc::CaptureError::new(Fault::Host, error))?;
            }
            Action::RecreateDevice => {
                hub.0.lock().unwrap_or_else(|e| e.into_inner())[self.surface.index()].clear_frame();
                hub.send(app, self.surface, SurfaceEvent::Repaint)
                    .map_err(|error| windows_wgc::CaptureError::new(Fault::Host, error))?;
            }
            Action::None | Action::Fail => {}
        }
        Ok(())
    }

    fn present_frame(
        &mut self,
        app: &AppHandle,
        backend: &mut OpenVrBackend,
        kind: OverlayKind,
    ) -> Result<bool, windows_wgc::CaptureError> {
        let hub = app.state::<SurfaceHub>();
        let (endpoint, generation) = {
            let entries = hub.0.lock().map_err(|e| e.to_string())?;
            let entry = &entries[self.surface.index()];
            let Some(target) = entry.capture_generation() else {
                return Ok(false);
            };
            let Some(endpoint) = entry.endpoint.clone() else {
                return Ok(false);
            };
            (endpoint, target)
        };
        let adapter = backend
            .surface_adapter_index()
            .map_err(|error| windows_wgc::CaptureError::new(recovery::Fault::Submission, error))?;
        let identity = (endpoint.host.host_epoch, adapter, generation);
        if self.identity != Some(identity) {
            if self.capture.is_some() || backend.has_surface_texture(kind) {
                self.pending_action = Some((recovery::Action::RecreateDevice, Instant::now()));
                self.visible_frame = false;
                let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                let entry = &mut entries[self.surface.index()];
                entry.clear_frame();
                entry.recovery.awaiting_frame(Instant::now());
                return Ok(false);
            }
            let (width, height) = self.surface.size();
            let counters = hub.0.lock().map_err(|e| e.to_string())?[self.surface.index()]
                .metrics
                .capture
                .clone();
            self.capture = Some(Rc::new(RefCell::new(windows_wgc::Capture::with_counters(
                &endpoint.item,
                adapter,
                width,
                height,
                counters,
            )?)));
            self.identity = Some(identity);
        }
        let capture = self.capture.as_ref().expect("capture exists").clone();
        self.capture_frame(app, &hub, |frame| {
            backend.upload_surface(kind, frame.shared_handle, capture)
        })
    }

    fn capture_frame(
        &mut self,
        app: &AppHandle,
        hub: &SurfaceHub,
        upload: impl FnOnce(windows_wgc::Captured) -> Result<(), String>,
    ) -> Result<bool, windows_wgc::CaptureError> {
        let capture = self.capture.as_ref().expect("capture exists").clone();
        let mut capture = capture.borrow_mut();
        capture.resume()?;
        if let Some(frame) = capture.poll()? {
            let key = SurfaceKey {
                generation: frame.generation,
                revision: frame.revision,
            };
            let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
            let entry = &mut entries[self.surface.index()];
            if entry.accepts_frame(key, frame.paint_serial, frame.requested_serial) {
                upload(frame).map_err(|error| {
                    windows_wgc::CaptureError::new(recovery::Fault::Submission, error)
                })?;
                entry.commit_paint(key, frame.paint_serial);
                self.visible_frame = true;
                self.retained_key = Some(key);
                self.retained_activity = entry.activity_request;
                drop(entries);
                capture.submitted(frame)?;
            } else {
                capture.reject_frame(frame);
            }
        }
        let now = Instant::now();
        let requested = {
            let entries = hub.0.lock().map_err(|e| e.to_string())?;
            let entry = &entries[self.surface.index()];
            (entry.needs_paint() && self.copy_due(now))
                .then_some(entry.paint)
                .flatten()
        };
        if capture.start(requested)? {
            self.copied(now);
        }
        if capture.take_repaint_request() {
            hub.send(app, self.surface, SurfaceEvent::Repaint)
                .map_err(|error| windows_wgc::CaptureError::new(recovery::Fault::Host, error))?;
        }
        Ok(self.visible_frame)
    }

    fn copy_due(&self, now: Instant) -> bool {
        self.next_copy.is_none_or(|at| now >= at)
    }

    fn copied(&mut self, now: Instant) {
        let interval = if self.surface == SurfaceId::OcrProgress {
            Duration::from_millis(34)
        } else {
            super::runtime::SURFACE_FRAME_INTERVAL
        };
        // Carry sub-tick lateness forward without accumulating work across idle periods.
        self.next_copy = Some(
            self.next_copy
                .map(|at| at + interval)
                .filter(|at| *at > now)
                .unwrap_or(now + interval),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_menu_keeps_capsule_and_panel_hits_separate() {
        assert_eq!(SurfaceId::Menu.size(), (1280, 1280));
        let value: SurfaceLayout = serde_json::from_value(serde_json::json!({
            "key": {"generation": 1, "revision": 1},
            "regions": [
                {"id": 4, "bounds": [0.6, 0.84, 0.725, 0.965], "circle": true, "menu": {"page": "players"}},
                {"id": 17, "bounds": [0.1, 0.3, 0.4, 0.4], "menu": {"display": {"show_all_tracks": null}}}
            ],
            "menu_bounds": [0.0125, 0.80625, 0.9875, 0.99375],
            "panels": [{"bounds": [0.0125, 0.0125, 0.9875, 0.7875], "radius": [0.025, 0.025]}],
            "page_count": 1
        })).unwrap();
        value.validate(SurfaceId::Menu).unwrap();
        assert!(value.pointer_contains([0.5, 0.5]));
        assert!(value.pointer_contains([0.5, 0.9]));
        assert!(!value.pointer_contains([0.5, 0.8]));
        assert_eq!(value.hit([0.2, 0.35]), Some(17));
        assert!(value.validate(SurfaceId::Dialogue).is_err());
    }

    #[test]
    fn copy_budget_does_not_halve_the_worker_tick_rate() {
        let mut renderer = SurfaceRenderer::new(SurfaceId::Menu);
        let started = Instant::now();
        let mut copies = 0;
        for tick in 0..64 {
            let now = started + Duration::from_millis(tick * 16);
            if renderer.copy_due(now) {
                renderer.copied(now);
                copies += 1;
            }
        }
        assert_eq!(copies, 61, "roughly 60 copies per second, not 30");
    }

    #[test]
    fn active_worker_and_capture_keep_even_frame_intervals() {
        let mut renderer = SurfaceRenderer::new(SurfaceId::Reader);
        let started = Instant::now();
        let interval = super::super::runtime::SURFACE_FRAME_INTERVAL;
        let mut previous = None;
        for tick in 0..120 {
            let now = started + interval * tick;
            if renderer.copy_due(now) {
                if let Some(previous) = previous {
                    assert!(
                        now.duration_since(previous) <= Duration::from_millis(17),
                        "capture must not skip an active worker tick"
                    );
                }
                renderer.copied(now);
                previous = Some(now);
            }
        }
    }

    #[test]
    fn reader_scroll_key_survives_text_but_not_selection_or_lifecycle_changes() {
        use super::super::{ocr_status::OcrState, ocr_wrist::Reader};
        let mut entry = active_entry();
        let mut view = SurfaceView::Reader {
            dark: true,
            labels: WristLabels::default(),
            reader: Reader::default().view(OcrState::Visible, false, false),
            font_size: 28,
            opacity: 1.0,
        };
        entry.publish(view.clone(), None, None);
        let key = entry.state.as_ref().unwrap().key;
        let mut reader_layout = layout(key);
        reader_layout.regions[0].menu = None;
        entry
            .accept_layout(SurfaceId::Reader, reader_layout)
            .unwrap();
        assert!(entry.commit(key));
        assert!(entry.scroll_key().is_none(), "the host must be ready");
        entry.ready = true;
        let original = entry.state.as_ref().unwrap().scroll_key;
        assert_eq!(original, key);
        assert_eq!(entry.scroll_key(), Some(original));
        if let SurfaceView::Reader { reader, .. } = &mut view {
            reader.translation = "More streamed text".into();
        }
        entry.publish(view.clone(), None, None);
        assert_eq!(entry.state.as_ref().unwrap().scroll_key, original);
        assert_eq!(entry.scroll_key(), Some(original));
        assert!(
            entry.active_layout().is_none(),
            "clicks still wait for new pixels"
        );
        if let SurfaceView::Reader { reader, .. } = &mut view {
            reader.translation_label = "Japanese".into();
        }
        entry.publish(view, None, None);
        assert_ne!(entry.state.as_ref().unwrap().scroll_key, original);
        assert!(entry.scroll_key().is_none());
        let selected = entry.state.as_ref().unwrap().scroll_key;
        entry.set_active(false, Instant::now());
        assert!(entry.scroll_key().is_none());
        entry.set_active(true, Instant::now());
        assert_ne!(entry.state.as_ref().unwrap().scroll_key, selected);
        assert!(entry.scroll_key().is_none());
        let awake = entry.state.as_ref().unwrap().scroll_key;
        entry.reload();
        assert_ne!(entry.state.as_ref().unwrap().scroll_key, awake);
        assert!(entry.scroll_key().is_none());
    }

    #[test]
    fn dialogue_scroll_survives_captions_but_waits_for_a_new_selection_to_be_displayed() {
        let mut entry = active_entry();
        entry.ready = true;
        let mut view = SurfaceView::Dialogue {
            visibility: Default::default(),
            max_visible_players: None,
            max_entries: 10,
            controls: ControlSnapshot::default(),
            messages: vec![],
            notice: None,
            labels: Default::default(),
            mode: Mode::Wrist,
            follow_view: false,
            busy: false,
            error: None,
            font_size: 28,
            opacity: 1.0,
        };
        entry.publish(view.clone(), None, None);
        let key = entry.state.as_ref().unwrap().key;
        assert!(entry.scroll_key().is_none());
        let mut dialogue_layout = layout(key);
        dialogue_layout.regions[0].menu = None;
        assert!(entry
            .accept_layout_paint(SurfaceId::Dialogue, dialogue_layout.clone(), 1)
            .unwrap());
        assert!(entry.commit_paint(key, 1));
        assert_eq!(entry.scroll_key(), Some(key));

        if let SurfaceView::Dialogue { messages, .. } = &mut view {
            messages.push(WristMessage {
                speaker: None,
                text: "Streaming caption".into(),
                side: super::super::presentation::MessageSide::Left,
                original: None,
            });
        }
        entry.publish(view.clone(), None, None);
        assert_eq!(entry.scroll_key(), Some(key));
        assert!(entry.active_layout().is_none());

        if let SurfaceView::Dialogue { visibility, .. } = &mut view {
            visibility.hidden_tracks.push("player".into());
        }
        entry.publish(view, None, None);
        let selected = entry.state.as_ref().unwrap().key;
        assert!(entry.scroll_key().is_none());
        assert!(!entry.commit_paint(key, 1));
        dialogue_layout.key = selected;
        assert!(entry
            .accept_layout_paint(SurfaceId::Dialogue, dialogue_layout, 2)
            .unwrap());
        assert!(
            entry.scroll_key().is_none(),
            "layout alone is not a displayed frame"
        );
        assert!(entry.commit_paint(selected, 2));
        assert_eq!(entry.scroll_key(), Some(selected));
    }

    #[test]
    fn copy_budget_keeps_ocr_slower_and_does_not_catch_up_after_idle() {
        let mut renderer = SurfaceRenderer::new(SurfaceId::OcrProgress);
        let started = Instant::now();
        renderer.copied(started);
        assert!(!renderer.copy_due(started + Duration::from_millis(33)));
        assert!(renderer.copy_due(started + Duration::from_millis(34)));
        let resumed = started + Duration::from_secs(10);
        assert!(renderer.copy_due(resumed));
        renderer.copied(resumed);
        assert!(!renderer.copy_due(resumed));
        assert!(!renderer.copy_due(resumed + Duration::from_millis(33)));
        assert!(renderer.copy_due(resumed + Duration::from_millis(34)));
    }

    fn active_entry() -> Entry {
        let mut entry = Entry::default();
        entry.set_active(true, Instant::now());
        entry
    }

    fn advance(entry: &mut Entry, now: Instant) -> recovery::Action {
        let observation = entry.recovery_observation(None);
        entry.recovery.advance(observation, now)
    }

    #[test]
    fn retained_frame_expires_and_revokes_input_without_hiding_normal_updates() {
        let now = Instant::now();
        let mut entry = active_entry();
        entry.publish(SurfaceView::OcrSelection { dark: true }, None, None);
        entry.page_ready();
        entry.host_awake = true;
        let key = entry.state.as_ref().unwrap().key;
        entry
            .accept_layout_paint(SurfaceId::Menu, layout(key), 1)
            .unwrap();
        assert!(entry.commit_paint(key, 1));
        let mut renderer = SurfaceRenderer::new(SurfaceId::Menu);
        renderer.identity = Some((entry.host_epoch, 0, entry.generation));
        renderer.visible_frame = true;
        renderer.retained_key = Some(key);
        renderer.retained_activity = entry.activity_request;
        let observation = entry.recovery_observation(Some(recovery::Fault::Session));
        entry.recovery.advance(observation, now);
        assert!(renderer.frame_visible(&entry, now + Duration::from_millis(499)));
        assert!(!renderer.frame_visible(&entry, now + Duration::from_millis(500)));
        assert!(entry.active_layout().is_none());
        assert!(entry.scroll_key().is_none());
        assert!(entry.pointer_layout().is_none());
        renderer.surface = SurfaceId::OcrResult;
        assert!(!renderer.frame_visible(&entry, now));
        renderer.surface = SurfaceId::Menu;
        entry.recovery.submitted(now);
        entry.publish(SurfaceView::OcrSelection { dark: false }, None, None);
        assert!(
            renderer.frame_visible(&entry, now + Duration::from_secs(2)),
            "ordinary updates may retain old pixels"
        );
        let observation = entry.recovery_observation(Some(recovery::Fault::Session));
        entry.recovery.advance(observation, now);
        assert!(
            !renderer.frame_visible(&entry, now),
            "fault retention cannot cross content keys"
        );
    }

    #[test]
    fn paint_receipts_cannot_hide_an_older_unfulfilled_content_deadline() {
        let now = Instant::now();
        let mut entry = active_entry();
        entry.publish(SurfaceView::OcrSelection { dark: true }, None, None);
        entry.page_ready();
        let key = entry.state.as_ref().unwrap().key;
        entry
            .accept_layout_paint(SurfaceId::Menu, layout(key), 1)
            .unwrap();
        entry.commit_paint(key, 1);
        entry.pending_since = Some(now);
        entry
            .pending_paints
            .accept(2, now + Duration::from_millis(400));
        assert_eq!(
            advance(&mut entry, now + Duration::from_millis(499)),
            recovery::Action::None
        );
        assert_eq!(
            advance(&mut entry, now + Duration::from_millis(500)),
            recovery::Action::RestartCapture
        );
    }

    #[test]
    fn renderer_reset_cannot_clear_failed_entry_or_allow_publish() {
        let hub = SurfaceHub::default();
        let mut renderer = SurfaceRenderer::new(SurfaceId::Menu);
        {
            let mut entries = hub.0.lock().unwrap();
            let entry = &mut entries[SurfaceId::Menu.index()];
            entry.set_active(true, Instant::now());
            renderer.capture_fault = Some(windows_wgc::CaptureError::new(
                recovery::Fault::Unsupported,
                "unsupported",
            ));
        }
        renderer.advance_recovery(&hub, Instant::now());
        renderer.reset();
        let mut entries = hub.0.lock().unwrap();
        let entry = &mut entries[SurfaceId::Menu.index()];
        let generation = entry.generation;
        assert!(!entry.publish(SurfaceView::OcrSelection { dark: true }, None, None));
        assert_eq!(entry.generation, generation);
        assert!(entry.state.is_none());
        assert_eq!(entry.recovery.phase, recovery::Phase::Failed);
    }

    #[test]
    fn older_valid_frame_leaves_newer_paint_pending_and_overload_fails_closed() {
        let now = Instant::now();
        let mut entry = active_entry();
        entry.publish(SurfaceView::OcrSelection { dark: true }, None, None);
        entry.page_ready();
        let key = entry.state.as_ref().unwrap().key;
        entry
            .accept_layout_paint(SurfaceId::Menu, layout(key), 1)
            .unwrap();
        assert!(entry.accept_paint(key, 2));
        entry.pending_paints.clear();
        entry.pending_paints.accept(1, now);
        entry
            .pending_paints
            .accept(2, now + Duration::from_millis(100));
        assert!(entry.commit_paint(key, 1));
        assert_eq!(
            advance(&mut entry, now + Duration::from_millis(599)),
            recovery::Action::None
        );
        assert_eq!(
            advance(&mut entry, now + Duration::from_millis(600)),
            recovery::Action::RestartCapture
        );
        for serial in 3..=257 {
            assert!(entry.accept_paint(key, serial));
        }
        assert!(!entry.accept_paint(key, 258));
        assert!(entry.error.is_some());
        assert!(entry.active_layout().is_none());
    }

    #[test]
    fn dormant_content_has_no_deadline_and_cannot_accept_input_or_pixels() {
        let mut entry = Entry::default();
        entry.publish(
            SurfaceView::Menu {
                players: Default::default(),
                ui: Default::default(),
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            None,
            None,
        );
        let key = entry.state.as_ref().unwrap().key;
        assert_eq!(
            advance(&mut entry, Instant::now() + Duration::from_secs(120)),
            recovery::Action::None
        );
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
                players: Default::default(),
                ui: Default::default(),
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
                    players: Default::default(),
                    ui: Default::default(),
                    controls,
                    hold_progress: None,
                },
                None,
                None,
            );
        }
        assert_eq!(
            advance(&mut entry, now + Duration::from_secs(120)),
            recovery::Action::None
        );
        assert!(entry.set_active(true, now + Duration::from_secs(120)));
        assert!(entry.activity_request > sleep_request);
        assert_ne!(entry.state.as_ref().unwrap().key, old);
        assert!(!entry.accept_layout(SurfaceId::Menu, layout(old)).unwrap());
        assert_eq!(
            advance(&mut entry, now + Duration::from_secs(122)),
            recovery::Action::None
        );
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
                players: Default::default(),
                ui: Default::default(),
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
                players: Default::default(),
                ui: Default::default(),
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
                players: Default::default(),
                ui: Default::default(),
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
        moved.regions[0].bounds = [0.2, 0., 0.4, 0.2];
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
                bounds: [0., 0., 0.2, 0.2],
                circle: true,
                dashboard: None,
                menu: Some(super::super::wrist_menu::Command::Control(
                    super::super::control_protocol::ControlAction::SetWristEnabled(true),
                )),
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
                players: Default::default(),
                ui: Default::default(),
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
                players: Default::default(),
                ui: Default::default(),
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
                players: Default::default(),
                ui: Default::default(),
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
                players: Default::default(),
                ui: Default::default(),
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
        entry.ready = true;
        entry.host_awake = true;
        let mut controls = ControlSnapshot::default();
        entry.publish(
            SurfaceView::Menu {
                players: Default::default(),
                ui: Default::default(),
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
        let target = entry.capture_generation().unwrap();

        controls.revision += 1;
        entry.publish(
            SurfaceView::Menu {
                players: Default::default(),
                ui: Default::default(),
                controls,
                hold_progress: None,
            },
            None,
            None,
        );
        let current = entry.state.as_ref().unwrap().key;
        assert_ne!(current, old);
        assert_eq!(entry.capture_generation(), Some(target));
        assert!(entry.has_frame);
        assert!(entry.active_layout().is_none());
        assert_eq!(entry.pointer_layout().unwrap().key, old);
        assert!(!entry.accepts_frame(old, 1, 1));

        assert!(entry
            .accept_layout_paint(SurfaceId::Menu, layout(current), 2)
            .unwrap());
        assert_eq!(entry.capture_generation(), Some(target));
        assert!(entry.active_layout().is_none());
        assert_eq!(entry.pointer_layout().unwrap().key, old);
        assert!(entry.commit_paint(current, 2));
        assert_eq!(entry.active_layout().unwrap().key, current);
        assert_eq!(entry.pointer_layout().unwrap().key, current);

        entry.set_active(false, Instant::now());
        assert!(entry.capture_generation().is_none());
        assert!(entry.pointer_layout().is_none());
        entry.set_active(true, Instant::now());
        assert!(entry.capture_generation().is_none());
        assert!(entry.pointer_layout().is_none());
        entry.host_awake = true;
        entry.reload();
        assert!(entry.capture_generation().is_none());
    }

    #[test]
    fn menu_hold_progress_keeps_the_submitted_hit_layout() {
        let mut entry = Entry::default();
        entry.set_active(true, Instant::now());
        let mut controls = ControlSnapshot::default();
        entry.publish(
            SurfaceView::Menu {
                players: Default::default(),
                ui: Default::default(),
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
                    players: Default::default(),
                    ui: Default::default(),
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
                players: Default::default(),
                ui: Default::default(),
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
                players: Default::default(),
                ui: Default::default(),
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
            menu: None,
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
        value.regions[0].menu = None;
        assert!(value.validate(SurfaceId::Dashboard).is_err());
        value.regions[0].dashboard = Some(DashboardCommand::Page(
            super::super::dashboard::DashboardPage::Language,
        ));
        assert!(value.validate(SurfaceId::Dashboard).is_ok());
        assert_eq!(value.hit([0.1, 0.1]), Some(42));
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
    fn cold_host_does_not_spend_its_first_frame_budget_on_webview_startup() {
        let now = Instant::now();
        let mut entry = Entry::default();
        entry.set_active(true, now);
        entry.pending_since = Some(now);
        assert_eq!(
            advance(&mut entry, now + Duration::from_secs(5)),
            recovery::Action::None
        );
        assert_eq!(
            advance(&mut entry, now + Duration::from_secs(19)),
            recovery::Action::None
        );
        assert_eq!(
            advance(&mut entry, now + Duration::from_secs(20)),
            recovery::Action::RecreateHost
        );
    }

    #[test]
    fn first_frame_and_layout_deadlines_do_not_slide_with_streaming_updates() {
        let mut entry = active_entry();
        entry.ready = true;
        let now = Instant::now();
        entry.publish(
            SurfaceView::Menu {
                players: Default::default(),
                ui: Default::default(),
                controls: ControlSnapshot::default(),
                hold_progress: None,
            },
            None,
            None,
        );
        entry.pending_since = Some(now);
        entry.recovery.page_ready(now);
        assert_eq!(
            advance(&mut entry, now + Duration::from_millis(2999)),
            recovery::Action::None
        );
        assert_eq!(
            advance(&mut entry, now + Duration::from_secs(3)),
            recovery::Action::RecreateHost
        );
        let key = entry.state.as_ref().unwrap().key;
        entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap();
        assert!(entry.commit(key));
        assert_eq!(
            advance(&mut entry, now + Duration::from_secs(60)),
            recovery::Action::None
        );
        let mut controls = ControlSnapshot {
            revision: 2,
            ..Default::default()
        };
        entry.publish(
            SurfaceView::Menu {
                players: Default::default(),
                ui: Default::default(),
                controls: controls.clone(),
                hold_progress: None,
            },
            None,
            None,
        );
        let next = now + Duration::from_secs(61);
        entry.pending_since = Some(next);
        controls.revision = 3;
        entry.publish(
            SurfaceView::Menu {
                players: Default::default(),
                ui: Default::default(),
                controls,
                hold_progress: None,
            },
            None,
            None,
        );
        assert_eq!(
            advance(&mut entry, next + Duration::from_millis(499)),
            recovery::Action::None
        );
        assert_eq!(
            advance(&mut entry, next + Duration::from_millis(500)),
            recovery::Action::RecreateHost
        );
    }

    #[test]
    fn host_failure_revokes_input_and_allows_a_fresh_host() {
        let mut entry = active_entry();
        entry.publish(
            SurfaceView::Menu {
                players: Default::default(),
                ui: Default::default(),
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
        entry.creating = true;
        entry.ready = true;
        entry.error = Some("capture failed".into());
        entry.invalidate_host();
        assert_ne!(entry.host_epoch, host_epoch);
        assert!(entry.endpoint.is_none() && !entry.creating && !entry.ready);
        assert!(entry.error.is_none());
        assert!(entry.active_layout().is_none());
        assert!(!entry.commit(key));
        assert!(!entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap());
        assert!(entry.pending_since.is_none());
    }
}
