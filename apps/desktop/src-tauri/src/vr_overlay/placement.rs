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
}

struct Drag {
    device: u32,
    relative: Pose,
    pose: Pose,
}
struct Pending {
    request: Request,
    pose: Option<Pose>,
}

#[derive(Default)]
pub struct Placement {
    mode: Mode,
    world: Option<Pose>,
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
            self.revision += 1;
        }
    }
    pub fn initialize_world(&mut self, pose: Pose) {
        if self.mode == Mode::World && self.world.is_none() && valid(pose) {
            self.world = Some(pose);
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
        if self.busy() || (mode == Mode::World && !pose.is_some_and(valid)) {
            return None;
        }
        self.drag = None;
        self.error = None;
        self.revision += 1;
        if mode == self.mode {
            if mode == Mode::World {
                self.world = pose;
            }
            return None;
        }
        self.sequence = self.sequence.checked_add(1)?;
        let request = Request {
            id: self.sequence,
            mode,
        };
        self.pending = Some(Pending { request, pose });
        Some(request)
    }
    pub fn resolve(&mut self, id: u64, success: bool, error: Option<String>) -> bool {
        if !self.pending.as_ref().is_some_and(|p| p.request.id == id) {
            return false;
        }
        let pending = self.pending.take().expect("matching placement request");
        if success {
            self.mode = pending.request.mode;
            self.world = pending.pose;
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
}
