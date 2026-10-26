use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::control_protocol::ControlAction;
use super::player_display::{Action, View};

pub const PLAYERS_PER_PAGE: usize = 5;
pub const LANGUAGES_PER_PAGE: usize = 12;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Page {
    #[default]
    Home,
    Players,
    Languages,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    #[default]
    Current,
    History,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    Control(ControlAction),
    Page(Page),
    Group(Group),
    PlayerPage(usize),
    LanguagePage(usize),
    Display(Action),
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ui {
    pub page: Page,
    pub group: Group,
    pub player_page: usize,
    pub language_page: usize,
}

impl Ui {
    pub fn apply(&mut self, command: &Command, view: &View) -> bool {
        let previous = self.clone();
        match command {
            Command::Page(page) => {
                if self.page == Page::Home && *page == Page::Players {
                    self.group = if view.player_mode {
                        Group::Current
                    } else {
                        Group::History
                    };
                    self.player_page = 0;
                }
                self.page = *page;
            }
            Command::Group(group) => {
                if self.group != *group {
                    self.player_page = 0;
                }
                self.group = *group;
            }
            Command::PlayerPage(page) => self.player_page = *page,
            Command::LanguagePage(page) => self.language_page = *page,
            Command::Control(_) | Command::Display(_) => return false,
        }
        self.clamp(view);
        *self != previous
    }

    pub fn clamp(&mut self, view: &View) {
        let players = view
            .players
            .iter()
            .filter(|player| match self.group {
                Group::Current => player.current,
                Group::History => !player.current,
            })
            .count();
        self.player_page = self
            .player_page
            .min(players.saturating_sub(1) / PLAYERS_PER_PAGE);
        self.language_page = self
            .language_page
            .min(view.languages.len().saturating_sub(1) / LANGUAGES_PER_PAGE);
    }
}

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
    fn opening_personnel_chooses_the_available_group_and_navigation_stays_local() {
        let view = super::super::player_display::View::default();
        let mut ui = Ui::default();
        assert!(ui.apply(&Command::Page(Page::Players), &view));
        assert_eq!(ui.group, Group::History);
        assert!(ui.apply(&Command::Page(Page::Languages), &view));
        let before = ui.clone();
        assert!(!ui.apply(
            &Command::Display(super::super::player_display::Action::ShowAllTracks(())),
            &view,
        ));
        assert!(!ui.apply(
            &Command::Control(super::super::control_protocol::ControlAction::SetOscEnabled(true)),
            &view,
        ));
        assert_eq!(ui, before);
        assert!(ui.apply(&Command::Page(Page::Home), &view));
        let view = super::super::player_display::View {
            player_mode: true,
            ..view
        };
        assert!(ui.apply(&Command::Page(Page::Players), &view));
        assert_eq!(ui.group, Group::Current);
    }

    #[test]
    fn personnel_pages_clamp_by_group_and_languages_clamp_after_lists_shrink() {
        use super::super::player_display::{Language, Player, View};
        let mut view = View {
            player_mode: true,
            players: (0..16)
                .map(|index| Player {
                    current: index < 11,
                    ..Default::default()
                })
                .collect(),
            languages: vec![Language::default(); 25],
            ..Default::default()
        };
        let mut ui = Ui::default();
        ui.apply(&Command::Page(Page::Players), &view);
        ui.apply(&Command::PlayerPage(usize::MAX), &view);
        assert_eq!(ui.player_page, 2);
        ui.apply(&Command::Group(Group::History), &view);
        ui.apply(&Command::PlayerPage(usize::MAX), &view);
        assert_eq!(ui.player_page, 0);
        ui.apply(&Command::LanguagePage(usize::MAX), &view);
        assert_eq!(ui.language_page, 2);
        ui.apply(&Command::Group(Group::Current), &view);
        ui.apply(&Command::PlayerPage(2), &view);
        view.players.truncate(5);
        view.languages.truncate(12);
        ui.clamp(&view);
        assert_eq!(ui.player_page, 0);
        assert_eq!(ui.language_page, 0);
        view.players.clear();
        view.languages.clear();
        ui.clamp(&view);
        assert_eq!(ui.player_page, 0);
        assert_eq!(ui.language_page, 0);
    }

    #[test]
    fn menu_navigation_uses_semantic_wire_commands() {
        assert_eq!(
            serde_json::to_value(Command::Page(Page::Players)).unwrap(),
            serde_json::json!({"page": "players"})
        );
        assert_eq!(
            serde_json::to_value(Command::Group(Group::History)).unwrap(),
            serde_json::json!({"group": "history"})
        );
        assert_eq!(
            serde_json::to_value(Command::PlayerPage(2)).unwrap(),
            serde_json::json!({"player_page": 2})
        );
        assert_eq!(
            serde_json::to_value(Command::Control(
                super::super::control_protocol::ControlAction::SetOscEnabled(true)
            ))
            .unwrap(),
            serde_json::json!({"control": {"set_osc_enabled": true}})
        );
    }

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
