//! Platform-neutral surface identity, layout and bridge validation.
//! Only OCR-specific view payloads remain gated with their platform implementation.
use super::dashboard::DashboardCommand;
#[cfg(windows)]
use super::ocr_wrist::{Action, Button};
#[cfg(any(windows, linux_vr))]
use super::{
    control_protocol::ControlSnapshot,
    dashboard::{DashboardUi, DashboardViewModel},
    ocr_progress::ProgressView,
    placement::Mode,
    presentation::WristMessage,
};
#[cfg(any(windows, linux_vr))]
use super::{ocr_plane::OcrScene, ocr_wrist::View, wrist_labels::WristLabels};
use serde::{Deserialize, Serialize};
#[cfg(any(windows, linux_vr))]
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    #[cfg(any(windows, linux_vr))]
    pub(crate) const ALL: [Self; 10] = [
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
    #[cfg(any(windows, linux_vr))]
    pub(crate) fn index(self) -> usize {
        self as usize
    }
    pub(crate) fn name(self) -> &'static str {
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
    pub(crate) fn region_count(self) -> usize {
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

#[cfg(any(windows, linux_vr))]
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
    #[cfg(any(windows, linux_vr))]
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
        dashboard: Arc<DashboardViewModel>,
        ui: DashboardUi,
    },
    #[cfg(all(debug_assertions, feature = "vr-preview"))]
    DashboardThumbnail {
        dark: bool,
    },
    OcrSelection {
        dark: bool,
    },
    #[cfg(any(windows, linux_vr))]
    OcrResult {
        dark: bool,
        scene: Arc<OcrScene>,
    },
}

#[cfg(any(windows, linux_vr))]
impl SurfaceView {
    pub(crate) fn same_scroll_view(&self, other: &Self) -> bool {
        match (self, other) {
            #[cfg(any(windows, linux_vr))]
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
            #[cfg(any(windows, linux_vr))]
            (Self::OcrResult { dark: a, scene: av }, Self::OcrResult { dark: b, scene: bv }) => {
                a == b && (Arc::ptr_eq(av, bv) || av == bv)
            }
            (
                Self::Dashboard {
                    dark: a,
                    dashboard: av,
                    ui: au,
                },
                Self::Dashboard {
                    dark: b,
                    dashboard: bv,
                    ui: bu,
                },
            ) => a == b && au == bu && (Arc::ptr_eq(av, bv) || av == bv),
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

#[cfg(any(windows, linux_vr))]
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

#[cfg(any(windows, linux_vr))]
#[derive(Clone, Copy, PartialEq, Serialize)]
pub struct SurfaceActivity {
    pub active: bool,
    pub request: u32,
}

// Matches the fields that reset the reader viewport in VrSurface.tsx.
#[cfg(any(windows, linux_vr))]
#[derive(Clone, PartialEq)]
pub(crate) struct ReaderIdentity(
    super::ocr_wrist::ViewMode,
    usize,
    String,
    String,
    bool,
    usize,
);

#[cfg(any(windows, linux_vr))]
pub(crate) fn reader_identity(view: &SurfaceView) -> Option<ReaderIdentity> {
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

#[cfg(any(windows, linux_vr))]
#[derive(Clone, Serialize)]
pub(crate) struct SurfacePointer {
    pub key: SurfaceKey,
    pub point: Option<[f32; 2]>,
}

#[cfg(any(windows, linux_vr))]
#[derive(Clone, Serialize)]
pub(crate) struct SurfaceScroll {
    pub key: SurfaceKey,
    pub delta: f32,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_bounds: Option<[u32; 4]>,
}

impl SurfaceLayout {
    pub fn validate(&self, surface: SurfaceId) -> Result<(), String> {
        if let Some([l, t, r, b]) = self.capture_bounds {
            let (width, height) = surface.size();
            if surface != SurfaceId::OcrResult || l >= r || t >= b || r > width || b > height {
                return Err("Invalid OCR capture bounds".into());
            }
        }
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
                    Command::Control(action) => region.id == action.region_id(),
                    Command::Slider(slider) => slider.valid() && region.id >= 4,
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

    pub fn menu_action(&self, id: u8, point: [f32; 2]) -> Option<super::wrist_menu::Command> {
        let region = self.regions.iter().find(|region| region.id == id)?;
        match region.menu.as_ref()? {
            super::wrist_menu::Command::Slider(slider) => {
                let [left, _, right, _] = region.bounds;
                let width = right - left;
                if width <= 0.0 {
                    return None;
                }
                slider.command((point[0] - left) / width)
            }
            command => Some(command.clone()),
        }
    }

    #[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
pub fn reader_hover(action: Option<Action>) -> Option<u8> {
    (0..8).find(|&id| reader_action(id) == action)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct HostId {
    pub surface: SurfaceId,
    pub host_epoch: u32,
}

#[cfg(any(windows, linux_vr))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocumentSession {
    pub host: HostId,
    pub navigation_id: u64,
    pub nonce: String,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BridgeError {
    InvalidRequest,
    TooLarge,
    #[cfg(any(windows, linux_vr))]
    InvalidSource,
    #[cfg(any(windows, linux_vr))]
    StaleSession,
    #[cfg(any(windows, linux_vr))]
    InvalidLayout,
}

#[cfg(any(windows, linux_vr))]
impl BridgeError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::TooLarge => "request_too_large",
            Self::InvalidSource => "invalid_source",
            Self::StaleSession => "stale_session",
            Self::InvalidLayout => "invalid_layout",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "op",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum Operation {
    Ready {},
    Layout {
        layout: SurfaceLayout,
        paint_serial: u32,
    },
    Paint {
        key: SurfaceKey,
        paint_serial: u32,
    },
    Snapshot {
        key: Option<SurfaceKey>,
        activity_request: u32,
        scroll_top: Option<f64>,
    },
    Fault {
        stage: FaultStage,
    },
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FaultStage {
    Startup,
    Render,
    Unhandled,
}

#[cfg(any(windows, linux_vr))]
impl FaultStage {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Startup => "page_startup_failed",
            Self::Render => "page_render_failed",
            Self::Unhandled => "page_unhandled_error",
        }
    }
}

#[derive(Debug)]
pub(crate) enum PageRequest {
    Connect,
    Call {
        nonce: String,
        request_id: u64,
        operation: Operation,
    },
}

#[cfg(any(windows, linux_vr))]
#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum PageReply {
    State(Box<Option<SurfaceState>>),
    Accepted(bool),
}

#[cfg(any(windows, linux_vr))]
#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "payload", rename_all = "snake_case")]
pub(crate) enum SurfaceEvent {
    View(Box<SurfaceState>),
    Activity(SurfaceActivity),
    Pointer(SurfacePointer),
    Scroll(SurfaceScroll),
    Repaint,
}

pub(crate) fn decode_request(json: &str) -> Result<PageRequest, BridgeError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Envelope {
        version: u8,
        op: String,
        nonce: Option<String>,
        request_id: Option<u64>,
        payload: Option<serde_json::Value>,
    }
    if json.len() > 262144 {
        return Err(BridgeError::TooLarge);
    }
    let message: Envelope = serde_json::from_str(json).map_err(|_| BridgeError::InvalidRequest)?;
    if message.version != 1 {
        return Err(BridgeError::InvalidRequest);
    }
    if message.op == "connect" {
        return if message.nonce.is_none()
            && message.request_id.is_none()
            && message.payload.is_none()
        {
            Ok(PageRequest::Connect)
        } else {
            Err(BridgeError::InvalidRequest)
        };
    }
    let nonce = message
        .nonce
        .filter(|nonce| !nonce.is_empty() && nonce.len() <= 128)
        .ok_or(BridgeError::InvalidRequest)?;
    let request_id = message
        .request_id
        .filter(|id| (1..=9007199254740991).contains(id))
        .ok_or(BridgeError::InvalidRequest)?;
    let operation =
        serde_json::from_value(serde_json::json!({ "op": message.op, "payload": message.payload }))
            .map_err(|_| BridgeError::InvalidRequest)?;
    Ok(PageRequest::Call {
        nonce,
        request_id,
        operation,
    })
}

#[cfg(any(windows, linux_vr))]
impl DocumentSession {
    pub fn authorize(&self, source: &str, request: &PageRequest) -> Result<(), BridgeError> {
        if source != self.source {
            return Err(BridgeError::InvalidSource);
        }
        if let PageRequest::Call { nonce, .. } = request {
            if nonce != &self.nonce {
                return Err(BridgeError::StaleSession);
            }
        }
        Ok(())
    }
}

pub(crate) struct ResourcePolicy {
    origin: tauri::Url,
    embedded: bool,
    assets: std::collections::HashSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ResourceRoute {
    Network,
    Asset(String),
    Reject(u16),
}

impl ResourcePolicy {
    pub fn new(
        dev_url: Option<&tauri::Url>,
        assets: impl Iterator<Item = String>,
    ) -> Result<Self, String> {
        let mut origin = dev_url
            .cloned()
            .unwrap_or_else(|| tauri::Url::parse("https://vrcs-surface.invalid/").unwrap());
        if dev_url.is_some()
            && (!matches!(origin.scheme(), "http" | "https")
                || !matches!(origin.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
                || !origin.username().is_empty()
                || origin.password().is_some())
        {
            return Err("Surface dev URL must use a loopback HTTP origin".into());
        }
        origin.set_path("/");
        origin.set_query(None);
        origin.set_fragment(None);
        Ok(Self {
            origin,
            embedded: dev_url.is_none(),
            assets: assets
                .map(|path| path.trim_start_matches('/').to_owned())
                .collect(),
        })
    }

    #[cfg(linux_vr)]
    pub fn with_embedded_origin(mut self, origin: &str) -> Result<Self, String> {
        let mut origin = tauri::Url::parse(origin)
            .map_err(|error| format!("Invalid surface origin: {error}"))?;
        origin.set_path("/");
        origin.set_query(None);
        origin.set_fragment(None);
        self.origin = origin;
        self.embedded = true;
        Ok(self)
    }

    pub fn entry_url(&self, host: HostId) -> String {
        format!(
            "{}vr-surface.html?surface={}&host_epoch={}",
            self.origin,
            host.surface.name(),
            host.host_epoch
        )
    }

    pub fn route(&self, method: &str, uri: &str) -> ResourceRoute {
        if !matches!(method, "GET" | "HEAD") {
            return ResourceRoute::Reject(405);
        }
        let Ok(url) = tauri::Url::parse(uri) else {
            return ResourceRoute::Reject(403);
        };
        if !self.same_origin(&url) || !url.username().is_empty() || url.password().is_some() {
            return ResourceRoute::Reject(403);
        }
        if !self.embedded {
            return ResourceRoute::Network;
        }
        // Reject ambiguous raw paths before URL normalization can remove dot segments.
        let raw_path = uri
            .split_once("://")
            .and_then(|(_, tail)| tail.find('/').map(|at| &tail[at..]))
            .unwrap_or("/")
            .split(['?', '#'])
            .next()
            .unwrap_or("/");
        if raw_path.contains(['%', '\\'])
            || raw_path.split('/').any(|part| part == "." || part == "..")
        {
            return ResourceRoute::Reject(403);
        }
        let path = url.path().trim_start_matches('/');
        if path.starts_with("tests/") || path.ends_with(".html") && path != "vr-surface.html" {
            return ResourceRoute::Reject(403);
        }
        if self.assets.contains(path) {
            ResourceRoute::Asset(path.into())
        } else {
            ResourceRoute::Reject(404)
        }
    }

    fn same_origin(&self, url: &tauri::Url) -> bool {
        if matches!(self.origin.scheme(), "http" | "https") {
            url.origin() == self.origin.origin()
        } else {
            // Custom schemes have opaque origins, so compare their components.
            url.scheme() == self.origin.scheme()
                && url.host_str() == self.origin.host_str()
                && url.port_or_known_default() == self.origin.port_or_known_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn layout(key: SurfaceKey) -> SurfaceLayout {
        SurfaceLayout {
            key,
            regions: vec![Region {
                id: 0,
                bounds: [0., 0., 0.2, 0.2],
                circle: true,
                dashboard: None,
                menu: Some(crate::vr_overlay::wrist_menu::Command::Control(
                    crate::vr_overlay::control_protocol::ControlAction::SetWristEnabled(true),
                )),
            }],
            menu_bounds: None,
            panels: vec![],
            page_count: 1,
            unplaced: vec![],
            capture_bounds: None,
        }
    }

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
    fn photo_gesture_region_keeps_the_personnel_navigation_region() {
        use crate::vr_overlay::{control_protocol::ControlAction, wrist_menu};
        let mut layout = layout(SurfaceKey {
            generation: 1,
            revision: 1,
        });
        let gesture = wrist_menu::Command::Control(ControlAction::SetOcrGestureEnabled(false));
        layout.regions[0].id = 4;
        layout.regions[0].menu = Some(wrist_menu::Command::Page(wrist_menu::Page::Players));
        layout.regions.push(Region {
            id: 30,
            bounds: [0.3, 0., 0.5, 0.2],
            circle: true,
            dashboard: None,
            menu: Some(gesture.clone()),
        });
        assert!(layout.validate(SurfaceId::Menu).is_ok());
        assert_eq!(layout.hit([0.4, 0.1]), Some(30));
        assert_eq!(layout.menu_command(30), Some(gesture.clone()));
        assert_eq!(
            layout.menu_command(4),
            Some(wrist_menu::Command::Page(wrist_menu::Page::Players))
        );
        layout.regions[0].menu = Some(gesture);
        assert!(layout.validate(SurfaceId::Menu).is_err());
    }

    #[test]
    fn slider_regions_resolve_release_positions_to_quantized_values() {
        use crate::vr_overlay::{
            player_display::Action,
            wrist_menu::{Command, Slider, SliderAction},
        };
        let mut layout = layout(SurfaceKey {
            generation: 1,
            revision: 1,
        });
        layout.regions[0] = Region {
            id: 32,
            bounds: [0.1, 0.4, 0.9, 0.5],
            circle: false,
            dashboard: None,
            menu: Some(Command::Slider(Slider {
                action: SliderAction::ProximityRadius,
                min: 0.0,
                max: 10.0,
                step: 0.1,
            })),
        };
        assert!(layout.validate(SurfaceId::Menu).is_ok());
        assert_eq!(
            layout.menu_action(32, [0.1, 0.45]),
            Some(Command::Display(Action::SetProximityRadius(0.0)))
        );
        assert_eq!(
            layout.menu_action(32, [0.5, 0.45]),
            Some(Command::Display(Action::SetProximityRadius(5.0)))
        );
        assert_eq!(
            layout.menu_action(32, [0.9, 0.45]),
            Some(Command::Display(Action::SetProximityRadius(10.0)))
        );
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
                capture_bounds: None,
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
            crate::vr_overlay::dashboard::DashboardPage::Language,
        ));
        assert!(value.validate(SurfaceId::Dashboard).is_ok());
        assert_eq!(value.hit([0.1, 0.1]), Some(42));
        assert!(value.validate(SurfaceId::Menu).is_err());
    }

    #[test]
    fn oversized_or_unknown_requests_are_rejected() {
        assert!(matches!(
            decode_request(r#"{"version":1,"op":"connect"}"#),
            Ok(PageRequest::Connect)
        ));
        let good =
            json!({ "version": 1, "nonce": "test", "request_id": 1, "op": "ready", "payload": {} });
        let decode_call = |request: &serde_json::Value| {
            let PageRequest::Call {
                nonce,
                request_id,
                operation,
            } = decode_request(&request.to_string()).unwrap()
            else {
                panic!("Expected an authenticated call");
            };
            assert_eq!(nonce, "test");
            assert_eq!(request_id, 1);
            operation
        };
        assert!(matches!(decode_call(&good), Operation::Ready {}));

        let key = SurfaceKey {
            generation: 7,
            revision: 11,
        };
        let mut request = good.clone();
        request["op"] = json!("layout");
        request["payload"] = json!({ "layout": layout(key), "paint_serial": 13 });
        let Operation::Layout {
            layout: decoded,
            paint_serial,
        } = decode_call(&request)
        else {
            panic!("Expected a layout call");
        };
        assert_eq!(
            serde_json::to_value(decoded).unwrap(),
            request["payload"]["layout"]
        );
        assert_eq!(paint_serial, 13);

        request["op"] = json!("paint");
        request["payload"] = json!({ "key": key, "paint_serial": 17 });
        let Operation::Paint {
            key: decoded,
            paint_serial,
        } = decode_call(&request)
        else {
            panic!("Expected a paint call");
        };
        assert_eq!(decoded, key);
        assert_eq!(paint_serial, 17);

        request["op"] = json!("snapshot");
        request["payload"] = json!({ "key": key, "activity_request": 19, "scroll_top": 12.5 });
        let Operation::Snapshot {
            key: decoded,
            activity_request,
            scroll_top,
        } = decode_call(&request)
        else {
            panic!("Expected a snapshot call");
        };
        assert_eq!(decoded, Some(key));
        assert_eq!(activity_request, 19);
        assert_eq!(scroll_top, Some(12.5));

        request["op"] = json!("fault");
        for (name, expected) in [
            ("startup", FaultStage::Startup),
            ("render", FaultStage::Render),
            ("unhandled", FaultStage::Unhandled),
        ] {
            request["payload"] = json!({ "stage": name });
            let Operation::Fault { stage } = decode_call(&request) else {
                panic!("Expected a fault call");
            };
            assert_eq!(stage, expected);
        }
        for (op, payload) in [
            (
                "layout",
                json!({ "layout": layout(key), "paint_serial": "13" }),
            ),
            ("layout", json!({ "layout": layout(key) })),
            ("paint", json!({ "key": key, "paint_serial": -1 })),
            (
                "paint",
                json!({ "key": { "generation": 7 }, "paint_serial": 17 }),
            ),
            (
                "snapshot",
                json!({ "key": key, "activity_request": "19", "scroll_top": 12.5 }),
            ),
            (
                "snapshot",
                json!({ "key": key, "activity_request": 19, "scroll_top": "12.5" }),
            ),
            ("fault", json!({ "stage": "unknown" })),
            (
                "fault",
                json!({ "stage": "render", "command": "read_file" }),
            ),
        ] {
            request["op"] = json!(op);
            request["payload"] = payload;
            assert!(
                decode_request(&request.to_string()).is_err(),
                "accepted {request}"
            );
        }
        let invalid = [
            ("version", json!(2)),
            ("request_id", json!(0)),
            ("request_id", json!(-1)),
            ("request_id", json!(9007199254740992_u64)),
            ("op", json!("invoke")),
            ("nonce", json!("")),
            ("surface", json!("menu")),
            ("payload", json!({ "command": "read_file" })),
        ];
        for (key, value) in invalid {
            let mut request = good.clone();
            request[key] = value;
            assert!(
                decode_request(&request.to_string()).is_err(),
                "accepted {key}"
            );
        }
        assert!(decode_request(r#"{"version":1,"op":"connect","nonce":"injected"}"#).is_err());
        assert!(matches!(
            decode_request(&" ".repeat(262145)),
            Err(BridgeError::TooLarge)
        ));
    }

    #[test]
    fn dev_resources_use_the_exact_configured_loopback_origin() {
        let url = tauri::Url::parse("http://127.0.0.1:15341/").unwrap();
        let policy = ResourcePolicy::new(Some(&url), std::iter::empty()).unwrap();
        assert_eq!(
            policy.entry_url(HostId {
                surface: SurfaceId::Reader,
                host_epoch: 7
            }),
            "http://127.0.0.1:15341/vr-surface.html?surface=reader&host_epoch=7"
        );
        assert_eq!(
            policy.route("GET", "http://127.0.0.1:15341/@vite/client"),
            ResourceRoute::Network
        );
        for uri in [
            "http://127.0.0.1:1420/src/main.tsx",
            "http://localhost:15341/src/main.tsx",
            "https://127.0.0.1:15341/src/main.tsx",
            "http://127.0.0.1:15341@evil.invalid/a",
        ] {
            assert_eq!(policy.route("GET", uri), ResourceRoute::Reject(403));
        }
        assert!(ResourcePolicy::new(
            Some(&tauri::Url::parse("https://example.com/").unwrap()),
            std::iter::empty()
        )
        .is_err());
    }

    #[test]
    fn embedded_resources_require_an_exact_asset_and_never_use_spa_fallback() {
        let policy = ResourcePolicy::new(
            None,
            [
                "vr-surface.html",
                "index.html",
                "assets/surface.js",
                "assets/surface.css",
                "tests/fixture.js",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        let route =
            |method, path| policy.route(method, &format!("https://vrcs-surface.invalid{path}"));
        assert_eq!(
            route("GET", "/vr-surface.html?surface=reader&host_epoch=1"),
            ResourceRoute::Asset("vr-surface.html".into())
        );
        assert_eq!(
            route("HEAD", "/assets/surface.css"),
            ResourceRoute::Asset("assets/surface.css".into())
        );
        assert_eq!(
            route("GET", "/assets/missing.js"),
            ResourceRoute::Reject(404)
        );
        assert_eq!(
            route("POST", "/assets/surface.js"),
            ResourceRoute::Reject(405)
        );
        for path in [
            "/index.html",
            "/tests/fixture.js",
            "/assets/../vr-surface.html",
            "/assets/%2e%2e/vr-surface.html",
            "/%2561ssets/surface.js",
            "/assets\\surface.js",
        ] {
            assert_eq!(
                route("GET", path),
                ResourceRoute::Reject(403),
                "accepted {path}"
            );
        }
        assert_eq!(
            policy.route("GET", "file:///assets/surface.js"),
            ResourceRoute::Reject(403)
        );
        assert_eq!(
            policy.route("GET", "https://example.com/assets/surface.js"),
            ResourceRoute::Reject(403)
        );
    }
}
