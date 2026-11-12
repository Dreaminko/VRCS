//! OpenXR overlay implementation behind the shared `Backend` facade.
//!
//! The runtime owns the session, swapchains and quad layers. This adapter maps
//! the neutral backend calls onto `XrRuntime`; OpenVR-only values never cross
//! back into the shared layer.

use std::time::Duration;

use openxr as xr;
use vrcs_core::{VrOverlayHeadsetConfig, VrOverlayWristConfig};

use super::backend::{menu_config, ControllerBinding, OverlayKind};
use super::pose::{
    pose_from_quaternion, DeviceId, PointerId, PointerInput, Pose, SpaceId, TrackedPose,
};
use super::transform;
use super::xr::{PanelId, PanelSpace, XrRuntime};

pub struct OpenXrBackend {
    runtime: XrRuntime,
}

fn panel(kind: OverlayKind) -> PanelId {
    match kind {
        OverlayKind::PlayerAudio => PanelId::PlayerAudio,
        OverlayKind::Headset => PanelId::Headset,
        OverlayKind::Wrist => PanelId::Wrist,
        OverlayKind::WristMenu => PanelId::WristMenu,
        OverlayKind::OcrWrist => PanelId::OcrWrist,
        OverlayKind::OcrProgress => PanelId::OcrProgress,
        OverlayKind::OcrFrame => PanelId::OcrFrame,
        OverlayKind::OcrResult => PanelId::OcrResult,
        OverlayKind::Dashboard => PanelId::Dashboard,
    }
}

const ALL_KINDS: [OverlayKind; 9] = [
    OverlayKind::PlayerAudio,
    OverlayKind::Headset,
    OverlayKind::Wrist,
    OverlayKind::WristMenu,
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
        let s = (trace + 1.0).sqrt() * 2.0;
        (
            (m[2][1] - m[1][2]) / s,
            (m[0][2] - m[2][0]) / s,
            (m[1][0] - m[0][1]) / s,
            0.25 * s,
        )
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        (
            0.25 * s,
            (m[0][1] + m[1][0]) / s,
            (m[0][2] + m[2][0]) / s,
            (m[2][1] - m[1][2]) / s,
        )
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        (
            (m[0][1] + m[1][0]) / s,
            0.25 * s,
            (m[1][2] + m[2][1]) / s,
            (m[0][2] - m[2][0]) / s,
        )
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        (
            (m[0][2] + m[2][0]) / s,
            (m[1][2] + m[2][1]) / s,
            0.25 * s,
            (m[1][0] - m[0][1]) / s,
        )
    };
    xr::Posef {
        orientation: xr::Quaternionf { x, y, z, w },
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
    if index == 0 {
        "left"
    } else {
        "right"
    }
}

/// The Linux WebKit host returns straight-alpha RGBA; OpenXR layer blending
/// consumes premultiplied colour.
fn premultiply(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        for channel in 0..3 {
            pixel[channel] = ((u32::from(pixel[channel]) * alpha + 127) / 255) as u8;
        }
    }
}

impl OpenXrBackend {
    pub fn connect() -> Result<Self, String> {
        Ok(Self {
            runtime: XrRuntime::open(false)?,
        })
    }

    pub fn quit_requested(&mut self) -> bool {
        false
    }

    pub fn begin_frame(&mut self) -> Result<bool, String> {
        self.runtime.begin_frame()
    }

    pub fn end_frame(&mut self) -> Result<(), String> {
        self.runtime.end_frame()
    }

    pub fn surface_adapter_index(&self) -> Result<u32, String> {
        Ok(0)
    }

    pub fn ensure_headset(&mut self, config: &VrOverlayHeadsetConfig) -> Result<(), String> {
        self.runtime.set_panel(
            PanelId::Headset,
            PanelSpace::View,
            xr_pose(transform::headset(config)),
            width(config.width_m),
        );
        Ok(())
    }

    pub fn ensure_wrist(
        &mut self,
        config: &VrOverlayWristConfig,
    ) -> Result<ControllerBinding, String> {
        let hand = hand_index(config);
        self.runtime.set_panel(
            PanelId::Wrist,
            PanelSpace::Grip(hand),
            xr_pose(transform::wrist(config)),
            width(config.width_m),
        );
        Ok(ControllerBinding {
            role: Some(hand_name(hand).into()),
            available: self.runtime.controller_pose(hand).is_some(),
        })
    }

    pub fn ensure_wrist_menu(&mut self, hand: &str) -> Result<(), String> {
        let config = menu_config(hand);
        let index = usize::from(hand == "right");
        self.runtime.set_panel(
            PanelId::WristMenu,
            PanelSpace::Grip(index),
            xr_pose(transform::wrist(&config)),
            width(config.width_m),
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
            .set_panel(PanelId::Wrist, space, xr_pose(pose), width(config.width_m));
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

    pub fn controller_device(&self, config: &VrOverlayWristConfig) -> Option<DeviceId> {
        let hand = hand_index(config);
        self.runtime
            .controller_pose(hand)
            .map(|_| DeviceId(hand as u32))
    }

    pub fn wrist_pose(&self, config: &VrOverlayWristConfig) -> Option<Pose> {
        let hand = hand_index(config);
        let controller = self.runtime.controller_pose(hand)?;
        Some(transform::compose(
            controller.pose,
            transform::wrist(config),
        ))
    }

    pub fn wrist_angle(&self, config: &VrOverlayWristConfig) -> Option<f32> {
        let panel = self.wrist_pose(config)?;
        let head = self.ocr_tracking().ok()?.0;
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
    }

    pub fn intersect(
        &self,
        kind: OverlayKind,
        _device: DeviceId,
        ray: TrackedPose,
    ) -> Option<([f32; 2], [f32; 3], f32)> {
        let info = self.runtime.panel(panel(kind))?;
        if !info.visible {
            return None;
        }
        let panel_pose = matrix_pose(info.pose);
        let world = match info.space {
            PanelSpace::Stage => panel_pose,
            PanelSpace::View => transform::compose(self.ocr_tracking().ok()?.0, panel_pose),
            PanelSpace::Grip(hand) => {
                transform::compose(self.runtime.controller_pose(hand)?.pose, panel_pose)
            }
        };
        let local = transform::compose(transform::inverse(world), ray.pose);
        let origin = [local[0][3], local[1][3], local[2][3]];
        let direction = [-local[0][2], -local[1][2], -local[2][2]];
        if direction[2].abs() <= 1e-5 {
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
        Some((uv, world_hit, distance))
    }

    pub fn cursor_interval(&self) -> Duration {
        Duration::from_millis(16)
    }

    pub fn update_cursor(
        &mut self,
        _target: Option<(OverlayKind, [f32; 3], SpaceId, u32)>,
    ) -> Result<(), String> {
        Ok(())
    }

    pub fn pointer_inputs(&self) -> Vec<PointerInput> {
        self.runtime.pointer_inputs()
    }

    pub fn update_ocr_input(&mut self, _path: &std::path::Path) -> Result<(), String> {
        Ok(())
    }

    pub fn ocr_input(
        &mut self,
        _gesture_enabled: bool,
        _reading_focus: bool,
        _vision_enabled: bool,
    ) -> Result<crate::vr_overlay::ocr_input::InputActions, String> {
        Err("vr.ocr_input_unavailable".into())
    }

    pub fn freeze_ocr_frame(&mut self) {}

    pub fn open_ocr_bindings(&mut self) -> Result<(), String> {
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
        false
    }

    pub fn ensure_dashboard(&mut self) -> Result<(), String> {
        self.runtime.set_panel(
            PanelId::Dashboard,
            PanelSpace::View,
            xr::Posef {
                orientation: xr::Quaternionf::IDENTITY,
                position: xr::Vector3f {
                    x: 0.0,
                    y: 0.0,
                    z: -1.2,
                },
            },
            width(1.0),
        );
        Ok(())
    }

    pub fn show_dashboard_placeholder(&mut self, _error: Option<&str>) -> Result<(), String> {
        Ok(())
    }

    pub fn dashboard_visible(&self) -> bool {
        self.runtime.visible(PanelId::Dashboard)
    }

    pub fn poll_dashboard_events(
        &self,
    ) -> Result<Vec<crate::vr_overlay::dashboard::DashboardPointerEvent>, String> {
        Ok(Vec::new())
    }

    pub fn visible(&self, kind: OverlayKind) -> bool {
        self.runtime.visible(panel(kind))
    }

    pub fn tracking_reset(&mut self) -> bool {
        false
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
        let mut pixels = texture.pixels.clone();
        premultiply(&mut pixels);
        self.runtime
            .upload(panel(kind), &pixels, texture.width, texture.height)
    }

    pub fn clear_surface_texture(&mut self, kind: OverlayKind) -> Result<(), String> {
        self.runtime.clear(panel(kind));
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
    }

    pub fn hide_all(&mut self) {
        for kind in ALL_KINDS {
            self.runtime.set_visible(panel(kind), false);
        }
    }

    pub fn reset(&mut self, kind: OverlayKind) {
        self.hide(kind);
        self.runtime.clear(panel(kind));
    }

    pub fn ensure_player_audio(&mut self, pose: Pose, _space: SpaceId) -> Result<(), String> {
        self.runtime.set_panel(
            PanelId::PlayerAudio,
            PanelSpace::Stage,
            xr_pose(pose),
            width(0.75),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
