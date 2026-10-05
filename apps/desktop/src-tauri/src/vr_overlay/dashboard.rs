use serde::{Deserialize, Serialize};

pub const DASHBOARD_WIDTH: u32 = 1440;
pub const DASHBOARD_HEIGHT: u32 = 900;

pub fn pointer_from_openvr(x: f32, y: f32) -> (f32, f32) {
    (x, DASHBOARD_HEIGHT as f32 - y)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardViewModel {
    pub labels: DashboardLabels,
    pub enabled: bool,
    pub headset: DashboardHeadset,
    pub wrist: DashboardWrist,
    pub ocr: DashboardOcr,
    pub status: String,
    pub save_state: DashboardSaveState,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardLabels {
    pub title: String,
    pub subtitle: String,
    pub master: String,
    pub headset: String,
    pub wrist: String,
    pub ocr: String,
    pub content: String,
    pub hand: String,
    pub width: String,
    pub opacity: String,
    pub gesture: String,
    pub preview: String,
    pub bindings: String,
    pub saving: String,
    pub saved: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardHeadset {
    pub enabled: bool,
    pub content: String,
    pub width: String,
    pub opacity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardWrist {
    pub enabled: bool,
    pub hand: String,
    pub content: String,
    pub width: String,
    pub opacity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardOcr {
    pub enabled: bool,
    pub backend: String,
    pub gesture: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DashboardSaveState {
    Idle,
    Saving,
    Saved,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DashboardAction {
    ToggleMaster,
    ToggleHeadset,
    CycleHeadsetContent,
    HeadsetWidthDown,
    HeadsetWidthUp,
    HeadsetOpacityDown,
    HeadsetOpacityUp,
    PreviewHeadset,
    ToggleWrist,
    CycleWristHand,
    CycleWristContent,
    WristWidthDown,
    WristWidthUp,
    WristOpacityDown,
    WristOpacityUp,
    PreviewWrist,
    ToggleOcr,
    ToggleOcrGesture,
    OpenOcrBindings,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DashboardPointerEvent {
    Move { x: f32, y: f32 },
    Down { x: f32, y: f32 },
    Up { x: f32, y: f32 },
    Shown,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DashboardControl {
    Master,
    HeadsetEnabled,
    HeadsetContent,
    HeadsetWidthDown,
    HeadsetWidthUp,
    HeadsetOpacityDown,
    HeadsetOpacityUp,
    HeadsetPreview,
    WristEnabled,
    WristHand,
    WristContent,
    WristWidthDown,
    WristWidthUp,
    WristOpacityDown,
    WristOpacityUp,
    WristPreview,
    OcrEnabled,
    OcrGesture,
    OcrBindings,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }
}

const CONTROLS: &[(DashboardControl, Rect)] = &[
    (
        DashboardControl::Master,
        Rect {
            left: 48.,
            top: 124.,
            right: 1392.,
            bottom: 212.,
        },
    ),
    (
        DashboardControl::HeadsetEnabled,
        Rect {
            left: 600.,
            top: 252.,
            right: 688.,
            bottom: 324.,
        },
    ),
    (
        DashboardControl::HeadsetContent,
        Rect {
            left: 78.,
            top: 338.,
            right: 678.,
            bottom: 410.,
        },
    ),
    (
        DashboardControl::HeadsetWidthDown,
        Rect {
            left: 78.,
            top: 482.,
            right: 140.,
            bottom: 538.,
        },
    ),
    (
        DashboardControl::HeadsetWidthUp,
        Rect {
            left: 304.,
            top: 482.,
            right: 366.,
            bottom: 538.,
        },
    ),
    (
        DashboardControl::HeadsetOpacityDown,
        Rect {
            left: 390.,
            top: 482.,
            right: 452.,
            bottom: 538.,
        },
    ),
    (
        DashboardControl::HeadsetOpacityUp,
        Rect {
            left: 616.,
            top: 482.,
            right: 678.,
            bottom: 538.,
        },
    ),
    (
        DashboardControl::HeadsetPreview,
        Rect {
            left: 78.,
            top: 616.,
            right: 678.,
            bottom: 672.,
        },
    ),
    (
        DashboardControl::WristEnabled,
        Rect {
            left: 1284.,
            top: 252.,
            right: 1372.,
            bottom: 324.,
        },
    ),
    (
        DashboardControl::WristHand,
        Rect {
            left: 762.,
            top: 338.,
            right: 1050.,
            bottom: 410.,
        },
    ),
    (
        DashboardControl::WristContent,
        Rect {
            left: 1074.,
            top: 338.,
            right: 1362.,
            bottom: 410.,
        },
    ),
    (
        DashboardControl::WristWidthDown,
        Rect {
            left: 762.,
            top: 482.,
            right: 824.,
            bottom: 538.,
        },
    ),
    (
        DashboardControl::WristWidthUp,
        Rect {
            left: 988.,
            top: 482.,
            right: 1050.,
            bottom: 538.,
        },
    ),
    (
        DashboardControl::WristOpacityDown,
        Rect {
            left: 1074.,
            top: 482.,
            right: 1136.,
            bottom: 538.,
        },
    ),
    (
        DashboardControl::WristOpacityUp,
        Rect {
            left: 1300.,
            top: 482.,
            right: 1362.,
            bottom: 538.,
        },
    ),
    (
        DashboardControl::WristPreview,
        Rect {
            left: 762.,
            top: 616.,
            right: 1362.,
            bottom: 672.,
        },
    ),
    (
        DashboardControl::OcrEnabled,
        Rect {
            left: 340.,
            top: 748.,
            right: 428.,
            bottom: 820.,
        },
    ),
    (
        DashboardControl::OcrGesture,
        Rect {
            left: 740.,
            top: 748.,
            right: 828.,
            bottom: 820.,
        },
    ),
    (
        DashboardControl::OcrBindings,
        Rect {
            left: 1040.,
            top: 752.,
            right: 1362.,
            bottom: 824.,
        },
    ),
];

#[derive(Debug)]
pub struct DashboardState {
    interactive: bool,
    hovered: Option<DashboardControl>,
    pressed: Option<DashboardControl>,
}

impl DashboardState {
    pub fn new() -> Self {
        Self {
            interactive: true,
            hovered: None,
            pressed: None,
        }
    }

    pub fn set_interactive(&mut self, interactive: bool) {
        self.interactive = interactive;
        if !interactive {
            self.pressed = None;
        }
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        let hovered = self.interactive.then(|| control_at(x, y)).flatten();
        let changed = self.hovered != hovered;
        self.hovered = hovered;
        changed
    }

    pub fn pointer_down(&mut self, x: f32, y: f32) -> bool {
        let pressed = self.interactive.then(|| control_at(x, y)).flatten();
        let changed = self.pressed != pressed;
        self.pressed = pressed;
        changed
    }

    pub fn pointer_up(&mut self, x: f32, y: f32) -> Option<DashboardAction> {
        let released = self.interactive.then(|| control_at(x, y)).flatten();
        let action = (released == self.pressed)
            .then_some(released)
            .flatten()
            .map(action_for);
        self.pressed = None;
        action
    }

    pub(super) fn hovered(&self) -> Option<DashboardControl> {
        self.hovered
    }

    pub(super) fn pressed(&self) -> Option<DashboardControl> {
        self.pressed
    }
}

impl Default for DashboardState {
    fn default() -> Self {
        Self::new()
    }
}

fn control_at(x: f32, y: f32) -> Option<DashboardControl> {
    CONTROLS
        .iter()
        .find_map(|(control, rect)| rect.contains(x, y).then_some(*control))
}

fn action_for(control: DashboardControl) -> DashboardAction {
    match control {
        DashboardControl::Master => DashboardAction::ToggleMaster,
        DashboardControl::HeadsetEnabled => DashboardAction::ToggleHeadset,
        DashboardControl::HeadsetContent => DashboardAction::CycleHeadsetContent,
        DashboardControl::HeadsetWidthDown => DashboardAction::HeadsetWidthDown,
        DashboardControl::HeadsetWidthUp => DashboardAction::HeadsetWidthUp,
        DashboardControl::HeadsetOpacityDown => DashboardAction::HeadsetOpacityDown,
        DashboardControl::HeadsetOpacityUp => DashboardAction::HeadsetOpacityUp,
        DashboardControl::HeadsetPreview => DashboardAction::PreviewHeadset,
        DashboardControl::WristEnabled => DashboardAction::ToggleWrist,
        DashboardControl::WristHand => DashboardAction::CycleWristHand,
        DashboardControl::WristContent => DashboardAction::CycleWristContent,
        DashboardControl::WristWidthDown => DashboardAction::WristWidthDown,
        DashboardControl::WristWidthUp => DashboardAction::WristWidthUp,
        DashboardControl::WristOpacityDown => DashboardAction::WristOpacityDown,
        DashboardControl::WristOpacityUp => DashboardAction::WristOpacityUp,
        DashboardControl::WristPreview => DashboardAction::PreviewWrist,
        DashboardControl::OcrEnabled => DashboardAction::ToggleOcr,
        DashboardControl::OcrGesture => DashboardAction::ToggleOcrGesture,
        DashboardControl::OcrBindings => DashboardAction::OpenOcrBindings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_switch_maps_to_a_semantic_action() {
        let mut state = DashboardState::default();

        state.pointer_down(900.0, 168.0);

        assert_eq!(
            state.pointer_up(900.0, 168.0),
            Some(DashboardAction::ToggleMaster)
        );
    }

    #[test]
    fn releasing_outside_the_pressed_control_does_not_activate_it() {
        let mut state = DashboardState::default();

        state.pointer_down(650.0, 278.0);

        assert_eq!(state.pointer_up(800.0, 278.0), None);
    }

    #[test]
    fn headset_step_buttons_have_distinct_actions() {
        let mut state = DashboardState::default();

        state.pointer_down(108.0, 510.0);
        let decrease = state.pointer_up(108.0, 510.0);
        state.pointer_down(336.0, 510.0);
        let increase = state.pointer_up(336.0, 510.0);

        assert_eq!(decrease, Some(DashboardAction::HeadsetWidthDown));
        assert_eq!(increase, Some(DashboardAction::HeadsetWidthUp));
    }

    #[test]
    fn disabled_controls_do_not_emit_actions() {
        let mut state = DashboardState::default();
        state.set_interactive(false);

        state.pointer_down(900.0, 168.0);

        assert_eq!(state.pointer_up(900.0, 168.0), None);
    }

    #[test]
    fn openvr_pointer_origin_is_converted_to_top_left() {
        assert_eq!(pointer_from_openvr(100.0, 200.0), (100.0, 700.0));
    }

    #[test]
    fn wide_dashboard_controls_use_the_expanded_card_area() {
        let mut state = DashboardState::default();

        state.pointer_down(500.0, 374.0);

        assert_eq!(
            state.pointer_up(500.0, 374.0),
            Some(DashboardAction::CycleHeadsetContent)
        );
    }

    #[test]
    fn ocr_bindings_use_the_full_width_utility_card() {
        let mut state = DashboardState::default();

        state.pointer_down(1200.0, 790.0);

        assert_eq!(
            state.pointer_up(1200.0, 790.0),
            Some(DashboardAction::OpenOcrBindings)
        );
    }
}
