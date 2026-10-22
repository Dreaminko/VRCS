//! React owns pixels and layout; the VR worker owns input and texture submission.
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
            Self::Dialogue => 3,
            Self::Reader => 8,
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
    Menu {
        controls: ControlSnapshot,
    },
    Dialogue {
        controls: ControlSnapshot,
        messages: Vec<WristMessage>,
        mode: Mode,
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
        dark: bool,
        text: String,
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
        dashboard: DashboardViewModel,
        ui: DashboardUi,
    },
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

#[derive(Clone, Serialize)]
pub struct SurfaceState {
    pub key: SurfaceKey,
    pub view: SurfaceView,
    pub hovered: Option<u8>,
    pub pressed: Option<u8>,
    pub pointer: Option<[f32; 2]>,
}

#[derive(Clone, Serialize)]
struct SurfacePointer {
    key: SurfaceKey,
    point: Option<[f32; 2]>,
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
pub struct SurfaceLayout {
    pub key: SurfaceKey,
    pub regions: Vec<Region>,
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
    submitted: Option<SurfaceKey>,
    hwnd: Option<isize>,
    creating: bool,
    ready: bool,
    error: Option<String>,
    pending_since: Option<Instant>,
    has_frame: bool,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            generation: 1,
            host_epoch: 1,
            state: None,
            layout: None,
            submitted: None,
            hwnd: None,
            creating: false,
            ready: false,
            error: None,
            pending_since: None,
            has_frame: false,
        }
    }
}

impl Entry {
    fn publish(&mut self, view: SurfaceView, hovered: Option<u8>, pressed: Option<u8>) -> bool {
        let changed = self.state.as_ref().is_none_or(|state| state.view != view);
        let feedback = self
            .state
            .as_ref()
            .is_none_or(|state| state.hovered != hovered || state.pressed != pressed);
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
            });
            self.layout = None;
            self.submitted = None;
        } else if let Some(state) = &mut self.state {
            state.hovered = hovered;
            state.pressed = pressed;
        }
        if self.submitted.is_none() {
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
        self.submitted = None;
        self.ready = false;
        self.has_frame = false;
        self.pending_since = None;
        if let Some(state) = &mut self.state {
            state.key = SurfaceKey {
                generation: self.generation,
                revision: 1,
            };
            state.hovered = None;
            state.pressed = None;
            state.pointer = None;
        }
    }

    fn invalidate_host(&mut self) {
        self.reload();
        self.host_epoch = self.host_epoch.wrapping_add(1).max(1);
        self.hwnd = None;
        self.creating = false;
        self.error = None;
    }

    fn timed_out(&self, now: Instant) -> bool {
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
        if self
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

    fn commit(&mut self, key: SurfaceKey) -> bool {
        if self.state.as_ref().is_some_and(|state| state.key == key)
            && self.layout.as_ref().is_some_and(|layout| layout.key == key)
        {
            self.submitted = Some(key);
            self.pending_since = None;
            self.has_frame = true;
            true
        } else {
            false
        }
    }

    fn active_layout(&self) -> Option<&SurfaceLayout> {
        self.layout.as_ref().filter(|layout| {
            Some(layout.key) == self.submitted
                && self
                    .state
                    .as_ref()
                    .is_some_and(|state| state.key == layout.key)
        })
    }
}

#[derive(Default)]
pub struct SurfaceHub(Mutex<[Entry; 9]>);

impl SurfaceHub {
    pub fn generation(&self, surface: SurfaceId) -> u32 {
        self.0.lock().unwrap_or_else(|e| e.into_inner())[surface.index()].generation
    }

    pub fn layout(&self, surface: SurfaceId) -> Option<SurfaceLayout> {
        self.0.lock().ok()?[surface.index()]
            .active_layout()
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
            let changed = entry.publish(view, hovered, pressed);
            let create = !entry.creating && entry.hwnd.is_none();
            if create {
                entry.creating = true;
            }
            (
                create.then_some(entry.host_epoch),
                (changed && entry.ready)
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
        capture: &mut windows_wgc::Capture,
        surface: SurfaceId,
        key: SurfaceKey,
    ) -> Result<windows_wgc::Captured, String> {
        let started = Instant::now();
        loop {
            if let Some(frame) = capture.poll()? {
                if (frame.generation, frame.revision) == (key.generation, key.revision) {
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
                let mut capture = windows_wgc::Capture::new(hwnd, 0, width, height)?;
                let first_frame = wait_for_frame(&mut capture, surface, key)?;
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
                hub.0.lock().map_err(|e| e.to_string())?[surface.index()].commit(key);
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
                    loop {
                        if let Some(frame) = capture.poll()? {
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
                            return Err(format!(
                                "{} hover did not update its captured texture",
                                surface.name()
                            ));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                }
                if hovered.is_none() {
                    let mut updated = view.clone();
                    match &mut updated {
                        SurfaceView::Headset { text, .. } => {
                            *text = "Updated caption / 字幕内容已更新".into();
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
                        SurfaceView::DashboardThumbnail { dark }
                        | SurfaceView::OcrSelection { dark } => {
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
                    let frame = wait_for_frame(&mut capture, surface, key)?;
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
                    capture = windows_wgc::Capture::new(hwnd, 0, width, height)?;
                    let frame = wait_for_frame(&mut capture, surface, recovered)?;
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
                    if !entries[surface.index()].commit(recovered) {
                        return Err("Recovered frame did not match the current layout".into());
                    }
                    key = recovered;
                }
                results.push(serde_json::json!({"surface":surface,"width":width,"height":height,"key":key,"desktop_hidden":true,"transparent_pixels":transparent,"visible_pixels":visible,"hover_updated":hovered.map(|_| true),"content_updated":hovered.is_none(),"submitted_texture_preserved":true,"reload_recovered":true,"host_recreated":true}));
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
    hub: State<'_, SurfaceHub>,
) -> Result<bool, String> {
    let epoch = require_host(&window, surface)?;
    let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
    let entry = &mut entries[surface.index()];
    if entry.host_epoch != epoch {
        return Ok(false);
    }
    entry.accept_layout(surface, layout)
}

fn create_host(app: &AppHandle, surface: SurfaceId, epoch: u32) -> Result<(), String> {
    let host_app = app.clone();
    app.run_on_main_thread(move || {
        {
            let hub = host_app.state::<SurfaceHub>();
            let entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
            if entries[surface.index()].host_epoch != epoch {
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
                .inner_size(width as f64, (height + 8) as f64)
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
            window.set_size(tauri::PhysicalSize::new(width, height + 8)).map_err(|e| e.to_string())?;
            window.set_ignore_cursor_events(true).map_err(|e| e.to_string())?;
            // WGC needs a shown window to keep producing frames.
            window.show().map_err(|e| e.to_string())?;
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
            Ok(hwnd) => entry.hwnd = hwnd,
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
}

impl SurfaceRenderer {
    pub fn new(surface: SurfaceId) -> Self {
        Self {
            surface,
            capture: None,
            identity: None,
            visible_frame: false,
            retry_at: None,
        }
    }
    pub fn reset(&mut self) {
        self.capture = None;
        self.identity = None;
        self.visible_frame = false;
        self.retry_at = None;
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
        if self.retry_at.is_some_and(|at| Instant::now() < at) {
            return Ok(false);
        }
        self.retry_at = None;
        let result = self.present_frame(app, backend, kind, view, hovered, pressed);
        if result.is_err() {
            let hub = app.state::<SurfaceHub>();
            hub.0.lock().unwrap_or_else(|e| e.into_inner())[self.surface.index()].invalidate_host();
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
            let Some(hwnd) = entry.hwnd.filter(|_| entry.ready) else {
                return Ok(false);
            };
            (hwnd, entry.generation)
        };
        let adapter = backend.surface_adapter_index()?;
        let identity = (hwnd, adapter, generation);
        if self.identity != Some(identity) {
            if self.identity.is_some() {
                backend.reset(kind);
                self.reset();
                let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
                entries[self.surface.index()].submitted = None;
                return Ok(false);
            }
            self.reset();
            let (width, height) = self.surface.size();
            self.capture = Some(windows_wgc::Capture::new(hwnd, adapter, width, height)?);
            self.identity = Some(identity);
        }
        let capture = self.capture.as_mut().expect("capture exists");
        if let Some(frame) = capture.poll()? {
            let key = SurfaceKey {
                generation: frame.generation,
                revision: frame.revision,
            };
            let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
            let entry = &mut entries[self.surface.index()];
            if entry
                .layout
                .as_ref()
                .is_some_and(|layout| layout.key == key)
                && entry.state.as_ref().is_some_and(|state| state.key == key)
            {
                backend.upload_surface(kind, frame.shared_handle)?;
                entry.commit(key);
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
    fn layout(key: SurfaceKey) -> SurfaceLayout {
        SurfaceLayout {
            key,
            regions: vec![Region {
                id: 0,
                bounds: [0., 0., 0.2, 0.8],
                circle: true,
                dashboard: None,
            }],
            page_count: 1,
            unplaced: vec![],
        }
    }
    #[test]
    fn pointer_updates_keep_the_submitted_layout_and_reject_inactive_or_invalid_hits() {
        let mut entry = Entry::default();
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
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
        let mut entry = Entry::default();
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
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
            },
            Some(0),
            Some(0),
        );
        assert_eq!(entry.state.as_ref().unwrap().key, key);
        assert!(entry.active_layout().is_some());
        let mut controls = ControlSnapshot::default();
        controls.revision += 1;
        entry.publish(SurfaceView::Menu { controls }, None, None);
        assert!(!entry.accept_layout(SurfaceId::Menu, layout(key)).unwrap());
        assert!(!entry.commit(key));
        assert!(entry.active_layout().is_none());
    }
    #[test]
    fn reload_revokes_old_frames_and_layouts() {
        let mut entry = Entry::default();
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
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
        let mut entry = Entry::default();
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
        let mut entry = Entry::default();
        let now = Instant::now();
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
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
            },
            None,
            None,
        );
        entry.pending_since = Some(now);
        controls.revision = 3;
        entry.publish(SurfaceView::Menu { controls }, None, None);
        assert!(!entry.timed_out(now + Duration::from_millis(499)));
        assert!(entry.timed_out(now + Duration::from_millis(500)));
    }

    #[test]
    fn host_failure_revokes_input_and_allows_a_fresh_host() {
        let mut entry = Entry::default();
        entry.publish(
            SurfaceView::Menu {
                controls: ControlSnapshot::default(),
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
