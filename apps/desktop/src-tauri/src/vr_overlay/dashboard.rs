use serde::{Deserialize, Serialize};

pub const DASHBOARD_WIDTH: u32 = 1440;
pub const DASHBOARD_HEIGHT: u32 = 900;

pub fn pointer_from_openvr(x: f32, y: f32, height: u32) -> (f32, f32) {
    (x, height as f32 - y)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardViewModel {
    pub labels: DashboardLabels,
    pub enabled: bool,
    #[serde(default)]
    pub capture: DashboardCapture,
    pub content: DashboardChoice,
    #[serde(default = "available_by_default")]
    pub ocr_available: bool,
    #[serde(default)]
    pub vision_available: bool,
    #[serde(default = "available_by_default")]
    pub osc_available: bool,
    pub headset: DashboardHeadset,
    pub wrist: DashboardWrist,
    pub ocr: DashboardOcr,
    #[serde(default)]
    pub vision: DashboardVision,
    pub language: DashboardLanguage,
    pub osc: DashboardOsc,
    pub status: String,
    pub save_state: DashboardSaveState,
    pub error: Option<String>,
}

fn available_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DashboardLabels {
    #[serde(default)]
    pub ocr_progress: super::ocr_progress::ProgressLabels,
    #[serde(default)]
    pub wrist_reader: super::wrist_labels::WristLabels,
    pub title: String,
    pub subtitle: String,
    pub master: String,
    #[serde(default)]
    pub capture_start: String,
    #[serde(default)]
    pub capture_stop: String,
    pub headset: String,
    pub wrist: String,
    pub ocr: String,
    #[serde(default)]
    pub vision: String,
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
    pub osc_strategy: String,
    pub osc_hint: String,
    pub close: String,
    pub content_hint: String,
    pub headset_hint: String,
    pub tools: String,
    pub vision_unconfigured: String,
    pub ocr_unavailable: String,
    pub hide_preview: String,
    pub confirm_delete: String,
    pub delete_hint: String,
    pub cancel: String,
    pub position_hint: String,
    pub rotation_hint: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DashboardCapture {
    pub running: bool,
    pub available: bool,
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
    pub strategy: DashboardChoice,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardHeadset {
    pub enabled: bool,
    #[serde(default)]
    pub preview_visible: bool,
    pub width: String,
    pub opacity: String,
    pub position: Vec<DashboardNumberField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardWrist {
    pub enabled: bool,
    pub hand: DashboardChoice,
    #[serde(default)]
    pub preview_visible: bool,
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
    #[serde(default = "available_by_default")]
    pub available: bool,
    pub enabled: bool,
    pub backend: String,
    pub gesture: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DashboardVision {
    pub enabled: bool,
    pub configured: bool,
    pub model: String,
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
    ToggleCapture,
    ToggleHeadset,
    HeadsetWidthDown,
    HeadsetWidthUp,
    HeadsetOpacityDown,
    HeadsetOpacityUp,
    PreviewHeadset,
    ToggleWrist,
    WristWidthDown,
    WristWidthUp,
    WristOpacityDown,
    WristOpacityUp,
    PreviewWrist,
    ToggleOcr,
    ToggleVision,
    ToggleOcrGesture,
    OpenOcrBindings,
    ToggleOsc,
    ToggleOscOriginal,
    ToggleOscMuteSync,
    SetDisplayContent(String),
    SetWristHand(String),
    SetOscStrategy(String),
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
    OscStrategy(usize),
    PickerClose,
    PickerPrevious,
    PickerNext,
    PickerOption(usize),
    Master,
    Capture,
    HeadsetEnabled,
    DisplayContent(usize),
    HeadsetWidthDown,
    HeadsetWidthUp,
    HeadsetOpacityDown,
    HeadsetOpacityUp,
    HeadsetPreview,
    HeadsetPosition,
    WristEnabled,
    WristHand(usize),
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
    VisionEnabled,
    OcrGesture,
    OcrBindings,
    ConfirmDelete,
    CancelDelete,
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
        DashboardControl::Capture,
        Rect::new(1282., 26., 1358., 102.),
    ),
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
        DashboardControl::Master,
        Rect::new(1056., 124., 1392., 212.),
    ),
    (
        DashboardControl::HeadsetEnabled,
        Rect::new(590., 346., 688., 404.),
    ),
    (
        DashboardControl::HeadsetWidthDown,
        Rect::new(78., 522., 140., 578.),
    ),
    (
        DashboardControl::HeadsetWidthUp,
        Rect::new(304., 522., 366., 578.),
    ),
    (
        DashboardControl::HeadsetOpacityDown,
        Rect::new(390., 522., 452., 578.),
    ),
    (
        DashboardControl::HeadsetOpacityUp,
        Rect::new(616., 522., 678., 578.),
    ),
    (
        DashboardControl::HeadsetPreview,
        Rect::new(458., 600., 678., 656.),
    ),
    (
        DashboardControl::HeadsetPosition,
        Rect::new(78., 600., 438., 656.),
    ),
    (
        DashboardControl::WristEnabled,
        Rect::new(1274., 346., 1372., 404.),
    ),
    (
        DashboardControl::WristWidthDown,
        Rect::new(762., 522., 824., 578.),
    ),
    (
        DashboardControl::WristWidthUp,
        Rect::new(988., 522., 1050., 578.),
    ),
    (
        DashboardControl::WristOpacityDown,
        Rect::new(1074., 522., 1136., 578.),
    ),
    (
        DashboardControl::WristOpacityUp,
        Rect::new(1300., 522., 1362., 578.),
    ),
    (
        DashboardControl::WristPreview,
        Rect::new(1142., 600., 1362., 656.),
    ),
    (
        DashboardControl::WristPosition,
        Rect::new(762., 600., 1122., 656.),
    ),
    (
        DashboardControl::OcrEnabled,
        Rect::new(294., 748., 390., 804.),
    ),
    (
        DashboardControl::OcrGesture,
        Rect::new(642., 748., 738., 804.),
    ),
    (
        DashboardControl::VisionEnabled,
        Rect::new(994., 748., 1094., 804.),
    ),
    (
        DashboardControl::OcrBindings,
        Rect::new(1114., 744., 1362., 808.),
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
        DisplayContent(index) => return segment_rect(Rect::new(568., 250., 1362., 306.), 2, index),
        WristHand(index) => return segment_rect(Rect::new(924., 416., 1362., 472.), 3, index),
        OscStrategy(index) => return segment_rect(Rect::new(78., 632., 1362., 688.), 3, index),
        ConfirmDelete => return Rect::new(752., 592., 1032., 656.),
        CancelDelete => return Rect::new(408., 592., 688., 656.),
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

fn segment_rect(rect: Rect, count: usize, index: usize) -> Rect {
    let width = (rect.right - rect.left - 8. * (count - 1) as f32) / count as f32;
    let left = rect.left + index as f32 * (width + 8.);
    Rect::new(left, rect.top, left + width, rect.bottom)
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
        | DashboardControl::Capture
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
        | DashboardControl::OscStrategy(_) => Some(DashboardPage::Osc),
        _ => Some(DashboardPage::Display),
    }
}

#[derive(Debug)]
pub struct DashboardState {
    interactive: bool,
    capture_available: bool,
    ocr_available: bool,
    vision_available: bool,
    vision_enabled: bool,
    vision_configured: bool,
    osc_available: bool,
    hovered: Option<DashboardControl>,
    pressed: Option<DashboardControl>,
    page: DashboardPage,
    picker: Option<DashboardControl>,
    picker_page: usize,
    language: DashboardLanguage,
    display_editor: Option<DisplayKind>,
    position: [Vec<DashboardNumberField>; 2],
    choices: [DashboardChoice; 3],
    confirmation: Option<(DashboardAction, String)>,
}

impl DashboardState {
    pub fn new() -> Self {
        Self {
            interactive: true,
            capture_available: false,
            ocr_available: true,
            vision_available: false,
            vision_enabled: false,
            vision_configured: false,
            osc_available: true,
            hovered: None,
            pressed: None,
            page: DashboardPage::Display,
            picker: None,
            picker_page: 0,
            language: DashboardLanguage::default(),
            display_editor: None,
            position: [Vec::new(), Vec::new()],
            choices: std::array::from_fn(|_| DashboardChoice::default()),
            confirmation: None,
        }
    }

    pub fn update_view(&mut self, view: &DashboardViewModel) {
        // The frontend serializes saves, so pending persistence must not block input.
        self.set_interactive(true);
        self.capture_available = view.capture.available;
        self.choices = [
            view.content.clone(),
            view.wrist.hand.clone(),
            view.osc.strategy.clone(),
        ];
        self.ocr_available = view.ocr_available && view.ocr.available;
        self.vision_available = view.vision_available;
        self.vision_enabled = view.vision.enabled;
        self.vision_configured = view.vision.configured;
        self.osc_available = view.osc_available;
        if !view.osc_available && self.page == DashboardPage::Osc {
            self.page = DashboardPage::Display;
            self.hovered = None;
            self.pressed = None;
        }
        if self.language != view.language {
            self.language = view.language.clone();
            self.confirmation = None;
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

    pub(super) fn confirmation(&self) -> Option<(&DashboardAction, &str)> {
        self.confirmation
            .as_ref()
            .map(|(action, label)| (action, label.as_str()))
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
        if self.confirmation.is_some() {
            return [
                DashboardControl::ConfirmDelete,
                DashboardControl::CancelDelete,
            ]
            .into_iter()
            .find(|&control| control_rect(control).contains(x, y));
        }
        if self.picker.is_none() && self.display_editor.is_none() {
            for (choice_index, choice) in self.choices.iter().enumerate() {
                for index in 0..choice.options.len() {
                    let control = match choice_index {
                        0 => DashboardControl::DisplayContent(index),
                        1 => DashboardControl::WristHand(index),
                        _ => DashboardControl::OscStrategy(index),
                    };
                    if control_page(control) == Some(self.page)
                        && self.control_enabled(control)
                        && control_rect(control).contains(x, y)
                    {
                        return Some(control);
                    }
                }
            }
        }
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
        if control == DashboardControl::Capture && !self.capture_available {
            return false;
        }
        if !self.ocr_available
            && matches!(
                control,
                DashboardControl::OcrEnabled | DashboardControl::OcrGesture
            )
        {
            return false;
        }
        if control == DashboardControl::VisionEnabled
            && (!self.vision_available || (!self.vision_enabled && !self.vision_configured))
        {
            return false;
        }
        if control == DashboardControl::OcrBindings && !self.ocr_available && !self.vision_available
        {
            return false;
        }
        if !self.osc_available
            && (control == DashboardControl::OscTab
                || control_page(control) == Some(DashboardPage::Osc))
        {
            return false;
        }
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
            DashboardControl::DisplayContent(index) => self.choices[0].options.get(index).is_some(),
            DashboardControl::WristHand(index) => self.choices[1].options.get(index).is_some(),
            DashboardControl::OscStrategy(index) => self.choices[2].options.get(index).is_some(),
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
            DashboardControl::ConfirmDelete => {
                self.hovered = None;
                self.confirmation.take().map(|(action, _)| action)
            }
            DashboardControl::CancelDelete => {
                self.confirmation = None;
                self.hovered = None;
                None
            }
            DashboardControl::TargetDelete(group, index) => {
                let label = self.language.targets(group).get(index)?.value.clone();
                self.confirmation = Some((DashboardAction::DeleteTarget { group, index }, label));
                self.hovered = None;
                None
            }
            DashboardControl::DisplayContent(index) => Some(DashboardAction::SetDisplayContent(
                self.choices[0].options.get(index)?.value.clone(),
            )),
            DashboardControl::WristHand(index) => Some(DashboardAction::SetWristHand(
                self.choices[1].options.get(index)?.value.clone(),
            )),
            DashboardControl::OscStrategy(index) => Some(DashboardAction::SetOscStrategy(
                self.choices[2].options.get(index)?.value.clone(),
            )),
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
                let option = self.picker_choice()?.options.get(index)?.clone();
                let value = option.value;
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
                if matches!(action, DashboardAction::DeleteLanguagePreset(_)) {
                    self.confirmation = Some((action, option.label));
                    None
                } else {
                    Some(action)
                }
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
        DashboardControl::Master => DashboardAction::ToggleMaster,
        DashboardControl::Capture => DashboardAction::ToggleCapture,
        DashboardControl::HeadsetEnabled => DashboardAction::ToggleHeadset,
        DashboardControl::HeadsetWidthDown => DashboardAction::HeadsetWidthDown,
        DashboardControl::HeadsetWidthUp => DashboardAction::HeadsetWidthUp,
        DashboardControl::HeadsetOpacityDown => DashboardAction::HeadsetOpacityDown,
        DashboardControl::HeadsetOpacityUp => DashboardAction::HeadsetOpacityUp,
        DashboardControl::HeadsetPreview => DashboardAction::PreviewHeadset,
        DashboardControl::WristEnabled => DashboardAction::ToggleWrist,
        DashboardControl::WristWidthDown => DashboardAction::WristWidthDown,
        DashboardControl::WristWidthUp => DashboardAction::WristWidthUp,
        DashboardControl::WristOpacityDown => DashboardAction::WristOpacityDown,
        DashboardControl::WristOpacityUp => DashboardAction::WristOpacityUp,
        DashboardControl::WristPreview => DashboardAction::PreviewWrist,
        DashboardControl::OcrEnabled => DashboardAction::ToggleOcr,
        DashboardControl::VisionEnabled => DashboardAction::ToggleVision,
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
    fn vision_toggle_is_independent_of_ocr_and_requires_configuration_to_enable() {
        let mut state = DashboardState {
            ocr_available: false,
            vision_available: true,
            ..DashboardState::default()
        };
        assert_eq!(click(&mut state, 1040., 778.), None);
        state.vision_configured = true;
        assert_eq!(
            click(&mut state, 1040., 778.),
            Some(DashboardAction::ToggleVision)
        );
        assert_eq!(
            serde_json::to_string(&DashboardAction::ToggleVision).unwrap(),
            "\"toggle_vision\""
        );
        state.vision_configured = false;
        state.vision_enabled = true;
        assert_eq!(
            click(&mut state, 1040., 778.),
            Some(DashboardAction::ToggleVision)
        );
        assert_eq!(
            click(&mut state, 1200., 790.),
            Some(DashboardAction::OpenOcrBindings)
        );
    }

    #[test]
    fn feature_switches_remove_ocr_and_osc_controls() {
        let mut state = DashboardState {
            ocr_available: false,
            osc_available: false,
            ..DashboardState::default()
        };
        assert_eq!(click(&mut state, 340., 778.), None);
        assert_eq!(click(&mut state, 880., 168.), None);
        assert_eq!(state.page(), DashboardPage::Display);
        state.ocr_available = true;
        state.osc_available = true;
        assert_eq!(
            click(&mut state, 340., 778.),
            Some(DashboardAction::ToggleOcr)
        );
        click(&mut state, 880., 168.);
        assert_eq!(state.page(), DashboardPage::Osc);
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
    fn position_limits_and_disabled_input_preserve_the_editor() {
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
        assert_eq!(click(&mut state, 1330., 528.), None);
        // Clicking behind the confirmation must not delete or toggle anything.
        assert_eq!(click(&mut state, 1330., 528.), None);
        assert_eq!(click(&mut state, 800., 168.), None);
        assert_eq!(state.page(), DashboardPage::Language);
        assert_eq!(click(&mut state, 550., 620.), None);
        assert_eq!(click(&mut state, 1330., 528.), None);
        assert_eq!(
            click(&mut state, 900., 620.),
            Some(DashboardAction::DeleteTarget {
                group: TargetGroup::Speaker,
                index: 1,
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
        assert_eq!(click(&mut state, 100., 360.), None);
        assert_eq!(
            click(&mut state, 900., 620.),
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

        state.pointer_down(108.0, 550.0);
        let decrease = state.pointer_up(108.0, 550.0);
        state.pointer_down(336.0, 550.0);
        let increase = state.pointer_up(336.0, 550.0);

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
        for height in [840, 872, DASHBOARD_HEIGHT] {
            assert_eq!(
                pointer_from_openvr(100.0, 200.0, height),
                (100.0, height as f32 - 200.0)
            );
        }
    }

    #[test]
    fn segments_emit_direct_values_and_stay_on_their_page() {
        let mut state = DashboardState {
            choices: std::array::from_fn(|_| DashboardChoice {
                value: "Preferred".into(),
                options: vec![
                    DashboardOption {
                        value: "preferred_only".into(),
                        label: "Preferred".into(),
                    },
                    DashboardOption {
                        value: "all_languages".into(),
                        label: "All".into(),
                    },
                ],
            }),
            ..DashboardState::default()
        };
        assert_eq!(
            click(&mut state, 1200., 278.),
            Some(DashboardAction::SetDisplayContent("all_languages".into()))
        );
        assert_eq!(
            serde_json::to_value(DashboardAction::SetDisplayContent("all_languages".into()))
                .unwrap(),
            serde_json::json!({ "set_display_content": "all_languages" })
        );
        state.choices[1].options[1].value = "right".into();
        assert_eq!(
            click(&mut state, 1140., 444.),
            Some(DashboardAction::SetWristHand("right".into()))
        );
        click(&mut state, 800., 168.);
        assert_eq!(click(&mut state, 1200., 278.), None);
        assert_eq!(
            click(&mut state, 700., 660.),
            Some(DashboardAction::SetOscStrategy("all_languages".into()))
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
