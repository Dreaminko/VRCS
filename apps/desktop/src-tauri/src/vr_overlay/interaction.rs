//! Application-local input routing. This never captures SteamVR or game input.
use std::collections::HashMap;

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
}

impl Router {
    pub fn active(&self) -> bool {
        self.owner.is_some()
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
        output.hover = samples
            .iter()
            .filter_map(|sample| {
                sample
                    .hit
                    .filter(|hit| valid_hit(*hit))
                    .map(|hit| (sample.device, hit))
            })
            .min_by(|a, b| a.1.distance.total_cmp(&b.1.distance));

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
                || owner.kind == Kind::Down && matching.is_none()
            {
                Some(Kind::Cancel)
            } else if state == Some(false) {
                Some(if owner.kind == Kind::Grab {
                    Kind::Release
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
}
