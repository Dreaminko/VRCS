//! OpenXR overlay implementation behind the shared `Backend` facade.
//!
//! The runtime owns the session, swapchains and quad layers. This adapter maps
//! the neutral backend calls onto `XrRuntime`; OpenVR-only values never cross
//! back into the shared layer.

use std::time::{Duration, Instant};

use openxr as xr;
use vrcs_core::{VrOverlayHeadsetConfig, VrOverlayWristConfig};

use super::backend::{
    ControllerBinding, CursorTarget, OverlayKind, RayHit, menu_wrist_config,
};
use super::dashboard::{DASHBOARD_HEIGHT, DASHBOARD_WIDTH, DashboardPointerEvent};
use super::pose::{
    DeviceId, PointerId, PointerInput, Pose, SpaceId, TrackedPose, pose_from_quaternion,
};
use super::transform;
use super::xr::{PanelId, PanelInfo, PanelSpace, XrRuntime};

const CURSOR_PX: u32 = 64;
const CURSOR_OFFSET_M: f32 = 0.003;

pub struct OpenXrBackend {
    runtime: XrRuntime,
    cursor_ready: bool,
    dashboard_open: bool,
    dashboard_pose: Option<xr::Posef>,
    dashboard_pointer: DashboardPointer,
    dashboard_cursor: Option<CursorTarget>,
    dashboard_placeholder: Option<String>,
    dashboard_scroll_at: Instant,
}

#[derive(Default)]
struct DashboardPointer {
    pressed: Option<usize>,
    hover: Option<[f32; 2]>,
    triggers: [bool; 2],
}

impl DashboardPointer {
    fn events(
        &mut self,
        hits: [Option<[f32; 2]>; 2],
        triggers: [bool; 2],
        primary: Option<usize>,
    ) -> Vec<DashboardPointerEvent> {
        let point = |uv: [f32; 2]| {
            (
                uv[0] * DASHBOARD_WIDTH as f32,
                uv[1] * DASHBOARD_HEIGHT as f32,
            )
        };
        let mut events = Vec::new();
        if let Some(hand) = self.pressed {
            if triggers[hand] {
                if let Some(uv) = hits[hand] {
                    if self.hover != Some(uv) {
                        let (x, y) = point(uv);
                        events.push(DashboardPointerEvent::Move { x, y });
                        self.hover = Some(uv);
                    }
                } else {
                    // Leaving the quad cancels the press, including a tracking
                    // dropout that happened while the trigger was held.
                    events.push(DashboardPointerEvent::Up { x: -1.0, y: -1.0 });
                    self.pressed = None;
                    self.hover = None;
                }
            } else {
                // Only a release on the quad may commit; releasing after the
                // ray left must not use the last hover position.
                events.push(match hits[hand] {
                    Some(uv) => {
                        let (x, y) = point(uv);
                        DashboardPointerEvent::Up { x, y }
                    }
                    None => DashboardPointerEvent::Up { x: -1.0, y: -1.0 },
                });
                self.pressed = None;
                self.hover = None;
            }
        } else {
            if let Some(uv) = primary
                .filter(|hand| hits[*hand].is_some())
                .and_then(|hand| hits[hand])
            {
                if self.hover != Some(uv) {
                    let (x, y) = point(uv);
                    events.push(DashboardPointerEvent::Move { x, y });
                    self.hover = Some(uv);
                }
            } else if self.hover.take().is_some() {
                events.push(DashboardPointerEvent::Move { x: -1.0, y: -1.0 });
            }
            // Either controller may press; the closest hand only drives hover.
            if let Some((hand, uv)) = (0..2)
                .filter(|hand| triggers[*hand] && !self.triggers[*hand])
                .find_map(|hand| hits[hand].map(|uv| (hand, uv)))
            {
                let (x, y) = point(uv);
                events.push(DashboardPointerEvent::Down { x, y });
                self.pressed = Some(hand);
                self.hover = Some(uv);
            }
        }
        self.triggers = triggers;
        events
    }
}

fn panel(kind: OverlayKind) -> PanelId {
    match kind {
        OverlayKind::PlayerAudio => PanelId::PlayerAudio,
        OverlayKind::Headset => PanelId::Headset,
        OverlayKind::Wrist => PanelId::Wrist,
        OverlayKind::WristMenu => PanelId::WristMenu,
        OverlayKind::EditToolbar => PanelId::EditToolbar,
        OverlayKind::OcrWrist => PanelId::OcrWrist,
        OverlayKind::OcrProgress => PanelId::OcrProgress,
        OverlayKind::OcrFrame => PanelId::OcrFrame,
        OverlayKind::OcrResult => PanelId::OcrResult,
        OverlayKind::Dashboard => PanelId::Dashboard,
    }
}

const ALL_KINDS: [OverlayKind; 10] = [
    OverlayKind::PlayerAudio,
    OverlayKind::Headset,
    OverlayKind::Wrist,
    OverlayKind::WristMenu,
    OverlayKind::EditToolbar,
    OverlayKind::OcrWrist,
    OverlayKind::OcrProgress,
    OverlayKind::OcrFrame,
    OverlayKind::OcrResult,
    OverlayKind::Dashboard,
];

fn xr_pose(pose: Pose) -> xr::Posef {
    let m = pose;
    let trace = m[0][0] + m[1][1] + m[2][2];
    let (x, y, z, w) = if trace > 0.0 {
        let s = (trace + 1.0).max(0.0).sqrt() * 2.0;
        if s > 1e-6 {
            (
                (m[2][1] - m[1][2]) / s,
                (m[0][2] - m[2][0]) / s,
                (m[1][0] - m[0][1]) / s,
                0.25 * s,
            )
        } else {
            (0.0, 0.0, 0.0, 1.0)
        }
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).max(0.0).sqrt() * 2.0;
        if s > 1e-6 {
            (
                0.25 * s,
                (m[0][1] + m[1][0]) / s,
                (m[0][2] + m[2][0]) / s,
                (m[2][1] - m[1][2]) / s,
            )
        } else {
            (0.0, 0.0, 0.0, 1.0)
        }
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).max(0.0).sqrt() * 2.0;
        if s > 1e-6 {
            (
                (m[0][1] + m[1][0]) / s,
                0.25 * s,
                (m[1][2] + m[2][1]) / s,
                (m[0][2] - m[2][0]) / s,
            )
        } else {
            (0.0, 0.0, 0.0, 1.0)
        }
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).max(0.0).sqrt() * 2.0;
        if s > 1e-6 {
            (
                (m[0][2] + m[2][0]) / s,
                (m[1][2] + m[2][1]) / s,
                0.25 * s,
                (m[1][0] - m[0][1]) / s,
            )
        } else {
            (0.0, 0.0, 0.0, 1.0)
        }
    };
    let norm = (x * x + y * y + z * z + w * w).sqrt();
    let orientation = if norm.is_finite() && norm > 1e-6 {
        xr::Quaternionf {
            x: x / norm,
            y: y / norm,
            z: z / norm,
            w: w / norm,
        }
    } else {
        xr::Quaternionf::IDENTITY
    };
    xr::Posef {
        orientation,
        position: xr::Vector3f {
            x: m[0][3],
            y: m[1][3],
            z: m[2][3],
        },
    }
}

fn matrix_pose(pose: xr::Posef) -> Pose {
    let p = pose.position;
    let q = pose.orientation;
    pose_from_quaternion([p.x, p.y, p.z], [q.x, q.y, q.z, q.w])
}

fn width(metres: f32) -> xr::Extent2Df {
    xr::Extent2Df {
        width: metres,
        height: metres,
    }
}

fn hand_index(config: &VrOverlayWristConfig) -> usize {
    let role = if config.hand == "dominant" {
        config.dominant_hand.as_str()
    } else {
        config.hand.as_str()
    };
    usize::from(role == "right")
}

fn hand_name(index: usize) -> &'static str {
    if index == 0 { "left" } else { "right" }
}

/// Converts straight-alpha RGBA textures to premultiplied colour for OpenXR layer blending.
fn premultiply(pixels: &mut [u8]) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
}

fn dashboard_stage_pose(head: Pose) -> xr::Posef {
    let hfx = -head[0][2];
    let hfz = -head[2][2];
    let len = (hfx * hfx + hfz * hfz).sqrt();
    let (fx, fz) = if len > 1e-4 {
        (hfx / len, hfz / len)
    } else {
        (0.0, -1.0)
    };
    xr_pose([
        [-fz, 0.0, -fx, head[0][3] + fx * 1.2],
        [0.0, 1.0, 0.0, head[1][3]],
        [fx, 0.0, -fz, head[2][3] + fz * 1.2],
    ])
}

impl OpenXrBackend {
    pub fn connect() -> Result<Self, String> {
        Ok(Self {
            runtime: XrRuntime::open(false)?,
            cursor_ready: false,
            dashboard_open: false,
            dashboard_pose: None,
            dashboard_pointer: DashboardPointer::default(),
            dashboard_cursor: None,
            dashboard_placeholder: None,
            dashboard_scroll_at: Instant::now(),
        })
    }

    pub fn quit_requested(&mut self) -> bool {
        false
    }

    pub fn begin_frame(&mut self) -> Result<bool, String> {
        self.runtime.begin_frame()
    }

    pub fn end_frame(&mut self) -> Result<bool, String> {
        self.runtime.end_frame()
    }

    #[cfg(windows)]
    pub fn surface_adapter_index(&self) -> Result<u32, String> {
        Ok(0)
    }

    pub fn ensure_headset(
        &mut self,
        config: &VrOverlayHeadsetConfig,
        absolute_override: Option<(Pose, SpaceId)>,
    ) -> Result<(), String> {
        let (space, pose) = match absolute_override {
            Some((pose, _)) => (PanelSpace::Stage, xr_pose(pose)),
            None => (PanelSpace::View, xr_pose(transform::headset(config))),
        };
        self.runtime
            .set_panel_width(PanelId::Headset, space, pose, config.width_m);
        Ok(())
    }

    pub fn ensure_edit_toolbar(&mut self, pose: Pose, _space: SpaceId) -> Result<(), String> {
        self.runtime
            .set_panel_width(PanelId::EditToolbar, PanelSpace::Stage, xr_pose(pose), 0.64);
        Ok(())
    }

    pub fn ensure_wrist(
        &mut self,
        config: &VrOverlayWristConfig,
    ) -> Result<ControllerBinding, String> {
        let hand = hand_index(config);
        self.runtime.set_panel_width(
            PanelId::Wrist,
            PanelSpace::Grip(hand),
            xr_pose(transform::wrist(config)),
            config.width_m,
        );
        Ok(ControllerBinding {
            role: Some(hand_name(hand).into()),
            available: self.runtime.controller_pose(hand).is_some(),
        })
    }

    pub fn ensure_wrist_menu(
        &mut self,
        config: &vrcs_core::VrOverlayWristMenuConfig,
    ) -> Result<(), String> {
        let wrist = menu_wrist_config(config);
        let index = usize::from(config.hand == "right");
        self.runtime.set_panel_width(
            PanelId::WristMenu,
            PanelSpace::Grip(index),
            xr_pose(transform::wrist_menu(config)),
            wrist.width_m,
        );
        Ok(())
    }

    pub fn ensure_world_wrist(
        &mut self,
        config: &VrOverlayWristConfig,
        pose: Pose,
        _space: SpaceId,
        head_relative: Option<Pose>,
    ) -> Result<(), String> {
        let (space, pose) = match head_relative {
            Some(relative) => (PanelSpace::View, relative),
            None => (PanelSpace::Stage, pose),
        };
        self.runtime
            .set_panel_width(PanelId::Wrist, space, xr_pose(pose), config.width_m);
        Ok(())
    }

    pub fn ocr_wrist_on_same_hand(&self, _config: &VrOverlayWristConfig) -> bool {
        false
    }

    pub fn controller_pose(&self, device: DeviceId) -> Option<TrackedPose> {
        self.runtime.controller_pose(device.0 as usize)
    }

    pub fn pointer_pose(&self, pointer: PointerId) -> Option<TrackedPose> {
        self.runtime.pointer_pose(pointer)
    }

    pub fn wrist_pose(&self, config: &VrOverlayWristConfig) -> Option<Pose> {
        let hand = hand_index(config);
        let controller = self.runtime.controller_pose(hand)?;
        Some(transform::compose(
            controller.pose,
            transform::wrist(config),
        ))
    }

    pub fn wrist_device_and_angle(
        &self,
        config: &VrOverlayWristConfig,
        head: Option<Pose>,
    ) -> (Option<DeviceId>, Option<f32>) {
        let hand = hand_index(config);
        let Some(controller) = self.runtime.controller_pose(hand) else {
            return (None, None);
        };
        let angle = head.and_then(|head| {
            let panel = transform::compose(controller.pose, transform::wrist(config));
            let toward: [f32; 3] = std::array::from_fn(|i| head[i][3] - panel[i][3]);
            let length = toward.iter().map(|v| v * v).sum::<f32>().sqrt();
            (length > 0.001).then(|| {
                (toward
                    .iter()
                    .enumerate()
                    .map(|(i, v)| v * panel[i][2])
                    .sum::<f32>()
                    / length)
                    .clamp(-1., 1.)
                    .acos()
                    .to_degrees()
            })
        });
        (Some(DeviceId(hand as u32)), angle)
    }

    fn world_panel(&self, info: &PanelInfo) -> Option<Pose> {
        let local = matrix_pose(info.pose);
        Some(match info.space {
            PanelSpace::Stage => local,
            PanelSpace::View => transform::compose(self.ocr_tracking().ok()?.0, local),
            PanelSpace::Grip(hand) => {
                transform::compose(self.runtime.controller_pose(hand)?.pose, local)
            }
        })
    }

    pub fn intersect(
        &self,
        kind: OverlayKind,
        _device: DeviceId,
        ray: TrackedPose,
    ) -> Option<RayHit> {
        let info = self.runtime.panel(panel(kind))?;
        if !info.visible {
            return None;
        }
        let world = self.world_panel(&info)?;
        let local = transform::compose(transform::inverse(world), ray.pose);
        let origin = [local[0][3], local[1][3], local[2][3]];
        let direction = [-local[0][2], -local[1][2], -local[2][2]];
        if direction[2] >= -1e-5 {
            return None;
        }
        let distance = -origin[2] / direction[2];
        if distance <= 0.0 {
            return None;
        }
        let local_hit = [
            origin[0] + direction[0] * distance,
            origin[1] + direction[1] * distance,
        ];
        let half_width = info.size.width / 2.0;
        let half_height = info.size.height / 2.0;
        if local_hit[0].abs() > half_width || local_hit[1].abs() > half_height {
            return None;
        }
        let uv = [
            (local_hit[0] + half_width) / info.size.width,
            (half_height - local_hit[1]) / info.size.height,
        ];
        let world_hit = [
            world[0][3] + world[0][0] * local_hit[0] + world[0][1] * local_hit[1],
            world[1][3] + world[1][0] * local_hit[0] + world[1][1] * local_hit[1],
            world[2][3] + world[2][0] * local_hit[0] + world[2][1] * local_hit[1],
        ];
        Some(RayHit {
            point: uv,
            position: world_hit,
            distance,
            panel: world,
            panel_width: info.size.width,
        })
    }

    pub fn cursor_interval(&self) -> Duration {
        Duration::from_millis(16)
    }

    pub fn update_cursor(&mut self, target: Option<CursorTarget>) -> Result<(), String> {
        // The shared pointer sampler only knows about its own surfaces; fall
        // back to the settings quad when it owns the pointer.
        let target = target.or_else(|| self.dashboard_cursor.filter(|_| self.dashboard_open));
        let Some(target) = target else {
            self.runtime.set_visible(PanelId::Cursor, false);
            return Ok(());
        };
        if !self.runtime.visible(panel(target.kind)) {
            self.runtime.set_visible(PanelId::Cursor, false);
            return Ok(());
        }
        let diameter_px = if target.kind == OverlayKind::WristMenu {
            30
        } else {
            24
        };
        let size = target.panel_width * diameter_px as f32 / target.texture_width.max(1) as f32;
        if !target.position.iter().all(|v| v.is_finite()) || !size.is_finite() || size <= 0.0 {
            self.runtime.set_visible(PanelId::Cursor, false);
            return Ok(());
        }
        if !self.cursor_ready {
            let mut pixels = super::cursor::texture();
            premultiply(&mut pixels);
            self.runtime
                .upload(PanelId::Cursor, &pixels, CURSOR_PX, CURSOR_PX)?;
            self.cursor_ready = true;
        }
        let world = target.panel;
        let normal = [world[0][2], world[1][2], world[2][2]];
        let mut pose = xr_pose(world);
        pose.position = xr::Vector3f {
            x: target.position[0] + normal[0] * CURSOR_OFFSET_M,
            y: target.position[1] + normal[1] * CURSOR_OFFSET_M,
            z: target.position[2] + normal[2] * CURSOR_OFFSET_M,
        };
        self.runtime
            .set_panel(PanelId::Cursor, PanelSpace::Stage, pose, width(size));
        self.runtime.set_visible(PanelId::Cursor, true);
        Ok(())
    }

    pub fn pointer_inputs(&self) -> Vec<PointerInput> {
        self.runtime.pointer_inputs()
    }

    pub fn update_ocr_input(&mut self, _path: &std::path::Path) -> Result<(), String> {
        Err("vr.ocr_input_unavailable".into())
    }

    pub fn ocr_tracking(&self) -> Result<(Pose, u32, SpaceId), String> {
        let head = self
            .runtime
            .head_pose()
            .ok_or("vr.openxr_head_tracking_unavailable")?;
        Ok((head.pose, 1, head.space))
    }

    pub fn system_dashboard_visible(&mut self) -> bool {
        // The application-owned settings quad stands in for SteamVR's
        // dashboard; the shared pointer pipeline goes modal while it is open.
        self.dashboard_open
    }

    pub fn ensure_dashboard(&mut self) -> Result<(), String> {
        if self.runtime.menu_clicked() {
            self.set_dashboard_open(!self.dashboard_open);
        }
        if self.dashboard_open && self.dashboard_pose.is_none() {
            self.dashboard_pose = self
                .runtime
                .head_pose()
                .map(|h| dashboard_stage_pose(h.pose));
        }
        let (space, pose) = match self.dashboard_pose {
            Some(pose) => (PanelSpace::Stage, pose),
            None => (
                PanelSpace::View,
                xr::Posef {
                    orientation: xr::Quaternionf::IDENTITY,
                    position: xr::Vector3f {
                        x: 0.0,
                        y: 0.0,
                        z: -1.2,
                    },
                },
            ),
        };
        self.runtime
            .set_panel_width(PanelId::Dashboard, space, pose, 1.0);
        self.runtime
            .set_visible(PanelId::Dashboard, self.dashboard_open);
        Ok(())
    }

    pub fn show_dashboard_placeholder(&mut self, error: Option<&str>) -> Result<(), String> {
        let message = match error {
            Some(error) => format!("VRCS\n\nDashboard unavailable. Retrying...\n\n{error}"),
            None => "VRCS\n\nLoading dashboard...".into(),
        };
        if self.dashboard_placeholder.as_deref() == Some(message.as_str()) {
            return Ok(());
        }
        let texture = super::renderer::dashboard_placeholder(&message)?;
        self.runtime.upload(
            PanelId::Dashboard,
            &texture.pixels,
            texture.width,
            texture.height,
        )?;
        self.runtime
            .set_visible(PanelId::Dashboard, self.dashboard_open);
        self.dashboard_placeholder = Some(message);
        Ok(())
    }

    pub fn dashboard_visible(&self) -> bool {
        self.dashboard_open
    }

    pub fn toggle_dashboard(&mut self) -> Result<bool, String> {
        self.set_dashboard_open(!self.dashboard_open);
        self.runtime
            .set_visible(PanelId::Dashboard, self.dashboard_open);
        Ok(self.dashboard_open)
    }

    pub fn poll_dashboard_events(&mut self) -> Result<Vec<DashboardPointerEvent>, String> {
        if !self.dashboard_open {
            self.clear_dashboard_pointer();
            return Ok(Vec::new());
        }
        if self.dashboard_placeholder.is_some() {
            self.clear_dashboard_pointer();
            return Ok(Vec::new());
        }
        if !self.runtime.focused() {
            // Losing focus must not commit the press that was in progress.
            let events = if self.dashboard_pointer.pressed.is_some() {
                vec![DashboardPointerEvent::Up { x: -1.0, y: -1.0 }]
            } else if self.dashboard_pointer.hover.is_some() {
                vec![DashboardPointerEvent::Move { x: -1.0, y: -1.0 }]
            } else {
                Vec::new()
            };
            self.clear_dashboard_pointer();
            // A trigger that is still held when focus returns must not count
            // as a fresh press; require a release first.
            self.dashboard_pointer.triggers = [true; 2];
            return Ok(events);
        }
        let inputs = self.runtime.pointer_inputs();
        let mut hits: [Option<[f32; 2]>; 2] = [None; 2];
        let mut best: Option<(usize, RayHit)> = None;
        for (hand, input) in inputs.iter().enumerate().take(2) {
            let Some(ray) = self.runtime.pointer_pose(input.pointer) else {
                continue;
            };
            let Some(hit) = self.intersect(OverlayKind::Dashboard, input.device, ray) else {
                continue;
            };
            hits[hand] = Some(hit.point);
            if best.is_none_or(|(_, closest)| hit.distance < closest.distance) {
                best = Some((hand, hit));
            }
        }
        self.dashboard_cursor = best.map(|(_, hit)| CursorTarget {
            kind: OverlayKind::Dashboard,
            position: hit.position,
            origin: SpaceId(0),
            panel: hit.panel,
            panel_width: hit.panel_width,
            texture_width: DASHBOARD_WIDTH,
        });
        let pressed = |hand: usize| {
            inputs
                .get(hand)
                .and_then(|input| input.trigger)
                .unwrap_or(false)
        };
        let mut events = self.dashboard_pointer.events(
            hits,
            [pressed(0), pressed(1)],
            best.map(|(hand, _)| hand),
        );
        let now = Instant::now();
        let elapsed = now
            .saturating_duration_since(self.dashboard_scroll_at)
            .as_secs_f32()
            .min(0.05);
        self.dashboard_scroll_at = now;
        let axis = best
            .and_then(|(hand, _)| inputs.get(hand))
            .map(|input| input.scroll_y)
            .unwrap_or(0.0);
        let delta = -axis * elapsed * 1.5;
        if delta.abs() > 1e-4 {
            events.push(DashboardPointerEvent::Scroll { delta });
        }
        Ok(events)
    }

    fn set_dashboard_open(&mut self, open: bool) {
        self.dashboard_open = open;
        self.dashboard_scroll_at = Instant::now();
        if open {
            self.dashboard_pose = self
                .runtime
                .head_pose()
                .map(|h| dashboard_stage_pose(h.pose));
            // A trigger held while the quad opened must not count as a click.
            self.dashboard_pointer.triggers = [false; 2];
            if self.runtime.focused() {
                for (hand, input) in self.runtime.pointer_inputs().iter().enumerate().take(2) {
                    self.dashboard_pointer.triggers[hand] = input.trigger == Some(true);
                }
            } else {
                self.dashboard_pointer.triggers = [true; 2];
            }
        } else {
            self.dashboard_pose = None;
            self.clear_dashboard_pointer();
        }
    }

    fn clear_dashboard_pointer(&mut self) {
        self.dashboard_pointer = DashboardPointer::default();
        self.dashboard_cursor = None;
        self.dashboard_scroll_at = Instant::now();
    }

    pub fn visible(&self, kind: OverlayKind) -> bool {
        self.runtime.visible(panel(kind))
    }

    pub fn tracking_reset(&mut self) -> bool {
        let reset = self.runtime.take_tracking_reset();
        if reset {
            self.dashboard_pose = None;
        }
        reset
    }

    pub fn has_surface_texture(&self, kind: OverlayKind) -> bool {
        self.runtime.has_image(panel(kind))
    }

    pub fn upload_surface(
        &mut self,
        kind: OverlayKind,
        frame: &crate::vr_overlay::surface::frame::Captured,
        _capture: std::rc::Rc<std::cell::RefCell<crate::vr_overlay::surface::frame::Capture>>,
    ) -> Result<(), String> {
        let texture = &frame.texture;
        self.runtime
            .upload(panel(kind), &texture.pixels, texture.width, texture.height)?;
        if kind == OverlayKind::Dashboard {
            self.dashboard_placeholder = None;
        }
        Ok(())
    }

    pub fn clear_surface_texture(&mut self, kind: OverlayKind) -> Result<(), String> {
        self.runtime.clear(panel(kind));
        if kind == OverlayKind::Dashboard {
            self.dashboard_placeholder = None;
        }
        Ok(())
    }

    pub fn set_opacity(&mut self, kind: OverlayKind, opacity: f32) -> Result<(), String> {
        self.runtime.set_opacity(panel(kind), opacity)
    }

    pub fn show(&mut self, kind: OverlayKind) -> Result<(), String> {
        self.runtime.set_visible(panel(kind), true);
        Ok(())
    }

    pub fn hide(&mut self, kind: OverlayKind) {
        self.runtime.set_visible(panel(kind), false);
        if kind == OverlayKind::Dashboard {
            self.dashboard_placeholder = None;
        }
    }

    pub fn close_dashboard(&mut self) {
        self.set_dashboard_open(false);
        self.runtime.set_visible(PanelId::Dashboard, false);
        self.dashboard_placeholder = None;
    }

    pub fn hide_all(&mut self) {
        for kind in ALL_KINDS {
            self.hide(kind);
        }
        self.set_dashboard_open(false);
        self.runtime.set_visible(PanelId::Cursor, false);
    }

    pub fn reset(&mut self, kind: OverlayKind) {
        if kind == OverlayKind::Dashboard {
            // A surface fault must not close the user's settings quad; keep
            // the modal open so the error placeholder and retry stay visible.
            self.runtime.clear(panel(kind));
            self.runtime
                .set_visible(PanelId::Dashboard, self.dashboard_open);
            self.dashboard_placeholder = None;
            self.clear_dashboard_pointer();
        } else {
            self.hide(kind);
            self.runtime.clear(panel(kind));
        }
    }

    pub fn ensure_player_audio(&mut self, pose: Pose, _space: SpaceId) -> Result<(), String> {
        self.runtime
            .set_panel_width(PanelId::PlayerAudio, PanelSpace::Stage, xr_pose(pose), 0.75);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_pointer_ignores_a_trigger_held_when_the_quad_opens() {
        let mut pointer = DashboardPointer {
            triggers: [true, false],
            ..Default::default()
        };
        let hit = Some([0.5, 0.5]);
        let opened = pointer.events([hit, None], [true, false], Some(0));
        assert!(
            !opened
                .iter()
                .any(|event| matches!(event, DashboardPointerEvent::Down { .. }))
        );
        assert!(
            pointer
                .events([hit, None], [false, false], Some(0))
                .is_empty()
        );
        assert!(matches!(
            pointer
                .events([hit, None], [true, false], Some(0))
                .as_slice(),
            [DashboardPointerEvent::Down { .. }]
        ));
        assert!(matches!(
            pointer
                .events([hit, None], [false, false], Some(0))
                .as_slice(),
            [DashboardPointerEvent::Up { .. }]
        ));
    }

    #[test]
    fn dashboard_pointer_releases_outside_without_activating() {
        let mut pointer = DashboardPointer::default();
        let hit = Some([0.5, 0.5]);
        assert!(
            pointer
                .events([hit, None], [true, false], Some(0))
                .iter()
                .any(|event| matches!(event, DashboardPointerEvent::Down { .. }))
        );
        assert!(matches!(
            pointer.events([None, None], [true, false], None).as_slice(),
            [DashboardPointerEvent::Up { x: -1.0, y: -1.0 }]
        ));
        assert!(
            pointer
                .events([None, None], [false, false], None)
                .is_empty()
        );
        assert!(pointer.pressed.is_none());
    }

    #[test]
    fn dashboard_pointer_release_after_leaving_does_not_use_the_last_hover() {
        let mut pointer = DashboardPointer::default();
        let hit = Some([0.25, 0.25]);
        let _ = pointer.events([hit, None], [true, false], Some(0));
        assert!(matches!(
            pointer
                .events([None, None], [false, false], None)
                .as_slice(),
            [DashboardPointerEvent::Up { x: -1.0, y: -1.0 }]
        ));
    }

    #[test]
    fn straight_alpha_is_premultiplied_for_xr_blending() {
        let mut pixels = [255u8, 128, 0, 128, 10, 20, 30, 255];
        premultiply(&mut pixels);
        assert_eq!(&pixels[0..4], &[128, 64, 0, 128]);
        assert_eq!(&pixels[4..8], &[10, 20, 30, 255]);
    }

    #[test]
    fn pose_round_trips_through_the_xr_quaternion() {
        let matrix = transform::matrix(20.0, -35.0, 10.0, [0.2, 1.6, -0.4]);
        let round_trip = matrix_pose(xr_pose(matrix));
        for row in 0..3 {
            for column in 0..4 {
                assert!(
                    (round_trip[row][column] - matrix[row][column]).abs() < 1e-5,
                    "{row},{column}"
                );
            }
        }
    }
}
