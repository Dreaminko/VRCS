use serde::{Deserialize, Serialize};

pub const DASHBOARD_WIDTH: u32 = 1440;
pub const DASHBOARD_HEIGHT: u32 = 900;

pub fn pointer_from_openvr(x: f32, y: f32, height: u32) -> (f32, f32) {
    (x, height as f32 - y)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardViewModel {
    #[serde(default)]
    pub caption_visibility: CaptionVisibility,
    #[serde(default)]
    pub caption_notice: Option<super::caption_status::CaptionNotice>,
    #[serde(default)]
    pub max_visible_players: Option<usize>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CaptionVisibility {
    pub hidden_tracks: Vec<String>,
    pub hidden_source_languages: Vec<String>,
}

fn available_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DashboardLabels {
    #[serde(default)]
    pub captions: super::caption_status::CaptionLabels,
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
    #[serde(default)]
    pub recenter_dialogue: String,
    #[serde(default)]
    pub attach_dialogue: String,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
    #[serde(default)]
    pub anchor_mode: String,
    #[serde(default)]
    pub follow_view: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    RecenterDialogue,
    AttachDialogue,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DashboardPointerEvent {
    Move { x: f32, y: f32 },
    Down { x: f32, y: f32 },
    Up { x: f32, y: f32 },
    Shown,
    Hidden,
}

/// Commands come from the hit map of the currently submitted React frame.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum DashboardCommand {
    Page(DashboardPage),
    Picker(DashboardPicker),
    PickerPage(i32),
    Select(usize),
    Position(DisplayKind),
    Close,
    Confirm,
    Cancel,
    Action(DashboardAction),
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DashboardPage {
    #[default]
    Display,
    Language,
    Osc,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum DashboardPicker {
    Recognition,
    Mode,
    Target { group: TargetGroup, index: usize },
    ApplyPreset,
    DeletePreset,
}

pub const PICKER_PAGE_SIZE: usize = 32;

impl DashboardPicker {
    fn choice(self, view: &DashboardViewModel) -> Option<&DashboardChoice> {
        Some(match self {
            Self::Recognition => &view.language.recognition,
            Self::Mode => &view.language.mode,
            Self::Target { group, index } => view.language.targets(group).get(index)?,
            Self::ApplyPreset | Self::DeletePreset => &view.language.presets,
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DashboardConfirmation {
    pub action: DashboardAction,
    pub label: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct DashboardUi {
    pub page: DashboardPage,
    pub picker: Option<DashboardPicker>,
    pub picker_page: usize,
    pub display_editor: Option<DisplayKind>,
    pub confirmation: Option<DashboardConfirmation>,
}

#[derive(Debug)]
pub struct DashboardState {
    pub ui: DashboardUi,
    pub hovered: Option<u8>,
    pub pressed: Option<u8>,
    interactive: bool,
    language: Option<DashboardLanguage>,
}

impl DashboardState {
    pub fn new() -> Self {
        Self {
            ui: DashboardUi::default(),
            hovered: None,
            pressed: None,
            interactive: true,
            language: None,
        }
    }

    pub fn clear_pointer(&mut self) {
        self.hovered = None;
        self.pressed = None;
    }

    #[cfg(any(test, all(windows, debug_assertions, feature = "vr-preview")))]
    pub fn set_interactive(&mut self, interactive: bool) {
        self.interactive = interactive;
        if !interactive {
            self.clear_pointer();
        }
    }

    pub fn update_view(&mut self, view: &DashboardViewModel) {
        if self.language.as_ref() != Some(&view.language) {
            if self.language.is_some() {
                self.ui.picker = None;
                self.ui.confirmation = None;
                self.clear_pointer();
            }
            self.language = Some(view.language.clone());
        }
        if !view.osc_available && self.ui.page == DashboardPage::Osc {
            self.ui = DashboardUi::default();
            self.clear_pointer();
        }
    }

    pub fn activate(
        &mut self,
        command: DashboardCommand,
        view: &DashboardViewModel,
    ) -> Option<DashboardAction> {
        if !self.interactive {
            return None;
        }
        self.update_view(view);
        self.clear_pointer();
        if self.ui.confirmation.is_some() {
            return match command {
                DashboardCommand::Confirm => {
                    let action = self.ui.confirmation.take()?.action;
                    self.action_allowed(&action, view).then_some(action)
                }
                DashboardCommand::Cancel => {
                    self.ui.confirmation = None;
                    None
                }
                _ => None,
            };
        }
        match command {
            DashboardCommand::Page(page) if page != DashboardPage::Osc || view.osc_available => {
                self.ui = DashboardUi {
                    page,
                    ..DashboardUi::default()
                };
                None
            }
            DashboardCommand::Picker(picker)
                if self.ui.page == DashboardPage::Language && self.ui.picker.is_none() =>
            {
                if picker
                    .choice(view)
                    .is_some_and(|choice| !choice.options.is_empty())
                {
                    self.ui.picker = Some(picker);
                    self.ui.picker_page = 0;
                }
                None
            }
            DashboardCommand::PickerPage(direction) if matches!(direction, -1 | 1) => {
                let choice = self.ui.picker?.choice(view)?;
                let page = self
                    .ui
                    .picker_page
                    .saturating_add_signed(direction as isize);
                if page * PICKER_PAGE_SIZE < choice.options.len() {
                    self.ui.picker_page = page;
                }
                None
            }
            DashboardCommand::Select(index) => {
                let picker = self.ui.picker?;
                if index / PICKER_PAGE_SIZE != self.ui.picker_page {
                    return None;
                }
                let option = picker.choice(view)?.options.get(index)?;
                let value = option.value.clone();
                let action = match picker {
                    DashboardPicker::Recognition => DashboardAction::SetRecognitionLanguage(value),
                    DashboardPicker::Mode => DashboardAction::SetTranslationMode(value),
                    DashboardPicker::Target { group, index } => {
                        DashboardAction::SetTargetLanguage {
                            group,
                            index,
                            value,
                        }
                    }
                    DashboardPicker::ApplyPreset => DashboardAction::ApplyLanguagePreset(value),
                    DashboardPicker::DeletePreset => DashboardAction::DeleteLanguagePreset(value),
                };
                self.ui.picker = None;
                self.emit_or_confirm(action, option.label.clone(), view)
            }
            DashboardCommand::Position(kind) if self.ui.page == DashboardPage::Display => {
                self.ui.display_editor = Some(kind);
                None
            }
            DashboardCommand::Close => {
                self.ui.picker = None;
                self.ui.display_editor = None;
                None
            }
            DashboardCommand::Action(action) => {
                if self.ui.picker.is_some()
                    && !matches!(
                        action,
                        DashboardAction::ToggleMaster | DashboardAction::ToggleCapture
                    )
                {
                    return None;
                }
                let label = match &action {
                    DashboardAction::DeleteTarget { group, index } => view
                        .language
                        .targets(*group)
                        .get(*index)
                        .map(|choice| choice.value.clone())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                self.emit_or_confirm(action, label, view)
            }
            _ => None,
        }
    }

    fn emit_or_confirm(
        &mut self,
        action: DashboardAction,
        label: String,
        view: &DashboardViewModel,
    ) -> Option<DashboardAction> {
        if !self.action_allowed(&action, view) {
            return None;
        }
        if matches!(
            action,
            DashboardAction::DeleteTarget { .. } | DashboardAction::DeleteLanguagePreset(_)
        ) {
            self.ui.confirmation = Some(DashboardConfirmation { action, label });
            None
        } else {
            Some(action)
        }
    }

    fn action_allowed(&self, action: &DashboardAction, view: &DashboardViewModel) -> bool {
        use DashboardAction::*;
        let page = match action {
            ToggleMaster | ToggleCapture => {
                return !matches!(action, ToggleCapture) || view.capture.available
            }
            SetRecognitionLanguage(_)
            | SetTranslationMode(_)
            | SetTargetLanguage { .. }
            | AddTarget(_)
            | DeleteTarget { .. }
            | MoveTarget { .. }
            | SaveLanguagePreset(_)
            | ApplyLanguagePreset(_)
            | DeleteLanguagePreset(_) => DashboardPage::Language,
            ToggleOsc | ToggleOscOriginal | ToggleOscMuteSync | SetOscStrategy(_) => {
                DashboardPage::Osc
            }
            _ => DashboardPage::Display,
        };
        if self.ui.page != page || page == DashboardPage::Osc && !view.osc_available {
            return false;
        }
        let contains = |choice: &DashboardChoice, value: &str| {
            choice.options.iter().any(|option| option.value == value)
        };
        match action {
            ToggleOcr | ToggleOcrGesture => view.ocr_available && view.ocr.available,
            ToggleVision => {
                view.vision_available && (view.vision.enabled || view.vision.configured)
            }
            OpenOcrBindings => view.vision_available || view.ocr_available && view.ocr.available,
            SetDisplayContent(value) => contains(&view.content, value),
            SetWristHand(value) => contains(&view.wrist.hand, value),
            SetOscStrategy(value) => contains(&view.osc.strategy, value),
            SetRecognitionLanguage(value) => contains(&view.language.recognition, value),
            SetTranslationMode(value) => contains(&view.language.mode, value),
            SetTargetLanguage {
                group,
                index,
                value,
            } => view
                .language
                .targets(*group)
                .get(*index)
                .is_some_and(|choice| contains(choice, value)),
            AddTarget(group) => view.language.can_add && view.language.targets(*group).len() < 3,
            DeleteTarget { group, index } => {
                let targets = view.language.targets(*group);
                targets.len() > 1 && *index < targets.len()
            }
            MoveTarget {
                group,
                index,
                offset,
            } => {
                let count = view.language.targets(*group).len();
                *index < count
                    && matches!(offset, -1 | 1)
                    && index
                        .checked_add_signed(*offset as isize)
                        .is_some_and(|target| target < count)
            }
            SaveLanguagePreset(name) => view.language.can_save_preset && name.is_empty(),
            ApplyLanguagePreset(value) | DeleteLanguagePreset(value) => {
                contains(&view.language.presets, value)
            }
            AdjustDisplayPosition {
                kind,
                field,
                direction,
            } => {
                let fields = if *kind == DisplayKind::Headset {
                    &view.headset.position
                } else {
                    &view.wrist.position
                };
                self.ui.display_editor == Some(*kind)
                    && fields.iter().any(|item| {
                        item.field == *field
                            && match direction {
                                -1 => item.can_decrease,
                                1 => item.can_increase,
                                _ => false,
                            }
                    })
            }
            ResetDisplayPosition(kind) => self.ui.display_editor == Some(*kind),
            RecenterDialogue | AttachDialogue => self.ui.display_editor == Some(DisplayKind::Wrist),
            _ => true,
        }
    }
}

impl Default for DashboardState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> DashboardViewModel {
        let choice = DashboardChoice {
            value: "en".into(),
            options: vec![
                DashboardOption {
                    value: "en".into(),
                    label: "English".into(),
                },
                DashboardOption {
                    value: "ja".into(),
                    label: "Japanese".into(),
                },
            ],
        };
        DashboardViewModel {
            caption_notice: None,
            max_visible_players: None,
            caption_visibility: CaptionVisibility::default(),
            labels: DashboardLabels::default(),
            enabled: false,
            capture: DashboardCapture {
                running: false,
                available: true,
            },
            content: choice.clone(),
            ocr_available: true,
            vision_available: true,
            osc_available: true,
            headset: DashboardHeadset {
                enabled: true,
                preview_visible: false,
                width: "1 m".into(),
                opacity: "80%".into(),
                position: vec![DashboardNumberField {
                    field: PositionField::Horizontal,
                    label: "X".into(),
                    value: "0".into(),
                    can_decrease: false,
                    can_increase: true,
                }],
            },
            wrist: DashboardWrist {
                enabled: true,
                anchor_mode: "wrist".into(),
                follow_view: false,
                hand: choice.clone(),
                preview_visible: false,
                width: "1 m".into(),
                opacity: "80%".into(),
                position: vec![],
            },
            ocr: DashboardOcr {
                available: true,
                enabled: true,
                backend: "winrt".into(),
                gesture: true,
            },
            vision: DashboardVision::default(),
            language: DashboardLanguage {
                recognition: choice.clone(),
                mode: choice.clone(),
                speaker_targets: vec![choice.clone(), choice.clone()],
                microphone_targets: vec![choice.clone()],
                can_add: true,
                presets: choice.clone(),
                can_save_preset: true,
            },
            osc: DashboardOsc {
                enabled: true,
                original: false,
                mute_sync: false,
                strategy: choice,
                endpoint: String::new(),
            },
            status: String::new(),
            save_state: DashboardSaveState::Idle,
            error: None,
        }
    }

    fn action(
        state: &mut DashboardState,
        view: &DashboardViewModel,
        action: DashboardAction,
    ) -> Option<DashboardAction> {
        state.activate(DashboardCommand::Action(action), view)
    }

    #[test]
    fn master_off_preserves_settings_access_but_availability_is_enforced() {
        let mut state = DashboardState::new();
        let mut view = view();
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleHeadset),
            Some(DashboardAction::ToggleHeadset)
        );
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleVision),
            None
        );
        view.vision.configured = true;
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleVision),
            Some(DashboardAction::ToggleVision)
        );
        view.capture.available = false;
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleCapture),
            None
        );
        view.ocr_available = false;
        assert_eq!(action(&mut state, &view, DashboardAction::ToggleOcr), None);
        state.set_interactive(false);
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleMaster),
            None
        );
    }

    #[test]
    fn picker_validates_current_page_options_and_preserves_action_encoding() {
        let mut state = DashboardState::new();
        let mut view = view();
        view.language.recognition.options = (0..40)
            .map(|index| DashboardOption {
                value: format!("language-{index}"),
                label: format!("Language {index}"),
            })
            .collect();
        state.activate(DashboardCommand::Page(DashboardPage::Language), &view);
        state.activate(
            DashboardCommand::Picker(DashboardPicker::Recognition),
            &view,
        );
        assert_eq!(state.activate(DashboardCommand::Select(32), &view), None);
        state.activate(DashboardCommand::PickerPage(1), &view);
        let selected = state.activate(DashboardCommand::Select(32), &view).unwrap();
        assert_eq!(
            serde_json::to_value(selected).unwrap(),
            serde_json::json!({ "set_recognition_language": "language-32" })
        );
        assert!(state.ui.picker.is_none());
        assert_eq!(
            action(
                &mut state,
                &view,
                DashboardAction::SetRecognitionLanguage("missing".into())
            ),
            None
        );
    }

    #[test]
    fn deletion_requires_confirmation_and_stale_settings_cancel_it() {
        let mut state = DashboardState::new();
        let mut view = view();
        state.activate(DashboardCommand::Page(DashboardPage::Language), &view);
        let remove = DashboardAction::DeleteTarget {
            group: TargetGroup::Speaker,
            index: 1,
        };
        assert_eq!(action(&mut state, &view, remove.clone()), None);
        assert!(state.ui.confirmation.is_some());
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleMaster),
            None
        );
        assert_eq!(
            state.activate(DashboardCommand::Confirm, &view),
            Some(remove.clone())
        );
        action(&mut state, &view, remove);
        view.language.speaker_targets.remove(1);
        state.update_view(&view);
        assert!(state.ui.confirmation.is_none());
        assert_eq!(state.activate(DashboardCommand::Confirm, &view), None);
        assert_eq!(
            action(
                &mut state,
                &view,
                DashboardAction::DeleteTarget {
                    group: TargetGroup::Speaker,
                    index: 0
                }
            ),
            None
        );
    }

    #[test]
    fn target_routes_and_position_limits_reject_invalid_commands() {
        let mut state = DashboardState::new();
        let mut view = view();
        state.activate(DashboardCommand::Page(DashboardPage::Language), &view);
        assert_eq!(
            action(
                &mut state,
                &view,
                DashboardAction::MoveTarget {
                    group: TargetGroup::Speaker,
                    index: 0,
                    offset: -1
                }
            ),
            None
        );
        assert_eq!(
            action(
                &mut state,
                &view,
                DashboardAction::MoveTarget {
                    group: TargetGroup::Speaker,
                    index: 99,
                    offset: 1
                }
            ),
            None
        );
        assert_eq!(
            action(
                &mut state,
                &view,
                DashboardAction::AddTarget(TargetGroup::Microphone)
            ),
            Some(DashboardAction::AddTarget(TargetGroup::Microphone))
        );
        state.activate(DashboardCommand::Page(DashboardPage::Display), &view);
        state.activate(DashboardCommand::Position(DisplayKind::Headset), &view);
        let adjust = |direction| DashboardAction::AdjustDisplayPosition {
            kind: DisplayKind::Headset,
            field: PositionField::Horizontal,
            direction,
        };
        assert_eq!(action(&mut state, &view, adjust(-1)), None);
        assert_eq!(action(&mut state, &view, adjust(1)), Some(adjust(1)));
        assert_eq!(action(&mut state, &view, adjust(2)), None);
        view.osc_available = false;
        state.activate(DashboardCommand::Page(DashboardPage::Osc), &view);
        assert_eq!(state.ui.page, DashboardPage::Display);
    }

    #[test]
    fn commands_round_trip_without_changing_business_action_protocol() {
        let command = DashboardCommand::Action(DashboardAction::SetWristHand("right".into()));
        let encoded =
            serde_json::json!({ "type": "action", "value": { "set_wrist_hand": "right" } });
        assert_eq!(serde_json::to_value(&command).unwrap(), encoded);
        assert_eq!(
            serde_json::from_value::<DashboardCommand>(encoded).unwrap(),
            command
        );
        assert_eq!(
            pointer_from_openvr(100., 200., DASHBOARD_HEIGHT),
            (100., 700.)
        );
    }
}
