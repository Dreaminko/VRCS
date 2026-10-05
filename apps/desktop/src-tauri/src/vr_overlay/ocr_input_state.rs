use std::time::{Duration, Instant};

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
