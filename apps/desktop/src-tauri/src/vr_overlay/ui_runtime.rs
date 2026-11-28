use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager as _};
use vrcs_core::VrOverlayConfig;

use super::backend::{Backend, BackendKind, CursorTarget, OverlayKind, menu_wrist_config};
use super::control_protocol::{ControlHub, ControlSource};
use super::interaction::{Hit, Key, Kind, Router, Sample, Surface};
use super::layout_edit::{
    EDIT_TOOLBAR_COMMIT_EVENT, EditTarget, EditToolbarAction, GrabTarget, LayoutEditSession,
};
use super::ocr_wrist::LaserEvent;
use super::placement::{Mode, Placement, Pose, Request};
use super::pose::{DeviceId, PointerId, PointerInput, SpaceId};
use super::surface::{SurfaceHub, SurfaceRenderer};
use super::surface_protocol::{SurfaceId, SurfaceLayout, SurfaceView};
use super::visibility::Fade;
use super::{dialogue, wrist_menu};

#[derive(Clone, Copy)]
struct Pointer {
    device: u32,
    key: Key,
    point: [f32; 2],
    position: [f32; 3],
    distance: f32,
    scroll_y: f32,
    origin: SpaceId,
    panel: [[f32; 4]; 3],
    panel_width: f32,
}

#[derive(Clone)]
pub(super) struct UiTick {
    pub reader_revision: u64,
    pub content_owns_input: bool,
    pub business: bool,
    pub dialogue_sample: bool,
    pub dialogue_content_active: bool,
    pub dialogue_retention: DialogueRetentionKey,
    pub menu_edit_overlay: Option<super::surface_protocol::EditSurfaceOverlay>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct DialogueRetentionKey {
    pub content: u64,
    pub policy: u64,
}

fn pointer_overlay(surface: Surface) -> (OverlayKind, SurfaceId) {
    match surface {
        Surface::Headset => (OverlayKind::Headset, SurfaceId::Headset),
        Surface::Menu => (OverlayKind::WristMenu, SurfaceId::Menu),
        Surface::Dialogue => (OverlayKind::Wrist, SurfaceId::Dialogue),
        Surface::Reader => (OverlayKind::OcrWrist, SurfaceId::Reader),
        Surface::EditToolbar => (OverlayKind::EditToolbar, SurfaceId::EditToolbar),
    }
}

fn surface_index(surface: Surface) -> usize {
    match surface {
        Surface::Menu => 0,
        Surface::Dialogue => 1,
        Surface::Reader => 2,
        Surface::Headset => 3,
        Surface::EditToolbar => 4,
    }
}

fn nearest_pointer(
    pointers: impl Iterator<Item = Pointer>,
    layouts: &[Option<Arc<SurfaceLayout>>; 5],
) -> Option<Pointer> {
    pointers
        .filter(|pointer| {
            layouts[surface_index(pointer.key.surface)]
                .as_ref()
                .is_some_and(|layout| layout.pointer_contains(pointer.point))
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

// Viewport heights per second, capped to avoid jumps after a stalled tick.
fn scroll_delta(axis: f32, elapsed: Duration) -> f32 {
    -axis * elapsed.as_secs_f32().min(0.05) * 1.5
}

#[derive(Clone, serde::Serialize)]
pub struct PlacementRequest {
    pub owner_epoch: u32,
    pub id: u64,
    pub mode: Mode,
    pub follow_view: bool,
}

pub fn dispatch_placement(
    app: &AppHandle,
    epoch: u32,
    placement: &Mutex<Placement>,
    request: Request,
) {
    let error = if epoch == 0 {
        Some("Controls are unavailable".into())
    } else {
        app.emit_to(
            "main",
            "vr-overlay-placement-request",
            PlacementRequest {
                owner_epoch: epoch,
                id: request.id,
                mode: request.mode,
                follow_view: request.follow_view,
            },
        )
        .err()
        .map(|e| e.to_string())
    };
    if error.is_some() {
        super::runtime::lock_unpoisoned(placement).resolve(request.id, false, error);
    }
}

#[derive(Default)]
struct ContextChanges {
    owner: bool,
    menu: bool,
    dialogue: bool,
}

#[derive(Default)]
struct UiContext {
    initialized: bool,
    menu_hand: String,
    wrist_hand: String,
    dominant_hand: String,
    owner_epoch: u32,
    owner_active: bool,
    wrist_enabled: bool,
    menu_enabled: bool,
}

impl UiContext {
    fn diff(
        &self,
        config: &VrOverlayConfig,
        owner_epoch: u32,
        owner_active: bool,
    ) -> ContextChanges {
        if !self.initialized {
            return ContextChanges {
                owner: true,
                menu: true,
                dialogue: true,
            };
        }
        let owner = self.owner_epoch != owner_epoch || self.owner_active != owner_active;
        let menu = owner
            || self.menu_hand != config.wrist_menu.hand
            || self.menu_enabled != config.wrist_menu.enabled;
        let dialogue = owner
            || self.wrist_hand != config.wrist.hand
            || self.dominant_hand != config.wrist.dominant_hand
            || self.wrist_enabled != (config.enabled && config.wrist.enabled);
        ContextChanges {
            owner,
            menu,
            dialogue,
        }
    }

    fn update(&mut self, config: &VrOverlayConfig, owner_epoch: u32, owner_active: bool) {
        self.initialized = true;
        self.menu_hand.clear();
        self.menu_hand.push_str(&config.wrist_menu.hand);
        self.wrist_hand.clear();
        self.wrist_hand.push_str(&config.wrist.hand);
        self.dominant_hand.clear();
        self.dominant_hand.push_str(&config.wrist.dominant_hand);
        self.owner_epoch = owner_epoch;
        self.owner_active = owner_active;
        self.wrist_enabled = config.enabled && config.wrist.enabled;
        self.menu_enabled = config.wrist_menu.enabled;
    }
}

pub struct UiRuntime {
    pub placement: Arc<Mutex<Placement>>,
    pub layout_edit: Arc<Mutex<LayoutEditSession>>,
    router: Router,
    lift: wrist_menu::Visibility,
    dialogue_lift: wrist_menu::Visibility,
    menu_fade: Fade,
    pub dialogue_fade: Fade,
    pub dialogue_active: bool,
    pub dialogue_available: bool,
    pub dialogue_occluded: bool,
    pub dialogue_retention: Option<DialogueRetentionKey>,
    pub dialogue_pending: Option<DialogueRetentionKey>,
    menu_frame: Option<SurfaceView>,
    menu_ui: wrist_menu::Ui,
    pub menu_visible: bool,
    pub dialogue_hover: Option<u8>,
    pub headset_hover: Option<u8>,
    pub headset_pressed: Option<u8>,
    pub toolbar_hover: Option<u8>,
    pub toolbar_pressed: Option<u8>,
    pub dialogue_visible: bool,
    pub dialogue_revision: Option<u64>,
    pub generation: u64,
    menu_generation: u64,
    dialogue_generation: u64,
    context: UiContext,
    origin: Option<SpaceId>,
    current_origin: Option<SpaceId>,
    pressed: Option<u8>,
    menu_error: Option<String>,
    last_input: Instant,
    pointer_targets: Vec<(OverlayKind, Key, Option<u32>)>,
    pointer_devices: Vec<(DeviceId, PointerId)>,
    pointer: Option<Pointer>,
    cursor_error: Option<String>,
    menu_surface: SurfaceRenderer,
    pub dialogue_surface: SurfaceRenderer,
    snapshot_version: Option<u64>,
    snapshot: Arc<(
        super::control_protocol::ControlSnapshot,
        super::player_display::View,
        u32,
        bool,
    )>,
}

impl UiRuntime {
    pub fn new(
        placement: Arc<Mutex<Placement>>,
        layout_edit: Arc<Mutex<LayoutEditSession>>,
    ) -> Self {
        Self {
            placement,
            layout_edit,
            router: Router::default(),
            lift: Default::default(),
            dialogue_lift: Default::default(),
            menu_fade: Default::default(),
            dialogue_fade: Default::default(),
            dialogue_active: false,
            dialogue_available: false,
            dialogue_occluded: false,
            dialogue_retention: None,
            dialogue_pending: None,
            menu_frame: None,
            menu_ui: Default::default(),
            menu_visible: false,
            dialogue_hover: None,
            headset_hover: None,
            headset_pressed: None,
            toolbar_hover: None,
            toolbar_pressed: None,
            dialogue_visible: false,
            dialogue_revision: None,
            generation: 1,
            menu_generation: 1,
            dialogue_generation: 1,
            context: Default::default(),
            origin: None,
            current_origin: None,
            pressed: None,
            menu_error: None,
            last_input: Instant::now(),
            pointer_targets: Vec::new(),
            pointer_devices: Vec::new(),
            pointer: None,
            cursor_error: None,
            menu_surface: SurfaceRenderer::new(SurfaceId::Menu),
            dialogue_surface: SurfaceRenderer::new(SurfaceId::Dialogue),
            snapshot_version: None,
            snapshot: Default::default(),
        }
    }

    pub fn current_origin(&self) -> Option<SpaceId> {
        self.current_origin
    }

    pub fn finish_surfaces(&mut self, app: &AppHandle, mut backend: Option<&mut Backend>) {
        self.menu_surface
            .finish_tick(app, backend.as_deref_mut(), OverlayKind::WristMenu);
        self.dialogue_surface
            .finish_tick(app, backend, OverlayKind::Wrist);
    }

    pub fn stop_surfaces(&mut self, app: &AppHandle, mut backend: Option<&mut Backend>) {
        self.menu_surface
            .stop(app, backend.as_deref_mut(), OverlayKind::WristMenu);
        self.dialogue_surface.stop(app, backend, OverlayKind::Wrist);
    }

    pub fn confirm_frames(&mut self, app: &AppHandle, accepted: bool) {
        self.menu_surface.confirm_frame(app, accepted);
        self.dialogue_surface.confirm_frame(app, accepted);
    }

    pub fn fast_tick(&self) -> bool {
        self.menu_visible
            || self.menu_fade.animating()
            || self.dialogue_fade.animating()
            || self.router.active()
            || self.pointer.is_some()
            || super::runtime::lock_unpoisoned(&self.layout_edit).is_active()
    }

    pub fn pointer_fast_tick(&self) -> bool {
        self.pointer.is_some()
            || self.menu_visible
            || self.router.active()
            || super::runtime::lock_unpoisoned(&self.layout_edit).is_active()
    }

    pub fn reset_dialogue_visibility(&mut self) {
        self.dialogue_fade.reset();
        self.dialogue_active = false;
        self.dialogue_retention = None;
        self.dialogue_pending = None;
        self.dialogue_revision = None;
        self.dialogue_hover = None;
    }

    fn dialogue_input_allowed(&self, current: DialogueRetentionKey) -> bool {
        self.dialogue_fade.input_allowed(self.dialogue_visible)
            && self
                .dialogue_retention
                .is_some_and(|retained| retained.policy == current.policy)
    }

    fn suspend_input(&mut self) {
        self.router.cancel();
        self.pointer_targets.clear();
        self.pointer_devices.clear();
        self.pointer = None;
        self.last_input = Instant::now();
        self.pressed = None;
        self.headset_hover = None;
        self.headset_pressed = None;
        self.toolbar_hover = None;
        self.toolbar_pressed = None;
        super::runtime::lock_unpoisoned(&self.placement).cancel_drag();
        super::runtime::lock_unpoisoned(&self.layout_edit).cancel_grab();
    }

    fn apply_context(
        &mut self,
        config: &VrOverlayConfig,
        owner_epoch: u32,
        owner_active: bool,
    ) -> bool {
        let changes = self.context.diff(config, owner_epoch, owner_active);
        if !changes.owner && !changes.menu && !changes.dialogue {
            return false;
        }
        if changes.owner {
            self.suspend_input();
            self.generation += 1;
        }
        if changes.menu {
            let _ = self.router.cancel_surface(Surface::Menu);
            self.pressed = None;
            self.menu_fade.reset();
            self.lift = Default::default();
            self.menu_ui = Default::default();
            self.menu_generation += 1;
        }
        if changes.dialogue {
            let _ = self.router.cancel_surface(Surface::Dialogue);
            self.reset_dialogue_visibility();
            self.dialogue_lift = Default::default();
            super::runtime::lock_unpoisoned(&self.placement).cancel();
            self.dialogue_generation += 1;
        }
        self.context.update(config, owner_epoch, owner_active);
        changes.owner
    }

    pub fn reset(&mut self) {
        self.router.cancel();
        self.pointer_targets.clear();
        self.pointer_devices.clear();
        self.pointer = None;
        self.cursor_error = None;
        self.last_input = Instant::now();
        self.generation += 1;
        self.menu_generation += 1;
        self.dialogue_generation += 1;
        self.origin = None;
        self.current_origin = None;
        self.snapshot_version = None;
        self.headset_hover = None;
        self.headset_pressed = None;
        self.toolbar_hover = None;
        self.toolbar_pressed = None;
        self.menu_frame = None;
        self.menu_ui = Default::default();
        self.menu_surface.reset();
        self.dialogue_surface.reset();
        self.menu_visible = false;
        self.dialogue_visible = false;
        self.dialogue_revision = None;
        self.dialogue_hover = None;
        self.lift = Default::default();
        self.dialogue_lift = Default::default();
        self.menu_fade.reset();
        self.reset_dialogue_visibility();
        self.pressed = None;
        super::runtime::lock_unpoisoned(&self.placement).reset_tracking();
    }

    pub fn place(
        &mut self,
        app: &AppHandle,
        backend: &Backend,
        controls: &Mutex<ControlHub>,
        mode: Mode,
    ) {
        let epoch = {
            let hub = super::runtime::lock_unpoisoned(controls);
            if hub.active() {
                hub.snapshot().owner_epoch
            } else {
                0
            }
        };
        if epoch == 0 {
            return;
        }
        let mut p = super::runtime::lock_unpoisoned(&self.placement);
        let pose = if mode == Mode::World {
            match backend.ocr_tracking() {
                Ok((head, _, _)) => {
                    p.update_head(head);
                    Some(dialogue::in_front(head))
                }
                Err(error) => {
                    p.error = Some(error);
                    return;
                }
            }
        } else {
            None
        };
        if let Some(request) = p.set_mode(mode, pose) {
            drop(p);
            dispatch_placement(app, epoch, &self.placement, request);
        }
    }

    fn probe_device_pointer(
        &self,
        backend: &Backend,
        layouts: &[Option<Arc<SurfaceLayout>>; 5],
        device: DeviceId,
        pointer: PointerId,
        scroll_y: f32,
    ) -> Option<Option<Pointer>> {
        let ray = backend.pointer_pose(pointer)?;
        if self.origin != Some(ray.space) {
            return None;
        }
        Some(nearest_pointer(
            self.pointer_targets
                .iter()
                .filter(|(_, _, bound)| *bound != Some(device.0))
                .filter_map(|&(kind, key, _)| {
                    let hit = backend.intersect(kind, device, ray)?;
                    Some(Pointer {
                        device: device.0,
                        key,
                        point: hit.point,
                        position: hit.position,
                        distance: hit.distance,
                        scroll_y,
                        origin: ray.space,
                        panel: hit.panel,
                        panel_width: hit.panel_width,
                    })
                }),
            layouts,
        ))
    }

    fn finish_pointer_selection(&mut self, pointers: Vec<Pointer>) {
        let candidates: Vec<_> = pointers.iter().map(|p| (p.device, p.distance)).collect();
        let device = self.router.select_pointer_device(&candidates);
        self.pointer = pointers.into_iter().find(|p| Some(p.device) == device);
    }

    fn sample_pointer(
        &mut self,
        backend: &Backend,
        layouts: &[Option<Arc<SurfaceLayout>>; 5],
    ) -> Vec<PointerInput> {
        let mut inputs = backend.pointer_inputs();
        let mut pointers = Vec::with_capacity(inputs.len());
        inputs.retain(|input| {
            match self.probe_device_pointer(
                backend,
                layouts,
                input.device,
                input.pointer,
                input.scroll_y,
            ) {
                None => false,
                Some(hit) => {
                    if let Some(pointer) = hit {
                        pointers.push(pointer);
                    }
                    true
                }
            }
        });
        self.pointer_devices.clear();
        self.pointer_devices
            .extend(inputs.iter().map(|input| (input.device, input.pointer)));
        self.finish_pointer_selection(pointers);
        inputs
    }

    pub fn sync_cursor(&mut self, app: &AppHandle, backend: &mut Backend) {
        let target = self.pointer.map(|p| {
            let (kind, surface) = pointer_overlay(p.key.surface);
            CursorTarget {
                kind,
                position: p.position,
                origin: p.origin,
                panel: p.panel,
                panel_width: p.panel_width,
                texture_width: surface.size().0,
            }
        });
        match backend.update_cursor(target) {
            Ok(()) => self.cursor_error = None,
            Err(error) => {
                if self.cursor_error.as_ref() != Some(&error) {
                    tracing::warn!(%error, "VR cursor unavailable");
                }
                self.cursor_error = Some(error);
            }
        }
        let slider_point = if self.menu_visible {
            self.pressed.and_then(|region| {
                let surfaces = app.state::<SurfaceHub>();
                let layout = surfaces.pointer_layout(SurfaceId::Menu)?;
                if !matches!(
                    layout.menu_command(region),
                    Some(wrist_menu::Command::Slider(_))
                ) {
                    return None;
                }
                self.pointer
                    .filter(|pointer| pointer.key.surface == Surface::Menu)
                    .map(|pointer| pointer.point)
            })
        } else {
            None
        };
        if let Err(error) = app
            .state::<SurfaceHub>()
            .point(app, SurfaceId::Menu, slider_point)
        {
            tracing::debug!(%error, "VR menu pointer unavailable");
        }
    }

    /// Updates only the shared cursor; panel content stays on the regular render tick.
    pub fn update_pointer(&mut self, app: &AppHandle, backend: &mut Backend) {
        if self.pointer_targets.is_empty()
            || self.pointer_devices.is_empty()
            || backend.system_dashboard_visible()
            || backend.ocr_tracking().is_err()
        {
            self.pointer = None;
        } else {
            let surfaces = app.state::<SurfaceHub>();
            let layouts = [
                surfaces.pointer_layout(SurfaceId::Menu),
                surfaces.pointer_layout(SurfaceId::Dialogue),
                surfaces.pointer_layout(SurfaceId::Reader),
                surfaces.pointer_layout(SurfaceId::Headset),
                surfaces.pointer_layout(SurfaceId::EditToolbar),
            ];
            let mut pointers = Vec::with_capacity(self.pointer_devices.len());
            for &(device, pointer) in &self.pointer_devices {
                if let Some(Some(pointer)) =
                    self.probe_device_pointer(backend, &layouts, device, pointer, 0.0)
                {
                    pointers.push(pointer);
                }
            }
            self.finish_pointer_selection(pointers);
        }
        self.sync_cursor(app, backend);
    }

    pub fn tick_with_cadence(
        &mut self,
        app: &AppHandle,
        backend: &mut Backend,
        config: &VrOverlayConfig,
        controls: &Mutex<ControlHub>,
        tick: UiTick,
    ) -> (Vec<LaserEvent>, bool) {
        let UiTick {
            reader_revision,
            content_owns_input,
            business,
            dialogue_sample,
            dialogue_content_active,
            dialogue_retention,
            menu_edit_overlay,
        } = tick;
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_input);
        if business {
            let hub = super::runtime::lock_unpoisoned(controls);
            let version = hub.ui_version();
            if self.snapshot_version != Some(version) {
                self.snapshot = Arc::new((
                    hub.snapshot(),
                    hub.player_display().clone(),
                    hub.player_display_revision(),
                    hub.active(),
                ));
                self.snapshot_version = Some(version);
            }
        }
        let snapshot = self.snapshot.clone();
        let (view, players, players_revision, owner_active) = snapshot.as_ref();
        let (players_revision, owner_active) = (
            *players_revision,
            *owner_active && super::runtime::lock_unpoisoned(controls).owns_epoch(view.owner_epoch),
        );
        let tracking = backend.ocr_tracking().ok();
        let origin = tracking.as_ref().map(|t| t.2);
        if backend.tracking_reset()
            || self.origin.is_some() && origin.is_some() && self.origin != origin
        {
            self.reset();
        }
        self.current_origin = origin;
        self.origin = origin.or(self.origin);
        let dashboard = backend.system_dashboard_visible();
        let owner_changed = self.apply_context(config, view.owner_epoch, owner_active);
        if !owner_changed && (dashboard || content_owns_input) {
            self.suspend_input();
        }
        let mut placement = super::runtime::lock_unpoisoned(&self.placement);
        if let Some((head, _, _)) = tracking {
            placement.update_head(head);
        }
        placement.sync_mode(Mode::from_config(&config.wrist.anchor_mode));
        placement.sync_follow_view(config.wrist.follow_view);
        if let Some((head, _, _)) = tracking {
            placement.initialize_world(dialogue::in_front(head));
        }
        if let Some(device) = placement.dragging() {
            if let Some(controller) = backend.controller_pose(DeviceId(device)) {
                placement.drag(device, controller.pose);
            } else {
                placement.cancel_drag();
            }
        }
        let head = tracking.map(|t| t.0);
        let (
            edit_active,
            edit_draft,
            edit_visible,
            edit_toolbar_pose,
            edit_grabbing_device,
            edit_grabbing_target,
        ) = {
            let mut edit = super::runtime::lock_unpoisoned(&self.layout_edit);
            if edit.is_active() {
                edit.ensure_toolbar_pose(head);
            }
            (
                edit.is_active(),
                edit.draft.clone(),
                edit.visible,
                edit.toolbar_pose,
                edit.grabbing_device(),
                edit.grabbing_target(),
            )
        };
        let active_menu_cfg = edit_draft
            .as_ref()
            .map(|d| &d.wrist_menu)
            .unwrap_or(&config.wrist_menu);
        let active_wrist_cfg = edit_draft
            .as_ref()
            .map(|d| &d.wrist)
            .unwrap_or(&config.wrist);
        let menu_wrist = menu_wrist_config(active_menu_cfg);
        let (menu_device, menu_angle) = backend.wrist_device_and_angle(&menu_wrist, head);
        let menu_angle = menu_angle.filter(|angle| angle.is_finite());
        if !edit_active && (menu_angle.is_none() || !config.wrist_menu.enabled) {
            self.menu_ui = Default::default();
            self.menu_fade.reset();
            self.lift = Default::default();
        }
        self.menu_ui.clamp(players);
        self.menu_visible = if edit_active {
            edit_visible[2] && menu_device.is_some()
        } else {
            config.wrist_menu.enabled && self.lift.update(menu_angle, self.pressed.is_some(), now)
        };
        let (dialogue_device, dialogue_angle) =
            backend.wrist_device_and_angle(active_wrist_cfg, head);
        let dialogue_angle = dialogue_angle.filter(|angle| angle.is_finite());
        self.dialogue_available = if edit_active {
            dialogue_device.is_some()
        } else {
            match placement.display_mode() {
                Mode::World => placement.absolute_pose().is_some() && tracking.is_some(),
                Mode::Wrist => dialogue_angle.is_some(),
            }
        };
        self.dialogue_occluded = if edit_active {
            false
        } else {
            placement.display_mode() == Mode::Wrist
                && !dialogue_sample
                && backend.visible(OverlayKind::OcrWrist)
                && backend.ocr_wrist_on_same_hand(&config.wrist)
        };
        if !self.dialogue_available {
            self.dialogue_lift = Default::default();
        }
        let dialogue_lift_visible = edit_active
            || placement.display_mode() == Mode::World
            || self
                .dialogue_lift
                .update(dialogue_angle, self.router.active(), now);
        self.dialogue_visible = if edit_active {
            edit_visible[1] && self.dialogue_available
        } else {
            config.enabled
                && config.wrist.enabled
                && self.dialogue_available
                && !self.dialogue_occluded
                && dialogue_content_active
                && (dialogue_sample || dialogue_lift_visible)
        };
        let surfaces = app.state::<SurfaceHub>();
        let generation = |surface| {
            let local = match surface {
                SurfaceId::Menu => self.menu_generation,
                SurfaceId::Dialogue => self.dialogue_generation,
                _ => self.generation,
            };
            (local << 32) | u64::from(surfaces.generation(surface))
        };
        let menu_layout = surfaces.layout(SurfaceId::Menu);
        let mut menu_controls = view.clone();
        // SteamVR owns its dashboard; only the OpenXR backend exposes the
        // application-owned settings quad from the wrist menu.
        if backend.kind() == BackendKind::OpenXr {
            menu_controls
                .labels
                .insert("settings_available".into(), "1".into());
        }
        let menu_view = business.then(|| SurfaceView::Menu {
            controls: menu_controls.clone(),
            players: players.clone(),
            ui: self.menu_ui.clone(),
            hold_progress: None,
            edit_mode: if edit_active {
                menu_edit_overlay.clone()
            } else {
                None
            },
        });
        let menu_key = Key {
            surface: Surface::Menu,
            generation: generation(SurfaceId::Menu),
            revision: if edit_active {
                1
            } else {
                menu_layout
                    .as_ref()
                    .map_or(0, |layout| u64::from(layout.key.revision))
            },
        };
        let dialogue_key = Key {
            surface: Surface::Dialogue,
            generation: generation(SurfaceId::Dialogue),
            revision: if edit_active { 1 } else { placement.revision },
        };
        let reader_key = Key {
            surface: Surface::Reader,
            generation: generation(SurfaceId::Reader),
            revision: reader_revision,
        };
        let headset_key = Key {
            surface: Surface::Headset,
            generation: generation(SurfaceId::Headset),
            revision: 1,
        };
        let toolbar_key = Key {
            surface: Surface::EditToolbar,
            generation: generation(SurfaceId::EditToolbar),
            revision: 1,
        };
        let mut keys = Vec::new();
        let dialogue_layout = surfaces.layout(SurfaceId::Dialogue);
        let reader_layout = surfaces.layout(SurfaceId::Reader);
        let headset_layout = surfaces.layout(SurfaceId::Headset);
        let toolbar_layout = surfaces.layout(SurfaceId::EditToolbar);
        let pointer_layouts = [
            surfaces.pointer_layout(SurfaceId::Menu),
            surfaces.pointer_layout(SurfaceId::Dialogue),
            surfaces.pointer_layout(SurfaceId::Reader),
            surfaces.pointer_layout(SurfaceId::Headset),
            surfaces.pointer_layout(SurfaceId::EditToolbar),
        ];
        let menu_input_ready = if edit_active {
            self.menu_visible
        } else {
            self.menu_fade.input_allowed(self.menu_visible)
        };
        let dialogue_input_ready = if edit_active {
            self.dialogue_visible
        } else {
            self.dialogue_input_allowed(dialogue_retention)
        };
        if edit_active {
            if self.menu_visible && backend.visible(OverlayKind::WristMenu) && menu_layout.is_some()
            {
                keys.push(menu_key);
            }
            if self.dialogue_visible
                && backend.visible(OverlayKind::Wrist)
                && dialogue_layout.is_some()
            {
                keys.push(dialogue_key);
            }
            if edit_visible[0] && backend.visible(OverlayKind::Headset) && headset_layout.is_some()
            {
                keys.push(headset_key);
            }
            if edit_toolbar_pose.is_some()
                && backend.visible(OverlayKind::EditToolbar)
                && toolbar_layout.is_some()
            {
                keys.push(toolbar_key);
            }
        } else {
            if menu_input_ready
                && menu_layout.is_some()
                && self.menu_frame.as_ref().is_some_and(|frame| {
                    menu_view
                        .as_ref()
                        .is_none_or(|view| frame.same_layout(view))
                })
            {
                keys.push(menu_key);
            }
            if dialogue_input_ready
                && backend.visible(OverlayKind::Wrist)
                && self.dialogue_revision == Some(placement.revision)
                && (dialogue_layout.is_some() || placement.dragging().is_some())
            {
                keys.push(dialogue_key);
            }
            if backend.visible(OverlayKind::OcrWrist) && reader_layout.is_some() {
                keys.push(reader_key);
            }
        }
        let input_allowed = !dashboard && !content_owns_input && owner_active && tracking.is_some();
        if !input_allowed {
            keys.clear();
        }
        self.pointer_targets = if edit_active {
            [
                (OverlayKind::WristMenu, menu_key, menu_device.map(|d| d.0)),
                (
                    OverlayKind::Wrist,
                    dialogue_key,
                    dialogue_device.map(|d| d.0),
                ),
                (OverlayKind::Headset, headset_key, None),
                (OverlayKind::EditToolbar, toolbar_key, None),
            ]
            .into_iter()
            .filter(|(kind, key, _)| {
                let idx = surface_index(key.surface);
                input_allowed && backend.visible(*kind) && pointer_layouts[idx].is_some()
            })
            .collect()
        } else {
            let pointer_visible = [
                menu_input_ready && backend.visible(OverlayKind::WristMenu),
                dialogue_input_ready
                    && backend.visible(OverlayKind::Wrist)
                    && self.dialogue_revision == Some(placement.revision),
                backend.visible(OverlayKind::OcrWrist),
            ];
            [
                (
                    OverlayKind::WristMenu,
                    menu_key,
                    menu_device.map(|device| device.0),
                ),
                (
                    OverlayKind::Wrist,
                    dialogue_key,
                    (placement.display_mode() == Mode::Wrist)
                        .then_some(dialogue_device)
                        .flatten()
                        .map(|device| device.0),
                ),
                (OverlayKind::OcrWrist, reader_key, None),
            ]
            .into_iter()
            .enumerate()
            .filter(|(index, _)| {
                input_allowed && pointer_visible[*index] && pointer_layouts[*index].is_some()
            })
            .map(|(_, target)| target)
            .collect()
        };
        let can_recenter = !edit_active
            && placement.display_mode() == Mode::World
            && !placement.busy()
            && view.items[0].requested;
        // The placement guard is needed again for action dispatch, after pointer sampling.
        drop(placement);
        let inputs = self.sample_pointer(backend, &pointer_layouts);
        let pointer = self.pointer;
        let active_slider = if edit_active {
            None
        } else {
            self.pressed.filter(|&region| {
                menu_layout.as_ref().is_some_and(|layout| {
                    matches!(
                        layout.menu_command(region),
                        Some(wrist_menu::Command::Slider(_))
                    )
                })
            })
        };
        let samples: Vec<_> = inputs
            .iter()
            .map(|input| {
                let hit = pointer
                    .filter(|p| p.device == input.device.0)
                    .and_then(|p| {
                        let region = if edit_active {
                            match p.key.surface {
                                Surface::Menu => match menu_layout.as_ref()?.hit(p.point) {
                                    Some(60) => 60,
                                    _ => 0,
                                },
                                Surface::Dialogue => {
                                    if dialogue_layout.as_ref()?.hit(p.point) == Some(1) {
                                        1
                                    } else {
                                        0
                                    }
                                }
                                Surface::Headset => {
                                    if headset_layout.as_ref()?.hit(p.point) == Some(1) {
                                        1
                                    } else {
                                        0
                                    }
                                }
                                Surface::EditToolbar => {
                                    toolbar_layout.as_ref()?.hit(p.point).unwrap_or(0)
                                }
                                Surface::Reader => return None,
                            }
                        } else {
                            match p.key.surface {
                                Surface::Menu => {
                                    active_slider.or_else(|| menu_layout.as_ref()?.hit(p.point))?
                                }
                                Surface::Dialogue => dialogue_layout.as_ref()?.hit(p.point)?,
                                Surface::Reader => reader_layout.as_ref()?.hit(p.point)?,
                                Surface::Headset | Surface::EditToolbar => return None,
                            }
                        };
                        Some(Hit {
                            key: p.key,
                            region,
                            point: p.point,
                            distance: p.distance,
                        })
                    });
                Sample {
                    device: input.device.0,
                    trigger: input.trigger,
                    grip: input.grip,
                    hit,
                }
            })
            .collect();
        let was_active = self.router.active();
        let mut output = self.router.step_mode(&samples, &keys, edit_active);
        let now = Instant::now();
        if can_recenter && let Some(event) = self.router.long_press(now) {
            output.events.push(event);
        }
        let hold_progress = can_recenter
            .then(|| self.router.hold_progress(now))
            .flatten();
        let ui_focus = was_active || self.router.active() || pointer.is_some();
        let mut events = Vec::new();
        let hovered = output
            .hover
            .filter(|(_, h)| h.key.surface == Surface::Menu)
            .map(|(_, h)| h.region);
        self.dialogue_hover = output
            .hover
            .filter(|(_, h)| h.key.surface == Surface::Dialogue)
            .map(|(_, h)| h.region);
        self.headset_hover = output
            .hover
            .filter(|(_, h)| h.key.surface == Surface::Headset)
            .map(|(_, h)| h.region);
        self.toolbar_hover = output
            .hover
            .filter(|(_, h)| h.key.surface == Surface::EditToolbar)
            .map(|(_, h)| h.region);
        self.last_input = now;
        if let Some(pointer) =
            pointer.filter(|p| p.key.surface == Surface::Reader && keys.contains(&p.key))
        {
            events.push(LaserEvent::Move {
                device: pointer.device,
                point: pointer.point.map(|v| v * SurfaceId::Reader.size().0 as f32),
            });
        } else {
            events.push(LaserEvent::Leave);
        }
        if let Some(pointer) = pointer.filter(|p| input_allowed && p.key.surface == Surface::Reader)
            && let Err(error) = surfaces.scroll_reader(app, scroll_delta(pointer.scroll_y, elapsed))
        {
            tracing::debug!(%error, "VR reader scroll unavailable");
        }
        if let Some(pointer) = pointer.filter(|p| {
            input_allowed
                && p.key.surface == Surface::Dialogue
                && !was_active
                && !self.router.active()
        }) && let Err(error) = surfaces.scroll(
            app,
            SurfaceId::Dialogue,
            scroll_delta(pointer.scroll_y, elapsed),
        ) {
            tracing::debug!(%error, "VR dialogue scroll unavailable");
        }
        let mut save = None;
        let mut action = None;
        if edit_active {
            let menu_anchor = menu_device.and_then(|d| backend.controller_pose(d).map(|p| p.pose));
            let wrist_anchor =
                dialogue_device.and_then(|d| backend.controller_pose(d).map(|p| p.pose));
            let mut commit_req = None;
            {
                let mut edit = super::runtime::lock_unpoisoned(&self.layout_edit);
                if let Some(grab_dev) = edit_grabbing_device {
                    if let Some(ctrl) = backend.controller_pose(DeviceId(grab_dev)).map(|p| p.pose)
                    {
                        let input = inputs.iter().find(|i| i.device.0 == grab_dev);
                        let ray = input
                            .and_then(|i| backend.pointer_pose(i.pointer))
                            .map(|p| p.pose);
                        let scroll = input.map_or(0.0, |i| i.scroll_y);
                        let anchor = match edit_grabbing_target {
                            Some(GrabTarget::Component(EditTarget::Wrist)) => wrist_anchor,
                            Some(GrabTarget::Component(EditTarget::WristMenu)) => menu_anchor,
                            _ => None,
                        };
                        edit.update_drag(
                            grab_dev,
                            ctrl,
                            ray,
                            head,
                            anchor,
                            scroll,
                            elapsed.as_secs_f32(),
                        );
                    } else {
                        edit.cancel_grab();
                    }
                }
                for event in output.events {
                    let ray_pose = |device: u32| -> Option<Pose> {
                        inputs
                            .iter()
                            .find(|i| i.device.0 == device)
                            .and_then(|i| backend.pointer_pose(i.pointer))
                            .map(|p| p.pose)
                    };
                    let ctrl_pose = |device: u32| -> Option<Pose> {
                        backend.controller_pose(DeviceId(device)).map(|p| p.pose)
                    };
                    match event.kind {
                        Kind::Grab => match (event.hit.key.surface, event.hit.region) {
                            (Surface::Headset, 1) => {
                                self.headset_pressed = Some(1);
                                if let (Some(ray), Some(panel)) = (
                                    ray_pose(event.device),
                                    head.and_then(|h| edit.headset_world_pose(h)),
                                ) {
                                    edit.begin_resize(
                                        EditTarget::Headset,
                                        event.device,
                                        ray,
                                        panel,
                                    );
                                }
                            }
                            (Surface::Headset, _) => {
                                self.headset_pressed = Some(0);
                                if let (Some(ctrl), Some(panel)) = (
                                    ctrl_pose(event.device),
                                    head.and_then(|h| edit.headset_world_pose(h)),
                                ) {
                                    edit.begin_move(
                                        GrabTarget::Component(EditTarget::Headset),
                                        event.device,
                                        ctrl,
                                        panel,
                                    );
                                }
                            }
                            (Surface::Dialogue, 1) => {
                                if let (Some(ray), Some(panel)) = (
                                    ray_pose(event.device),
                                    wrist_anchor.and_then(|a| edit.wrist_world_pose(a)),
                                ) {
                                    edit.begin_resize(EditTarget::Wrist, event.device, ray, panel);
                                }
                            }
                            (Surface::Dialogue, _) => {
                                if let (Some(ctrl), Some(panel)) = (
                                    ctrl_pose(event.device),
                                    wrist_anchor.and_then(|a| edit.wrist_world_pose(a)),
                                ) {
                                    edit.begin_move(
                                        GrabTarget::Component(EditTarget::Wrist),
                                        event.device,
                                        ctrl,
                                        panel,
                                    );
                                }
                            }
                            (Surface::Menu, 60) => {
                                self.pressed = Some(60);
                                if let (Some(ray), Some(panel)) = (
                                    ray_pose(event.device),
                                    menu_anchor.and_then(|a| edit.wrist_menu_world_pose(a)),
                                ) {
                                    edit.begin_resize(
                                        EditTarget::WristMenu,
                                        event.device,
                                        ray,
                                        panel,
                                    );
                                }
                            }
                            (Surface::Menu, _) => {
                                self.pressed = Some(0);
                                if let (Some(ctrl), Some(panel)) = (
                                    ctrl_pose(event.device),
                                    menu_anchor.and_then(|a| edit.wrist_menu_world_pose(a)),
                                ) {
                                    edit.begin_move(
                                        GrabTarget::Component(EditTarget::WristMenu),
                                        event.device,
                                        ctrl,
                                        panel,
                                    );
                                }
                            }
                            (Surface::EditToolbar, _) => {
                                self.toolbar_pressed = Some(0);
                                if let (Some(ctrl), Some(panel)) =
                                    (ctrl_pose(event.device), edit.toolbar_pose)
                                {
                                    edit.begin_move(GrabTarget::Toolbar, event.device, ctrl, panel);
                                }
                            }
                            _ => {}
                        },
                        Kind::Release => {
                            self.pressed = None;
                            self.headset_pressed = None;
                            self.toolbar_pressed = None;
                            edit.end_grab(event.device, head);
                        }
                        Kind::Cancel => {
                            self.pressed = None;
                            self.headset_pressed = None;
                            self.toolbar_pressed = None;
                            if edit.grabbing_device() == Some(event.device) {
                                edit.cancel_grab();
                            }
                        }
                        Kind::Down if event.hit.key.surface == Surface::EditToolbar => {
                            self.toolbar_pressed = Some(event.hit.region);
                        }
                        Kind::Up if event.hit.key.surface == Surface::EditToolbar => {
                            self.toolbar_pressed = None;
                            let act = match event.hit.region {
                                1 => Some(EditToolbarAction::ToggleAutoLevel),
                                2 => Some(EditToolbarAction::ToggleFaceHead),
                                3 => Some(EditToolbarAction::ToggleVisible(EditTarget::Headset)),
                                4 => Some(EditToolbarAction::ToggleVisible(EditTarget::Wrist)),
                                5 => Some(EditToolbarAction::ToggleVisible(EditTarget::WristMenu)),
                                6 => Some(EditToolbarAction::ResetSelected),
                                7 => Some(EditToolbarAction::ResetAll),
                                8 => Some(EditToolbarAction::Cancel),
                                9 => Some(EditToolbarAction::Save),
                                _ => None,
                            };
                            match act {
                                Some(EditToolbarAction::Save) => {
                                    if let Ok(req) = edit.begin_commit(view.owner_epoch) {
                                        commit_req = req;
                                    }
                                }
                                Some(other) => edit.apply_toolbar_action(other),
                                None => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
            if let Some(req) = commit_req {
                let req_id = req.id;
                if let Err(error) = app.emit_to("main", EDIT_TOOLBAR_COMMIT_EVENT, req) {
                    super::runtime::lock_unpoisoned(&self.layout_edit)
                        .resolve_commit(req_id, Some(error.to_string()));
                }
            }
        } else {
            let mut placement = super::runtime::lock_unpoisoned(&self.placement);
            for event in output.events {
                match event.hit.key.surface {
                    Surface::Menu => match event.kind {
                        Kind::Down => self.pressed = Some(event.hit.region),
                        Kind::LongPress => {
                            save = placement.set_mode(
                                Mode::World,
                                tracking.map(|(head, _, _)| dialogue::in_front(head)),
                            );
                        }
                        Kind::Up => {
                            self.pressed = None;
                            action = menu_layout.as_ref().and_then(|layout| {
                                layout.menu_action(event.hit.region, event.hit.point)
                            });
                        }
                        _ => self.pressed = None,
                    },
                    Surface::Dialogue => match event.kind {
                        Kind::Grab if !placement.busy() => {
                            if let (Some(controller), Some(panel)) = (
                                backend
                                    .controller_pose(DeviceId(event.device))
                                    .map(|pose| pose.pose),
                                placement
                                    .absolute_pose()
                                    .or_else(|| backend.wrist_pose(&config.wrist)),
                            ) {
                                placement.grab(event.device, controller, panel);
                            }
                        }
                        Kind::Release => save = placement.release(event.device),
                        Kind::Cancel => placement.cancel_drag(),
                        Kind::Up if !placement.busy() => match event.hit.region {
                            1 => save = placement.set_mode(Mode::Wrist, None),
                            2 => {
                                let pose = placement
                                    .absolute_pose()
                                    .or_else(|| backend.wrist_pose(&config.wrist));
                                save = placement.set_mode(Mode::World, pose);
                            }
                            3 => {
                                let enabled = !placement.follow_view();
                                save = placement.set_follow_view(enabled);
                            }
                            _ => {}
                        },
                        _ => {}
                    },
                    Surface::Reader => {
                        let point = event
                            .hit
                            .point
                            .map(|v| v * SurfaceId::Reader.size().0 as f32);
                        events.push(match event.kind {
                            Kind::Down => LaserEvent::Down {
                                device: event.device,
                                point,
                            },
                            Kind::Up => LaserEvent::Up {
                                device: event.device,
                                point,
                            },
                            _ => LaserEvent::Leave,
                        });
                    }
                    Surface::Headset | Surface::EditToolbar => {}
                }
            }
        }
        if let Some(request) = save {
            dispatch_placement(app, view.owner_epoch, &self.placement, request);
        }
        if let Some(action) = action {
            match action {
                wrist_menu::Command::Control(action) => super::runtime::dispatch_control(
                    app,
                    controls,
                    ControlSource::WristMenu,
                    action,
                ),
                wrist_menu::Command::Display(action) => super::runtime::dispatch_player_display(
                    app,
                    controls,
                    ControlSource::WristMenu,
                    (view.owner_epoch, players_revision),
                    action,
                ),
                wrist_menu::Command::Dashboard => {
                    if let Err(error) = backend.toggle_dashboard() {
                        tracing::debug!(%error, "VR settings toggle unavailable");
                    }
                }
                wrist_menu::Command::EditResize => {}
                navigation => {
                    self.menu_ui.apply(&navigation, players);
                }
            }
        }
        // Push the cursor pose to SteamVR before rebuilding any panel textures.
        self.sync_cursor(app, backend);
        let result = (|| -> Result<(), String> {
            let ready = if self.menu_visible && (business || edit_active) {
                let frame = SurfaceView::Menu {
                    controls: menu_controls.clone(),
                    players: players.clone(),
                    ui: self.menu_ui.clone(),
                    hold_progress,
                    edit_mode: if edit_active { menu_edit_overlay } else { None },
                };
                backend.ensure_wrist_menu(active_menu_cfg)?;
                let ready = self.menu_surface.present(
                    app,
                    backend,
                    OverlayKind::WristMenu,
                    frame.clone(),
                    hovered,
                    self.pressed,
                )?;
                self.menu_frame = Some(frame);
                ready
            } else if self.menu_visible {
                self.menu_surface.refresh(
                    app,
                    backend,
                    OverlayKind::WristMenu,
                    Some((hovered, self.pressed, hold_progress)),
                )?
            } else if self.menu_fade.active() {
                self.menu_surface
                    .hold_for_fade(app, backend, OverlayKind::WristMenu)
            } else {
                false
            };
            let ready = ready && (self.menu_fade.active() || self.menu_surface.current_frame(app));
            let fade = self.menu_fade.advance(self.menu_visible, ready, now);
            if ready && (edit_active || fade.factor > 0.0 || fade.animating) {
                backend.set_opacity(
                    OverlayKind::WristMenu,
                    if edit_active { 1.0 } else { fade.factor },
                )?;
                backend.show(OverlayKind::WristMenu)?;
            } else {
                backend.hide(OverlayKind::WristMenu);
            }
            if !self.menu_visible && !fade.animating {
                self.menu_surface
                    .stop(app, Some(backend), OverlayKind::WristMenu);
            }
            Ok(())
        })();
        if let Err(error) = result {
            backend.reset(OverlayKind::WristMenu);
            self.menu_fade.reset();
            self.menu_frame = None;
            self.menu_ui = Default::default();
            if self.menu_error.as_ref() != Some(&error) {
                tracing::warn!(%error,"Wrist menu unavailable");
            }
            self.menu_error = Some(error);
        } else {
            self.menu_error = None;
        }
        (events, ui_focus)
    }
}

#[cfg(test)]
mod tests {
    use super::super::surface_protocol;
    use super::*;

    #[test]
    fn dialogue_input_survives_content_updates_but_not_policy_or_visibility_changes() {
        let mut ui = UiRuntime::new(
            Arc::new(Mutex::new(Placement::default())),
            Arc::new(Mutex::new(LayoutEditSession::default())),
        );
        let now = Instant::now();
        let key = DialogueRetentionKey {
            content: 1,
            policy: 1,
        };
        ui.dialogue_visible = true;
        ui.dialogue_retention = Some(key);
        ui.dialogue_fade.advance(true, true, now);
        assert!(!ui.dialogue_input_allowed(key));
        ui.dialogue_fade
            .advance(true, true, now + Duration::from_millis(160));
        assert!(ui.dialogue_input_allowed(DialogueRetentionKey { content: 2, ..key }));
        assert!(!ui.dialogue_input_allowed(DialogueRetentionKey { policy: 2, ..key }));
        ui.dialogue_visible = false;
        assert!(!ui.dialogue_input_allowed(key));
        ui.reset_dialogue_visibility();
        assert!(!ui.fast_tick());
    }

    #[test]
    fn menu_pointer_covers_visible_capsule() {
        let key = Key {
            surface: Surface::Menu,
            generation: (1 << 32) | 1,
            revision: 2,
        };
        let mut layout = SurfaceLayout {
            key: surface_protocol::SurfaceKey {
                generation: 1,
                revision: 7,
            },
            regions: vec![surface_protocol::Region {
                id: 0,
                bounds: [0.4375, 0.8375, 0.5625, 0.9625],
                circle: true,
                dashboard: None,
                menu: Some(wrist_menu::Command::Control(
                    super::super::control_protocol::ControlAction::SetWristEnabled(true),
                )),
            }],
            menu_bounds: Some([0.0125, 0.80625, 0.9875, 0.99375]),
            panels: Vec::new(),
            page_count: 1,
            unplaced: Vec::new(),
            capture_bounds: None,
        };
        layout.validate(SurfaceId::Menu).unwrap();

        let menu = Pointer {
            device: 2,
            key,
            point: [0.5, 0.9],
            position: [0., 0., -1.],
            distance: 1.,
            scroll_y: 0.,
            origin: SpaceId(1),
            panel: [[0.; 4]; 3],
            panel_width: 1.,
        };
        let reader = Pointer {
            key: Key {
                surface: Surface::Reader,
                ..key
            },
            point: [0.5, 0.5],
            distance: 2.,
            ..menu
        };
        let reader_layout = serde_json::from_value(serde_json::json!({
            "key": { "generation": 1, "revision": 1 },
            "regions": [],
            "panels": [{ "bounds": [0.1, 0.1, 0.9, 0.9], "radius": [0.03, 0.03] }],
            "page_count": 1
        }))
        .unwrap();
        let mut layouts = [
            Some(Arc::new(layout.clone())),
            None,
            Some(Arc::new(reader_layout)),
            None,
            None,
        ];
        let selected = nearest_pointer([menu, reader].into_iter(), &layouts).unwrap();
        assert_eq!(selected.key.surface, Surface::Menu);

        for point in [[0.5, 0.98], [0.5, 0.82], [0.25, 0.9], [0.04, 0.9]] {
            assert!(layout.hit(point).is_none());
            let background = Pointer { point, ..menu };
            let selected = nearest_pointer([background, reader].into_iter(), &layouts).unwrap();
            assert_eq!(selected.key.surface, Surface::Menu, "{point:?}");
        }
        for point in [[0.01, 0.9], [0.5, 0.999], [0.02, 0.81], [0.98, 0.98]] {
            let outside = Pointer { point, ..menu };
            assert!(nearest_pointer([outside].into_iter(), &layouts).is_none());
            let selected = nearest_pointer([outside, reader].into_iter(), &layouts).unwrap();
            assert_eq!(selected.key.surface, Surface::Reader);
        }
        assert!(nearest_pointer([menu].into_iter(), &[None, None, None, None, None]).is_none());
        assert!(nearest_pointer([reader].into_iter(), &[None, None, None, None, None]).is_none());
        layout.regions.clear();
        layouts[0] = Some(Arc::new(layout));
        assert!(nearest_pointer([menu].into_iter(), &layouts).is_some());
    }

    #[test]
    fn dialogue_transparent_margins_and_gaps_do_not_cover_the_reader() {
        let layout: SurfaceLayout = serde_json::from_value(serde_json::json!({
            "key": { "generation": 1, "revision": 1 },
            "regions": [],
            "panels": [
                { "bounds": [0.01, 0.01, 0.99, 0.125], "radius": [0.0575, 0.0575] },
                { "bounds": [0.02, 0.145, 0.98, 0.98], "radius": [0.04, 0.04] }
            ],
            "page_count": 1
        }))
        .unwrap();
        let dialogue = Pointer {
            device: 2,
            key: Key {
                surface: Surface::Dialogue,
                generation: (1 << 32) | 1,
                revision: 1,
            },
            point: [0.5, 0.5],
            position: [0., 0., -1.],
            distance: 1.,
            scroll_y: 0.,
            origin: SpaceId(1),
            panel: [[0.; 4]; 3],
            panel_width: 1.,
        };
        let reader = Pointer {
            key: Key {
                surface: Surface::Reader,
                ..dialogue.key
            },
            distance: 2.,
            ..dialogue
        };
        let reader_layout = serde_json::from_value(serde_json::json!({
            "key": { "generation": 1, "revision": 1 },
            "regions": [],
            "panels": [{ "bounds": [0.1, 0.1, 0.9, 0.9], "radius": [0.03, 0.03] }],
            "page_count": 1
        }))
        .unwrap();
        let layouts = [
            None,
            Some(Arc::new(layout)),
            Some(Arc::new(reader_layout)),
            None,
            None,
        ];
        for point in [[0.005, 0.5], [0.5, 0.135], [0.011, 0.011], [0.021, 0.146]] {
            let front = Pointer { point, ..dialogue };
            let selected = nearest_pointer([front, reader].into_iter(), &layouts).unwrap();
            assert_eq!(selected.key.surface, Surface::Reader, "{point:?}");
        }
        for point in [[0.5, 0.05], [0.5, 0.5], [0.05, 0.5]] {
            let front = Pointer { point, ..dialogue };
            let selected = nearest_pointer([front, reader].into_iter(), &layouts).unwrap();
            assert_eq!(selected.key.surface, Surface::Dialogue, "{point:?}");
        }
    }

    #[test]
    fn reader_scroll_is_time_based_and_caps_stalled_ticks() {
        let at_60_hz: f32 = (0..60)
            .map(|_| scroll_delta(1.0, Duration::from_secs_f32(1.0 / 60.0)))
            .sum();
        let at_90_hz: f32 = (0..90)
            .map(|_| scroll_delta(1.0, Duration::from_secs_f32(1.0 / 90.0)))
            .sum();
        assert!((at_60_hz + 1.5).abs() < 0.001);
        assert!((at_60_hz - at_90_hz).abs() < 0.001);
        assert!(scroll_delta(-0.5, Duration::from_millis(16)) > 0.0);
        assert_eq!(scroll_delta(0.0, Duration::from_secs(1)), 0.0);
        assert!((scroll_delta(1.0, Duration::from_secs(1)) + 0.075).abs() < 0.001);
    }

    #[test]
    fn suspended_input_keeps_submitted_placement_until_its_receipt() {
        for success in [false, true] {
            let placement = Arc::new(Mutex::new(Placement::default()));
            let mut runtime = UiRuntime::new(
                placement.clone(),
                Arc::new(Mutex::new(LayoutEditSession::default())),
            );
            let pose = super::super::transform::matrix(0., 20., 0., [1., 1., -2.]);
            let request = placement
                .lock()
                .unwrap()
                .set_mode(Mode::World, Some(pose))
                .unwrap();
            runtime.suspend_input();
            runtime.suspend_input();
            let mut p = placement.lock().unwrap();
            assert!(p.busy());
            assert_eq!(p.absolute_pose(), Some(pose));
            assert!(p.resolve(
                request.id,
                success,
                (!success).then(|| "Save failed".into())
            ));
            assert!(!p.busy());
            assert_eq!(p.mode(), if success { Mode::World } else { Mode::Wrist });
            assert_eq!(p.error.is_some(), !success);
        }
    }

    #[test]
    fn context_changes_reset_only_the_affected_surface() {
        let placement = Arc::new(Mutex::new(Placement::default()));
        let mut runtime = UiRuntime::new(
            placement.clone(),
            Arc::new(Mutex::new(LayoutEditSession::default())),
        );
        let mut config = VrOverlayConfig {
            enabled: true,
            ..Default::default()
        };
        config.wrist_menu.enabled = true;
        let now = Instant::now();

        runtime.apply_context(&config, 1, true);
        let menu_gen = runtime.menu_generation;
        let dialogue_gen = runtime.dialogue_generation;

        assert!(!runtime.lift.update(Some(30.), false, now));
        assert!(
            runtime
                .lift
                .update(Some(30.), false, now + Duration::from_millis(150))
        );
        runtime.menu_fade.advance(true, true, now);
        runtime
            .menu_fade
            .advance(true, true, now + Duration::from_millis(160));
        runtime.menu_ui.page = wrist_menu::Page::Players;
        runtime.dialogue_fade.advance(true, true, now);
        runtime
            .dialogue_fade
            .advance(true, true, now + Duration::from_millis(160));
        let pose = super::super::transform::matrix(0., 20., 0., [1., 1., -2.]);
        let _ = placement.lock().unwrap().set_mode(Mode::World, Some(pose));

        // Toggling wrist subtitles resets only dialogue state and keeps the wrist menu intact.
        config.wrist.enabled = false;
        assert!(!runtime.apply_context(&config, 1, true));
        assert_eq!(runtime.menu_generation, menu_gen);
        assert_eq!(runtime.menu_ui.page, wrist_menu::Page::Players);
        assert!(runtime.menu_fade.input_allowed(true));
        assert!(
            runtime
                .lift
                .update(Some(30.), false, now + Duration::from_millis(200))
        );
        assert!(runtime.dialogue_generation > dialogue_gen);
        assert!(!runtime.dialogue_fade.active());
        assert!(!placement.lock().unwrap().busy());

        // Changing wrist menu hand resets only menu state and keeps dialogue intact.
        let menu_gen = runtime.menu_generation;
        let dialogue_gen = runtime.dialogue_generation;
        runtime
            .dialogue_fade
            .advance(true, true, now + Duration::from_millis(300));
        config.wrist_menu.hand = "right".into();
        assert!(!runtime.apply_context(&config, 1, true));
        assert!(runtime.menu_generation > menu_gen);
        assert_eq!(runtime.menu_ui.page, wrist_menu::Page::Home);
        assert!(!runtime.menu_fade.active());
        assert!(
            !runtime
                .lift
                .update(Some(30.), false, now + Duration::from_millis(350))
        );
        assert_eq!(runtime.dialogue_generation, dialogue_gen);
        assert!(runtime.dialogue_fade.active());
    }
}
