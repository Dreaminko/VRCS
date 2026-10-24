//! Application-local input routing. This never captures SteamVR or game input.
use std::collections::HashMap;
use std::time::{Duration, Instant};

const LONG_PRESS: Duration = Duration::from_millis(650);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Menu,
    Dialogue,
    Reader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub surface: Surface,
    pub generation: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub key: Key,
    pub region: u8,
    pub point: [f32; 2],
    pub distance: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub device: u32,
    pub trigger: Option<bool>,
    pub grip: Option<bool>,
    pub hit: Option<Hit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Down,
    Up,
    LongPress,
    Grab,
    Release,
    Cancel,
}

#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub device: u32,
    pub hit: Hit,
    pub kind: Kind,
}

#[derive(Default)]
pub struct Output {
    pub hover: Option<(u32, Hit)>,
    pub events: Vec<Event>,
}

#[derive(Default)]
pub struct Router {
    previous: HashMap<u32, (Option<bool>, Option<bool>)>,
    owner: Option<Event>,
    pressed_at: Option<Instant>,
    hover_device: Option<u32>,
}

impl Router {
    pub fn active(&self) -> bool {
        self.owner.is_some()
    }

    pub fn hold_progress(&self, now: Instant) -> Option<f32> {
        let owner = self.owner?;
        if owner.hit.key.surface != Surface::Menu || owner.hit.region != 0 {
            return None;
        }
        Some(
            (now.saturating_duration_since(self.pressed_at?)
                .as_secs_f32()
                / LONG_PRESS.as_secs_f32())
            .min(1.),
        )
    }

    pub fn long_press(&mut self, now: Instant) -> Option<Event> {
        if self.hold_progress(now)? < 1. {
            return None;
        }
        let owner = self.owner.as_mut()?;
        if owner.kind != Kind::Down {
            return None;
        }
        owner.kind = Kind::LongPress;
        Some(*owner)
    }

    pub fn select_pointer_device(&mut self, candidates: &[(u32, f32)]) -> Option<u32> {
        let present = |device| candidates.iter().any(|&(id, _)| id == device);
        self.hover_device = if let Some(owner) = self.owner {
            present(owner.device).then_some(owner.device)
        } else {
            self.hover_device
                .filter(|&device| present(device))
                .or_else(|| {
                    candidates
                        .iter()
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|c| c.0)
                })
        };
        self.hover_device
    }

    pub fn step(&mut self, samples: &[Sample], keys: &[Key]) -> Output {
        let mut output = Output::default();
        let valid_hit = |hit: Hit| {
            keys.contains(&hit.key)
                && hit.distance.is_finite()
                && hit.distance >= 0.
                && hit
                    .point
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v))
        };
        let hits: Vec<_> = samples
            .iter()
            .filter_map(|sample| {
                sample
                    .hit
                    .filter(|hit| valid_hit(*hit))
                    .map(|hit| (sample.device, hit))
            })
            .collect();
        if !hits.is_empty() {
            let candidates: Vec<_> = hits
                .iter()
                .map(|(device, hit)| (*device, hit.distance))
                .collect();
            let device = self.select_pointer_device(&candidates);
            output.hover = hits.into_iter().find(|(id, _)| Some(*id) == device);
        }

        if let Some(owner) = self.owner {
            let sample = samples.iter().find(|s| s.device == owner.device);
            let state = sample.and_then(|s| {
                if owner.kind == Kind::Grab {
                    s.grip
                } else {
                    s.trigger
                }
            });
            let matching = sample.and_then(|s| s.hit).filter(|hit| {
                hit.key == owner.hit.key && hit.region == owner.hit.region && valid_hit(*hit)
            });
            let kind = if !keys.contains(&owner.hit.key)
                || state.is_none()
                || owner.kind != Kind::Grab && matching.is_none()
            {
                Some(Kind::Cancel)
            } else if state == Some(false) {
                Some(if owner.kind == Kind::Grab {
                    Kind::Release
                } else if owner.kind == Kind::LongPress {
                    Kind::Cancel
                } else {
                    Kind::Up
                })
            } else {
                None
            };
            if let Some(kind) = kind {
                output.events.push(Event {
                    kind,
                    hit: matching.unwrap_or(owner.hit),
                    ..owner
                });
                self.owner = None;
                self.pressed_at = None;
            }
            // A release cannot become a press on another surface in the same sample.
        } else if let Some((device, hit)) = output.hover {
            let sample = samples
                .iter()
                .find(|s| s.device == device)
                .expect("hover sample");
            let previous = self.previous.get(&device).copied().unwrap_or_default();
            let kind = if hit.key.surface == Surface::Dialogue
                && hit.region == 0
                && sample.grip == Some(true)
                && previous.1 == Some(false)
            {
                Some(Kind::Grab)
            } else if sample.trigger == Some(true) && previous.0 == Some(false) {
                Some(Kind::Down)
            } else {
                None
            };
            if let Some(kind) = kind {
                let event = Event { device, hit, kind };
                self.owner = Some(event);
                self.pressed_at = (kind == Kind::Down).then(Instant::now);
                output.events.push(event);
            }
        }
        self.previous
            .retain(|device, _| samples.iter().any(|s| s.device == *device));
        for sample in samples {
            self.previous
                .insert(sample.device, (sample.trigger, sample.grip));
        }
        output
    }

    pub fn cancel(&mut self) -> Option<Event> {
        self.previous.clear();
        self.hover_device = None;
        self.pressed_at = None;
        self.owner.take().map(|event| Event {
            kind: Kind::Cancel,
            ..event
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(surface: Surface) -> Key {
        Key {
            surface,
            generation: 1,
            revision: 1,
        }
    }
    fn sample(down: bool, grip: bool, surface: Surface) -> Sample {
        Sample {
            device: 2,
            trigger: Some(down),
            grip: Some(grip),
            hit: Some(Hit {
                key: key(surface),
                region: 0,
                point: [0.5, 0.5],
                distance: 1.,
            }),
        }
    }
    #[test]
    fn held_trigger_entering_surface_requires_release_and_new_press() {
        let mut router = Router::default();
        let keys = [key(Surface::Menu)];
        assert!(router
            .step(&[sample(true, false, Surface::Menu)], &keys)
            .events
            .is_empty());
        router.step(&[sample(false, false, Surface::Menu)], &keys);
        assert_eq!(
            router
                .step(&[sample(true, false, Surface::Menu)], &keys)
                .events[0]
                .kind,
            Kind::Down
        );
        assert_eq!(
            router
                .step(&[sample(false, false, Surface::Menu)], &keys)
                .events[0]
                .kind,
            Kind::Up
        );
    }
    #[test]
    fn another_surface_or_revision_cannot_receive_an_old_release() {
        let mut router = Router::default();
        let keys = [key(Surface::Menu), key(Surface::Reader)];
        router.step(&[sample(false, false, Surface::Menu)], &keys);
        router.step(&[sample(true, false, Surface::Menu)], &keys);
        let result = router.step(&[sample(false, false, Surface::Reader)], &keys);
        assert_eq!(result.events[0].kind, Kind::Cancel);
        assert!(!result.events.iter().any(|e| e.kind == Kind::Up));
    }
    #[test]
    fn grip_drag_keeps_device_owner_after_ray_leaves_handle() {
        let mut router = Router::default();
        let keys = [key(Surface::Dialogue)];
        router.step(&[sample(false, false, Surface::Dialogue)], &keys);
        assert_eq!(
            router
                .step(&[sample(false, true, Surface::Dialogue)], &keys)
                .events[0]
                .kind,
            Kind::Grab
        );
        let mut away = sample(false, true, Surface::Dialogue);
        away.hit = None;
        assert!(router.step(&[away], &keys).events.is_empty());
        away.grip = Some(false);
        assert_eq!(router.step(&[away], &keys).events[0].kind, Kind::Release);
    }
    #[test]
    fn hidden_surface_and_tracking_loss_cancel_without_release() {
        let mut router = Router::default();
        let keys = [key(Surface::Dialogue)];
        router.step(&[sample(false, false, Surface::Dialogue)], &keys);
        router.step(&[sample(false, true, Surface::Dialogue)], &keys);
        assert_eq!(router.step(&[], &keys).events[0].kind, Kind::Cancel);
        assert!(router
            .step(&[sample(false, false, Surface::Dialogue)], &[])
            .events
            .is_empty());
    }

    #[test]
    fn hover_stays_with_the_same_hand_until_it_leaves() {
        let mut router = Router::default();
        let keys = [key(Surface::Menu), key(Surface::Reader)];
        let first = sample(false, false, Surface::Menu);
        let mut other = sample(false, false, Surface::Reader);
        other.device = 3;
        other.hit.as_mut().unwrap().distance = 0.5;
        router.step(&[first], &keys);
        assert_eq!(router.step(&[first, other], &keys).hover.unwrap().0, 2);
        assert_eq!(router.step(&[other], &keys).hover.unwrap().0, 3);
        assert!(router.step(&[], &keys).hover.is_none());
    }

    #[test]
    fn dragging_never_hands_hover_to_another_controller() {
        let mut router = Router::default();
        let keys = [key(Surface::Dialogue), key(Surface::Reader)];
        router.step(&[sample(false, false, Surface::Dialogue)], &keys);
        let mut owner = sample(false, true, Surface::Dialogue);
        router.step(&[owner], &keys);
        let mut other = sample(false, false, Surface::Reader);
        other.device = 3;
        other.hit.as_mut().unwrap().distance = 0.5;
        assert_eq!(router.step(&[owner, other], &keys).hover.unwrap().0, 2);
        owner.hit = None;
        assert!(router.step(&[owner, other], &keys).hover.is_none());
        assert!(router.active());
    }

    #[test]
    fn holding_dialogue_menu_button_fires_before_release() {
        let mut router = Router::default();
        let keys = [key(Surface::Menu)];
        router.step(&[sample(false, false, Surface::Menu)], &keys);
        router.step(&[sample(true, false, Surface::Menu)], &keys);
        let start = router.pressed_at.unwrap();
        let halfway = start + LONG_PRESS / 2;
        assert_eq!(router.hold_progress(halfway), Some(0.5));
        assert!(router.long_press(halfway).is_none());
        let complete = start + LONG_PRESS;
        assert_eq!(router.long_press(complete).unwrap().kind, Kind::LongPress);
        assert_eq!(router.hold_progress(complete), Some(1.));
        assert!(router.long_press(complete + LONG_PRESS).is_none());
        assert!(router.active());
        assert_eq!(
            router
                .step(&[sample(false, false, Surface::Menu)], &keys)
                .events[0]
                .kind,
            Kind::Cancel
        );
        assert_eq!(router.hold_progress(complete), None);
    }

    #[test]
    fn menu_hold_clears_on_release_or_loss_of_ownership() {
        let keys = [key(Surface::Menu)];
        let down = sample(true, false, Surface::Menu);
        let mut away = down;
        away.hit = None;
        let mut other = down;
        other.hit.as_mut().unwrap().region = 1;
        let mut stale = down;
        stale.hit.as_mut().unwrap().key.revision += 1;
        let mut unknown = down;
        unknown.trigger = None;
        for (samples, keys, expected) in [
            (
                vec![sample(false, false, Surface::Menu)],
                keys.to_vec(),
                Kind::Up,
            ),
            (vec![away], keys.to_vec(), Kind::Cancel),
            (vec![other], keys.to_vec(), Kind::Cancel),
            (vec![stale], keys.to_vec(), Kind::Cancel),
            (vec![down], vec![], Kind::Cancel),
            (vec![], keys.to_vec(), Kind::Cancel),
            (vec![unknown], keys.to_vec(), Kind::Cancel),
        ] {
            let mut router = Router::default();
            router.step(
                &[sample(false, false, Surface::Menu)],
                &[key(Surface::Menu)],
            );
            router.step(&[down], &[key(Surface::Menu)]);
            let halfway = router.pressed_at.unwrap() + LONG_PRESS / 2;
            assert_eq!(router.hold_progress(halfway), Some(0.5));
            assert_eq!(router.step(&samples, &keys).events[0].kind, expected);
            assert_eq!(router.hold_progress(halfway), None);
            assert!(router.long_press(halfway + LONG_PRESS).is_none());
            assert!(!router.active());
        }
        let mut router = Router::default();
        router.step(&[sample(false, false, Surface::Menu)], &keys);
        router.step(&[down], &keys);
        assert_eq!(router.cancel().unwrap().kind, Kind::Cancel);
        assert_eq!(router.hold_progress(Instant::now() + LONG_PRESS), None);
    }

    #[test]
    fn other_buttons_keep_their_click_behavior_without_hold_progress() {
        for (surface, region) in [
            (Surface::Menu, 1),
            (Surface::Menu, 2),
            (Surface::Menu, 3),
            (Surface::Dialogue, 0),
            (Surface::Reader, 0),
        ] {
            let mut router = Router::default();
            let keys = [key(surface)];
            let mut input = sample(false, false, surface);
            input.hit.as_mut().unwrap().region = region;
            router.step(&[input], &keys);
            input.trigger = Some(true);
            assert_eq!(router.step(&[input], &keys).events[0].kind, Kind::Down);
            let complete = router.pressed_at.unwrap() + LONG_PRESS;
            assert_eq!(router.hold_progress(complete), None);
            assert!(router.long_press(complete).is_none());
            input.trigger = Some(false);
            assert_eq!(router.step(&[input], &keys).events[0].kind, Kind::Up);
        }
    }
}
