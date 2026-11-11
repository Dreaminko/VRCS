use std::collections::HashMap;
use std::time::{Duration, Instant};

const POSITION_TTL: Duration = Duration::from_secs(15);

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

#[derive(Default)]
pub(super) struct Registry {
    local: Option<Tracked>,
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
        self.local = Some(Tracked {
            scope,
            position,
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

    #[cfg(any(windows, target_os = "linux"))]
    pub(super) fn remove(&mut self, id: &str) {
        self.players.remove(id);
    }

    #[cfg(any(windows, target_os = "linux"))]
    pub(super) fn clear(&mut self) {
        self.local = None;
        self.players.clear();
    }

    #[cfg(any(windows, target_os = "linux"))]
    pub(super) fn fresh_players(&self, now: Instant) -> usize {
        self.players
            .values()
            .filter(|value| fresh(value, now))
            .count()
    }

    #[cfg(any(windows, target_os = "linux"))]
    pub(super) fn local_fresh(&self, now: Instant) -> bool {
        self.local.as_ref().is_some_and(|value| fresh(value, now))
    }

    #[cfg(any(windows, target_os = "linux"))]
    pub(super) fn player_ids(&self) -> impl Iterator<Item = &str> {
        self.players.keys().map(String::as_str)
    }
}

fn fresh(value: &Tracked, now: Instant) -> bool {
    now.saturating_duration_since(value.observed) <= POSITION_TTL
}

fn newer(current: Option<&Tracked>, scope: Scope, tick: u32) -> bool {
    current.is_none_or(|value| {
        let delta = tick.wrapping_sub(value.position.tick);
        value.scope != scope || (delta != 0 && delta < u32::MAX / 2)
    })
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
    Some(DecodedMessage {
        actor,
        position: decode_payload(payload),
    })
}

fn decode_payload(payload: &[u8]) -> PositionDecode {
    if payload.len() < 5 {
        return PositionDecode::Unsupported;
    }
    let flags = payload[4];
    if matches!(flags, 0x00 | 0x02) {
        return PositionDecode::NoPosition;
    }
    if !matches!(flags, 0x03 | 0x07 | 0x47 | 0x87 | 0xc7)
        || payload.len() < 18
        || !matches!(payload[5], 0x03 | 0x13 | 0x23 | 0x33 | 0x43 | 0x53 | 0x63)
    {
        return PositionDecode::Unsupported;
    }
    let read = |offset| f32::from_le_bytes(payload[offset..offset + 4].try_into().unwrap());
    let position = Position {
        tick: u32::from_le_bytes(payload[0..4].try_into().unwrap()),
        x: read(6),
        y: read(10),
        z: read(14),
    };
    if position.x.is_finite() && position.y.is_finite() && position.z.is_finite() {
        PositionDecode::Sample(position)
    } else {
        PositionDecode::Unsupported
    }
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
