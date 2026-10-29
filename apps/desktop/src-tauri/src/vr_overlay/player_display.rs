use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::control_protocol::ControlSource;

pub const REQUEST_EVENT: &str = "vr-overlay-player-display-request";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    SetProcessingMode(String),
    SetUserSelected {
        user_id: String,
        selected: bool,
    },
    SelectCurrent(()),
    ClearSelection(()),
    SetTrackHidden {
        track_id: String,
        hidden: bool,
    },
    ShowAllTracks(()),
    SetFollowDefault(bool),
    SetSourceLanguageHidden {
        language: String,
        hidden: bool,
    },
    ClearHiddenLanguages(()),
    SetMicrophoneFilter(bool),
    SetMicrophoneSpeaker {
        generation: String,
        revision: u64,
        id: String,
        decision: String,
    },
}

impl Action {
    pub(super) fn allowed(&self, view: &View) -> bool {
        let processing = view.processing_available && view.player_mode;
        let selecting = processing && view.selection_mode == "selected";
        match self {
            Self::SetMicrophoneFilter(_) => view.microphone.is_some(),
            Self::SetMicrophoneSpeaker {
                generation,
                revision,
                id,
                decision,
            } => view.microphone.as_ref().is_some_and(|mic| {
                mic.enabled
                    && mic.running
                    && mic.supported
                    && mic.generation == *generation
                    && mic.revision == *revision
                    && matches!(decision.as_str(), "allow" | "block")
                    && mic.speakers.iter().any(|speaker| speaker.id == *id)
            }),
            Self::SetProcessingMode(mode) => {
                processing && matches!(mode.as_str(), "all" | "selected")
            }
            Self::SetUserSelected { user_id, .. } => {
                selecting
                    && !user_id.is_empty()
                    && view
                        .players
                        .iter()
                        .any(|player| player.user_id.as_ref() == Some(user_id))
            }
            Self::SelectCurrent(()) | Self::ClearSelection(()) => selecting,
            Self::SetTrackHidden { track_id, .. } => {
                !track_id.is_empty()
                    && view
                        .players
                        .iter()
                        .any(|player| player.track_id.as_ref() == Some(track_id))
            }
            Self::SetSourceLanguageHidden { language, .. } => {
                !view.follow_default
                    && !language.is_empty()
                    && view
                        .languages
                        .iter()
                        .any(|candidate| candidate.code == *language)
            }
            Self::ShowAllTracks(())
            | Self::SetFollowDefault(_)
            | Self::ClearHiddenLanguages(()) => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub owner_epoch: u32,
    pub request_id: u32,
    pub base_revision: u32,
    pub source: ControlSource,
    pub action: Action,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub track_id: Option<String>,
    pub user_id: Option<String>,
    pub name: String,
    pub detail: String,
    pub current: bool,
    pub selected: bool,
    pub hidden: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Language {
    pub code: String,
    pub label: String,
    pub hidden: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MicrophoneSpeaker {
    pub id: String,
    pub index: u64,
    pub name: String,
    pub decision: String,
    pub last_active: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Microphone {
    pub enabled: bool,
    pub supported: bool,
    pub running: bool,
    pub generation: String,
    pub revision: u64,
    pub speakers: Vec<MicrophoneSpeaker>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    pub available: bool,
    pub processing_available: bool,
    pub player_mode: bool,
    pub selection_mode: String,
    pub selected_count: usize,
    pub players: Vec<Player>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microphone: Option<Microphone>,
    pub languages: Vec<Language>,
    pub follow_default: bool,
    pub default_languages: Vec<String>,
    pub labels: BTreeMap<String, String>,
    pub busy: bool,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microphone_filter_can_be_enabled_before_capture_starts() {
        let view = View {
            microphone: Some(Microphone::default()),
            ..Default::default()
        };
        assert!(Action::SetMicrophoneFilter(true).allowed(&view));
        assert!(Action::SetMicrophoneFilter(false).allowed(&view));
        assert!(!Action::SetMicrophoneSpeaker {
            generation: String::new(),
            revision: 0,
            id: "unknown".into(),
            decision: "allow".into(),
        }
        .allowed(&view));
        assert!(!Action::SetMicrophoneFilter(true).allowed(&View::default()));
    }

    #[test]
    fn action_payloads_match_the_shared_typescript_contract() {
        let cases = [
            (
                Action::SetMicrophoneFilter(true),
                serde_json::json!({"set_microphone_filter": true}),
            ),
            (
                Action::SetMicrophoneSpeaker {
                    generation: "session".into(),
                    revision: 7,
                    id: "speaker-1".into(),
                    decision: "block".into(),
                },
                serde_json::json!({"set_microphone_speaker": {"generation": "session", "revision": 7, "id": "speaker-1", "decision": "block"}}),
            ),
            (
                Action::SetProcessingMode("selected".into()),
                serde_json::json!({"set_processing_mode": "selected"}),
            ),
            (
                Action::SetUserSelected {
                    user_id: "usr_known".into(),
                    selected: true,
                },
                serde_json::json!({"set_user_selected": {"user_id": "usr_known", "selected": true}}),
            ),
            (
                Action::SelectCurrent(()),
                serde_json::json!({"select_current": null}),
            ),
            (
                Action::ClearSelection(()),
                serde_json::json!({"clear_selection": null}),
            ),
            (
                Action::SetTrackHidden {
                    track_id: "track-1".into(),
                    hidden: true,
                },
                serde_json::json!({"set_track_hidden": {"track_id": "track-1", "hidden": true}}),
            ),
            (
                Action::ShowAllTracks(()),
                serde_json::json!({"show_all_tracks": null}),
            ),
            (
                Action::SetFollowDefault(false),
                serde_json::json!({"set_follow_default": false}),
            ),
            (
                Action::SetSourceLanguageHidden {
                    language: "en".into(),
                    hidden: true,
                },
                serde_json::json!({"set_source_language_hidden": {"language": "en", "hidden": true}}),
            ),
            (
                Action::ClearHiddenLanguages(()),
                serde_json::json!({"clear_hidden_languages": null}),
            ),
        ];
        for (action, wire) in cases {
            assert_eq!(serde_json::to_value(&action).unwrap(), wire);
            assert_eq!(serde_json::from_value::<Action>(wire).unwrap(), action);
        }
    }

    #[test]
    fn requests_preserve_owner_revision_and_source() {
        let wire = serde_json::json!({
            "owner_epoch": 3,
            "request_id": 8,
            "base_revision": 5,
            "source": "wrist_menu",
            "action": {"show_all_tracks": null}
        });
        let request: Request = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(request.owner_epoch, 3);
        assert_eq!(request.request_id, 8);
        assert_eq!(request.base_revision, 5);
        assert_eq!(
            request.source,
            super::super::control_protocol::ControlSource::WristMenu
        );
        assert_eq!(serde_json::to_value(request).unwrap(), wire);
    }
}
