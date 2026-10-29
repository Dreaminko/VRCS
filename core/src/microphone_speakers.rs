use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::models::SpeakerIdentity;

const MAX_SPEAKERS: usize = 32;
const MAX_UTTERANCES: usize = 2048;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpeakerDecision {
    #[default]
    Allow,
    Block,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MicrophoneSpeaker {
    pub id: String,
    pub index: u64,
    pub decision: SpeakerDecision,
    pub last_active_at: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct SpeakerSnapshot {
    pub enabled: bool,
    pub supported: bool,
    pub running: bool,
    pub generation: String,
    pub revision: u64,
    pub snapshot_sequence: u64,
    pub speakers: Vec<MicrophoneSpeaker>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpeakerSelection {
    pub id: String,
    pub decision: SpeakerDecision,
}

struct Utterance {
    speaker: Option<String>,
    revoked: bool,
}

#[derive(Default)]
struct State {
    snapshot: SpeakerSnapshot,
    utterances: HashMap<String, Utterance>,
    order: VecDeque<String>,
}

#[derive(Clone)]
pub(crate) struct MicrophoneSpeakers {
    state: Arc<Mutex<State>>,
    snapshots: watch::Sender<SpeakerSnapshot>,
}

impl Default for MicrophoneSpeakers {
    fn default() -> Self {
        Self {
            state: Arc::default(),
            snapshots: watch::channel(SpeakerSnapshot::default()).0,
        }
    }
}

impl MicrophoneSpeakers {
    pub fn snapshot(&self) -> SpeakerSnapshot {
        self.state
            .lock()
            .expect("microphone speakers")
            .snapshot
            .clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<SpeakerSnapshot> {
        self.snapshots.subscribe()
    }

    fn publish(&self, state: &mut State) {
        state.snapshot.snapshot_sequence += 1;
        self.snapshots.send_replace(state.snapshot.clone());
    }

    pub fn start(&self, supported: bool) {
        let mut state = self.state.lock().expect("microphone speakers");
        state.snapshot.supported = supported;
        state.snapshot.running = true;
        self.reset(&mut state, String::new());
    }

    pub fn begin_session(&self, generation: String) {
        let mut state = self.state.lock().expect("microphone speakers");
        self.reset(&mut state, generation);
    }

    pub fn stop(&self) {
        let mut state = self.state.lock().expect("microphone speakers");
        state.snapshot.running = false;
        self.reset(&mut state, String::new());
    }

    fn reset(&self, state: &mut State, generation: String) {
        state.snapshot.generation = generation;
        state.snapshot.speakers.clear();
        state.snapshot.revision += 1;
        if state.snapshot.enabled {
            for utterance in state.utterances.values_mut() {
                utterance.revoked = true;
            }
        }
        self.publish(state);
    }

    pub fn set_enabled(&self, enabled: bool) -> bool {
        let mut state = self.state.lock().expect("microphone speakers");
        if state.snapshot.enabled == enabled {
            return false;
        }
        state.snapshot.enabled = enabled;
        state.snapshot.revision += 1;
        for utterance in state.utterances.values_mut() {
            utterance.revoked = true;
        }
        self.publish(&mut state);
        true
    }

    pub fn discover(&self, message_id: &str, speaker: Option<&SpeakerIdentity>) {
        let mut state = self.state.lock().expect("microphone speakers");
        let new_speech = state
            .utterances
            .get(message_id)
            .is_none_or(|utterance| utterance.speaker.is_none());
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mut changed = false;
        let snapshot = &mut state.snapshot;
        if let Some(speaker) = speaker {
            if let Some(known) = snapshot
                .speakers
                .iter_mut()
                .find(|item| item.id == speaker.id)
            {
                if new_speech && known.last_active_at != now {
                    known.last_active_at = now.clone();
                    changed = true;
                }
            } else if snapshot.speakers.len() < MAX_SPEAKERS {
                snapshot.speakers.push(MicrophoneSpeaker {
                    id: speaker.id.clone(),
                    index: speaker.index,
                    decision: SpeakerDecision::Allow,
                    last_active_at: now,
                });
                changed = true;
            }
        }
        let revoked =
            state.snapshot.enabled
                && speaker.is_some_and(|identity| {
                    state.snapshot.speakers.iter().any(|item| {
                        item.id == identity.id && item.decision == SpeakerDecision::Block
                    })
                });
        if !state.utterances.contains_key(message_id) {
            state.order.push_back(message_id.into());
            state.utterances.insert(
                message_id.into(),
                Utterance {
                    speaker: speaker.map(|s| s.id.clone()),
                    revoked,
                },
            );
            while state.order.len() > MAX_UTTERANCES {
                if let Some(id) = state.order.pop_front() {
                    state.utterances.remove(&id);
                }
            }
        } else if let Some(speaker) = speaker {
            let utterance = state.utterances.get_mut(message_id).unwrap();
            if utterance
                .speaker
                .as_ref()
                .is_some_and(|id| id != &speaker.id)
            {
                utterance.revoked = true;
            }
            utterance.speaker = Some(speaker.id.clone());
            utterance.revoked |= revoked;
        }
        if changed {
            self.publish(&mut state);
        }
    }

    pub fn apply(
        &self,
        generation: &str,
        revision: u64,
        enabled: bool,
        selections: &[SpeakerSelection],
    ) -> Result<(SpeakerSnapshot, Vec<String>), &'static str> {
        let mut state = self.state.lock().expect("microphone speakers");
        if state.snapshot.generation != generation || state.snapshot.revision != revision {
            return Err("stale");
        }
        if enabled
            && (!state.snapshot.supported || !state.snapshot.running || generation.is_empty())
        {
            return Err("unsupported");
        }
        let mut ids = HashSet::new();
        for selection in selections {
            if !ids.insert(&selection.id) {
                return Err("invalid");
            }
            if !state
                .snapshot
                .speakers
                .iter()
                .any(|speaker| speaker.id == selection.id)
            {
                return Err("unknown");
            }
        }
        let toggled = state.snapshot.enabled != enabled;
        state.snapshot.enabled = enabled;
        for selection in selections {
            state
                .snapshot
                .speakers
                .iter_mut()
                .find(|speaker| speaker.id == selection.id)
                .unwrap()
                .decision = selection.decision;
        }
        let blocked: HashSet<_> = state
            .snapshot
            .speakers
            .iter()
            .filter(|item| item.decision == SpeakerDecision::Block)
            .map(|item| item.id.clone())
            .collect();
        let mut cancelled = Vec::new();
        for (id, utterance) in &mut state.utterances {
            if toggled
                || (enabled
                    && utterance
                        .speaker
                        .as_ref()
                        .is_some_and(|id| blocked.contains(id)))
            {
                if !utterance.revoked {
                    cancelled.push(id.clone());
                }
                utterance.revoked = true;
            }
        }
        state.snapshot.revision += 1;
        self.publish(&mut state);
        Ok((state.snapshot.clone(), cancelled))
    }

    pub fn allows(&self, source: &str, message_id: &str) -> bool {
        self.with_allowed(source, message_id, || ()).is_some()
    }

    // Keep the short commit and policy update under the same lock.
    pub fn with_allowed<T>(
        &self,
        source: &str,
        message_id: &str,
        commit: impl FnOnce() -> T,
    ) -> Option<T> {
        // Explicit history translation is independent of live capture filtering.
        if source != "microphone" || message_id.starts_with("translation-") {
            return Some(commit());
        }
        let state = self.state.lock().expect("microphone speakers");
        // Native IDs carry the connection generation, including in delayed jobs.
        if let Some(native) = message_id
            .strip_prefix("qwen-source-")
            .or_else(|| message_id.strip_prefix("qwen-preview-"))
        {
            if !state.utterances.contains_key(message_id)
                || (state.snapshot.enabled
                    && (state.snapshot.generation.is_empty()
                        || !native.starts_with(&format!("{}-", state.snapshot.generation))))
            {
                return None;
            }
        }
        let utterance = state.utterances.get(message_id);
        if utterance.is_some_and(|item| item.revoked) {
            return None;
        }
        if state.snapshot.enabled
            && !(state.snapshot.running
                && state.snapshot.supported
                && utterance
                    .and_then(|item| item.speaker.as_ref())
                    .is_some_and(|id| {
                        !state.snapshot.speakers.iter().any(|speaker| {
                            &speaker.id == id && speaker.decision == SpeakerDecision::Block
                        })
                    }))
        {
            return None;
        }
        Some(commit())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SpeakerIdentity;

    fn speaker(id: &str, index: u64) -> SpeakerIdentity {
        SpeakerIdentity {
            id: id.into(),
            index,
            display_name: None,
            user_id: None,
        }
    }

    #[test]
    fn microphone_speakers_default_to_allow_and_only_block_selected_people() {
        let filter = MicrophoneSpeakers::default();
        filter.set_enabled(true);
        filter.start(true);
        filter.begin_session("session".into());
        filter.discover("a", Some(&speaker("alice", 0)));
        filter.discover("b", Some(&speaker("bob", 1)));
        assert!(filter.allows("microphone", "a"));
        assert!(filter.allows("microphone", "b"));
        let snapshot = filter.snapshot();
        filter
            .apply(
                &snapshot.generation,
                snapshot.revision,
                true,
                &[
                    SpeakerSelection {
                        id: "alice".into(),
                        decision: SpeakerDecision::Allow,
                    },
                    SpeakerSelection {
                        id: "bob".into(),
                        decision: SpeakerDecision::Block,
                    },
                ],
            )
            .unwrap();
        filter.discover("next-a", Some(&speaker("alice", 0)));
        filter.discover("next-b", Some(&speaker("bob", 1)));
        filter.discover("new-person", Some(&speaker("charlie", 2)));
        assert!(filter.allows("microphone", "next-a"));
        assert!(!filter.allows("microphone", "next-b"));
        assert!(filter.allows("microphone", "new-person"));
        assert!(!filter.allows("microphone", "unidentified"));
        assert!(filter.allows("speaker", "next-b"));
        assert!(filter.allows("chatbox", "next-b"));
    }

    #[test]
    fn reconnect_discards_decisions_and_rejects_stale_controls() {
        let filter = MicrophoneSpeakers::default();
        filter.start(true);
        filter.begin_session("first".into());
        filter.discover("a", Some(&speaker("first-person", 0)));
        filter.discover("qwen-source-first-old", Some(&speaker("first-person", 0)));
        let first = filter.snapshot();
        filter
            .apply(
                &first.generation,
                first.revision,
                true,
                &[SpeakerSelection {
                    id: "first-person".into(),
                    decision: SpeakerDecision::Block,
                }],
            )
            .unwrap();
        filter.begin_session("second".into());
        filter.discover("b", Some(&speaker("second-person", 0)));
        assert!(filter.allows("microphone", "b"));
        filter.set_enabled(false);
        assert!(!filter.allows("microphone", "qwen-source-first-old"));
        assert!(filter
            .apply(&first.generation, first.revision, true, &[])
            .is_err());
        assert_eq!(
            filter.snapshot().speakers[0].decision,
            SpeakerDecision::Allow
        );
    }

    #[test]
    fn disabled_filter_keeps_normal_drained_results_after_capture_stops() {
        let filter = MicrophoneSpeakers::default();
        filter.start(true);
        filter.begin_session("session".into());
        filter.discover("qwen-source-session-tail", None);
        filter.stop();
        assert!(filter.allows("microphone", "qwen-source-session-tail"));
    }

    #[test]
    fn microphone_speakers_late_identity_resolves_to_default_allowed_speaker() {
        let filter = MicrophoneSpeakers::default();
        filter.set_enabled(true);
        filter.start(true);
        filter.begin_session("session".into());
        let person = speaker("person", 0);
        filter.discover("unknown", None);
        assert!(!filter.allows("microphone", "unknown"));
        filter.discover("unknown", Some(&person));
        assert!(filter.allows("microphone", "unknown"));
    }

    #[test]
    fn unblocking_does_not_replay_a_revoked_utterance() {
        let filter = MicrophoneSpeakers::default();
        filter.start(true);
        filter.begin_session("session".into());
        filter.discover("old", Some(&speaker("person", 0)));
        let snapshot = filter.snapshot();
        filter
            .apply(
                &snapshot.generation,
                snapshot.revision,
                true,
                &[SpeakerSelection {
                    id: "person".into(),
                    decision: SpeakerDecision::Block,
                }],
            )
            .unwrap();
        let snapshot = filter.snapshot();
        filter
            .apply(
                &snapshot.generation,
                snapshot.revision,
                true,
                &[SpeakerSelection {
                    id: "person".into(),
                    decision: SpeakerDecision::Allow,
                }],
            )
            .unwrap();
        filter.discover("old", Some(&speaker("person", 0)));
        filter.discover("new", Some(&speaker("person", 0)));
        assert!(!filter.allows("microphone", "old"));
        assert!(filter.allows("microphone", "new"));
    }
}
