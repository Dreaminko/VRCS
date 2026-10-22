use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Visibility {
    visible: bool,
    since: Option<Instant>,
}

impl Visibility {
    pub fn update(&mut self, angle: Option<f32>, held: bool, now: Instant) -> bool {
        let Some(angle) = angle.filter(|v| v.is_finite()) else {
            *self = Self::default();
            return false;
        };
        if held && self.visible {
            self.since = None;
            return true;
        }
        let changing = if self.visible {
            angle >= 65.
        } else {
            angle <= 45.
        };
        if changing {
            let since = *self.since.get_or_insert(now);
            let delay = if self.visible { 400 } else { 150 };
            if now.duration_since(since) >= Duration::from_millis(delay) {
                self.visible = !self.visible;
                self.since = None;
            }
        } else {
            self.since = None;
        }
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lift_hysteresis_holds_press_but_tracking_loss_hides_immediately() {
        let mut v = Visibility::default();
        let now = Instant::now();
        assert!(!v.update(Some(40.), false, now));
        assert!(v.update(Some(40.), false, now + Duration::from_millis(150)));
        assert!(v.update(Some(80.), true, now + Duration::from_secs(1)));
        assert!(v.update(Some(80.), false, now + Duration::from_secs(2)));
        assert!(!v.update(Some(80.), false, now + Duration::from_millis(2400)));
        assert!(!v.update(None, true, now + Duration::from_secs(3)));
    }
}
