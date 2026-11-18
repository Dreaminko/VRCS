use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const POSITION_TTL: Duration = Duration::from_secs(15);
/// Orientation changes may only be sent when turning; keep it shorter than the
/// position cache so a stale heading cannot keep ranking players.
const VIEW_TTL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum PositionDecode {
    Sample(Position),
    NoPosition,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DecodedMessage {
    pub actor: Option<u16>,
    pub position: PositionDecode,
    pub yaw_deg: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Position {
    pub tick: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Admission {
    Disabled,
    InRange,
    OutOfRange,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Scope {
    flow: u64,
    epoch: u64,
}

struct Tracked {
    scope: Scope,
    position: Position,
    observed: Instant,
}

struct Heading {
    scope: Scope,
    tick: u32,
    yaw_deg: f32,
    observed: Instant,
}

#[derive(Default)]
pub(super) struct Registry {
    local: Option<Tracked>,
    heading: Option<Heading>,
    players: HashMap<String, Tracked>,
}

impl Registry {
    pub(super) fn observe_local(
        &mut self,
        flow: u64,
        epoch: u64,
        position: Position,
        now: Instant,
    ) {
        let scope = Scope { flow, epoch };
        if !newer(self.local.as_ref(), scope, position.tick) {
            return;
        }
        if self
            .heading
            .as_ref()
            .is_some_and(|heading| heading.scope != scope)
        {
            self.heading = None;
        }
        self.local = Some(Tracked {
            scope,
            position,
            observed: now,
        });
    }

    pub(super) fn observe_local_yaw(
        &mut self,
        flow: u64,
        epoch: u64,
        tick: u32,
        yaw_deg: f32,
        now: Instant,
    ) {
        let scope = Scope { flow, epoch };
        if !yaw_deg.is_finite()
            || self.local.as_ref().is_none_or(|local| local.scope != scope)
            || self
                .heading
                .as_ref()
                .is_some_and(|heading| heading.scope == scope && !newer_tick(tick, heading.tick))
        {
            return;
        }
        self.heading = Some(Heading {
            scope,
            tick,
            yaw_deg,
            observed: now,
        });
    }

    pub(super) fn observe_player(
        &mut self,
        id: &str,
        flow: u64,
        epoch: u64,
        position: Position,
        now: Instant,
    ) {
        let scope = Scope { flow, epoch };
        if !newer(self.players.get(id), scope, position.tick) {
            return;
        }
        if self.players.len() >= 512 && !self.players.contains_key(id) {
            if let Some(oldest) = self
                .players
                .iter()
                .min_by_key(|(_, value)| value.observed)
                .map(|(id, _)| id.clone())
            {
                self.players.remove(&oldest);
            }
        }
        self.players.insert(
            id.into(),
            Tracked {
                scope,
                position,
                observed: now,
            },
        );
    }

    pub(super) fn admission(
        &self,
        id: &str,
        enabled: bool,
        radius_m: f32,
        now: Instant,
    ) -> Admission {
        if !enabled {
            return Admission::Disabled;
        }
        match self.distance(id, now) {
            Some(distance) if distance <= f64::from(radius_m) => Admission::InRange,
            Some(_) => Admission::OutOfRange,
            None => Admission::Unknown,
        }
    }

    pub(super) fn distance(&self, id: &str, now: Instant) -> Option<f64> {
        let local = self.local.as_ref().filter(|value| fresh(value, now))?;
        let player = self.players.get(id).filter(|value| fresh(value, now))?;
        if local.scope != player.scope {
            return None;
        }
        let delta_x = f64::from(local.position.x) - f64::from(player.position.x);
        let delta_y = f64::from(local.position.y) - f64::from(player.position.y);
        let delta_z = f64::from(local.position.z) - f64::from(player.position.z);
        Some((delta_x * delta_x + delta_y * delta_y + delta_z * delta_z).sqrt())
    }

    /// Returns the current horizontal-view set. Positions and heading remain
    /// separate samples: a fresh heading with a stale position ranks nobody.
    pub(super) fn visibility(
        &self,
        enabled: bool,
        horizontal_fov_deg: f32,
        now: Instant,
    ) -> (bool, bool, HashSet<String>, Option<Instant>) {
        let view = self
            .local
            .as_ref()
            .zip(self.heading.as_ref())
            .filter(|(local, heading)| {
                fresh(local, now)
                    && local.scope == heading.scope
                    && now.saturating_duration_since(heading.observed) < VIEW_TTL
            });
        let local_view = view.is_some();
        let Some((local, heading)) = view.filter(|_| enabled) else {
            return (enabled, local_view, HashSet::new(), None);
        };
        let mut valid_until = (local.observed + POSITION_TTL).min(heading.observed + VIEW_TTL);
        let half = f64::from(horizontal_fov_deg).to_radians() * 0.5;
        let minimum_cos = half.cos();
        let yaw = f64::from(heading.yaw_deg).to_radians();
        let (forward_x, forward_z) = yaw.sin_cos();
        let mut visible = HashSet::new();
        for (id, player) in &self.players {
            if !fresh(player, now) || local.scope != player.scope {
                continue;
            }
            let delta_x = f64::from(player.position.x) - f64::from(local.position.x);
            let delta_z = f64::from(player.position.z) - f64::from(local.position.z);
            let distance = delta_x.hypot(delta_z);
            if distance <= f64::EPSILON {
                continue;
            }
            let dot = forward_x * delta_x + forward_z * delta_z;
            if dot / distance >= minimum_cos - f64::EPSILON * 4.0 {
                visible.insert(id.clone());
                valid_until = valid_until.min(player.observed + POSITION_TTL);
            }
        }
        (enabled, local_view, visible, Some(valid_until))
    }

    pub(super) fn remove(&mut self, id: &str) {
        self.players.remove(id);
    }

    pub(super) fn clear(&mut self) {
        self.local = None;
        self.heading = None;
        self.players.clear();
    }

    pub(super) fn fresh_players(&self, now: Instant) -> usize {
        self.players
            .values()
            .filter(|value| fresh(value, now))
            .count()
    }

    pub(super) fn local_fresh(&self, now: Instant) -> bool {
        self.local.as_ref().is_some_and(|value| fresh(value, now))
    }

    pub(super) fn player_ids(&self) -> impl Iterator<Item = &str> {
        self.players.keys().map(String::as_str)
    }
}

fn fresh(value: &Tracked, now: Instant) -> bool {
    now.saturating_duration_since(value.observed) < POSITION_TTL
}

fn newer(current: Option<&Tracked>, scope: Scope, tick: u32) -> bool {
    current.is_none_or(|value| value.scope != scope || newer_tick(tick, value.position.tick))
}

pub(super) fn newer_tick(tick: u32, previous: u32) -> bool {
    let delta = tick.wrapping_sub(previous);
    delta != 0 && delta < (1 << 31)
}

pub(super) fn decode_message(message: &[u8]) -> Option<DecodedMessage> {
    let parameters = super::names::event_parameters(message, 12)?;
    let actor = parameters
        .iter()
        .find(|(code, _)| *code == 254)
        .and_then(|(_, value)| value.actor());
    let payload = parameters.iter().find_map(|(code, value)| {
        (*code == 245)
            .then_some(value)
            .and_then(|value| match value {
                super::names::Value::Bytes(bytes) => Some(*bytes),
                _ => None,
            })
    })?;
    let (position, yaw_deg) = decode_payload(payload);
    Some(DecodedMessage {
        actor,
        position,
        yaw_deg,
    })
}

fn decode_payload(payload: &[u8]) -> (PositionDecode, Option<f32>) {
    if payload.len() < 5 {
        return (PositionDecode::Unsupported, None);
    }
    let flags = payload[4];
    if matches!(flags, 0x00 | 0x02) {
        return (PositionDecode::NoPosition, None);
    }
    if !matches!(flags, 0x03 | 0x07 | 0x47 | 0x87 | 0xc7)
        || payload.len() < 18
        || !matches!(payload[5], 0x03 | 0x13 | 0x23 | 0x33 | 0x43 | 0x53 | 0x63)
    {
        return (PositionDecode::Unsupported, None);
    }
    let read = |offset| f32::from_le_bytes(payload[offset..offset + 4].try_into().unwrap());
    let position = Position {
        tick: u32::from_le_bytes(payload[0..4].try_into().unwrap()),
        x: read(6),
        y: read(10),
        z: read(14),
    };
    if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
        return (PositionDecode::Unsupported, None);
    }
    // The high two flag bits extend the 10-bit yaw; bit shifts must widen
    // before shifting or 0x47 loses its upper orientation bits.
    let yaw_deg = payload.get(18).map(|low| {
        let bits = (u16::from(flags >> 6) << 8) | u16::from(*low);
        f32::from(bits) * (360.0 / 1024.0)
    });
    (PositionDecode::Sample(position), yaw_deg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_three_axis_distance_and_rejects_stale_or_cross_scope_positions() {
        let now = Instant::now();
        let mut registry = Registry::default();
        registry.observe_local(
            1,
            2,
            Position {
                tick: 10,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            now,
        );
        registry.observe_player(
            "near",
            1,
            2,
            Position {
                tick: 10,
                x: 6.0,
                y: 8.0,
                z: 0.0,
            },
            now,
        );
        registry.observe_player(
            "other-scope",
            2,
            2,
            Position {
                tick: 10,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            now,
        );

        assert_eq!(
            registry.admission("near", true, 10.0, now),
            Admission::InRange
        );
        assert_eq!(registry.distance("near", now), Some(10.0));
        registry.observe_player(
            "near",
            1,
            2,
            Position {
                tick: 9,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            now,
        );
        assert_eq!(
            registry.admission("near", true, 10.0, now),
            Admission::InRange
        );
        assert_eq!(
            registry.admission("other-scope", true, 10.0, now),
            Admission::Unknown
        );
        assert_eq!(registry.distance("other-scope", now), None);
        assert_eq!(
            registry.admission(
                "near",
                true,
                10.0,
                now + POSITION_TTL + Duration::from_millis(1)
            ),
            Admission::Unknown
        );
        assert_eq!(
            registry.admission("near", false, 10.0, now),
            Admission::Disabled
        );
    }

    #[test]
    fn ranks_only_players_inside_the_horizontal_heading() {
        let now = Instant::now();
        let mut registry = Registry::default();
        registry.observe_local(
            1,
            2,
            Position {
                tick: 10,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            now,
        );
        registry.observe_local_yaw(1, 2, 10, 0.0, now);
        for (id, x, z) in [("front", 0.0, 5.0), ("back", 0.0, -5.0), ("side", 5.0, 0.0)] {
            registry.observe_player(
                id,
                1,
                2,
                Position {
                    tick: 10,
                    x,
                    y: 0.0,
                    z,
                },
                now,
            );
        }
        let (_, local_view, visible, _) = registry.visibility(true, 90.0, now);
        assert!(local_view);
        assert_eq!(visible, HashSet::from(["front".to_string()]));
        let (_, _, wide, _) = registry.visibility(true, 180.0, now);
        assert!(wide.contains("side") && !wide.contains("back"));
        assert_eq!(registry.visibility(false, 90.0, now).2.len(), 0);
        assert_eq!(
            registry
                .visibility(true, 90.0, now + VIEW_TTL + Duration::from_millis(1))
                .2
                .len(),
            0
        );
    }

    #[test]
    fn headings_use_degrees_across_cardinal_directions_and_wraparound() {
        let now = Instant::now();
        let mut registry = Registry::default();
        let origin = Position {
            tick: 1,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        registry.observe_local(1, 2, origin, now);
        for (id, x, z) in [
            ("north", 0.0, 5.0),
            ("east", 5.0, 0.0),
            ("south", 0.0, -5.0),
            ("west", -5.0, 0.0),
        ] {
            registry.observe_player(id, 1, 2, Position { x, z, ..origin }, now);
        }
        for (tick, yaw, expected) in [
            (1, 90.0, "east"),
            (2, 180.0, "south"),
            (3, 270.0, "west"),
            (4, 359.648_44, "north"),
            (5, 0.0, "north"),
        ] {
            registry.observe_local_yaw(1, 2, tick, yaw, now);
            assert_eq!(
                registry.visibility(true, 60.0, now).2,
                HashSet::from([expected.to_string()])
            );
        }
    }

    #[test]
    fn heading_ticks_are_independent_and_duplicates_cannot_extend_freshness() {
        let now = Instant::now();
        let later = now + Duration::from_millis(500);
        let mut registry = Registry::default();
        let origin = Position {
            tick: 10,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        registry.observe_local(1, 2, origin, now);
        registry.observe_player("east", 1, 2, Position { x: 5.0, ..origin }, now);
        // Position was seen first without its heading; the same tick can fill it.
        registry.observe_local_yaw(1, 2, 10, 90.0, later);
        registry.observe_local(1, 2, Position { tick: 11, ..origin }, later);
        registry.observe_local_yaw(1, 2, 9, 180.0, later);
        registry.observe_local_yaw(1, 2, 10, 180.0, later + Duration::from_millis(200));
        assert!(registry.visibility(true, 60.0, later).2.contains("east"));
        assert_eq!(
            registry.visibility(true, 60.0, later).3,
            Some(later + VIEW_TTL)
        );
        assert!(!registry.visibility(true, 60.0, later + VIEW_TTL).1);
        assert!(newer_tick(0, u32::MAX));
        assert!(!newer_tick(u32::MAX, 0));
        registry.observe_local_yaw(1, 3, 11, 180.0, later);
        assert!(registry.visibility(true, 60.0, later).2.contains("east"));
    }

    #[test]
    fn snapshot_deadline_covers_every_contributing_sample() {
        let now = Instant::now();
        let later = now + POSITION_TTL - Duration::from_millis(500);
        let origin = Position {
            tick: 1,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let mut registry = Registry::default();
        registry.observe_local(1, 2, origin, later);
        registry.observe_local_yaw(1, 2, 1, 0.0, later);
        registry.observe_player("front", 1, 2, Position { z: 5.0, ..origin }, now);
        assert_eq!(
            registry.visibility(true, 60.0, later).3,
            Some(now + POSITION_TTL)
        );
        assert!(registry
            .visibility(true, 60.0, now + POSITION_TTL)
            .2
            .is_empty());
        registry.observe_player(
            "front",
            1,
            2,
            Position {
                tick: 2,
                z: 5.0,
                ..origin
            },
            later,
        );
        registry.observe_local(1, 2, Position { tick: 2, ..origin }, now + POSITION_TTL);
        registry.observe_local_yaw(1, 2, 2, 0.0, now + POSITION_TTL);
        assert!(registry
            .visibility(true, 60.0, now + POSITION_TTL)
            .2
            .contains("front"));
        registry.clear();
        assert!(!registry.visibility(true, 60.0, now + POSITION_TTL).1);
    }

    #[test]
    fn decodes_yaw_from_the_high_flag_bits() {
        let mut payload = vec![0u8; 19];
        payload[4] = 0x47;
        payload[5] = 0x33;
        payload[18] = 0;
        assert_eq!(decode_payload(&payload).1, Some(90.0));
        payload[4] = 0xc7;
        payload[18] = 0xff;
        let yaw = decode_payload(&payload).1.unwrap();
        assert!((yaw - 359.648_44).abs() < 0.001);
        assert_eq!(decode_payload(&payload[..18]).1, None);
    }

    #[test]
    fn decodes_only_known_event_12_position_layouts() {
        let sample = decode_message(&event_12(17, Some([0x47, 0x33]), Some([1.5, -2.0, 3.25])))
            .expect("event 12");
        assert_eq!(sample.actor, Some(17));
        assert_eq!(
            sample.position,
            PositionDecode::Sample(Position {
                tick: 0,
                x: 1.5,
                y: -2.0,
                z: 3.25,
            })
        );

        let local = decode_message(&event_12_outbound([0.0, 0.0, 0.0])).expect("outbound event 12");
        assert_eq!(local.actor, None);
        assert_eq!(
            local.position,
            PositionDecode::Sample(Position {
                tick: 0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            })
        );

        let empty = decode_message(&event_12(17, Some([0x02, 0]), None)).expect("event 12");
        assert_eq!(empty.position, PositionDecode::NoPosition);

        let unknown = decode_message(&event_12(17, Some([0x01, 0x33]), Some([0.0, 0.0, 0.0])))
            .expect("event 12");
        assert_eq!(unknown.position, PositionDecode::Unsupported);
    }

    fn event_12(actor: u8, branch: Option<[u8; 2]>, position: Option<[f32; 3]>) -> Vec<u8> {
        let mut payload = vec![0; 4];
        payload.push(branch.expect("branch")[0]);
        payload.push(branch.expect("branch")[1]);
        if let Some([x, y, z]) = position {
            payload.extend_from_slice(&x.to_le_bytes());
            payload.extend_from_slice(&y.to_le_bytes());
            payload.extend_from_slice(&z.to_le_bytes());
        }
        let mut message = vec![0xf3, 4, 12, 2, 245, 0x43];
        message.push(payload.len() as u8);
        message.extend(payload);
        message.extend([254, 11, actor]);
        message
    }

    fn event_12_outbound([x, y, z]: [f32; 3]) -> Vec<u8> {
        let mut payload = vec![0; 4];
        payload.extend([0x47, 0x33]);
        payload.extend_from_slice(&x.to_le_bytes());
        payload.extend_from_slice(&y.to_le_bytes());
        payload.extend_from_slice(&z.to_le_bytes());
        let mut message = vec![0xf3, 2, 0xfd, 2, 244, 11, 12, 245, 0x43];
        message.push(payload.len() as u8);
        message.extend(payload);
        message
    }
}
