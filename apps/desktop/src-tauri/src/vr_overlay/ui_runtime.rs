use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager as _};
use vrcs_core::VrOverlayConfig;

use super::backend::{menu_config, OpenVrBackend, OverlayKind};
use super::control_protocol::{ControlAction, ControlHub, ControlSnapshot, ControlSource};
use super::interaction::{Hit, Key, Kind, Router, Sample, Surface};
use super::ocr_wrist::LaserEvent;
use super::placement::{Mode, Placement, Request};
use super::surface::{SurfaceHub, SurfaceId, SurfaceRenderer, SurfaceView};
use super::{dialogue, wrist_menu};

#[derive(Clone, serde::Serialize)]
pub struct PlacementRequest {
    pub owner_epoch: u32,
    pub id: u64,
    pub mode: Mode,
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
    menu_frame: Option<(ControlSnapshot, Option<u8>, Option<u8>)>,
    pub menu_visible: bool,
    pub dialogue_hover: Option<u8>,
    pub dialogue_visible: bool,
    pub dialogue_revision: Option<u64>,
    pub generation: u64,
    context: (String, String, u32, bool, bool, bool),
    origin: Option<i32>,
    pressed: Option<u8>,
    menu_error: Option<String>,
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
            menu_visible: false,
            dialogue_hover: None,
            dialogue_visible: false,
            dialogue_revision: None,
            generation: 1,
            context: Default::default(),
            origin: None,
            pressed: None,
            menu_error: None,
            menu_surface: SurfaceRenderer::new(SurfaceId::Menu),
            dialogue_surface: SurfaceRenderer::new(SurfaceId::Dialogue),
        }
    }

    pub fn fast_tick(&self) -> bool {
        self.menu_visible || self.dialogue_visible || self.router.active()
    }

    fn suspend_input(&mut self) {
        self.router.cancel();
        self.pressed = None;
        if let Ok(mut p) = self.placement.lock() {
            p.cancel_drag();
        }
    }

    pub fn reset(&mut self) {
        self.router.cancel();
        self.generation += 1;
        self.origin = None;
        self.menu_frame = None;
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
                    Ok((head, _, _)) => Some(dialogue::in_front(head)),
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

    pub fn tick(
        &mut self,
        app: &AppHandle,
        backend: &mut OpenVrBackend,
        config: &VrOverlayConfig,
        controls: &Mutex<ControlHub>,
        reader_revision: u64,
        content_owns_input: bool,
    ) -> (Vec<LaserEvent>, bool) {
        let (view, owner_active) = controls
            .lock()
            .map(|hub| (hub.snapshot(), hub.active()))
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
                if let Ok(mut p) = self.placement.lock() {
                    p.cancel();
                }
                self.generation += 1;
            }
            self.context = context;
        }
        let mut placement = self.placement.lock().unwrap_or_else(|e| e.into_inner());
        placement.sync_mode(Mode::from_config(&config.wrist.anchor_mode));
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
        self.menu_visible = config.wrist_menu.enabled
            && self.lift.update(
                backend.wrist_angle(&menu_config),
                self.pressed.is_some(),
                Instant::now(),
            );
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
        let menu_key = Key {
            surface: Surface::Menu,
            generation: generation(SurfaceId::Menu),
            revision: u64::from(view.revision),
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
        let menu_layout = surfaces.layout(SurfaceId::Menu);
        let dialogue_layout = surfaces.layout(SurfaceId::Dialogue);
        let reader_layout = surfaces.layout(SurfaceId::Reader);
        if self.menu_visible
            && menu_layout.is_some()
            && self
                .menu_frame
                .as_ref()
                .is_some_and(|frame| frame.0 == view)
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
        if dashboard || content_owns_input || !owner_active {
            keys.clear();
        }
        let mut pointers = Vec::new();
        let samples: Vec<_> = backend
            .ui_inputs()
            .into_iter()
            .map(|input| {
                let targets = [
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
                ];
                let hit = targets
                    .into_iter()
                    .filter(|(_, key, bound)| keys.contains(key) && *bound != Some(input.device))
                    .filter_map(|(kind, key, _)| {
                        let (point, distance) =
                            backend.intersect(kind, input.device, input.source)?;
                        pointers.push((key.surface, point, distance));
                        let region = match key.surface {
                            Surface::Menu => menu_layout.as_ref()?.hit(point)?,
                            Surface::Dialogue => dialogue_layout.as_ref()?.hit(point)?,
                            Surface::Reader => reader_layout.as_ref()?.hit(point)?,
                        };
                        Some(Hit {
                            key,
                            region,
                            point,
                            distance,
                        })
                    })
                    .min_by(|a, b| a.distance.total_cmp(&b.distance));
                Sample {
                    device: input.device,
                    trigger: input.trigger,
                    grip: input.grip,
                    hit,
                }
            })
            .collect();
        let was_active = self.router.active();
        let output = self.router.step(&samples, &keys);
        let pointer = output
            .hover
            .map(|(_, hit)| (hit.key.surface, hit.point, hit.distance))
            .or_else(|| pointers.into_iter().min_by(|a, b| a.2.total_cmp(&b.2)));
        for (surface, id) in [
            (Surface::Menu, SurfaceId::Menu),
            (Surface::Dialogue, SurfaceId::Dialogue),
            (Surface::Reader, SurfaceId::Reader),
        ] {
            let point = pointer.filter(|p| p.0 == surface).map(|p| p.1);
            if let Err(error) = surfaces.point(app, id, point) {
                tracing::debug!(%error, "VR surface pointer unavailable");
            }
        }
        let ui_focus = was_active || self.router.active() || output.hover.is_some();
        let mut events = Vec::new();
        let hovered = output
            .hover
            .filter(|(_, h)| h.key.surface == Surface::Menu)
            .map(|(_, h)| h.region);
        self.dialogue_hover = output
            .hover
            .filter(|(_, h)| h.key.surface == Surface::Dialogue)
            .map(|(_, h)| h.region);
        if let Some((device, hit)) = output
            .hover
            .filter(|(_, h)| h.key.surface == Surface::Reader)
        {
            events.push(LaserEvent::Move {
                device,
                point: hit.point.map(|v| v * SurfaceId::Reader.size().0 as f32),
            });
        } else {
            events.push(LaserEvent::Leave);
        }
        let mut save = None;
        let mut action = None;
        for event in output.events {
            match event.hit.key.surface {
                Surface::Menu => match event.kind {
                    Kind::Down => self.pressed = Some(event.hit.region),
                    Kind::Up => {
                        self.pressed = None;
                        let i = event.hit.region as usize;
                        action = Some(ControlAction::for_index(i, !view.items[i].requested));
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
            super::runtime::dispatch_control(app, controls, ControlSource::WristMenu, action);
        }
        if self.menu_visible {
            let frame = (view, hovered, self.pressed);
            let result = backend
                .ensure_wrist_menu(&config.wrist_menu.hand)
                .and_then(|_| {
                    if self.menu_surface.present(
                        app,
                        backend,
                        OverlayKind::WristMenu,
                        SurfaceView::Menu {
                            controls: frame.0.clone(),
                        },
                        frame.1,
                        frame.2,
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
    use super::*;

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
