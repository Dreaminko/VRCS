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
    pub language: DashboardLanguage,
    pub osc: DashboardOsc,
    pub status: String,
    pub save_state: DashboardSaveState,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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
    pub position: String,
    pub rotation: String,
    pub reset_position: String,
    pub gesture: String,
    pub preview: String,
    pub bindings: String,
    pub saving: String,
    pub saved: String,
    pub display_tab: String,
    pub language_tab: String,
    pub osc_tab: String,
    pub recognition_language: String,
    pub translation_mode: String,
    pub translation_languages: String,
    pub speaker_language: String,
    pub microphone_language: String,
    pub add_target: String,
    pub presets: String,
    pub save_preset: String,
    pub apply_preset: String,
    pub delete: String,
    pub osc_original: String,
    pub osc_enabled: String,
    pub osc_mute_sync: String,
    pub osc_mute_toast: String,
    pub osc_strategy: String,
    pub osc_hint: String,
    pub close: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DashboardChoice {
    pub value: String,
    pub options: Vec<DashboardOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DashboardLanguage {
    pub recognition: DashboardChoice,
    pub mode: DashboardChoice,
    pub speaker_targets: Vec<DashboardChoice>,
    pub microphone_targets: Vec<DashboardChoice>,
    pub can_add: bool,
    pub presets: DashboardChoice,
    pub can_save_preset: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetGroup {
    Speaker,
    Microphone,
}

impl DashboardLanguage {
    fn targets(&self, group: TargetGroup) -> &[DashboardChoice] {
        match group {
            TargetGroup::Speaker => &self.speaker_targets,
            TargetGroup::Microphone => &self.microphone_targets,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardOsc {
    pub enabled: bool,
    pub original: bool,
    pub mute_sync: bool,
    pub mute_toast: bool,
    pub strategy: String,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardHeadset {
    pub enabled: bool,
    pub content: String,
    pub width: String,
    pub opacity: String,
    pub position: Vec<DashboardNumberField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardWrist {
    pub enabled: bool,
    pub hand: String,
    pub content: String,
    pub width: String,
    pub opacity: String,
    pub position: Vec<DashboardNumberField>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayKind {
    Headset,
    Wrist,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PositionField {
    #[serde(rename = "offset_x_m")]
    Horizontal,
    #[serde(rename = "offset_y_m")]
    Vertical,
    #[serde(rename = "distance_m")]
    Distance,
    #[serde(rename = "offset_z_m")]
    Depth,
    #[serde(rename = "pitch_deg")]
    Pitch,
    #[serde(rename = "yaw_deg")]
    Yaw,
    #[serde(rename = "roll_deg")]
    Roll,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardNumberField {
    pub field: PositionField,
    pub label: String,
    pub value: String,
    pub can_decrease: bool,
    pub can_increase: bool,
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
    ToggleOsc,
    ToggleOscOriginal,
    ToggleOscMuteSync,
    ToggleOscMuteToast,
    CycleOscStrategy,
    SetRecognitionLanguage(String),
    SetTranslationMode(String),
    SetTargetLanguage {
        group: TargetGroup,
        index: usize,
        value: String,
    },
    AddTarget(TargetGroup),
    DeleteTarget {
        group: TargetGroup,
        index: usize,
    },
    MoveTarget {
        group: TargetGroup,
        index: usize,
        offset: i32,
    },
    SaveLanguagePreset(String),
    ApplyLanguagePreset(String),
    DeleteLanguagePreset(String),
    AdjustDisplayPosition {
        kind: DisplayKind,
        field: PositionField,
        direction: i32,
    },
    ResetDisplayPosition(DisplayKind),
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
    DisplayTab,
    LanguageTab,
    OscTab,
    RecognitionLanguage,
    TranslationMode,
    TargetLanguage(TargetGroup, usize),
    TargetUp(TargetGroup, usize),
    TargetDown(TargetGroup, usize),
    TargetDelete(TargetGroup, usize),
    TargetAdd(TargetGroup),
    PresetApply,
    PresetSave,
    PresetDelete,
    OscEnabled,
    OscOriginal,
    OscMuteSync,
    OscMuteToast,
    OscStrategy,
    PickerClose,
    PickerPrevious,
    PickerNext,
    PickerOption(usize),
    Master,
    HeadsetEnabled,
    HeadsetContent,
    HeadsetWidthDown,
    HeadsetWidthUp,
    HeadsetOpacityDown,
    HeadsetOpacityUp,
    HeadsetPreview,
    HeadsetPosition,
    WristEnabled,
    WristHand,
    WristContent,
    WristWidthDown,
    WristWidthUp,
    WristOpacityDown,
    WristOpacityUp,
    WristPreview,
    WristPosition,
    PositionStep(PositionField, i32),
    PositionClose,
    PositionPreview,
    PositionReset,
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
    const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }
}

const CONTROLS: &[(DashboardControl, Rect)] = &[
    (
        DashboardControl::DisplayTab,
        Rect::new(48., 124., 368., 212.),
    ),
    (
        DashboardControl::LanguageTab,
        Rect::new(384., 124., 704., 212.),
    ),
    (DashboardControl::OscTab, Rect::new(720., 124., 1040., 212.)),
    (
        DashboardControl::RecognitionLanguage,
        Rect::new(78., 246., 678., 336.),
    ),
    (
        DashboardControl::TranslationMode,
        Rect::new(762., 246., 1362., 336.),
    ),
    (
        DashboardControl::PresetApply,
        Rect::new(306., 774., 684., 834.),
    ),
    (
        DashboardControl::PresetSave,
        Rect::new(704., 774., 1142., 834.),
    ),
    (
        DashboardControl::PresetDelete,
        Rect::new(1162., 774., 1362., 834.),
    ),
    (
        DashboardControl::OscEnabled,
        Rect::new(78., 280., 1362., 360.),
    ),
    (
        DashboardControl::OscOriginal,
        Rect::new(78., 380., 1362., 460.),
    ),
    (
        DashboardControl::OscMuteSync,
        Rect::new(78., 480., 1362., 560.),
    ),
    (
        DashboardControl::OscMuteToast,
        Rect::new(78., 580., 1362., 660.),
    ),
    (
        DashboardControl::OscStrategy,
        Rect::new(78., 684., 1362., 780.),
    ),
    (
        DashboardControl::Master,
        Rect {
            left: 1084.,
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
            left: 458.,
            top: 588.,
            right: 678.,
            bottom: 672.,
        },
    ),
    (
        DashboardControl::HeadsetPosition,
        Rect::new(78., 588., 438., 672.),
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
            left: 1142.,
            top: 588.,
            right: 1362.,
            bottom: 672.,
        },
    ),
    (
        DashboardControl::WristPosition,
        Rect::new(762., 588., 1122., 672.),
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

pub(super) const PICKER_PAGE_SIZE: usize = 32;
pub(super) const PICKER_CLOSE_RECT: Rect = Rect::new(1180., 250., 1362., 306.);
pub(super) const PICKER_PREVIOUS_RECT: Rect = Rect::new(864., 250., 994., 306.);
pub(super) const PICKER_NEXT_RECT: Rect = Rect::new(1010., 250., 1140., 306.);

pub(super) fn picker_option_rect(index: usize) -> Rect {
    let slot = index % PICKER_PAGE_SIZE;
    let left = 78. + (slot % 4) as f32 * 325.;
    let top = 334. + (slot / 4) as f32 * 64.;
    Rect::new(left, top, left + 309., top + 56.)
}

pub(super) fn control_rect(control: DashboardControl) -> Rect {
    use DashboardControl::*;
    match control {
        PositionStep(field, direction) => {
            let rect = position_field_rect(field);
            return if direction < 0 {
                Rect::new(rect.left, rect.top + 44., rect.left + 62., rect.bottom)
            } else {
                Rect::new(rect.right - 62., rect.top + 44., rect.right, rect.bottom)
            };
        }
        PositionClose => return PICKER_CLOSE_RECT,
        PositionPreview => return Rect::new(880., 250., 1160., 306.),
        PositionReset => return Rect::new(78., 762., 598., 824.),
        _ => {}
    }
    let (group, index) = match control {
        TargetLanguage(group, index)
        | TargetUp(group, index)
        | TargetDown(group, index)
        | TargetDelete(group, index) => (group, index),
        TargetAdd(group) => (group, 0),
        _ => {
            return CONTROLS
                .iter()
                .find(|(candidate, _)| *candidate == control)
                .unwrap()
                .1
        }
    };
    let left = if group == TargetGroup::Microphone {
        78.
    } else {
        762.
    };
    let top = 430. + index as f32 * 78.;
    match control {
        TargetLanguage(..) => Rect::new(left, top, left + 400., top + 64.),
        TargetUp(..) => Rect::new(left + 412., top, left + 468., top + 64.),
        TargetDown(..) => Rect::new(left + 478., top, left + 534., top + 64.),
        TargetDelete(..) => Rect::new(left + 544., top, left + 600., top + 64.),
        TargetAdd(..) => Rect::new(left, 666., left + 600., 726.),
        _ => unreachable!(),
    }
}

pub(super) fn position_field_rect(field: PositionField) -> Rect {
    let index = match field {
        PositionField::Horizontal => 0,
        PositionField::Vertical => 1,
        PositionField::Distance | PositionField::Depth => 2,
        PositionField::Pitch => 3,
        PositionField::Yaw => 4,
        PositionField::Roll => 5,
    };
    let left = 78. + (index % 3) as f32 * 442.;
    let top = if index < 3 { 370. } else { 568. };
    Rect::new(left, top, left + 400., top + 118.)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum DashboardPage {
    #[default]
    Display,
    Language,
    Osc,
}

fn control_page(control: DashboardControl) -> Option<DashboardPage> {
    match control {
        DashboardControl::Master
        | DashboardControl::DisplayTab
        | DashboardControl::LanguageTab
        | DashboardControl::OscTab => None,
        DashboardControl::RecognitionLanguage
        | DashboardControl::TranslationMode
        | DashboardControl::TargetLanguage(..)
        | DashboardControl::TargetUp(..)
        | DashboardControl::TargetDown(..)
        | DashboardControl::TargetDelete(..)
        | DashboardControl::TargetAdd(..)
        | DashboardControl::PresetApply
        | DashboardControl::PresetSave
        | DashboardControl::PresetDelete => Some(DashboardPage::Language),
        DashboardControl::OscEnabled
        | DashboardControl::OscOriginal
        | DashboardControl::OscMuteSync
        | DashboardControl::OscMuteToast
        | DashboardControl::OscStrategy => Some(DashboardPage::Osc),
        _ => Some(DashboardPage::Display),
    }
}

#[derive(Debug)]
pub struct DashboardState {
    interactive: bool,
    hovered: Option<DashboardControl>,
    pressed: Option<DashboardControl>,
    page: DashboardPage,
    picker: Option<DashboardControl>,
    picker_page: usize,
    language: DashboardLanguage,
    display_editor: Option<DisplayKind>,
    position: [Vec<DashboardNumberField>; 2],
}

impl DashboardState {
    pub fn new() -> Self {
        Self {
            interactive: true,
            hovered: None,
            pressed: None,
            page: DashboardPage::Display,
            picker: None,
            picker_page: 0,
            language: DashboardLanguage::default(),
            display_editor: None,
            position: [Vec::new(), Vec::new()],
        }
    }

    pub fn update_view(&mut self, view: &DashboardViewModel) {
        self.set_interactive(view.save_state != DashboardSaveState::Saving);
        if self.language != view.language {
            self.language = view.language.clone();
            self.pressed = None;
            self.hovered = None;
            self.picker = None;
        }
        let position = [&view.headset.position, &view.wrist.position];
        for (index, fields) in position.into_iter().enumerate() {
            if &self.position[index] != fields {
                self.position[index] = fields.clone();
                self.pressed = None;
                self.hovered = None;
            }
        }
    }

    pub(super) fn page(&self) -> DashboardPage {
        self.page
    }
    pub(super) fn picker(&self) -> Option<DashboardControl> {
        self.picker
    }
    pub(super) fn picker_page(&self) -> usize {
        self.picker_page
    }

    pub(super) fn display_editor(&self) -> Option<DisplayKind> {
        self.display_editor
    }

    fn position_fields(&self) -> &[DashboardNumberField] {
        match self.display_editor {
            Some(DisplayKind::Headset) => &self.position[0],
            Some(DisplayKind::Wrist) => &self.position[1],
            None => &[],
        }
    }

    fn choice(&self, control: DashboardControl) -> Option<&DashboardChoice> {
        match control {
            DashboardControl::RecognitionLanguage => Some(&self.language.recognition),
            DashboardControl::TranslationMode => Some(&self.language.mode),
            DashboardControl::TargetLanguage(group, index) => {
                self.language.targets(group).get(index)
            }
            DashboardControl::PresetApply | DashboardControl::PresetDelete => {
                Some(&self.language.presets)
            }
            _ => None,
        }
    }

    pub(super) fn picker_choice(&self) -> Option<&DashboardChoice> {
        self.picker.and_then(|control| self.choice(control))
    }

    fn control_at(&self, x: f32, y: f32) -> Option<DashboardControl> {
        if self.page == DashboardPage::Display && self.display_editor.is_some() {
            for field in self.position_fields() {
                for direction in [-1, 1] {
                    let control = DashboardControl::PositionStep(field.field, direction);
                    if self.control_enabled(control) && control_rect(control).contains(x, y) {
                        return Some(control);
                    }
                }
            }
            for control in [
                DashboardControl::PositionClose,
                DashboardControl::PositionPreview,
                DashboardControl::PositionReset,
            ] {
                if self.control_enabled(control) && control_rect(control).contains(x, y) {
                    return Some(control);
                }
            }
        }
        if self.page == DashboardPage::Language && self.picker.is_none() {
            for group in [TargetGroup::Microphone, TargetGroup::Speaker] {
                for index in 0..self.language.targets(group).len().min(3) {
                    for control in [
                        DashboardControl::TargetLanguage(group, index),
                        DashboardControl::TargetUp(group, index),
                        DashboardControl::TargetDown(group, index),
                        DashboardControl::TargetDelete(group, index),
                    ] {
                        if self.control_enabled(control) && control_rect(control).contains(x, y) {
                            return Some(control);
                        }
                    }
                }
                let control = DashboardControl::TargetAdd(group);
                if self.control_enabled(control) && control_rect(control).contains(x, y) {
                    return Some(control);
                }
            }
        }
        for &(control, rect) in CONTROLS {
            let page = control_page(control);
            if page.is_some_and(|page| page != self.page)
                || (self.picker.is_some() && page.is_some())
                || (self.display_editor.is_some() && page == Some(DashboardPage::Display))
            {
                continue;
            }
            if !self.control_enabled(control) {
                continue;
            }
            if rect.contains(x, y) {
                return Some(control);
            }
        }
        let choice = self.picker_choice()?;
        if PICKER_CLOSE_RECT.contains(x, y) {
            return Some(DashboardControl::PickerClose);
        }
        if self.picker_page > 0 && PICKER_PREVIOUS_RECT.contains(x, y) {
            return Some(DashboardControl::PickerPrevious);
        }
        if (self.picker_page + 1) * PICKER_PAGE_SIZE < choice.options.len()
            && PICKER_NEXT_RECT.contains(x, y)
        {
            return Some(DashboardControl::PickerNext);
        }
        let start = self.picker_page * PICKER_PAGE_SIZE;
        (start..choice.options.len().min(start + PICKER_PAGE_SIZE))
            .find(|&index| picker_option_rect(index).contains(x, y))
            .map(DashboardControl::PickerOption)
    }

    pub(super) fn control_enabled(&self, control: DashboardControl) -> bool {
        if !self.interactive {
            return false;
        }
        if self
            .choice(control)
            .is_some_and(|choice| choice.options.is_empty())
        {
            return false;
        }
        match control {
            DashboardControl::PositionStep(field, direction) => self
                .position_fields()
                .iter()
                .find(|item| item.field == field)
                .is_some_and(|item| {
                    if direction < 0 {
                        item.can_decrease
                    } else {
                        item.can_increase
                    }
                }),
            DashboardControl::TargetUp(_, index) => index > 0,
            DashboardControl::TargetDown(group, index) => {
                index + 1 < self.language.targets(group).len()
            }
            DashboardControl::TargetDelete(group, _) => self.language.targets(group).len() > 1,
            DashboardControl::TargetAdd(group) => {
                self.language.can_add && self.language.targets(group).len() < 3
            }
            DashboardControl::PresetSave => self.language.can_save_preset,
            _ => true,
        }
    }

    pub fn set_interactive(&mut self, interactive: bool) {
        self.interactive = interactive;
        if !interactive {
            self.pressed = None;
        }
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        let hovered = self.interactive.then(|| self.control_at(x, y)).flatten();
        let changed = self.hovered != hovered;
        self.hovered = hovered;
        changed
    }

    pub fn pointer_down(&mut self, x: f32, y: f32) -> bool {
        let pressed = self.interactive.then(|| self.control_at(x, y)).flatten();
        let changed = self.pressed != pressed;
        self.pressed = pressed;
        changed
    }

    pub fn pointer_up(&mut self, x: f32, y: f32) -> Option<DashboardAction> {
        let released = self.interactive.then(|| self.control_at(x, y)).flatten();
        let control = (released == self.pressed).then_some(released).flatten();
        self.pressed = None;
        match control? {
            tab @ (DashboardControl::DisplayTab
            | DashboardControl::LanguageTab
            | DashboardControl::OscTab) => {
                self.page = match tab {
                    DashboardControl::LanguageTab => DashboardPage::Language,
                    DashboardControl::OscTab => DashboardPage::Osc,
                    _ => DashboardPage::Display,
                };
                self.picker = None;
                self.display_editor = None;
                self.hovered = None;
                None
            }
            DashboardControl::HeadsetPosition | DashboardControl::WristPosition => {
                self.display_editor = Some(if control == Some(DashboardControl::HeadsetPosition) {
                    DisplayKind::Headset
                } else {
                    DisplayKind::Wrist
                });
                self.hovered = None;
                None
            }
            DashboardControl::PositionClose => {
                self.display_editor = None;
                self.hovered = None;
                None
            }
            DashboardControl::PositionStep(field, direction) => {
                Some(DashboardAction::AdjustDisplayPosition {
                    kind: self.display_editor?,
                    field,
                    direction,
                })
            }
            DashboardControl::PositionReset => {
                Some(DashboardAction::ResetDisplayPosition(self.display_editor?))
            }
            DashboardControl::PositionPreview => Some(match self.display_editor? {
                DisplayKind::Headset => DashboardAction::PreviewHeadset,
                DisplayKind::Wrist => DashboardAction::PreviewWrist,
            }),
            control @ (DashboardControl::RecognitionLanguage
            | DashboardControl::TranslationMode
            | DashboardControl::TargetLanguage(..)
            | DashboardControl::PresetApply
            | DashboardControl::PresetDelete) => {
                self.picker = Some(control);
                self.picker_page = 0;
                self.hovered = None;
                None
            }
            DashboardControl::PickerClose => {
                self.picker = None;
                self.hovered = None;
                None
            }
            DashboardControl::PickerPrevious => {
                self.picker_page -= 1;
                self.hovered = None;
                None
            }
            DashboardControl::PickerNext => {
                self.picker_page += 1;
                self.hovered = None;
                None
            }
            DashboardControl::PickerOption(index) => {
                let value = self.picker_choice()?.options.get(index)?.value.clone();
                let action = match self.picker? {
                    DashboardControl::RecognitionLanguage => {
                        DashboardAction::SetRecognitionLanguage(value)
                    }
                    DashboardControl::TranslationMode => DashboardAction::SetTranslationMode(value),
                    DashboardControl::TargetLanguage(group, index) => {
                        DashboardAction::SetTargetLanguage {
                            group,
                            index,
                            value,
                        }
                    }
                    DashboardControl::PresetApply => DashboardAction::ApplyLanguagePreset(value),
                    DashboardControl::PresetDelete => DashboardAction::DeleteLanguagePreset(value),
                    _ => return None,
                };
                self.picker = None;
                self.hovered = None;
                Some(action)
            }
            control => action_for(control),
        }
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

fn action_for(control: DashboardControl) -> Option<DashboardAction> {
    Some(match control {
        DashboardControl::TargetAdd(group) => DashboardAction::AddTarget(group),
        DashboardControl::TargetDelete(group, index) => {
            DashboardAction::DeleteTarget { group, index }
        }
        DashboardControl::TargetUp(group, index) => DashboardAction::MoveTarget {
            group,
            index,
            offset: -1,
        },
        DashboardControl::TargetDown(group, index) => DashboardAction::MoveTarget {
            group,
            index,
            offset: 1,
        },
        DashboardControl::PresetSave => DashboardAction::SaveLanguagePreset(String::new()),
        DashboardControl::OscEnabled => DashboardAction::ToggleOsc,
        DashboardControl::OscOriginal => DashboardAction::ToggleOscOriginal,
        DashboardControl::OscMuteSync => DashboardAction::ToggleOscMuteSync,
        DashboardControl::OscMuteToast => DashboardAction::ToggleOscMuteToast,
        DashboardControl::OscStrategy => DashboardAction::CycleOscStrategy,
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
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(state: &mut DashboardState, x: f32, y: f32) -> Option<DashboardAction> {
        state.pointer_down(x, y);
        state.pointer_up(x, y)
    }

    #[test]
    fn position_editor_maps_each_axis_to_the_selected_overlay() {
        let mut state = DashboardState::default();
        for (index, third) in [PositionField::Distance, PositionField::Depth]
            .into_iter()
            .enumerate()
        {
            state.position[index] = [
                PositionField::Horizontal,
                PositionField::Vertical,
                third,
                PositionField::Pitch,
                PositionField::Yaw,
                PositionField::Roll,
            ]
            .into_iter()
            .map(|field| DashboardNumberField {
                field,
                label: String::new(),
                value: String::new(),
                can_decrease: true,
                can_increase: true,
            })
            .collect();
        }
        for (kind, x) in [(DisplayKind::Headset, 200.), (DisplayKind::Wrist, 900.)] {
            assert_eq!(click(&mut state, x, 630.), None);
            assert_eq!(state.display_editor(), Some(kind));
            let fields = state.position_fields().to_vec();
            for field in fields {
                for direction in [-1, 1] {
                    let rect = control_rect(DashboardControl::PositionStep(field.field, direction));
                    assert_eq!(
                        click(
                            &mut state,
                            (rect.left + rect.right) / 2.,
                            (rect.top + rect.bottom) / 2.
                        ),
                        Some(DashboardAction::AdjustDisplayPosition {
                            kind,
                            field: field.field,
                            direction
                        })
                    );
                }
            }
            assert_eq!(click(&mut state, 108., 510.), None);
            assert_eq!(
                click(&mut state, 1000., 278.),
                Some(if kind == DisplayKind::Headset {
                    DashboardAction::PreviewHeadset
                } else {
                    DashboardAction::PreviewWrist
                })
            );
            assert_eq!(
                click(&mut state, 200., 792.),
                Some(DashboardAction::ResetDisplayPosition(kind))
            );
            assert_eq!(click(&mut state, 1250., 278.), None);
            assert_eq!(state.display_editor(), None);
        }
        assert_eq!(
            serde_json::to_value(DashboardAction::AdjustDisplayPosition {
                kind: DisplayKind::Wrist,
                field: PositionField::Depth,
                direction: -1
            })
            .unwrap(),
            serde_json::json!({ "adjust_display_position": { "kind": "wrist", "field": "offset_z_m", "direction": -1 } })
        );
        click(&mut state, 200., 630.);
        click(&mut state, 500., 168.);
        assert_eq!(state.display_editor(), None);
    }

    #[test]
    fn position_limits_and_saving_disable_adjustments_without_closing_the_editor() {
        let mut state = DashboardState::default();
        state.position[0] = vec![DashboardNumberField {
            field: PositionField::Horizontal,
            label: "Horizontal".into(),
            value: "-2.00 m".into(),
            can_decrease: false,
            can_increase: true,
        }];
        click(&mut state, 200., 630.);
        assert_eq!(click(&mut state, 108., 450.), None);
        assert_eq!(
            click(&mut state, 450., 450.),
            Some(DashboardAction::AdjustDisplayPosition {
                kind: DisplayKind::Headset,
                field: PositionField::Horizontal,
                direction: 1
            })
        );
        state.set_interactive(false);
        assert_eq!(click(&mut state, 450., 450.), None);
        assert_eq!(click(&mut state, 200., 792.), None);
        assert_eq!(click(&mut state, 1000., 278.), None);
        assert_eq!(state.display_editor(), Some(DisplayKind::Headset));
    }

    #[test]
    fn target_rows_emit_group_and_index_and_enforce_route_limits() {
        let mut state = DashboardState::default();
        let choice = DashboardChoice {
            value: "English".into(),
            options: vec![DashboardOption {
                value: "fr".into(),
                label: "French".into(),
            }],
        };
        state.language.microphone_targets = vec![choice.clone()];
        state.language.speaker_targets = vec![choice; 3];
        state.language.can_add = true;
        click(&mut state, 500., 168.);
        for group in [TargetGroup::Microphone, TargetGroup::Speaker] {
            let count = state.language.targets(group).len();
            for index in 0..count {
                let rect = control_rect(DashboardControl::TargetLanguage(group, index));
                click(&mut state, rect.left + 20., rect.top + 20.);
                let action = click(&mut state, 100., 360.).unwrap();
                assert_eq!(
                    action,
                    DashboardAction::SetTargetLanguage {
                        group,
                        index,
                        value: "fr".into()
                    }
                );
                assert_eq!(
                    serde_json::to_value(action).unwrap()["set_target_language"]["index"],
                    index
                );
            }
        }
        assert_eq!(click(&mut state, 518., 450.), None);
        assert_eq!(click(&mut state, 650., 450.), None);
        assert_eq!(click(&mut state, 900., 694.), None);
        assert_eq!(
            click(&mut state, 200., 694.),
            Some(DashboardAction::AddTarget(TargetGroup::Microphone))
        );
        assert_eq!(
            click(&mut state, 1202., 528.),
            Some(DashboardAction::MoveTarget {
                group: TargetGroup::Speaker,
                index: 1,
                offset: -1
            })
        );
        assert_eq!(
            click(&mut state, 1330., 528.),
            Some(DashboardAction::DeleteTarget {
                group: TargetGroup::Speaker,
                index: 1
            })
        );
        click(&mut state, 800., 168.);
        assert_eq!(
            click(&mut state, 1330., 528.),
            Some(DashboardAction::ToggleOscMuteSync)
        );
    }

    #[test]
    fn preset_pickers_emit_ids_and_full_capacity_disables_save() {
        let mut state = DashboardState::default();
        state.language.presets.options = vec![DashboardOption {
            value: "preset-id".into(),
            label: "Custom preset".into(),
        }];
        click(&mut state, 500., 168.);
        assert_eq!(click(&mut state, 900., 800.), None);
        click(&mut state, 400., 800.);
        assert_eq!(
            click(&mut state, 100., 360.),
            Some(DashboardAction::ApplyLanguagePreset("preset-id".into()))
        );
        click(&mut state, 1250., 800.);
        assert_eq!(
            click(&mut state, 100., 360.),
            Some(DashboardAction::DeleteLanguagePreset("preset-id".into()))
        );
        state.language.can_save_preset = true;
        assert_eq!(
            click(&mut state, 900., 800.),
            Some(DashboardAction::SaveLanguagePreset(String::new()))
        );
        state.set_interactive(false);
        assert_eq!(click(&mut state, 400., 800.), None);
        assert_eq!(state.picker(), None);
    }

    #[test]
    fn language_picker_emits_the_selected_value_and_can_be_cancelled() {
        let mut state = DashboardState::default();
        state.language.recognition.options = vec![
            DashboardOption {
                value: "auto".into(),
                label: "Automatic".into(),
            },
            DashboardOption {
                value: "en".into(),
                label: "English".into(),
            },
        ];
        assert_eq!(click(&mut state, 500., 168.), None);
        assert_eq!(click(&mut state, 200., 280.), None);
        assert_eq!(
            click(&mut state, 430., 360.),
            Some(DashboardAction::SetRecognitionLanguage("en".into()))
        );
        assert_eq!(state.picker(), None);
        assert_eq!(
            serde_json::to_value(DashboardAction::SetRecognitionLanguage("en".into())).unwrap(),
            serde_json::json!({ "set_recognition_language": "en" })
        );
        click(&mut state, 200., 280.);
        assert_eq!(click(&mut state, 1240., 278.), None);
        assert_eq!(state.picker(), None);
    }

    #[test]
    fn paged_language_picker_keeps_values_and_bounds_in_sync() {
        let mut state = DashboardState::default();
        state.language.speaker_targets = vec![DashboardChoice {
            value: String::new(),
            options: (0..40)
                .map(|index| DashboardOption {
                    value: format!("language-{index}"),
                    label: format!("Language {index}"),
                })
                .collect(),
        }];
        click(&mut state, 500., 168.);
        click(&mut state, 800., 450.);
        assert_eq!(click(&mut state, 900., 278.), None);
        assert_eq!(state.picker_page(), 0);
        assert_eq!(click(&mut state, 1050., 278.), None);
        assert_eq!(state.picker_page(), 1);
        assert_eq!(click(&mut state, 1050., 278.), None);
        assert_eq!(state.picker_page(), 1);
        assert_eq!(
            click(&mut state, 100., 360.),
            Some(DashboardAction::SetTargetLanguage {
                group: TargetGroup::Speaker,
                index: 0,
                value: "language-32".into()
            })
        );
    }

    #[test]
    fn empty_and_disabled_language_choices_do_not_open_a_picker() {
        let mut state = DashboardState::default();
        click(&mut state, 500., 168.);
        assert_eq!(click(&mut state, 200., 280.), None);
        assert_eq!(state.picker(), None);
        state.language.recognition.options = vec![DashboardOption {
            value: "en".into(),
            label: "English".into(),
        }];
        state.set_interactive(false);
        assert_eq!(click(&mut state, 200., 280.), None);
        assert_eq!(state.picker(), None);
    }

    #[test]
    fn tab_navigation_is_local_and_hides_controls_from_other_pages() {
        let mut state = DashboardState::default();
        state.pointer_down(800., 168.);
        assert_eq!(state.pointer_up(800., 168.), None);
        state.pointer_down(500., 374.);
        assert_eq!(state.pointer_up(500., 374.), None);
        state.pointer_down(200., 320.);
        let action = state.pointer_up(200., 320.);
        assert_eq!(
            action.map(|action| serde_json::to_value(action).unwrap()),
            Some(serde_json::json!("toggle_osc"))
        );
    }

    #[test]
    fn language_tabs_do_not_emit_settings_actions() {
        let mut state = DashboardState::default();
        state.pointer_down(500., 168.);
        assert_eq!(state.pointer_up(500., 168.), None);
        state.pointer_down(108., 510.);
        assert_eq!(state.pointer_up(108., 510.), None);
    }

    #[test]
    fn master_switch_maps_to_a_semantic_action() {
        let mut state = DashboardState::default();

        state.pointer_down(1200.0, 168.0);

        assert_eq!(
            state.pointer_up(1200.0, 168.0),
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

        state.pointer_down(1200.0, 168.0);

        assert_eq!(state.pointer_up(1200.0, 168.0), None);
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
