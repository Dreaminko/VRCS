use std::time::{Duration, Instant};

const FADE_IN: Duration = Duration::from_millis(160);
const FADE_OUT: Duration = Duration::from_millis(200);
const INPUT_THRESHOLD: f32 = 0.85;

#[derive(Default)]
pub struct Fade {
    factor: f32,
    from: f32,
    target: bool,
    started: Option<Instant>,
}

pub struct FadeFrame {
    pub factor: f32,
    pub animating: bool,
}

impl Fade {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn active(&self) -> bool {
        self.target || self.started.is_some()
    }

    pub fn input_allowed(&self, target_visible: bool) -> bool {
        target_visible && self.target && self.factor >= INPUT_THRESHOLD
    }

    pub fn advance(&mut self, target_visible: bool, frame_ready: bool, now: Instant) -> FadeFrame {
        if !frame_ready {
            self.reset();
        } else {
            if let Some(started) = self.started {
                let end = f32::from(self.target);
                let duration = if self.target { FADE_IN } else { FADE_OUT }.as_secs_f32()
                    * (end - self.from).abs();
                let t = (now.saturating_duration_since(started).as_secs_f32() / duration).min(1.0);
                self.factor = self.from + (end - self.from) * t * t * (3.0 - 2.0 * t);
                if t >= 1.0 {
                    self.factor = end;
                    self.started = None;
                }
            }
            if self.target != target_visible {
                self.target = target_visible;
                self.from = self.factor;
                self.started = (self.factor != f32::from(target_visible)).then_some(now);
            }
        }
        FadeFrame {
            factor: self.factor,
            animating: self.started.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn fade_waits_for_a_frame_and_uses_fixed_deadlines() {
        let now = Instant::now();
        let mut fade = Fade::default();
        assert_eq!(fade.advance(true, false, now).factor, 0.0);
        let ready = now + Duration::from_secs(1);
        assert_eq!(fade.advance(true, true, ready).factor, 0.0);
        assert!(
            (fade
                .advance(true, true, ready + Duration::from_millis(80))
                .factor
                - 0.5)
                .abs()
                < 0.001
        );
        assert_eq!(
            fade.advance(true, true, ready + Duration::from_millis(160))
                .factor,
            1.0
        );
        assert!(fade.input_allowed(true));
        let exit = ready + Duration::from_secs(1);
        assert_eq!(fade.advance(false, true, exit).factor, 1.0);
        assert!(!fade.input_allowed(false));
        assert!(
            (fade
                .advance(false, true, exit + Duration::from_millis(100))
                .factor
                - 0.5)
                .abs()
                < 0.001
        );
        let hidden = fade.advance(false, true, exit + Duration::from_millis(200));
        assert_eq!(hidden.factor, 0.0);
        assert!(!hidden.animating);
        assert!(!fade.active());
    }

    #[test]
    fn reversal_is_continuous_and_missing_frames_reset_the_transition() {
        let now = Instant::now();
        let mut fade = Fade::default();
        fade.advance(true, true, now);
        let middle = now + Duration::from_millis(80);
        assert!((fade.advance(false, true, middle).factor - 0.5).abs() < 0.001);
        assert_eq!(fade.advance(false, true, middle).factor, 0.5);
        assert_eq!(
            fade.advance(false, true, middle + Duration::from_millis(100))
                .factor,
            0.0
        );
        fade.advance(true, true, middle + Duration::from_secs(1));
        assert_eq!(
            fade.advance(true, false, middle + Duration::from_secs(2))
                .factor,
            0.0
        );
        assert!(!fade.active());
        assert_eq!(
            fade.advance(true, true, middle + Duration::from_secs(3))
                .factor,
            0.0
        );
    }
}
