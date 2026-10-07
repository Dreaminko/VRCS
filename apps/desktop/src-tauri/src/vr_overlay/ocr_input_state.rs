use super::ocr_gesture::FrameCorners;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub struct FrameTracking {
    pub origin: u64,
    pub corners: FrameCorners,
    pub activating: bool,
    pub dragging: bool,
}

#[derive(Default)]
pub struct FrameUpdate {
    pub frame: Option<FrameCorners>,
    pub confirmed: bool,
    pub cancelled: bool,
}

#[derive(Default)]
pub struct FrameSession {
    holds: [HoldAction; 2],
    active: Option<(usize, u64)>,
}

impl FrameSession {
    pub fn update(
        &mut self,
        tracking: [Option<FrameTracking>; 2],
        confirm: bool,
        cancel: bool,
        now: Instant,
    ) -> FrameUpdate {
        if cancel {
            let cancelled = self.active.is_some();
            *self = Self::default();
            return FrameUpdate {
                cancelled,
                ..FrameUpdate::default()
            };
        }
        let mut activated = None;
        for (index, sample) in tracking.iter().enumerate() {
            let origin = sample
                .filter(|sample| sample.origin != 0)
                .map(|sample| sample.origin);
            let pressed = sample.is_some_and(|sample| {
                if self.active == Some((index, sample.origin)) {
                    sample.dragging
                } else {
                    sample.activating
                }
            });
            if self.holds[index].update(origin, pressed, now)
                && self.active.is_none()
                && activated.is_none()
            {
                activated = origin.map(|origin| (index, origin));
            }
        }
        if self.active.is_none() {
            self.active = activated;
        }
        let Some((source, origin)) = self.active else {
            return FrameUpdate::default();
        };
        let Some(sample) =
            tracking[source].filter(|sample| sample.origin == origin && sample.dragging)
        else {
            *self = Self::default();
            return FrameUpdate {
                cancelled: true,
                ..FrameUpdate::default()
            };
        };
        if confirm {
            self.active = None;
        }
        FrameUpdate {
            frame: Some(sample.corners),
            confirmed: confirm,
            cancelled: false,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum HandWait {
    Waiting,
    Ready,
    Invalid,
}

pub struct HandReleaseWait {
    started: Instant,
    clear_since: Option<Instant>,
}

impl HandReleaseWait {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            clear_since: None,
        }
    }
    pub fn poll(&mut self, available: bool, in_view: bool, now: Instant) -> HandWait {
        if !available || now.saturating_duration_since(self.started) >= Duration::from_secs(5) {
            return HandWait::Invalid;
        }
        if in_view {
            self.clear_since = None;
            return HandWait::Waiting;
        }
        self.clear_since.get_or_insert(now);
        if self
            .clear_since
            .is_some_and(|at| now.saturating_duration_since(at) >= Duration::from_millis(100))
        {
            HandWait::Ready
        } else {
            HandWait::Waiting
        }
    }
}

#[derive(Default)]
pub struct HoldAction {
    origin: Option<u64>,
    armed: bool,
    pressed_at: Option<Instant>,
    fired: bool,
}

impl HoldAction {
    pub fn update(&mut self, origin: Option<u64>, pressed: bool, now: Instant) -> bool {
        if origin != self.origin || origin.is_none() {
            *self = Self {
                origin,
                ..Self::default()
            };
        }
        if origin.is_none() {
            return false;
        }
        if !pressed {
            self.armed = true;
            self.pressed_at = None;
            self.fired = false;
            return false;
        }
        if !self.armed || self.fired {
            return false;
        }
        let started = *self.pressed_at.get_or_insert(now);
        if now.saturating_duration_since(started) < Duration::from_millis(650) {
            return false;
        }
        self.fired = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracking(activating: bool) -> FrameTracking {
        FrameTracking {
            origin: 7,
            corners: FrameCorners {
                corners: [[-0.2, -0.1, -0.5], [0.2, 0.1, -0.5]],
            },
            activating,
            dragging: activating,
        }
    }

    fn activate(source: usize, now: Instant) -> FrameSession {
        let mut session = FrameSession::default();
        let mut input = [None; 2];
        input[source] = Some(tracking(false));
        assert!(session.update(input, false, false, now).frame.is_none());
        input[source] = Some(tracking(true));
        assert!(session.update(input, false, false, now).frame.is_none());
        assert!(session
            .update(input, false, false, now + Duration::from_millis(650))
            .frame
            .is_some());
        session
    }

    #[test]
    fn frame_session_drags_after_activation_and_keeps_corners_on_confirmation() {
        let now = Instant::now();
        let mut session = activate(0, now);
        let mut drag = tracking(false);
        drag.dragging = true;
        drag.corners.corners[1] = [0.7, 0.4, -0.8];
        let tick = now + Duration::from_secs(1);
        let update = session.update([Some(drag), None], false, false, tick);
        assert_eq!(update.frame.unwrap().corners[1], [0.7, 0.4, -0.8]);
        assert!(!update.confirmed);
        let confirmed = session.update([Some(drag), None], true, false, tick);
        assert!(confirmed.confirmed);
        assert_eq!(confirmed.frame.unwrap().corners[1], [0.7, 0.4, -0.8]);
        assert!(
            !session
                .update([Some(drag), None], false, false, tick)
                .cancelled
        );
    }

    #[test]
    fn held_frame_inputs_must_be_released_before_activation() {
        let now = Instant::now();
        for source in 0..2 {
            let mut session = FrameSession::default();
            let mut input = [None; 2];
            input[source] = Some(tracking(true));
            assert!(session.update(input, false, false, now).frame.is_none());
            assert!(session
                .update(input, false, false, now + Duration::from_secs(2))
                .frame
                .is_none());
            activate(source, now);
        }
    }

    #[test]
    fn releasing_or_losing_frame_tracking_cancels_without_confirming() {
        let now = Instant::now();
        for source in 0..2 {
            for cause in 0..4 {
                let mut session = activate(source, now);
                let mut sample = tracking(true);
                let mut input = [None; 2];
                match cause {
                    0 => sample.dragging = false,
                    1 => sample.origin = 8,
                    _ => {}
                }
                if cause != 2 {
                    input[source] = Some(sample);
                }
                let update = session.update(input, true, cause == 3, now + Duration::from_secs(1));
                assert!(update.cancelled, "source {source}, cause {cause}");
                assert!(!update.confirmed);
                assert!(update.frame.is_none());
            }
        }
    }

    #[test]
    fn returning_hands_restart_the_capture_wait_and_lost_tracking_cancels_it() {
        let now = Instant::now();
        let mut wait = HandReleaseWait::new(now);
        assert_eq!(wait.poll(true, false, now), HandWait::Waiting);
        assert_eq!(
            wait.poll(true, true, now + Duration::from_millis(50)),
            HandWait::Waiting
        );
        assert_eq!(
            wait.poll(true, true, now + Duration::from_millis(150)),
            HandWait::Waiting
        );
        assert_eq!(
            wait.poll(true, false, now + Duration::from_millis(200)),
            HandWait::Waiting
        );
        assert_eq!(
            wait.poll(true, false, now + Duration::from_millis(300)),
            HandWait::Ready
        );
        assert_eq!(
            wait.poll(false, false, now + Duration::from_millis(350)),
            HandWait::Invalid
        );
        assert_eq!(
            wait.poll(true, false, now + Duration::from_secs(5)),
            HandWait::Invalid
        );
    }

    #[test]
    fn held_input_fires_once_and_requires_release_after_binding_or_device_changes() {
        let now = Instant::now();
        let mut action = HoldAction::default();
        assert!(!action.update(Some(1), true, now));
        assert!(!action.update(Some(1), true, now + Duration::from_secs(2)));
        assert!(!action.update(Some(1), false, now));
        assert!(!action.update(Some(1), true, now));
        assert!(!action.update(Some(1), true, now + Duration::from_millis(600)));
        assert!(action.update(Some(1), true, now + Duration::from_millis(700)));
        assert!(!action.update(Some(1), true, now + Duration::from_secs(2)));
        assert!(!action.update(None, false, now));
        assert!(!action.update(Some(2), true, now));
        assert!(!action.update(Some(2), true, now + Duration::from_secs(2)));
        assert!(!action.update(Some(2), false, now));
        assert!(!action.update(Some(2), true, now));
        assert!(action.update(Some(2), true, now + Duration::from_secs(1)));
    }
}
