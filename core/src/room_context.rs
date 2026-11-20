use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::watch;

use crate::models::SpeakerIdentity;
use crate::player_audio::session::NetworkConsumers;
use crate::player_audio::PlayerInfo;
use crate::vrchat_chatbox::TaskValidity;
use crate::vrchat_log;

const MAX_WORLD_NAME_CHARS: usize = 128;
const MAX_DISPLAY_NAME_CHARS: usize = 128;

pub(crate) fn sanitize_world_name(raw: &str) -> Option<String> {
    sanitize_label(raw, MAX_WORLD_NAME_CHARS)
}

pub(crate) fn sanitize_display_name(raw: &str) -> Option<String> {
    sanitize_label(raw, MAX_DISPLAY_NAME_CHARS)
}

fn sanitize_label(raw: &str, max_chars: usize) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control()
                || matches!(
                    c,
                    '\u{200B}'..='\u{200F}'
                        | '\u{202A}'..='\u{202E}'
                        | '\u{2066}'..='\u{2069}'
                        | '\u{FEFF}'
                )
            {
                ' '
            } else {
                c
            }
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated: String = collapsed.chars().take(max_chars).collect();
    let trimmed = truncated.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RoomContextAvailability {
    #[default]
    Unavailable,
    DegradedLogOnly,
    Ready,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RoomContextTier {
    #[default]
    None,
    LogOnly,
    NetworkChatbox,
    NetworkPlayerAudio,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RoomMember {
    pub speaker_key: String,
    pub label: String,
    pub display_name: String,
    pub user_id: Option<String>,
    pub track_ids: Vec<String>,
    pub active: bool,
    pub selected: bool,
    pub distance_cm: Option<u32>,
    pub order: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct RoomContextSnapshot {
    pub revision: u64,
    pub scope_id: Option<String>,
    pub world_name: Option<String>,
    pub availability: RoomContextAvailability,
    pub tier: RoomContextTier,
    pub members: Vec<RoomMember>,
    pub validity: TaskValidity,
}

impl Default for RoomContextSnapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            scope_id: None,
            world_name: None,
            availability: RoomContextAvailability::Unavailable,
            tier: RoomContextTier::None,
            members: Vec::new(),
            validity: TaskValidity::new(),
        }
    }
}

impl PartialEq for RoomContextSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.scope_id == other.scope_id
            && self.world_name == other.world_name
            && self.availability == other.availability
            && self.tier == other.tier
            && self.members == other.members
    }
}

impl Eq for RoomContextSnapshot {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct RoomContextHealth {
    pub availability: RoomContextAvailability,
    pub tier: RoomContextTier,
    pub revision: u64,
    pub scope_id: Option<String>,
    pub world_name: Option<String>,
    pub member_count: usize,
    pub active_member_count: usize,
}

impl RoomContextSnapshot {
    pub(crate) fn health(&self) -> RoomContextHealth {
        RoomContextHealth {
            availability: self.availability,
            tier: self.tier,
            revision: self.revision,
            scope_id: self.scope_id.clone(),
            world_name: self.world_name.clone(),
            member_count: self.members.len(),
            active_member_count: self.members.iter().filter(|m| m.active).count(),
        }
    }

    pub(crate) fn member_names(&self, max_names: usize) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut names = Vec::new();
        for member in &self.members {
            if names.len() >= max_names {
                break;
            }
            if seen.insert(member.display_name.to_lowercase()) {
                names.push(member.display_name.clone());
            }
        }
        names
    }

    pub(crate) fn asr_terms(&self, include_world_name: bool, max_terms: usize) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut terms = Vec::new();
        if include_world_name {
            if let Some(world) = &self.world_name {
                if seen.insert(world.to_lowercase()) && terms.len() < max_terms {
                    terms.push(world.clone());
                }
            }
        }
        for member in &self.members {
            if terms.len() >= max_terms {
                break;
            }
            if seen.insert(member.display_name.to_lowercase()) {
                terms.push(member.display_name.clone());
            }
        }
        terms
    }

    pub(crate) fn member_for_speaker(
        &self,
        speaker: Option<&SpeakerIdentity>,
    ) -> Option<&RoomMember> {
        let speaker = speaker?;
        if let Some(user_id) = speaker.user_id.as_deref() {
            if let Some(found) = self
                .members
                .iter()
                .find(|member| member.user_id.as_deref() == Some(user_id))
            {
                return Some(found);
            }
        }
        self.members.iter().find(|member| {
            member.speaker_key == speaker.id || member.track_ids.iter().any(|id| id == &speaker.id)
        })
    }
}

struct State {
    log_room_key: Option<String>,
    log_world_name: Option<String>,
    consumers: NetworkConsumers,
    network_stream_ready: bool,
    network_members: Vec<RoomMember>,
    label_map: HashMap<String, String>,
    next_label: u64,
    validity: TaskValidity,
    revision: u64,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    fn new() -> Self {
        Self {
            log_room_key: None,
            log_world_name: None,
            consumers: NetworkConsumers::default(),
            network_stream_ready: false,
            network_members: Vec::new(),
            label_map: HashMap::new(),
            next_label: 1,
            validity: TaskValidity::new(),
            revision: 0,
        }
    }

    fn reset_room_validity(&mut self) {
        self.validity.invalidate();
        self.validity = TaskValidity::new();
        self.network_members.clear();
        self.network_stream_ready = false;
        self.label_map.clear();
        self.next_label = 1;
    }

    fn resolve_tier_and_availability(&self) -> (RoomContextTier, RoomContextAvailability) {
        if self.log_room_key.is_none()
            && self.network_members.is_empty()
            && !self.network_stream_ready
        {
            return (RoomContextTier::None, RoomContextAvailability::Unavailable);
        }
        if self.consumers.voice {
            let availability = if self.network_stream_ready {
                RoomContextAvailability::Ready
            } else {
                RoomContextAvailability::DegradedLogOnly
            };
            (RoomContextTier::NetworkPlayerAudio, availability)
        } else if self.consumers.chatbox {
            let availability = if self.network_stream_ready {
                RoomContextAvailability::Ready
            } else {
                RoomContextAvailability::DegradedLogOnly
            };
            (RoomContextTier::NetworkChatbox, availability)
        } else {
            (
                RoomContextTier::LogOnly,
                RoomContextAvailability::DegradedLogOnly,
            )
        }
    }

    fn publish(&mut self, tx: &watch::Sender<RoomContextSnapshot>) {
        let (tier, availability) = self.resolve_tier_and_availability();
        let next_scope = self.log_room_key.clone();
        let next_world = self.log_world_name.clone();
        let next_members = self.network_members.clone();
        let changed = {
            let current = tx.borrow();
            current.scope_id != next_scope
                || current.world_name != next_world
                || current.availability != availability
                || current.tier != tier
                || current.members != next_members
        };
        if !changed {
            return;
        }
        self.revision += 1;
        tx.send_replace(RoomContextSnapshot {
            revision: self.revision,
            scope_id: next_scope,
            world_name: next_world,
            availability,
            tier,
            members: next_members,
            validity: self.validity.clone(),
        });
    }
}

#[derive(Clone)]
pub(crate) struct RoomContextStore {
    state: Arc<Mutex<State>>,
    tx: Arc<watch::Sender<RoomContextSnapshot>>,
}

impl Default for RoomContextStore {
    fn default() -> Self {
        let (tx, _) = watch::channel(RoomContextSnapshot::default());
        Self {
            state: Arc::new(Mutex::new(State::new())),
            tx: Arc::new(tx),
        }
    }
}

impl RoomContextStore {
    pub(crate) fn reader(&self) -> RoomContextReader {
        RoomContextReader {
            tx: Arc::clone(&self.tx),
        }
    }

    pub(crate) fn network_publisher(&self) -> NetworkCapturePublisher {
        NetworkCapturePublisher {
            store: self.clone(),
        }
    }

    pub(crate) fn sync_log(&self, log: &vrchat_log::Snapshot) {
        let next_room_key = log.process.filter(|_| log.ready).and_then(|process| {
            log.world_session_token
                .map(|token| format!("vrc-room:{}:{}:{token}", process.pid, process.created))
        });
        let mut state = self.state.lock().expect("room context state");
        if state.log_room_key != next_room_key {
            state.log_room_key = next_room_key;
            state.reset_room_validity();
        }
        state.log_world_name = state
            .log_room_key
            .as_ref()
            .and(log.world_name.as_deref())
            .and_then(sanitize_world_name);
        state.publish(&self.tx);
    }

    pub(crate) fn notify_settings_changed(&self) {
        self.tx
            .send_modify(|snapshot| snapshot.revision = snapshot.revision.wrapping_add(1));
    }
}

#[derive(Clone)]
pub(crate) struct RoomContextReader {
    tx: Arc<watch::Sender<RoomContextSnapshot>>,
}

impl Default for RoomContextReader {
    fn default() -> Self {
        RoomContextStore::default().reader()
    }
}

impl RoomContextReader {
    pub(crate) fn snapshot(&self) -> RoomContextSnapshot {
        self.tx.borrow().clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<RoomContextSnapshot> {
        self.tx.subscribe()
    }

    pub(crate) fn health(&self) -> RoomContextHealth {
        self.tx.borrow().health()
    }
}

#[derive(Clone)]
pub(crate) struct NetworkCapturePublisher {
    store: RoomContextStore,
}

impl NetworkCapturePublisher {
    pub(crate) fn set_consumers(&self, consumers: NetworkConsumers) {
        let mut state = self.store.state.lock().expect("room context state");
        state.consumers = consumers;
        if !consumers.any() {
            state.network_stream_ready = false;
            state.network_members.clear();
        }
        state.publish(&self.store.tx);
    }

    pub(crate) fn set_stream_ready(&self, ready: bool) {
        let mut state = self.store.state.lock().expect("room context state");
        state.network_stream_ready = ready;
        if !ready {
            state.network_members.clear();
        }
        state.publish(&self.store.tx);
    }

    pub(crate) fn replace_members(&self, players: &[PlayerInfo]) {
        let mut state = self.store.state.lock().expect("room context state");
        let mut deduped: HashMap<String, RoomMember> = HashMap::new();
        for player in players {
            let Some(display_name) = player
                .speaker
                .display_name
                .as_deref()
                .and_then(sanitize_display_name)
            else {
                continue;
            };
            let speaker_key = player
                .speaker
                .user_id
                .clone()
                .unwrap_or_else(|| player.speaker.id.clone());
            let distance_cm = player
                .distance_m
                .filter(|d| d.is_finite() && *d >= 0.0)
                .map(|d| (d * 100.0).min(u32::MAX as f64).round() as u32);
            match deduped.get_mut(&speaker_key) {
                Some(existing) => {
                    existing.selected |= player.selected;
                    existing.active |= player.active;
                    existing.distance_cm = match (existing.distance_cm, distance_cm) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                    existing.order = existing.order.min(player.speaker.index);
                    if !existing.track_ids.contains(&player.speaker.id) {
                        existing.track_ids.push(player.speaker.id.clone());
                    }
                }
                None => {
                    let label = if let Some(existing) = state.label_map.get(&speaker_key) {
                        existing.clone()
                    } else {
                        let next = format!("p{}", state.next_label);
                        state.next_label += 1;
                        state.label_map.insert(speaker_key.clone(), next.clone());
                        next
                    };
                    deduped.insert(
                        speaker_key.clone(),
                        RoomMember {
                            speaker_key,
                            label,
                            display_name,
                            user_id: player.speaker.user_id.clone(),
                            track_ids: vec![player.speaker.id.clone()],
                            active: player.active,
                            selected: player.selected,
                            distance_cm,
                            order: player.speaker.index,
                        },
                    );
                }
            }
        }
        let mut members: Vec<_> = deduped.into_values().collect();
        for member in &mut members {
            member.track_ids.sort();
        }
        members.sort_by_key(|member| {
            (
                !member.selected,
                !member.active,
                member.distance_cm.unwrap_or(u32::MAX),
                member.order,
            )
        });
        state.network_members = members;
        state.publish(&self.store.tx);
    }

    pub(crate) fn invalidate_network_room(&self) {
        let mut state = self.store.state.lock().expect("room context state");
        state.reset_room_validity();
        state.publish(&self.store.tx);
    }

    pub(crate) fn end_capture(&self) {
        let mut state = self.store.state.lock().expect("room context state");
        state.consumers = NetworkConsumers::default();
        state.network_stream_ready = false;
        state.network_members.clear();
        state.publish(&self.store.tx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vrchat_process::ProcessIdentity;

    fn log_snapshot(token: Option<u64>, world: Option<&str>) -> vrchat_log::Snapshot {
        vrchat_log::Snapshot {
            process: Some(ProcessIdentity {
                pid: 42,
                created: 100,
            }),
            ready: token.is_some(),
            room_token: token,
            world_session_token: token,
            world_name: world.map(str::to_owned),
            error: None,
        }
    }

    fn player(
        track: &str,
        index: u64,
        name: Option<&str>,
        user_id: Option<&str>,
        selected: bool,
        active: bool,
        distance_m: Option<f64>,
    ) -> PlayerInfo {
        PlayerInfo {
            speaker: SpeakerIdentity {
                id: track.into(),
                index,
                display_name: name.map(str::to_owned),
                user_id: user_id.map(str::to_owned),
            },
            selected,
            active,
            distance_m,
        }
    }

    #[test]
    fn log_only_mode_tracks_world_scope_and_invalidates_on_leave() {
        let store = RoomContextStore::default();
        let reader = store.reader();
        assert_eq!(
            reader.snapshot().availability,
            RoomContextAvailability::Unavailable
        );

        store.sync_log(&log_snapshot(Some(77), Some("Club Orion\n\tRoom")));
        let first = reader.snapshot();
        assert_eq!(first.tier, RoomContextTier::LogOnly);
        assert_eq!(first.availability, RoomContextAvailability::DegradedLogOnly);
        assert_eq!(first.world_name.as_deref(), Some("Club Orion Room"));
        assert!(first.scope_id.is_some());
        assert!(first.validity.is_current());

        store.sync_log(&log_snapshot(None, None));
        let left = reader.snapshot();
        assert!(!first.validity.is_current());
        assert_eq!(left.availability, RoomContextAvailability::Unavailable);
        assert_eq!(left.scope_id, None);
    }

    #[test]
    fn member_dedup_priority_sort_and_stable_labels() {
        let store = RoomContextStore::default();
        let publisher = store.network_publisher();
        let reader = store.reader();

        store.sync_log(&log_snapshot(Some(99), Some("Cafe")));
        publisher.set_consumers(NetworkConsumers {
            voice: true,
            chatbox: true,
        });
        publisher.set_stream_ready(true);
        publisher.replace_members(&[
            player("t1", 1, Some("Bob"), Some("usr_b"), false, true, Some(5.0)),
            player(
                "t2",
                2,
                Some("Alice"),
                Some("usr_a"),
                true,
                false,
                Some(8.0),
            ),
            player(
                "t3",
                3,
                Some("Alice"),
                Some("usr_a"),
                false,
                true,
                Some(2.0),
            ),
            player("t4", 4, None, Some("usr_noname"), true, true, Some(1.0)),
        ]);

        let snap = reader.snapshot();
        assert_eq!(snap.tier, RoomContextTier::NetworkPlayerAudio);
        assert_eq!(snap.availability, RoomContextAvailability::Ready);
        assert_eq!(snap.members.len(), 2);
        // Alice is selected + active and closer after merge.
        assert_eq!(snap.members[0].display_name, "Alice");
        assert_eq!(snap.members[1].display_name, "Bob");
        let alice_label = snap.members[0].label.clone();
        let bob_label = snap.members[1].label.clone();

        // Removing Bob keeps Alice's assigned room label stable.
        publisher.replace_members(&[player(
            "t3",
            3,
            Some("Alice"),
            Some("usr_a"),
            false,
            false,
            Some(4.0),
        )]);
        let updated = reader.snapshot();
        assert_eq!(updated.members[0].label, alice_label);
        assert_ne!(alice_label, bob_label);
    }

    #[test]
    fn sanitization_strips_control_and_bidi_chars_and_truncates() {
        assert_eq!(
            sanitize_display_name(" \u{202E}Eva\x07\nTest\u{200B} "),
            Some("Eva Test".into())
        );
        assert_eq!(sanitize_world_name("\x00\u{202A}\t  "), None);
        let long = "あ".repeat(150);
        assert_eq!(sanitize_world_name(&long).unwrap().chars().count(), 128);
    }
}
