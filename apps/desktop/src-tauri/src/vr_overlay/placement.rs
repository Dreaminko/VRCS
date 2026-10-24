use super::transform::{compose, inverse};
use serde::{Deserialize, Serialize};

pub type Pose = [[f32; 4]; 3];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Wrist,
    World,
}

impl Mode {
    pub fn from_config(value: &str) -> Self {
        if value == "world" {
            Self::World
        } else {
            Self::Wrist
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Request {
    pub id: u64,
    pub mode: Mode,
    pub follow_view: bool,
}

struct Drag {
    device: u32,
    relative: Pose,
    pose: Pose,
}
struct Pending {
    request: Request,
    pose: Option<Pose>,
    relative: Option<Pose>,
}

#[derive(Default)]
pub struct Placement {
    mode: Mode,
    follow_view: bool,
    head: Option<Pose>,
    world: Option<Pose>,
    relative: Option<Pose>,
    drag: Option<Drag>,
    pending: Option<Pending>,
    sequence: u64,
    pub revision: u64,
    pub error: Option<String>,
}

impl Placement {
    #[cfg(any(test, all(windows, debug_assertions, feature = "vr-preview")))]
    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn display_mode(&self) -> Mode {
        if self.drag.is_some() {
            Mode::World
        } else {
            self.pending
                .as_ref()
                .map(|p| p.request.mode)
                .unwrap_or(self.mode)
        }
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn follow_view(&self) -> bool {
        self.pending
            .as_ref()
            .map(|p| p.request.follow_view)
            .unwrap_or(self.follow_view)
    }
    pub fn head_relative_pose(&self) -> Option<Pose> {
        if self.drag.is_some() {
            return None;
        }
        if let Some(pending) = &self.pending {
            return (pending.request.mode == Mode::World && pending.request.follow_view)
                .then_some(pending.relative)
                .flatten();
        }
        (self.mode == Mode::World && self.follow_view)
            .then_some(self.relative)
            .flatten()
    }
    pub fn update_head(&mut self, head: Pose) {
        if !valid(head) {
            return;
        }
        self.head = Some(head);
        // Keep the confirmed pose current so a failed save can restore it.
        if self.mode == Mode::World && self.follow_view {
            self.relative = self
                .relative
                .or_else(|| self.world.map(|pose| compose(inverse(head), pose)));
            self.world = self.relative.map(|relative| compose(head, relative));
        }
        if let Some(pending) = &mut self.pending {
            if pending.request.mode == Mode::World && pending.request.follow_view {
                pending.relative = pending
                    .relative
                    .or_else(|| pending.pose.map(|pose| compose(inverse(head), pose)));
                pending.pose = pending.relative.map(|relative| compose(head, relative));
            }
        }
    }
    pub fn dragging(&self) -> Option<u32> {
        self.drag.as_ref().map(|d| d.device)
    }
    pub fn absolute_pose(&self) -> Option<Pose> {
        if let Some(drag) = &self.drag {
            return Some(drag.pose);
        }
        if let Some(pending) = &self.pending {
            return pending.pose;
        }
        (self.mode == Mode::World).then_some(self.world).flatten()
    }
    pub fn sync_mode(&mut self, mode: Mode) {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.request.mode == mode)
        {
            return;
        }
        if self.mode != mode {
            self.cancel();
            self.mode = mode;
            self.relative =
                self.capture_relative(self.world, mode == Mode::World && self.follow_view);
            self.revision += 1;
        }
    }
    pub fn sync_follow_view(&mut self, enabled: bool) {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.request.follow_view == enabled)
        {
            return;
        }
        if self.follow_view != enabled {
            self.cancel();
            self.follow_view = enabled;
            self.relative = self.capture_relative(self.world, self.mode == Mode::World && enabled);
            self.revision += 1;
        }
    }
    pub fn initialize_world(&mut self, pose: Pose) {
        if self.mode == Mode::World && self.world.is_none() && valid(pose) {
            self.world = Some(pose);
            self.relative = self.capture_relative(self.world, self.follow_view);
        }
    }
    fn capture_relative(&self, pose: Option<Pose>, follow_view: bool) -> Option<Pose> {
        if follow_view {
            Some(compose(inverse(self.head?), pose?))
        } else {
            None
        }
    }
    pub fn grab(&mut self, device: u32, controller: Pose, panel: Pose) -> bool {
        if self.busy() || self.drag.is_some() || !valid(controller) || !valid(panel) {
            return false;
        }
        self.drag = Some(Drag {
            device,
            relative: compose(inverse(controller), panel),
            pose: panel,
        });
        self.error = None;
        true
    }
    pub fn drag(&mut self, device: u32, controller: Pose) -> Option<Pose> {
        let drag = self
            .drag
            .as_mut()
            .filter(|d| d.device == device && valid(controller))?;
        drag.pose = compose(controller, drag.relative);
        Some(drag.pose)
    }
    pub fn release(&mut self, device: u32) -> Option<Request> {
        if self.dragging() != Some(device) {
            return None;
        }
        let pose = self.drag.take()?.pose;
        self.set_mode(Mode::World, Some(pose))
    }
    pub fn set_mode(&mut self, mode: Mode, pose: Option<Pose>) -> Option<Request> {
        self.set_target(mode, self.follow_view, pose)
    }
    pub fn set_follow_view(&mut self, enabled: bool) -> Option<Request> {
        if self.display_mode() != Mode::World
            || self.drag.is_some()
            || self.follow_view() == enabled
            || enabled && self.head.is_none()
        {
            return None;
        }
        self.set_target(Mode::World, enabled, self.absolute_pose())
    }
    fn set_target(&mut self, mode: Mode, follow_view: bool, pose: Option<Pose>) -> Option<Request> {
        if self.busy() || (mode == Mode::World && !pose.is_some_and(valid)) {
            return None;
        }
        self.drag = None;
        self.error = None;
        self.revision += 1;
        let relative = self.capture_relative(pose, mode == Mode::World && follow_view);
        if mode == self.mode && follow_view == self.follow_view {
            if mode == Mode::World {
                self.world = pose;
                self.relative = relative;
            }
            return None;
        }
        self.sequence = self.sequence.checked_add(1)?;
        let request = Request {
            id: self.sequence,
            mode,
            follow_view,
        };
        self.pending = Some(Pending {
            request,
            pose,
            relative,
        });
        Some(request)
    }
    pub fn resolve(&mut self, id: u64, success: bool, error: Option<String>) -> bool {
        if !self.pending.as_ref().is_some_and(|p| p.request.id == id) {
            return false;
        }
        let pending = self.pending.take().expect("matching placement request");
        if success {
            self.mode = pending.request.mode;
            self.follow_view = pending.request.follow_view;
            self.world = pending.pose;
            self.relative = pending.relative;
        }
        self.error = error;
        self.revision += 1;
        true
    }
    pub fn cancel(&mut self) {
        if self.drag.take().is_some() || self.pending.take().is_some() {
            self.revision += 1;
        }
        // Evaluate both removals even when a drag was active.
        self.pending = None;
    }
    pub fn cancel_drag(&mut self) {
        if self.drag.take().is_some() {
            self.revision += 1;
        }
    }
    pub fn reset_tracking(&mut self) {
        self.cancel();
        self.world = None;
        self.head = None;
        self.relative = None;
        self.revision += 1;
    }
}

fn valid(pose: Pose) -> bool {
    pose.iter().flatten().all(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::super::transform::matrix;
    use super::*;

    fn close(a: Pose, b: Pose) {
        for (a, b) in a.into_iter().flatten().zip(b.into_iter().flatten()) {
            assert!((a - b).abs() < 1e-4, "{a} != {b}");
        }
    }
    #[test]
    fn grab_preserves_initial_pose_and_applies_controller_delta() {
        let mut p = Placement::default();
        let panel = matrix(12., 24., 36., [1., 2., 3.]);
        let controller = matrix(0., 90., 0., [0.2, 0.3, 0.4]);
        assert!(p.grab(7, controller, panel));
        close(p.drag(7, controller).unwrap(), panel);
        let moved = matrix(0., 90., 0., [0.5, 0.4, 0.6]);
        close(
            p.drag(7, moved).unwrap(),
            compose(matrix(0., 0., 0., [0.3, 0.1, 0.2]), panel),
        );
    }
    #[test]
    fn release_waits_for_save_and_failure_restores_confirmed_anchor() {
        let mut p = Placement::default();
        let pose = matrix(0., 0., 0., [1., 2., 3.]);
        p.grab(7, pose, pose);
        let request = p.release(7).unwrap();
        assert!(p.busy());
        assert!(!p.grab(8, pose, pose));
        assert!(p.resolve(request.id, false, Some("save failed".into())));
        assert_eq!(p.mode(), Mode::Wrist);
        assert!(p.absolute_pose().is_none());
        assert!(!p.resolve(request.id, true, None));
    }
    #[test]
    fn world_pose_survives_release_and_cancellation_but_not_tracking_reset() {
        let mut p = Placement::default();
        let pose = matrix(0., 45., 0., [1., 2., 3.]);
        let request = p.set_mode(Mode::World, Some(pose)).unwrap();
        p.resolve(request.id, true, None);
        p.grab(2, matrix(0., 0., 0., [0.; 3]), pose);
        p.drag(2, matrix(0., 0., 0., [1., 0., 0.]));
        p.cancel();
        close(p.absolute_pose().unwrap(), pose);
        p.reset_tracking();
        assert_eq!(p.mode(), Mode::World);
        assert!(p.absolute_pose().is_none());
    }
    #[test]
    fn world_drag_does_not_save_again_and_reattach_uses_wrist_pose() {
        let mut p = Placement::default();
        p.sync_mode(Mode::World);
        let pose = matrix(0., 0., 0., [0., 0., -1.]);
        p.initialize_world(pose);
        p.grab(1, pose, pose);
        let moved = matrix(0., 20., 0., [0.1, 0., -1.]);
        p.drag(1, moved);
        assert!(p.release(1).is_none());
        close(p.absolute_pose().unwrap(), moved);
        let request = p.set_mode(Mode::Wrist, None).unwrap();
        p.resolve(request.id, true, None);
        assert!(p.absolute_pose().is_none());
    }

    fn world_panel(head: Pose, panel: Pose) -> Placement {
        let mut p = Placement::default();
        p.update_head(head);
        p.sync_mode(Mode::World);
        p.initialize_world(panel);
        p
    }

    #[test]
    fn follow_preserves_relative_pose_without_revising_content_and_freezes_when_disabled() {
        let head = matrix(5., 30., 3., [1., 1.7, 2.]);
        let relative = matrix(-8., 12., 0., [0.2, -0.12, -1.]);
        let panel = compose(head, relative);
        let mut p = world_panel(head, panel);
        let request = p.set_follow_view(true).unwrap();
        assert!(request.follow_view);
        close(p.absolute_pose().unwrap(), panel);
        p.resolve(request.id, true, None);
        let revision = p.revision;
        let moved_head = matrix(-15., 90., 10., [2., 1.6, 3.]);
        p.update_head(moved_head);
        close(p.head_relative_pose().unwrap(), relative);
        let moved_panel = compose(moved_head, relative);
        close(p.absolute_pose().unwrap(), moved_panel);
        assert_eq!(p.revision, revision);
        let request = p.set_follow_view(false).unwrap();
        p.resolve(request.id, true, None);
        p.update_head(head);
        close(p.absolute_pose().unwrap(), moved_panel);
        assert!(p.head_relative_pose().is_none());
    }

    #[test]
    fn dragging_suspends_follow_and_release_recaptures_the_offset() {
        let head = matrix(0., 0., 0., [0., 1.7, 0.]);
        let panel = matrix(0., 0., 0., [0., 1.5, -1.]);
        let mut p = world_panel(head, panel);
        p.sync_follow_view(true);
        let controller = matrix(0., 0., 0., [0.; 3]);
        assert!(p.grab(1, controller, panel));
        assert!(p.head_relative_pose().is_none());
        let moved_head = matrix(0., 30., 0., [0.2, 1.7, 0.]);
        p.update_head(moved_head);
        close(p.absolute_pose().unwrap(), panel);
        let dragged = p.drag(1, matrix(10., 0., 0., [0.3, 0., 0.])).unwrap();
        assert!(p.release(1).is_none());
        close(p.absolute_pose().unwrap(), dragged);
        let relative = compose(inverse(moved_head), dragged);
        p.update_head(head);
        close(p.absolute_pose().unwrap(), compose(head, relative));
    }

    #[test]
    fn failed_follow_save_restores_the_confirmed_placement() {
        let head = matrix(0., 0., 0., [0., 1.7, 0.]);
        let panel = matrix(0., 0., 0., [0., 1.5, -1.]);
        let moved_head = matrix(0., 60., 0., [1., 1.7, 0.]);
        let mut p = world_panel(head, panel);
        let request = p.set_follow_view(true).unwrap();
        p.sync_follow_view(false);
        p.sync_follow_view(true);
        p.update_head(moved_head);
        assert!(p.busy());
        assert!(p.follow_view());
        p.resolve(request.id, false, Some("save failed".into()));
        assert!(!p.follow_view());
        close(p.absolute_pose().unwrap(), panel);

        p.sync_follow_view(true);
        let relative = p.head_relative_pose().unwrap();
        let request = p.set_follow_view(false).unwrap();
        p.update_head(head);
        close(p.absolute_pose().unwrap(), panel);
        p.resolve(request.id, false, Some("save failed".into()));
        close(p.absolute_pose().unwrap(), compose(head, relative));
        assert!(p.follow_view());
    }

    #[test]
    fn wrist_mode_and_tracking_reset_keep_the_preference_without_stale_transforms() {
        let head = matrix(0., 0., 0., [0., 1.7, 0.]);
        let panel = matrix(0., 0., 0., [0., 1.5, -1.]);
        let mut p = world_panel(head, panel);
        p.sync_follow_view(true);
        let request = p.set_mode(Mode::Wrist, None).unwrap();
        assert!(request.follow_view);
        p.resolve(request.id, true, None);
        assert!(p.follow_view());
        assert!(p.head_relative_pose().is_none());
        assert!(p.set_follow_view(false).is_none());
        let request = p.set_mode(Mode::World, Some(panel)).unwrap();
        p.resolve(request.id, true, None);
        p.reset_tracking();
        assert!(p.follow_view());
        assert!(p.head_relative_pose().is_none());
        let recovered_head = matrix(0., 45., 0., [5., 1.7, 5.]);
        p.update_head(recovered_head);
        let recovered_panel = compose(recovered_head, matrix(0., 0., 0., [0., -0.12, -1.]));
        p.initialize_world(recovered_panel);
        close(p.absolute_pose().unwrap(), recovered_panel);
        assert!(p.head_relative_pose().is_some());
    }
}
