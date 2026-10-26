use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Manager};

use super::{activate, Action, Prompt};
use crate::vr_overlay::backend::{OpenVrBackend, OverlayKind};
use crate::vr_overlay::control_protocol::ControlHub;
use crate::vr_overlay::interaction::{Hit, Key, Kind, Router, Sample, Surface};
use crate::vr_overlay::surface::{SurfaceHub, SurfaceId, SurfaceRenderer, SurfaceView};

pub struct Runtime {
    renderer: SurfaceRenderer,
    router: Router,
    identity: Option<(u32, u32, i32)>,
    pose: Option<[[f32; 4]; 3]>,
    notice: Option<Prompt>,
    pointer: Option<([f32; 3], i32)>,
    last_input: Instant,
    error: Option<String>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            renderer: SurfaceRenderer::new(SurfaceId::PlayerAudio),
            router: Router::default(),
            identity: None,
            pose: None,
            notice: None,
            pointer: None,
            last_input: Instant::now(),
            error: None,
        }
    }
}

impl Runtime {
    pub fn active(&self) -> bool {
        self.notice.is_some()
    }

    pub fn reset(&mut self) {
        self.renderer.reset();
        self.router.cancel();
        self.identity = None;
        self.pose = None;
        self.notice = None;
        self.pointer = None;
    }

    pub fn finish_surfaces(&mut self, app: &AppHandle, backend: Option<&mut OpenVrBackend>) {
        self.renderer
            .finish_tick(app, backend, OverlayKind::PlayerAudio);
    }

    pub fn stop_surfaces(&mut self, app: &AppHandle, backend: Option<&mut OpenVrBackend>) {
        self.renderer.stop(app, backend, OverlayKind::PlayerAudio);
    }

    pub fn update_pointer(&mut self, app: &AppHandle, backend: &mut OpenVrBackend) {
        if backend.system_dashboard_visible() || !backend.visible(OverlayKind::PlayerAudio) {
            self.pointer = None;
        } else {
            self.sample(app, backend);
        }
        if let Err(error) = backend.update_cursor(self.pointer.map(|(point, origin)| {
            (
                OverlayKind::PlayerAudio,
                point,
                origin,
                SurfaceId::PlayerAudio.size().0,
            )
        })) {
            tracing::debug!(%error, "VR consent cursor unavailable");
        }
    }

    fn sample(&mut self, app: &AppHandle, backend: &OpenVrBackend) -> Option<(Vec<Sample>, Key)> {
        let (epoch, request, origin) = self.identity?;
        let surfaces = app.state::<SurfaceHub>();
        let layout = surfaces.layout(SurfaceId::PlayerAudio)?;
        let contour = surfaces.pointer_layout(SurfaceId::PlayerAudio)?;
        // This router has its own key domain and never routes events into the OCR reader.
        let key = Key {
            surface: Surface::Reader,
            generation: (u64::from(epoch) << 32) | u64::from(layout.key.generation),
            revision: (u64::from(request) << 32) | u64::from(layout.key.revision),
        };
        let inputs = backend.ui_inputs();
        let pointers: Vec<_> = inputs
            .iter()
            .filter_map(|input| {
                let ray = backend.controller_pointer_pose(
                    openvr::TrackedDeviceIndex(input.device),
                    input.source,
                )?;
                if ray.1 != origin {
                    return None;
                }
                let (point, position, distance) =
                    backend.intersect(OverlayKind::PlayerAudio, input.device, ray)?;
                contour.pointer_contains(point).then_some((
                    input.device,
                    point,
                    position,
                    distance,
                    input.scroll_y,
                ))
            })
            .collect();
        let candidates: Vec<_> = pointers.iter().map(|p| (p.0, p.3)).collect();
        let selected = self.router.select_pointer_device(&candidates);
        let pointer = pointers.iter().find(|p| Some(p.0) == selected);
        self.pointer = pointer.map(|p| (p.2, origin));
        let now = Instant::now();
        if let Some(pointer) = pointer {
            let delta = -pointer.4
                * now
                    .saturating_duration_since(self.last_input)
                    .as_secs_f32()
                    .min(0.05)
                * 1.5;
            let _ = surfaces.scroll(app, SurfaceId::PlayerAudio, delta);
        }
        self.last_input = now;
        let samples = inputs
            .iter()
            .map(|input| Sample {
                device: input.device,
                trigger: input.trigger,
                grip: input.grip,
                hit: pointers.iter().find(|p| p.0 == input.device).and_then(|p| {
                    layout.hit(p.1).map(|region| Hit {
                        key,
                        region,
                        point: p.1,
                        distance: p.3,
                    })
                }),
            })
            .collect();
        Some((samples, key))
    }

    pub fn tick(
        &mut self,
        app: &AppHandle,
        backend: &mut OpenVrBackend,
        controls: &Mutex<ControlHub>,
    ) -> bool {
        let (epoch, dark, notice) = controls
            .lock()
            .map(|hub| {
                (
                    hub.snapshot().owner_epoch,
                    hub.snapshot().dark,
                    hub.player_audio().notice.clone(),
                )
            })
            .unwrap_or_default();
        let dashboard = backend.system_dashboard_visible();
        let Some(notice) = notice else {
            self.notice = None;
            self.identity = None;
            self.pointer = None;
            self.router.cancel();
            backend.hide(OverlayKind::PlayerAudio);
            return false;
        };
        self.notice = Some(notice.clone());
        if dashboard {
            self.router.cancel();
            self.pointer = None;
            self.identity = None;
            backend.hide(OverlayKind::PlayerAudio);
            return true;
        }
        let Ok((head, _, origin)) = backend.ocr_tracking() else {
            self.router.cancel();
            self.pointer = None;
            self.identity = None;
            backend.hide(OverlayKind::PlayerAudio);
            return true;
        };
        if self.identity != Some((epoch, notice.request_id, origin)) {
            self.router.cancel();
            self.pose = Some(crate::vr_overlay::dialogue::in_front(head));
            self.identity = Some((epoch, notice.request_id, origin));
            self.last_input = Instant::now();
        }
        let mut hovered = None;
        let mut pressed = None;
        if let Some((samples, key)) = self.sample(app, backend) {
            let output = self.router.step(&samples, &[key]);
            hovered = output.hover.map(|(_, hit)| hit.region);
            for event in output.events {
                if event.kind == Kind::Up {
                    let action = match event.hit.region {
                        0 => Action::Cancel,
                        1 => Action::Confirm,
                        2 => Action::Previous,
                        _ => Action::Next,
                    };
                    activate(
                        app,
                        controls,
                        SurfaceId::PlayerAudio,
                        notice.request_id,
                        action,
                    );
                }
            }
            if self.router.active() {
                pressed = hovered;
            }
        } else {
            self.router.cancel();
            self.pointer = None;
        }
        let result = backend
            .ensure_player_audio(self.pose.expect("consent pose"), origin)
            .and_then(|_| {
                if self.renderer.present(
                    app,
                    backend,
                    OverlayKind::PlayerAudio,
                    SurfaceView::PlayerAudio { dark, notice },
                    hovered,
                    pressed,
                )? {
                    backend.show(OverlayKind::PlayerAudio)
                } else {
                    backend.hide(OverlayKind::PlayerAudio);
                    Ok(())
                }
            });
        match result {
            Ok(()) => self.error = None,
            Err(error) => {
                if self.error.as_ref() != Some(&error) {
                    tracing::warn!(%error, "VR consent overlay unavailable");
                }
                self.error = Some(error);
                backend.reset(OverlayKind::PlayerAudio);
                self.renderer.reset();
            }
        }
        true
    }
}
