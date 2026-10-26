use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager as _};
use vrcs_core::VrOverlayConfig;

use super::backend::{menu_config, OpenVrBackend, OverlayKind};
use super::control_protocol::{ControlHub, ControlSource};
use super::interaction::{Hit, Key, Kind, Router, Sample, Surface};
use super::ocr_wrist::LaserEvent;
use super::placement::{Mode, Placement, Request};
use super::surface::{SurfaceHub, SurfaceId, SurfaceLayout, SurfaceRenderer, SurfaceView};
use super::{dialogue, wrist_menu};

#[derive(Clone, Copy)]
struct Pointer {
    device: u32,
    key: Key,
    point: [f32; 2],
    position: [f32; 3],
    distance: f32,
    scroll_y: f32,
    origin: i32,
}

fn pointer_overlay(surface: Surface) -> (OverlayKind, SurfaceId) {
    match surface {
        Surface::Menu => (OverlayKind::WristMenu, SurfaceId::Menu),
        Surface::Dialogue => (OverlayKind::Wrist, SurfaceId::Dialogue),
        Surface::Reader => (OverlayKind::OcrWrist, SurfaceId::Reader),
    }
}

fn nearest_pointer(
    pointers: impl Iterator<Item = Pointer>,
    layouts: &[Option<SurfaceLayout>; 3],
) -> Option<Pointer> {
    pointers
        .filter(|pointer| {
            let index = match pointer.key.surface {
                Surface::Menu => 0,
                Surface::Dialogue => 1,
                Surface::Reader => 2,
            };
            layouts[index]
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
        if let Ok(mut p) = placement.lock() {
            p.resolve(request.id, false, error);
        }
    }
}

pub struct UiRuntime {
    pub placement: Arc<Mutex<Placement>>,
    router: Router,
    lift: wrist_menu::Visibility,
    dialogue_lift: wrist_menu::Visibility,
    menu_frame: Option<SurfaceView>,
    menu_ui: wrist_menu::Ui,
    pub menu_visible: bool,
    pub dialogue_hover: Option<u8>,
    pub dialogue_visible: bool,
    pub dialogue_revision: Option<u64>,
    pub generation: u64,
    context: (String, String, u32, bool, bool, bool),
    origin: Option<i32>,
    pressed: Option<u8>,
    menu_error: Option<String>,
    last_input: Instant,
    pointer_targets: Vec<(OverlayKind, Key, Option<u32>)>,
    pointer: Option<Pointer>,
    cursor_error: Option<String>,
    menu_surface: SurfaceRenderer,
    pub dialogue_surface: SurfaceRenderer,
}

impl UiRuntime {
    pub fn new(placement: Arc<Mutex<Placement>>) -> Self {
        Self {
            placement,
            router: Router::default(),
            lift: Default::default(),
            dialogue_lift: Default::default(),
            menu_frame: None,
            menu_ui: Default::default(),
            menu_visible: false,
            dialogue_hover: None,
            dialogue_visible: false,
            dialogue_revision: None,
            generation: 1,
            context: Default::default(),
            origin: None,
            pressed: None,
            menu_error: None,
            last_input: Instant::now(),
            pointer_targets: Vec::new(),
            pointer: None,
            cursor_error: None,
            menu_surface: SurfaceRenderer::new(SurfaceId::Menu),
            dialogue_surface: SurfaceRenderer::new(SurfaceId::Dialogue),
        }
    }

    pub fn finish_surfaces(&mut self, app: &AppHandle, mut backend: Option<&mut OpenVrBackend>) {
        self.menu_surface
            .finish_tick(app, backend.as_deref_mut(), OverlayKind::WristMenu);
        self.dialogue_surface
            .finish_tick(app, backend, OverlayKind::Wrist);
    }

    pub fn stop_surfaces(&mut self, app: &AppHandle, mut backend: Option<&mut OpenVrBackend>) {
        self.menu_surface
            .stop(app, backend.as_deref_mut(), OverlayKind::WristMenu);
        self.dialogue_surface.stop(app, backend, OverlayKind::Wrist);
    }

    pub fn fast_tick(&self) -> bool {
        self.menu_visible || self.dialogue_visible || self.router.active()
    }

    fn suspend_input(&mut self) {
        self.router.cancel();
        self.pointer_targets.clear();
        self.pointer = None;
        self.last_input = Instant::now();
        self.pressed = None;
        if let Ok(mut p) = self.placement.lock() {
            p.cancel_drag();
        }
    }

    pub fn reset(&mut self) {
        self.router.cancel();
        self.pointer_targets.clear();
        self.pointer = None;
        self.cursor_error = None;
        self.last_input = Instant::now();
        self.generation += 1;
        self.origin = None;
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
        self.pressed = None;
        if let Ok(mut p) = self.placement.lock() {
            p.reset_tracking();
        }
    }

    pub fn place(
        &mut self,
        app: &AppHandle,
        backend: &OpenVrBackend,
        controls: &Mutex<ControlHub>,
        mode: Mode,
    ) {
        let epoch = controls
            .lock()
            .ok()
            .filter(|hub| hub.active())
            .map(|hub| hub.snapshot().owner_epoch)
            .unwrap_or(0);
        if epoch == 0 {
            return;
        }
        if let Ok(mut p) = self.placement.lock() {
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
    }

    fn sample_pointer(
        &mut self,
        backend: &OpenVrBackend,
        layouts: &[Option<SurfaceLayout>; 3],
    ) -> Vec<super::ocr_input::UiInput> {
        let mut inputs = backend.ui_inputs();
        let mut pointers = Vec::with_capacity(inputs.len());
        inputs.retain(|input| {
            let Some(ray) = backend
                .controller_pointer_pose(openvr::TrackedDeviceIndex(input.device), input.source)
            else {
                return false;
            };
            if self.origin != Some(ray.1) {
                return false;
            }
            if let Some(pointer) = nearest_pointer(
                self.pointer_targets
                    .iter()
                    .filter(|(_, _, bound)| *bound != Some(input.device))
                    .filter_map(|&(kind, key, _)| {
                        let (point, position, distance) =
                            backend.intersect(kind, input.device, ray)?;
                        Some(Pointer {
                            device: input.device,
                            key,
                            point,
                            position,
                            distance,
                            scroll_y: input.scroll_y,
                            origin: ray.1,
                        })
                    }),
                layouts,
            ) {
                pointers.push(pointer);
            }
            true
        });
        let candidates: Vec<_> = pointers.iter().map(|p| (p.device, p.distance)).collect();
        let device = self.router.select_pointer_device(&candidates);
        self.pointer = pointers.into_iter().find(|p| Some(p.device) == device);
        inputs
    }

    /// Updates only the shared cursor; panel content stays on the regular render tick.
    pub fn update_pointer(&mut self, app: &AppHandle, backend: &mut OpenVrBackend) {
        if self.pointer_targets.is_empty()
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
            ];
            self.sample_pointer(backend, &layouts);
        }
        let target = self.pointer.map(|p| {
            let (kind, surface) = pointer_overlay(p.key.surface);
            (kind, p.position, p.origin, surface.size().0)
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
    }

    pub fn tick(
        &mut self,
        app: &AppHandle,
        backend: &mut OpenVrBackend,
        config: &VrOverlayConfig,
        controls: &Mutex<ControlHub>,
        reader_revision: u64,
        content_owns_input: bool,
    ) -> (Vec<LaserEvent>, bool) {
        let (view, players, players_revision, owner_active) = controls
            .lock()
            .map(|hub| {
                (
                    hub.snapshot(),
                    hub.player_display().clone(),
                    hub.player_display_revision(),
                    hub.active(),
                )
            })
            .unwrap_or_default();
        let context = (
            config.wrist_menu.hand.clone(),
            format!("{}:{}", config.wrist.hand, config.wrist.dominant_hand),
            view.owner_epoch,
            owner_active,
            config.enabled && config.wrist.enabled,
            config.wrist_menu.enabled,
        );
        let tracking = backend.ocr_tracking().ok();
        let origin = tracking.as_ref().map(|t| t.2);
        if backend.tracking_reset()
            || self.origin.is_some() && origin.is_some() && self.origin != origin
        {
            self.reset();
        }
        self.origin = origin.or(self.origin);
        let dashboard = backend.system_dashboard_visible();
        if context != self.context || dashboard || content_owns_input {
            self.suspend_input();
            if context != self.context {
                self.menu_ui = Default::default();
                if let Ok(mut p) = self.placement.lock() {
                    p.cancel();
                }
                self.generation += 1;
            }
            self.context = context;
        }
        let mut placement = self.placement.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((head, _, _)) = tracking {
            placement.update_head(head);
        }
        placement.sync_mode(Mode::from_config(&config.wrist.anchor_mode));
        placement.sync_follow_view(config.wrist.follow_view);
        if let Some((head, _, _)) = tracking {
            placement.initialize_world(dialogue::in_front(head));
        }
        if let Some(device) = placement.dragging() {
            if let Some((controller, _)) = backend.controller_pose(device) {
                placement.drag(device, controller);
            } else {
                placement.cancel_drag();
            }
        }
        let menu_config = menu_config(&config.wrist_menu.hand);
        let menu_angle = backend.wrist_angle(&menu_config);
        if menu_angle.is_none() {
            self.menu_ui = Default::default();
        }
        self.menu_ui.clamp(&players);
        self.menu_visible = config.wrist_menu.enabled
            && self
                .lift
                .update(menu_angle, self.pressed.is_some(), Instant::now());
        self.dialogue_visible = config.enabled
            && config.wrist.enabled
            && match placement.display_mode() {
                Mode::World => placement.absolute_pose().is_some() && tracking.is_some(),
                Mode::Wrist => {
                    self.dialogue_lift.update(
                        backend.wrist_angle(&config.wrist),
                        self.router.active(),
                        Instant::now(),
                    ) && !(backend.visible(OverlayKind::OcrWrist)
                        && backend.ocr_wrist_on_same_hand(&config.wrist))
                }
            };
        let surfaces = app.state::<SurfaceHub>();
        let generation =
            |surface| (self.generation << 32) | u64::from(surfaces.generation(surface));
        let menu_layout = surfaces.layout(SurfaceId::Menu);
        let menu_view = SurfaceView::Menu {
            controls: view.clone(),
            players: players.clone(),
            ui: self.menu_ui.clone(),
            hold_progress: None,
        };
        let menu_key = Key {
            surface: Surface::Menu,
            generation: generation(SurfaceId::Menu),
            revision: menu_layout
                .as_ref()
                .map_or(0, |layout| u64::from(layout.key.revision)),
        };
        let dialogue_key = Key {
            surface: Surface::Dialogue,
            generation: generation(SurfaceId::Dialogue),
            revision: placement.revision,
        };
        let reader_key = Key {
            surface: Surface::Reader,
            generation: generation(SurfaceId::Reader),
            revision: reader_revision,
        };
        let mut keys = Vec::new();
        let dialogue_layout = surfaces.layout(SurfaceId::Dialogue);
        let reader_layout = surfaces.layout(SurfaceId::Reader);
        let pointer_layouts = [
            surfaces.pointer_layout(SurfaceId::Menu),
            surfaces.pointer_layout(SurfaceId::Dialogue),
            surfaces.pointer_layout(SurfaceId::Reader),
        ];
        if self.menu_visible
            && menu_layout.is_some()
            && self
                .menu_frame
                .as_ref()
                .is_some_and(|frame| frame.same_layout(&menu_view))
        {
            keys.push(menu_key);
        }
        if self.dialogue_visible
            && backend.visible(OverlayKind::Wrist)
            && self.dialogue_revision == Some(placement.revision)
            && (dialogue_layout.is_some() || placement.dragging().is_some())
        {
            keys.push(dialogue_key);
        }
        if backend.visible(OverlayKind::OcrWrist) && reader_layout.is_some() {
            keys.push(reader_key);
        }
        let input_allowed = !dashboard && !content_owns_input && owner_active && tracking.is_some();
        if !input_allowed {
            keys.clear();
        }
        let pointer_visible = [
            self.menu_visible && backend.visible(OverlayKind::WristMenu),
            self.dialogue_visible
                && backend.visible(OverlayKind::Wrist)
                && self.dialogue_revision == Some(placement.revision),
            backend.visible(OverlayKind::OcrWrist),
        ];
        self.pointer_targets = [
            (
                OverlayKind::WristMenu,
                menu_key,
                backend.controller_device(&menu_config),
            ),
            (
                OverlayKind::Wrist,
                dialogue_key,
                if placement.display_mode() == Mode::Wrist {
                    backend.controller_device(&config.wrist)
                } else {
                    None
                },
            ),
            (OverlayKind::OcrWrist, reader_key, None),
        ]
        .into_iter()
        .enumerate()
        .filter(|(index, _)| {
            input_allowed && pointer_visible[*index] && pointer_layouts[*index].is_some()
        })
        .map(|(_, target)| target)
        .collect();
        let can_recenter =
            placement.display_mode() == Mode::World && !placement.busy() && view.items[0].requested;
        // The placement guard is needed again for action dispatch, after pointer sampling.
        drop(placement);
        let inputs = self.sample_pointer(backend, &pointer_layouts);
        let pointer = self.pointer;
        let samples: Vec<_> = inputs
            .into_iter()
            .map(|input| {
                let hit = pointer.filter(|p| p.device == input.device).and_then(|p| {
                    let region = match p.key.surface {
                        Surface::Menu => menu_layout.as_ref()?.hit(p.point)?,
                        Surface::Dialogue => dialogue_layout.as_ref()?.hit(p.point)?,
                        Surface::Reader => reader_layout.as_ref()?.hit(p.point)?,
                    };
                    Some(Hit {
                        key: p.key,
                        region,
                        point: p.point,
                        distance: p.distance,
                    })
                });
                Sample {
                    device: input.device,
                    trigger: input.trigger,
                    grip: input.grip,
                    hit,
                }
            })
            .collect();
        let was_active = self.router.active();
        let mut output = self.router.step(&samples, &keys);
        let now = Instant::now();
        if can_recenter {
            if let Some(event) = self.router.long_press(now) {
                output.events.push(event);
            }
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
        let elapsed = now.saturating_duration_since(self.last_input);
        self.last_input = now;
        if let Some(pointer) =
            pointer.filter(|p| p.key.surface == Surface::Reader && keys.contains(&p.key))
        {
            events.push(LaserEvent::Move {
                device: pointer.device,
                point: pointer.point.map(|v| v * SurfaceId::Reader.size().0 as f32),
            });
            if let Err(error) = surfaces.scroll_reader(app, scroll_delta(pointer.scroll_y, elapsed))
            {
                tracing::debug!(%error, "VR reader scroll unavailable");
            }
        } else {
            events.push(LaserEvent::Leave);
        }
        if let Some(pointer) = pointer.filter(|p| {
            p.key.surface == Surface::Dialogue
                && keys.contains(&p.key)
                && !was_active
                && !self.router.active()
        }) {
            if let Err(error) = surfaces.scroll(
                app,
                SurfaceId::Dialogue,
                scroll_delta(pointer.scroll_y, elapsed),
            ) {
                tracing::debug!(%error, "VR dialogue scroll unavailable");
            }
        }
        let mut save = None;
        let mut action = None;
        let mut placement = self.placement.lock().unwrap_or_else(|e| e.into_inner());
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
                        action = menu_layout
                            .as_ref()
                            .and_then(|layout| layout.menu_command(event.hit.region));
                    }
                    _ => self.pressed = None,
                },
                Surface::Dialogue => match event.kind {
                    Kind::Grab if !placement.busy() => {
                        if let (Some((controller, _)), Some(panel)) = (
                            backend.controller_pose(event.device),
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
            }
        }
        drop(placement);
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
                navigation => {
                    self.menu_ui.apply(&navigation, &players);
                }
            }
        }
        if self.menu_visible {
            let frame = SurfaceView::Menu {
                controls: view,
                players,
                ui: self.menu_ui.clone(),
                hold_progress,
            };
            let result = backend
                .ensure_wrist_menu(&config.wrist_menu.hand)
                .and_then(|_| {
                    if self.menu_surface.present(
                        app,
                        backend,
                        OverlayKind::WristMenu,
                        frame.clone(),
                        hovered,
                        self.pressed,
                    )? {
                        self.menu_frame = Some(frame);
                        backend.show(OverlayKind::WristMenu)
                    } else {
                        backend.hide(OverlayKind::WristMenu);
                        Ok(())
                    }
                });
            if let Err(error) = result {
                backend.reset(OverlayKind::WristMenu);
                self.menu_frame = None;
                if self.menu_error.as_ref() != Some(&error) {
                    tracing::warn!(%error,"Wrist menu unavailable");
                }
                self.menu_error = Some(error);
            } else {
                self.menu_error = None;
            }
        } else {
            backend.hide(OverlayKind::WristMenu);
        }
        (events, ui_focus)
    }
}

#[cfg(test)]
mod tests {
    use super::super::surface;
    use super::*;

    #[test]
    fn menu_pointer_covers_visible_capsule() {
        let key = Key {
            surface: Surface::Menu,
            generation: (1 << 32) | 1,
            revision: 2,
        };
        let mut layout = SurfaceLayout {
            key: surface::SurfaceKey {
                generation: 1,
                revision: 7,
            },
            regions: vec![surface::Region {
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
        };
        layout.validate(SurfaceId::Menu).unwrap();

        let menu = Pointer {
            device: 2,
            key,
            point: [0.5, 0.9],
            position: [0., 0., -1.],
            distance: 1.,
            scroll_y: 0.,
            origin: 1,
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
        let mut layouts = [Some(layout.clone()), None, Some(reader_layout)];
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
        assert!(nearest_pointer([menu].into_iter(), &[None, None, None]).is_none());
        assert!(nearest_pointer([reader].into_iter(), &[None, None, None]).is_none());
        layout.regions.clear();
        layouts[0] = Some(layout);
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
            origin: 1,
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
        let layouts = [None, Some(layout), Some(reader_layout)];
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
            let mut runtime = UiRuntime::new(placement.clone());
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
}
