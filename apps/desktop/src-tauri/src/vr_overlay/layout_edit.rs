use serde::{Deserialize, Serialize};
use vrcs_core::{
    VrOverlayConfig, VrOverlayHeadsetConfig, VrOverlayWristConfig, VrOverlayWristMenuConfig,
};

use super::placement::Pose;
use super::transform::{
    WRIST_MENU_CAPSULE_OFFSET, compose, decompose, face_point, inverse, level_roll, matrix, wrist,
    wrist_menu,
};

pub const EDIT_TOOLBAR_COMMIT_EVENT: &str = "vr-overlay-layout-edit-commit";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditTarget {
    Headset,
    Wrist,
    WristMenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrabTarget {
    Component(EditTarget),
    Toolbar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditPhase {
    Inactive,
    WaitingForDashboardClose,
    Active,
    Saving { request_id: u64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditDraft {
    pub headset: VrOverlayHeadsetConfig,
    pub wrist: VrOverlayWristConfig,
    pub wrist_menu: VrOverlayWristMenuConfig,
}

impl EditDraft {
    pub fn from_config(config: &VrOverlayConfig) -> Self {
        Self {
            headset: config.headset.clone(),
            wrist: config.wrist.clone(),
            wrist_menu: config.wrist_menu.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EditHeadsetPatch {
    pub offset_x_m: f32,
    pub offset_y_m: f32,
    pub distance_m: f32,
    pub pitch_deg: f32,
    pub yaw_deg: f32,
    pub roll_deg: f32,
    pub width_m: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EditWristPatch {
    pub offset_x_m: f32,
    pub offset_y_m: f32,
    pub offset_z_m: f32,
    pub pitch_deg: f32,
    pub yaw_deg: f32,
    pub roll_deg: f32,
    pub width_m: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EditWristMenuPatch {
    pub offset_x_m: f32,
    pub offset_y_m: f32,
    pub offset_z_m: f32,
    pub pitch_deg: f32,
    pub yaw_deg: f32,
    pub roll_deg: f32,
    pub width_m: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EditCommitRequest {
    pub owner_epoch: u32,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headset: Option<EditHeadsetPatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wrist: Option<EditWristPatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wrist_menu: Option<EditWristMenuPatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditToolbarAction {
    ToggleAutoLevel,
    ToggleFaceHead,
    ToggleVisible(EditTarget),
    ResetSelected,
    ResetAll,
    Cancel,
    Save,
}

#[derive(Debug, Clone)]
enum GrabKind {
    MoveFree {
        relative: Pose,
        ray_push: f32,
    },
    ResizeUniform {
        plane: Pose,
        initial_width: f32,
        anchor_local: [f32; 2],
        axis_dir: [f32; 2],
        initial_proj: f32,
    },
}

#[derive(Debug, Clone)]
struct ActiveGrab {
    target: GrabTarget,
    device: u32,
    kind: GrabKind,
}

#[derive(Debug, Clone)]
pub struct LayoutEditSession {
    pub phase: EditPhase,
    pub original: Option<EditDraft>,
    pub draft: Option<EditDraft>,
    grab: Option<ActiveGrab>,
    pub headset_world_override: Option<Pose>,
    pub selected: Option<EditTarget>,
    pub auto_level: bool,
    pub face_head: bool,
    pub visible: [bool; 3],
    pub toolbar_pose: Option<Pose>,
    pub error: Option<String>,
    sequence: u64,
    pub revision: u64,
}

impl Default for LayoutEditSession {
    fn default() -> Self {
        Self {
            phase: EditPhase::Inactive,
            original: None,
            draft: None,
            grab: None,
            headset_world_override: None,
            selected: None,
            auto_level: false,
            face_head: false,
            visible: [true, true, true],
            toolbar_pose: None,
            error: None,
            sequence: 0,
            revision: 0,
        }
    }
}

fn round_cm(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn round_width(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

fn round_deg(v: f32) -> f32 {
    v.round()
}

fn wrap_deg(deg: f32) -> f32 {
    let mut a = (deg + 180.0).rem_euclid(360.0) - 180.0;
    if a == -180.0 && deg > 0.0 {
        a = 180.0;
    }
    a
}

fn valid_pose(pose: Pose) -> bool {
    pose.iter().flatten().all(|v| v.is_finite())
}

fn default_toolbar_pose(head: Pose) -> Pose {
    let candidate = compose(
        head,
        super::transform::matrix(-14.0, 0.0, 0.0, [0.0, -0.34, -0.92]),
    );
    level_roll(candidate)
}

fn target_index(target: EditTarget) -> usize {
    match target {
        EditTarget::Headset => 0,
        EditTarget::Wrist => 1,
        EditTarget::WristMenu => 2,
    }
}

fn apply_headset_local(cfg: &mut VrOverlayHeadsetConfig, local: Pose) {
    let (pitch, yaw, roll, t) = decompose(local);
    cfg.offset_x_m = round_cm(t[0].clamp(-2.0, 2.0));
    cfg.offset_y_m = round_cm(t[1].clamp(-2.0, 2.0));
    cfg.distance_m = round_cm((-t[2]).clamp(0.25, 5.0));
    cfg.pitch_deg = round_deg(pitch.clamp(-90.0, 90.0));
    cfg.yaw_deg = round_deg(wrap_deg(yaw).clamp(-180.0, 180.0));
    cfg.roll_deg = round_deg(wrap_deg(roll).clamp(-180.0, 180.0));
}

fn apply_wrist_local(cfg: &mut VrOverlayWristConfig, local: Pose) {
    let (pitch, yaw, roll, t) = decompose(local);
    cfg.offset_x_m = round_cm(t[0].clamp(-0.5, 0.5));
    cfg.offset_y_m = round_cm(t[1].clamp(-0.5, 0.5));
    cfg.offset_z_m = round_cm(t[2].clamp(-0.5, 0.5));
    cfg.pitch_deg = round_deg(wrap_deg(pitch).clamp(-180.0, 180.0));
    cfg.yaw_deg = round_deg(wrap_deg(yaw).clamp(-180.0, 180.0));
    cfg.roll_deg = round_deg(wrap_deg(roll).clamp(-180.0, 180.0));
}

fn apply_wrist_menu_local(cfg: &mut VrOverlayWristMenuConfig, local: Pose) {
    let (pitch, yaw, roll, t) = decompose(local);
    cfg.offset_x_m = round_cm(t[0].clamp(-0.5, 0.5));
    cfg.offset_y_m = round_cm(t[1].clamp(-0.5, 0.5));
    cfg.offset_z_m = round_cm(t[2].clamp(-0.5, 0.5));
    cfg.pitch_deg = round_deg(wrap_deg(pitch).clamp(-180.0, 180.0));
    cfg.yaw_deg = round_deg(wrap_deg(yaw).clamp(-180.0, 180.0));
    cfg.roll_deg = round_deg(wrap_deg(roll).clamp(-180.0, 180.0));
}

fn intersect_ray_plane(ray: Pose, plane: Pose) -> Option<[f32; 2]> {
    let local = compose(inverse(plane), ray);
    let origin = [local[0][3], local[1][3], local[2][3]];
    let dir = [-local[0][2], -local[1][2], -local[2][2]];
    if dir[2].abs() < 0.08 {
        return None;
    }
    let t = -origin[2] / dir[2];
    if !t.is_finite() || t <= 0.0 {
        return None;
    }
    Some([origin[0] + dir[0] * t, origin[1] + dir[1] * t])
}

impl LayoutEditSession {
    pub fn is_active(&self) -> bool {
        matches!(self.phase, EditPhase::Active | EditPhase::Saving { .. })
    }

    pub fn is_saving(&self) -> bool {
        matches!(self.phase, EditPhase::Saving { .. })
    }

    pub fn grabbing_device(&self) -> Option<u32> {
        self.grab.as_ref().map(|g| g.device)
    }

    pub fn grabbing_target(&self) -> Option<GrabTarget> {
        self.grab.as_ref().map(|g| g.target)
    }

    pub fn is_resizing(&self) -> bool {
        matches!(
            self.grab.as_ref().map(|g| &g.kind),
            Some(GrabKind::ResizeUniform { .. })
        )
    }

    pub fn component_visible(&self, target: EditTarget) -> bool {
        self.draft.is_some() && self.visible[target_index(target)]
    }

    pub fn is_dirty(&self) -> bool {
        let (Some(orig), Some(draft)) = (&self.original, &self.draft) else {
            return false;
        };
        orig != draft
    }

    pub fn enter(&mut self, config: &VrOverlayConfig, dashboard_open: bool, head: Option<Pose>) {
        let draft = EditDraft::from_config(config);
        self.original = Some(draft.clone());
        self.draft = Some(draft);
        self.grab = None;
        self.headset_world_override = None;
        self.selected = None;
        self.visible = [true, true, true];
        self.toolbar_pose = head.filter(|p| valid_pose(*p)).map(default_toolbar_pose);
        self.error = None;
        self.phase = if dashboard_open {
            EditPhase::WaitingForDashboardClose
        } else {
            EditPhase::Active
        };
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn on_dashboard_visibility(&mut self, visible: bool, head: Option<Pose>) {
        if self.phase == EditPhase::WaitingForDashboardClose && !visible {
            self.phase = EditPhase::Active;
            if self.toolbar_pose.is_none() {
                self.toolbar_pose = head.filter(|p| valid_pose(*p)).map(default_toolbar_pose);
            }
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn ensure_toolbar_pose(&mut self, head: Option<Pose>) {
        if self.toolbar_pose.is_none() {
            self.toolbar_pose = head.filter(|p| valid_pose(*p)).map(default_toolbar_pose);
        }
    }

    pub fn begin_move(
        &mut self,
        target: GrabTarget,
        device: u32,
        controller_pose: Pose,
        panel_world: Pose,
    ) -> bool {
        if self.phase != EditPhase::Active
            || self.grab.is_some()
            || !valid_pose(controller_pose)
            || !valid_pose(panel_world)
        {
            return false;
        }
        let Some(_) = &self.draft else {
            return false;
        };
        self.error = None;
        match target {
            GrabTarget::Component(EditTarget::WristMenu) => {
                self.selected = Some(EditTarget::WristMenu);
                self.grab = Some(ActiveGrab {
                    target,
                    device,
                    kind: GrabKind::MoveFree {
                        relative: compose(inverse(controller_pose), panel_world),
                        ray_push: 0.0,
                    },
                });
            }
            GrabTarget::Component(EditTarget::Headset) => {
                self.selected = Some(EditTarget::Headset);
                self.headset_world_override = Some(panel_world);
                self.grab = Some(ActiveGrab {
                    target,
                    device,
                    kind: GrabKind::MoveFree {
                        relative: compose(inverse(controller_pose), panel_world),
                        ray_push: 0.0,
                    },
                });
            }
            GrabTarget::Component(EditTarget::Wrist) => {
                self.selected = Some(EditTarget::Wrist);
                self.grab = Some(ActiveGrab {
                    target,
                    device,
                    kind: GrabKind::MoveFree {
                        relative: compose(inverse(controller_pose), panel_world),
                        ray_push: 0.0,
                    },
                });
            }
            GrabTarget::Toolbar => {
                self.grab = Some(ActiveGrab {
                    target,
                    device,
                    kind: GrabKind::MoveFree {
                        relative: compose(inverse(controller_pose), panel_world),
                        ray_push: 0.0,
                    },
                });
            }
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn begin_resize(
        &mut self,
        target: EditTarget,
        device: u32,
        ray_pose: Pose,
        panel_world: Pose,
    ) -> bool {
        if self.phase != EditPhase::Active
            || self.grab.is_some()
            || !valid_pose(ray_pose)
            || !valid_pose(panel_world)
        {
            return false;
        }
        let Some(draft) = &self.draft else {
            return false;
        };
        let initial_width = match target {
            EditTarget::Headset => draft.headset.width_m,
            EditTarget::Wrist => draft.wrist.width_m,
            EditTarget::WristMenu => draft.wrist_menu.width_m,
        };
        let anchor_local = match target {
            EditTarget::Headset | EditTarget::Wrist => [0.0, 0.0],
            EditTarget::WristMenu => [0.0, -WRIST_MENU_CAPSULE_OFFSET * initial_width],
        };
        let Some(hit) = intersect_ray_plane(ray_pose, panel_world) else {
            return false;
        };
        let d0 = [hit[0] - anchor_local[0], hit[1] - anchor_local[1]];
        let s0 = d0[0].hypot(d0[1]);
        if !s0.is_finite() || s0 < 1e-3 {
            return false;
        }
        self.error = None;
        self.selected = Some(target);
        self.grab = Some(ActiveGrab {
            target: GrabTarget::Component(target),
            device,
            kind: GrabKind::ResizeUniform {
                plane: panel_world,
                initial_width,
                anchor_local,
                axis_dir: [d0[0] / s0, d0[1] / s0],
                initial_proj: s0,
            },
        });
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn update_drag(
        &mut self,
        device: u32,
        controller_pose: Pose,
        ray_pose: Option<Pose>,
        head_pose: Option<Pose>,
        anchor_pose: Option<Pose>,
        scroll_y: f32,
        dt_secs: f32,
    ) -> bool {
        let Some(grab) = &mut self.grab else {
            return false;
        };
        if grab.device != device || !valid_pose(controller_pose) {
            return false;
        }
        let Some(draft) = &mut self.draft else {
            return false;
        };
        let dt = dt_secs.clamp(0.0, 0.05);
        match &mut grab.kind {
            GrabKind::MoveFree { relative, ray_push } => {
                if scroll_y.is_finite() && scroll_y.abs() > 0.01 {
                    *ray_push += scroll_y * dt * 1.2;
                }
                let mut world = compose(controller_pose, *relative);
                if *ray_push != 0.0 {
                    // Push along controller -Z pointing axis.
                    for i in 0..3 {
                        world[i][3] += -controller_pose[i][2] * *ray_push;
                    }
                }
                match grab.target {
                    GrabTarget::Component(EditTarget::Headset) => {
                        if self.face_head
                            && let Some(head) = head_pose
                            && let Some(faced) = face_point(
                                [world[0][3], world[1][3], world[2][3]],
                                [head[0][3], head[1][3], head[2][3]],
                            )
                        {
                            world = faced;
                        } else if self.auto_level {
                            world = level_roll(world);
                        }
                        self.headset_world_override = Some(world);
                        if let Some(head) = head_pose {
                            apply_headset_local(&mut draft.headset, compose(inverse(head), world));
                        }
                    }
                    GrabTarget::Component(EditTarget::Wrist) => {
                        let Some(anchor) = anchor_pose.filter(|p| valid_pose(*p)) else {
                            return false;
                        };
                        if self.auto_level {
                            world = level_roll(world);
                        }
                        let local = compose(inverse(anchor), world);
                        apply_wrist_local(&mut draft.wrist, local);
                    }
                    GrabTarget::Toolbar => {
                        self.toolbar_pose = Some(level_roll(world));
                    }
                    GrabTarget::Component(EditTarget::WristMenu) => {
                        let Some(anchor) = anchor_pose.filter(|p| valid_pose(*p)) else {
                            return false;
                        };
                        if self.auto_level {
                            world = level_roll(world);
                        }
                        let local_quad = compose(inverse(anchor), world);
                        let local_capsule = compose(
                            local_quad,
                            matrix(
                                0.0,
                                0.0,
                                0.0,
                                [0.0, -WRIST_MENU_CAPSULE_OFFSET * draft.wrist_menu.width_m, 0.0],
                            ),
                        );
                        apply_wrist_menu_local(&mut draft.wrist_menu, local_capsule);
                    }
                }
                true
            }
            GrabKind::ResizeUniform {
                plane,
                initial_width,
                anchor_local,
                axis_dir,
                initial_proj,
            } => {
                let Some(ray) = ray_pose.filter(|p| valid_pose(*p)) else {
                    return false;
                };
                let Some(hit) = intersect_ray_plane(ray, *plane) else {
                    return false;
                };
                let proj = (hit[0] - anchor_local[0]) * axis_dir[0]
                    + (hit[1] - anchor_local[1]) * axis_dir[1];
                if !proj.is_finite() || proj <= 1e-3 {
                    return false;
                }
                let ratio = proj / *initial_proj;
                let GrabTarget::Component(target) = grab.target else {
                    return false;
                };
                match target {
                    EditTarget::Headset => {
                        draft.headset.width_m = round_cm((*initial_width * ratio).clamp(0.25, 3.0));
                    }
                    EditTarget::Wrist => {
                        draft.wrist.width_m = round_cm((*initial_width * ratio).clamp(0.10, 1.0));
                    }
                    EditTarget::WristMenu => {
                        draft.wrist_menu.width_m =
                            round_width((*initial_width * ratio).clamp(0.12, 0.50));
                    }
                }
                true
            }
        }
    }

    pub fn end_grab(&mut self, device: u32, head_pose: Option<Pose>) -> bool {
        let Some(grab) = self.grab.as_ref() else {
            return false;
        };
        if grab.device != device {
            return false;
        }
        let grab = self.grab.take().expect("checked grab");
        if grab.target == GrabTarget::Component(EditTarget::Headset)
            && let Some(world) = self.headset_world_override.take()
            && let Some(head) = head_pose.filter(|p| valid_pose(*p))
            && let Some(draft) = &mut self.draft
        {
            apply_headset_local(&mut draft.headset, compose(inverse(head), world));
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn cancel_grab(&mut self) {
        if self.grab.take().is_some() {
            self.headset_world_override = None;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn apply_toolbar_action(&mut self, action: EditToolbarAction) {
        if action == EditToolbarAction::Cancel {
            self.cancel();
            return;
        }
        if self.phase != EditPhase::Active {
            return;
        }
        match action {
            EditToolbarAction::ToggleAutoLevel => self.auto_level = !self.auto_level,
            EditToolbarAction::ToggleFaceHead => self.face_head = !self.face_head,
            EditToolbarAction::ToggleVisible(target) => {
                let idx = target_index(target);
                self.visible[idx] = !self.visible[idx];
            }
            EditToolbarAction::ResetSelected => {
                if let Some(target) = self.selected {
                    self.reset_target(target);
                }
            }
            EditToolbarAction::ResetAll => self.reset_all(),
            EditToolbarAction::Cancel | EditToolbarAction::Save => {}
        }
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn reset_target(&mut self, target: EditTarget) {
        if self.draft.is_none() {
            return;
        }
        self.cancel_grab();
        let draft = self.draft.as_mut().expect("draft checked above");
        match target {
            EditTarget::Headset => {
                let def = VrOverlayHeadsetConfig::default();
                draft.headset.offset_x_m = def.offset_x_m;
                draft.headset.offset_y_m = def.offset_y_m;
                draft.headset.distance_m = def.distance_m;
                draft.headset.pitch_deg = def.pitch_deg;
                draft.headset.yaw_deg = def.yaw_deg;
                draft.headset.roll_deg = def.roll_deg;
                draft.headset.width_m = def.width_m;
            }
            EditTarget::Wrist => {
                let def = VrOverlayWristConfig::default();
                draft.wrist.offset_x_m = def.offset_x_m;
                draft.wrist.offset_y_m = def.offset_y_m;
                draft.wrist.offset_z_m = def.offset_z_m;
                draft.wrist.pitch_deg = def.pitch_deg;
                draft.wrist.yaw_deg = def.yaw_deg;
                draft.wrist.roll_deg = def.roll_deg;
                draft.wrist.width_m = def.width_m;
            }
            EditTarget::WristMenu => {
                let def = VrOverlayWristMenuConfig::default();
                draft.wrist_menu.offset_x_m = def.offset_x_m;
                draft.wrist_menu.offset_y_m = def.offset_y_m;
                draft.wrist_menu.offset_z_m = def.offset_z_m;
                draft.wrist_menu.pitch_deg = def.pitch_deg;
                draft.wrist_menu.yaw_deg = def.yaw_deg;
                draft.wrist_menu.roll_deg = def.roll_deg;
                draft.wrist_menu.width_m = def.width_m;
            }
        }
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn reset_all(&mut self) {
        self.reset_target(EditTarget::Headset);
        self.reset_target(EditTarget::Wrist);
        self.reset_target(EditTarget::WristMenu);
    }

    pub fn begin_commit(&mut self, owner_epoch: u32) -> Result<Option<EditCommitRequest>, String> {
        if !self.is_active() || self.is_saving() {
            return Ok(None);
        }
        self.cancel_grab();
        let (Some(orig), Some(draft)) = (&self.original, &self.draft) else {
            self.cancel();
            return Ok(None);
        };
        let headset_changed = orig.headset != draft.headset;
        let wrist_changed = orig.wrist != draft.wrist;
        let menu_changed = orig.wrist_menu != draft.wrist_menu;
        if !headset_changed && !wrist_changed && !menu_changed {
            self.cancel();
            return Ok(None);
        }
        if owner_epoch == 0 {
            let err = "Controls are unavailable".to_string();
            self.error = Some(err.clone());
            self.revision = self.revision.wrapping_add(1);
            return Err(err);
        }
        self.sequence = self.sequence.wrapping_add(1);
        let id = self.sequence;
        self.error = None;
        self.phase = EditPhase::Saving { request_id: id };
        self.revision = self.revision.wrapping_add(1);
        Ok(Some(EditCommitRequest {
            owner_epoch,
            id,
            headset: headset_changed.then(|| EditHeadsetPatch {
                offset_x_m: draft.headset.offset_x_m,
                offset_y_m: draft.headset.offset_y_m,
                distance_m: draft.headset.distance_m,
                pitch_deg: draft.headset.pitch_deg,
                yaw_deg: draft.headset.yaw_deg,
                roll_deg: draft.headset.roll_deg,
                width_m: draft.headset.width_m,
            }),
            wrist: wrist_changed.then(|| EditWristPatch {
                offset_x_m: draft.wrist.offset_x_m,
                offset_y_m: draft.wrist.offset_y_m,
                offset_z_m: draft.wrist.offset_z_m,
                pitch_deg: draft.wrist.pitch_deg,
                yaw_deg: draft.wrist.yaw_deg,
                roll_deg: draft.wrist.roll_deg,
                width_m: draft.wrist.width_m,
            }),
            wrist_menu: menu_changed.then(|| EditWristMenuPatch {
                offset_x_m: draft.wrist_menu.offset_x_m,
                offset_y_m: draft.wrist_menu.offset_y_m,
                offset_z_m: draft.wrist_menu.offset_z_m,
                pitch_deg: draft.wrist_menu.pitch_deg,
                yaw_deg: draft.wrist_menu.yaw_deg,
                roll_deg: draft.wrist_menu.roll_deg,
                width_m: draft.wrist_menu.width_m,
            }),
        }))
    }

    pub fn resolve_commit(&mut self, id: u64, error: Option<String>) -> bool {
        let EditPhase::Saving { request_id } = self.phase else {
            return false;
        };
        if request_id != id {
            return false;
        }
        if let Some(error) = error {
            self.phase = EditPhase::Active;
            self.error = Some(error);
        } else {
            self.cancel();
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn cancel(&mut self) {
        self.phase = EditPhase::Inactive;
        self.original = None;
        self.draft = None;
        self.grab = None;
        self.headset_world_override = None;
        self.selected = None;
        self.toolbar_pose = None;
        self.error = None;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn headset_world_pose(&self, head: Pose) -> Option<Pose> {
        self.headset_world_override.or_else(|| {
            self.draft
                .as_ref()
                .map(|draft| compose(head, super::transform::headset(&draft.headset)))
        })
    }

    pub fn wrist_world_pose(&self, anchor: Pose) -> Option<Pose> {
        self.draft
            .as_ref()
            .map(|draft| compose(anchor, wrist(&draft.wrist)))
    }

    pub fn wrist_menu_world_pose(&self, anchor: Pose) -> Option<Pose> {
        self.draft
            .as_ref()
            .map(|draft| compose(anchor, wrist_menu(&draft.wrist_menu)))
    }

    #[cfg(any(windows, linux_vr))]
    pub fn surface_overlay(
        &self,
        target: EditTarget,
        labels: &super::dashboard::DashboardLabels,
    ) -> Option<super::surface_protocol::EditSurfaceOverlay> {
        if !self.is_active() {
            return None;
        }
        let draft = self.draft.as_ref()?;
        let label_or = |value: &str, fallback: &str| {
            if value.trim().is_empty() {
                fallback.to_owned()
            } else {
                value.to_owned()
            }
        };
        let (title, scale_label, shared_hint) = match target {
            EditTarget::Headset => (
                label_or(&labels.headset, "Headset"),
                format!("{:.2}m", draft.headset.width_m),
                None,
            ),
            EditTarget::Wrist => (
                label_or(&labels.wrist, "Wrist"),
                format!("{:.2}m", draft.wrist.width_m),
                Some(label_or(
                    &labels.layout_edit_shared_wrist,
                    "OCR / Vision shared",
                )),
            ),
            EditTarget::WristMenu => (
                label_or(&labels.wrist_menu, "Wrist Menu"),
                format!("{:.2}m", draft.wrist_menu.width_m),
                None,
            ),
        };
        let grabbing = self.grabbing_target() == Some(GrabTarget::Component(target));
        let resizing = grabbing && self.is_resizing();
        Some(super::surface_protocol::EditSurfaceOverlay {
            title,
            scale_label,
            selected: self.selected == Some(target),
            active_grab: grabbing && !resizing,
            active_resize: resizing,
            shared_hint,
        })
    }

    #[cfg(any(windows, linux_vr))]
    pub fn toolbar_view(
        &self,
        labels: &super::dashboard::DashboardLabels,
    ) -> Option<super::surface_protocol::EditToolbarView> {
        if !self.is_active() {
            return None;
        }
        let label_or = |value: &str, fallback: &str| {
            if value.trim().is_empty() {
                fallback.to_owned()
            } else {
                value.to_owned()
            }
        };
        let target_label = match self.selected {
            Some(EditTarget::Headset) => label_or(&labels.headset, "Headset"),
            Some(EditTarget::Wrist) => label_or(&labels.wrist, "Wrist"),
            Some(EditTarget::WristMenu) => label_or(&labels.wrist_menu, "Wrist Menu"),
            None => String::new(),
        };
        let saving = self.is_saving();
        let dirty = self.is_dirty();
        let status_label = if saving {
            label_or(&labels.saving, "Saving...")
        } else if dirty {
            "•".into()
        } else {
            String::new()
        };
        Some(super::surface_protocol::EditToolbarView {
            title: label_or(&labels.layout_edit, "Layout Edit"),
            target_label,
            status_label,
            auto_level: self.auto_level,
            face_head: self.face_head,
            show_headset: self.component_visible(EditTarget::Headset),
            show_wrist: self.component_visible(EditTarget::Wrist),
            show_wrist_menu: self.component_visible(EditTarget::WristMenu),
            dirty,
            saving,
            error: self.error.clone(),
            label_auto_level: label_or(&labels.layout_edit_auto_level, "Level"),
            label_face_head: label_or(&labels.layout_edit_face_head, "Face HMD"),
            label_headset: label_or(&labels.headset, "Headset"),
            label_wrist: label_or(&labels.wrist, "Wrist"),
            label_wrist_menu: label_or(&labels.wrist_menu, "Menu"),
            label_reset_selected: label_or(&labels.layout_edit_reset_selected, "Reset Item"),
            label_reset_all: label_or(&labels.layout_edit_reset, "Reset All"),
            label_cancel: label_or(&labels.layout_edit_cancel, "Cancel"),
            label_save: label_or(&labels.layout_edit_save, "Save & Exit"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::transform::{headset as headset_matrix, matrix};
    use super::*;

    fn test_config() -> VrOverlayConfig {
        let mut cfg = VrOverlayConfig {
            enabled: true,
            ..Default::default()
        };
        cfg.wrist_menu.enabled = true;
        cfg
    }

    #[test]
    fn headset_grab_detaches_to_world_and_reprojects_on_release() {
        let mut session = LayoutEditSession::default();
        let cfg = test_config();
        let head1 = matrix(0.0, 0.0, 0.0, [0.0, 1.7, 0.0]);
        session.enter(&cfg, false, Some(head1));
        assert_eq!(session.phase, EditPhase::Active);

        let panel1 = compose(head1, headset_matrix(&cfg.headset));
        let ctrl1 = matrix(0.0, 0.0, 0.0, [0.2, 1.4, -0.4]);
        assert!(session.begin_move(GrabTarget::Component(EditTarget::Headset), 1, ctrl1, panel1));
        assert_eq!(session.headset_world_override, Some(panel1));

        // Move controller right by 0.25m while head turns; world pose follows controller.
        let ctrl2 = matrix(0.0, 0.0, 0.0, [0.45, 1.4, -0.4]);
        let head2 = matrix(0.0, 90.0, 0.0, [0.0, 1.7, 0.0]);
        assert!(session.update_drag(1, ctrl2, None, Some(head2), None, 0.0, 0.016));
        let world = session.headset_world_override.unwrap();
        assert!((world[0][3] - 0.25).abs() < 1e-4);

        assert!(session.end_grab(1, Some(head2)));
        assert!(session.headset_world_override.is_none());
        let saved = &session.draft.as_ref().unwrap().headset;
        assert!((saved.offset_x_m - 1.2).abs() < 0.02);
        assert!((saved.distance_m - 0.25).abs() < 0.02);
    }

    #[test]
    fn wrist_menu_moves_freely_in_6dof_and_scales_capsule_in_place() {
        let mut session = LayoutEditSession::default();
        let cfg = test_config();
        let anchor = matrix(0.0, 0.0, 0.0, [0.0, 1.0, -0.3]);
        session.enter(&cfg, false, None);
        assert!(!session.auto_level);
        session.apply_toolbar_action(EditToolbarAction::ToggleAutoLevel);
        assert!(session.auto_level);
        session.apply_toolbar_action(EditToolbarAction::ToggleAutoLevel);
        assert!(!session.auto_level);

        let panel = compose(anchor, wrist_menu(&cfg.wrist_menu));
        let ctrl = matrix(0.0, 0.0, 0.0, [0.0, 1.06, -0.12]);
        assert!(session.begin_move(
            GrabTarget::Component(EditTarget::WristMenu),
            2,
            ctrl,
            panel
        ));
        // Freely translate and rotate wrist menu relative to controller anchor.
        let moved_ctrl = matrix(15.0, 30.0, 45.0, [-0.08, 1.02, -0.06]);
        assert!(session.update_drag(2, moved_ctrl, None, None, Some(anchor), 0.0, 0.016));
        let moved_world = session.wrist_menu_world_pose(anchor).unwrap();
        let expected_capsule = compose(
            compose(moved_ctrl, compose(inverse(ctrl), panel)),
            matrix(
                0.0,
                0.0,
                0.0,
                [0.0, -WRIST_MENU_CAPSULE_OFFSET * cfg.wrist_menu.width_m, 0.0],
            ),
        );
        let actual_capsule = compose(
            moved_world,
            matrix(
                0.0,
                0.0,
                0.0,
                [0.0, -WRIST_MENU_CAPSULE_OFFSET * cfg.wrist_menu.width_m, 0.0],
            ),
        );
        for row in 0..3 {
            assert!((actual_capsule[row][3] - expected_capsule[row][3]).abs() < 0.02);
        }
        let menu = session.draft.as_ref().unwrap().wrist_menu.clone();
        assert!((menu.offset_x_m - (-0.08)).abs() <= 0.02);
        assert!(menu.yaw_deg.abs() > 5.0 || menu.roll_deg.abs() > 5.0);
        session.end_grab(2, None);

        // Resize from top-right corner by 1.5x while capsule stays anchored at its 6DoF pose.
        let panel_before = compose(anchor, wrist_menu(&menu));
        let old_w = menu.width_m;
        let ray_start = compose(
            panel_before,
            matrix(0.0, 0.0, 0.0, [old_w * 0.4, old_w * 0.4, 0.5]),
        );
        assert!(session.begin_resize(EditTarget::WristMenu, 2, ray_start, panel_before));
        let ray_scaled = compose(
            panel_before,
            matrix(0.0, 0.0, 0.0, [old_w * 0.6, old_w * 0.8, 0.5]),
        );
        assert!(session.update_drag(
            2,
            ray_scaled,
            Some(ray_scaled),
            None,
            Some(anchor),
            0.0,
            0.016
        ));
        let scaled_menu = session.draft.as_ref().unwrap().wrist_menu.clone();
        assert!((scaled_menu.width_m - (old_w * 1.5)).abs() < 0.01);
        assert_eq!(scaled_menu.offset_x_m, menu.offset_x_m);
        assert_eq!(scaled_menu.offset_y_m, menu.offset_y_m);
        assert_eq!(scaled_menu.offset_z_m, menu.offset_z_m);
        assert_eq!(scaled_menu.pitch_deg, menu.pitch_deg);
        assert_eq!(scaled_menu.yaw_deg, menu.yaw_deg);
        assert_eq!(scaled_menu.roll_deg, menu.roll_deg);
        session.end_grab(2, None);

        let commit = session.begin_commit(1).unwrap().unwrap();
        let committed = commit.wrist_menu.unwrap();
        assert_eq!(committed.width_m, scaled_menu.width_m);
        assert_eq!(committed.yaw_deg, scaled_menu.yaw_deg);
        assert_eq!(committed.roll_deg, scaled_menu.roll_deg);
    }

    #[test]
    fn commit_waits_for_result_and_cancel_discards_draft() {
        let mut session = LayoutEditSession::default();
        let cfg = test_config();
        session.enter(&cfg, true, None);
        assert_eq!(session.phase, EditPhase::WaitingForDashboardClose);
        session.on_dashboard_visibility(false, None);
        assert_eq!(session.phase, EditPhase::Active);

        // Clean session exits without emitting commit.
        assert!(session.begin_commit(1).unwrap().is_none());
        assert_eq!(session.phase, EditPhase::Inactive);

        session.enter(&cfg, false, None);
        session.draft.as_mut().unwrap().headset.width_m = 1.6;
        let req = session.begin_commit(3).unwrap().unwrap();
        assert_eq!(req.owner_epoch, 3);
        assert_eq!(req.headset.unwrap().width_m, 1.6);
        assert!(session.is_saving());

        assert!(session.resolve_commit(req.id, Some("failed".into())));
        assert_eq!(session.phase, EditPhase::Active);
        assert_eq!(session.error.as_deref(), Some("failed"));

        session.apply_toolbar_action(EditToolbarAction::Cancel);
        assert_eq!(session.phase, EditPhase::Inactive);
        assert!(session.draft.is_none());
    }

    #[test]
    fn disabled_components_remain_editable_without_mutating_enabled_flag() {
        let mut session = LayoutEditSession::default();
        let mut cfg = test_config();
        cfg.headset.enabled = false;
        cfg.wrist.enabled = false;
        cfg.wrist_menu.enabled = false;

        session.enter(&cfg, false, None);
        assert!(session.component_visible(EditTarget::Headset));
        assert!(session.component_visible(EditTarget::Wrist));
        assert!(session.component_visible(EditTarget::WristMenu));

        session.apply_toolbar_action(EditToolbarAction::ToggleVisible(EditTarget::Headset));
        assert!(!session.component_visible(EditTarget::Headset));
        session.apply_toolbar_action(EditToolbarAction::ToggleVisible(EditTarget::Headset));
        assert!(session.component_visible(EditTarget::Headset));

        session.draft.as_mut().unwrap().headset.width_m = 1.45;
        let req = session.begin_commit(2).unwrap().unwrap();
        assert_eq!(req.headset.unwrap().width_m, 1.45);
        assert!(!session.draft.as_ref().unwrap().headset.enabled);
    }
}
