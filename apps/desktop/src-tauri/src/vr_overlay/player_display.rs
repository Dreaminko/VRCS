use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::control_protocol::ControlSource;

pub const REQUEST_EVENT: &str = "vr-overlay-player-display-request";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
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
    SetProximityRadius(f32),
    SetViewPriority {
        enabled: bool,
        horizontal_fov_deg: f32,
    },
    SetSourceMode(String),
    SetUserSourceDisabled {
        user_id: String,
        source: String,
        disabled: bool,
    },
}

impl Action {
    pub(super) fn allowed(&self, view: &View) -> bool {
        match self {
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
            Self::SetProximityRadius(radius_m) => {
                view.available
                    && view.player_mode
                    && view.proximity.is_some()
                    && radius_m.is_finite()
                    && (0.0..=10.0).contains(radius_m)
            }
            Self::SetViewPriority {
                horizontal_fov_deg, ..
            } => {
                view.available
                    && view.player_mode
                    && view.view_priority.is_some()
                    && horizontal_fov_deg.is_finite()
                    && (30.0..=180.0).contains(horizontal_fov_deg)
            }
            Self::SetSourceMode(mode) => {
                view.available && matches!(mode.as_str(), "everyone" | "allowlist")
            }
            Self::SetUserSourceDisabled {
                user_id, source, ..
            } => {
                view.available
                    && matches!(source.as_str(), "vrchat_chatbox" | "speaker")
                    && view
                        .players
                        .iter()
                        .any(|player| player.user_id.as_ref() == Some(user_id))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub hidden: bool,
    #[serde(default)]
    pub chatbox_disabled: bool,
    #[serde(default)]
    pub voice_disabled: bool,
    /// The account has a hidden track, so voice cannot be resumed before unhiding.
    #[serde(default)]
    pub voice_locked: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Language {
    pub code: String,
    pub label: String,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proximity {
    pub enabled: bool,
    pub radius_m: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewPriority {
    pub enabled: bool,
    pub horizontal_fov_deg: f32,
    #[serde(default)]
    pub local_view: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub available: bool,
    pub processing_available: bool,
    pub player_mode: bool,
    #[serde(default)]
    pub source_mode: String,
    pub players: Vec<Player>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proximity: Option<Proximity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_priority: Option<ViewPriority>,
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
    fn proximity_actions_require_player_mode_published_controls_and_a_bounded_radius() {
        let mut view = View {
            available: true,
            player_mode: true,
            proximity: Some(Proximity {
                enabled: false,
                radius_m: 10.0,
            }),
            ..Default::default()
        };
        assert!(Action::SetProximityRadius(0.0).allowed(&view));
        assert!(Action::SetProximityRadius(0.9).allowed(&view));
        assert!(Action::SetProximityRadius(5.5).allowed(&view));
        assert!(!Action::SetProximityRadius(-0.1).allowed(&view));
        assert!(!Action::SetProximityRadius(10.1).allowed(&view));
        assert!(!Action::SetProximityRadius(f32::NAN).allowed(&view));
        view.player_mode = false;
        assert!(!Action::SetProximityRadius(5.0).allowed(&view));
        view.player_mode = true;
        view.proximity = None;
        assert!(!Action::SetProximityRadius(5.0).allowed(&view));
    }

    #[test]
    fn source_mode_actions_require_an_available_view_and_a_known_mode() {
        let mut view = View {
            available: true,
            ..Default::default()
        };
        assert!(Action::SetSourceMode("everyone".into()).allowed(&view));
        assert!(Action::SetSourceMode("allowlist".into()).allowed(&view));
        assert!(!Action::SetSourceMode("system".into()).allowed(&view));
        assert!(!Action::SetSourceMode(String::new()).allowed(&view));
        view.available = false;
        assert!(!Action::SetSourceMode("everyone".into()).allowed(&view));
    }

    #[test]
    fn action_payloads_match_the_shared_typescript_contract() {
        let cases = [
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
            (
                Action::SetProximityRadius(5.5),
                serde_json::json!({"set_proximity_radius": 5.5}),
            ),
            (
                Action::SetViewPriority {
                    enabled: true,
                    horizontal_fov_deg: 120.0,
                },
                serde_json::json!({"set_view_priority": {"enabled": true, "horizontal_fov_deg": 120.0}}),
            ),
            (
                Action::SetSourceMode("everyone".into()),
                serde_json::json!({"set_source_mode": "everyone"}),
            ),
            (
                Action::SetUserSourceDisabled {
                    user_id: "usr_known".into(),
                    source: "vrchat_chatbox".into(),
                    disabled: true,
                },
                serde_json::json!({"set_user_source_disabled": {"user_id": "usr_known", "source": "vrchat_chatbox", "disabled": true}}),
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
