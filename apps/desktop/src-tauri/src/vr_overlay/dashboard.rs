use serde::{Deserialize, Serialize};

#[cfg(any(windows, linux_vr))]
pub const DASHBOARD_WIDTH: u32 = 1440;
#[cfg(any(windows, linux_vr, test))]
pub const DASHBOARD_HEIGHT: u32 = 900;

#[cfg(any(windows, linux_vr, test))]
pub fn pointer_from_openvr(x: f32, y: f32, height: u32) -> (f32, f32) {
    (x, height as f32 - y)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardViewModel {
    #[serde(default)]
    pub locale: String,
    #[serde(default)]
    pub player_audio_notice: Option<super::player_audio_ui::Prompt>,
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
    #[serde(default)]
    pub network_capture: DashboardNetworkCapture,
    pub content: DashboardChoice,
    #[serde(default = "available_by_default")]
    pub ocr_available: bool,
    #[serde(default)]
    pub vision_available: bool,
    #[serde(default = "available_by_default")]
    pub osc_available: bool,
    pub headset: DashboardHeadset,
    pub wrist: DashboardWrist,
    #[serde(default)]
    pub wrist_menu: DashboardWristMenu,
    pub ocr: DashboardOcr,
    #[serde(default)]
    pub vision: DashboardVision,
    pub language: DashboardLanguage,
    pub osc: DashboardOsc,
    #[serde(default)]
    pub unitext: DashboardUnitext,
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
    #[serde(default)]
    pub network_capture_start: String,
    #[serde(default)]
    pub network_capture_stop: String,
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
    #[serde(default)]
    pub osc_title: String,
    pub recognition_language: String,
    #[serde(default)]
    pub speaker_recognition_language: String,
    #[serde(default)]
    pub microphone_recognition_language: String,
    #[serde(default)]
    pub recognition_auto: String,
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
    #[serde(default)]
    pub unitext_title: String,
    #[serde(default)]
    pub unitext_include_microphone: String,
    #[serde(default)]
    pub unitext_mute_sync: String,
    #[serde(default)]
    pub unitext_include_speaker: String,
    #[serde(default)]
    pub unitext_preserve_original: String,
    #[serde(default)]
    pub unitext_port_conflict: String,
    pub close: String,
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
    #[serde(default)]
    pub back: String,
    #[serde(default)]
    pub preferred: String,
    #[serde(default)]
    pub master_off_banner: String,
    #[serde(default)]
    pub recognition_locked: String,
    #[serde(default)]
    pub translation_off_hint: String,
    #[serde(default)]
    pub delete_preset_title: String,
    #[serde(default)]
    pub no_presets: String,
    #[serde(default)]
    pub anchor_mode: String,
    #[serde(default)]
    pub anchor_world: String,
    #[serde(default)]
    pub follow_view: String,
    #[serde(default)]
    pub wrist_menu: String,
    #[serde(default)]
    pub layout_edit: String,
    #[serde(default)]
    pub layout_edit_enter: String,
    #[serde(default)]
    pub layout_edit_waiting: String,
    #[serde(default)]
    pub layout_edit_save: String,
    #[serde(default)]
    pub layout_edit_cancel: String,
    #[serde(default)]
    pub layout_edit_reset: String,
    #[serde(default)]
    pub layout_edit_reset_selected: String,
    #[serde(default)]
    pub layout_edit_auto_level: String,
    #[serde(default)]
    pub layout_edit_face_head: String,
    #[serde(default)]
    pub layout_edit_shared_wrist: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DashboardCapture {
    pub running: bool,
    pub available: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DashboardNetworkCapture {
    pub visible: bool,
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

fn default_max_recognition_selected() -> usize {
    4
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardRecognitionOption {
    pub value: String,
    pub label: String,
    #[serde(default)]
    pub can_prioritize: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DashboardRecognitionChoice {
    pub summary: String,
    #[serde(default)]
    pub selected: Vec<DashboardRecognitionOption>,
    #[serde(default)]
    pub options: Vec<DashboardOption>,
    #[serde(default = "default_max_recognition_selected")]
    pub max_selected: usize,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub note_is_warning: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DashboardLanguage {
    #[serde(default)]
    pub speaker_recognition: DashboardRecognitionChoice,
    #[serde(default)]
    pub microphone_recognition: DashboardRecognitionChoice,
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

#[cfg(any(windows, linux_vr, test))]
impl DashboardLanguage {
    pub(crate) fn recognition(&self, group: TargetGroup) -> &DashboardRecognitionChoice {
        match group {
            TargetGroup::Speaker => &self.speaker_recognition,
            TargetGroup::Microphone => &self.microphone_recognition,
        }
    }

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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardUnitext {
    pub enabled: bool,
    pub include_microphone: bool,
    pub mute_sync: bool,
    pub include_speaker: bool,
    pub preserve_original: bool,
    #[serde(default)]
    pub port_conflict: bool,
    #[serde(default)]
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
    #[serde(default)]
    pub appearance: Vec<DashboardAppearanceField>,
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
    #[serde(default)]
    pub appearance: Vec<DashboardAppearanceField>,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AppearanceField {
    #[serde(rename = "width_m")]
    Width,
    #[serde(rename = "opacity")]
    Opacity,
    #[serde(rename = "font_size_px")]
    FontSize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardAppearanceField {
    pub field: AppearanceField,
    pub label: String,
    pub value: String,
    pub can_decrease: bool,
    pub can_increase: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DashboardWristMenu {
    pub enabled: bool,
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
    ToggleNetworkCapture,
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
    ToggleWristFollowView,
    ToggleWristMenu,
    ToggleOcr,
    ToggleVision,
    ToggleOcrGesture,
    OpenOcrBindings,
    ToggleOsc,
    ToggleOscOriginal,
    ToggleOscMuteSync,
    ToggleUnitext,
    ToggleUnitextIncludeMicrophone,
    ToggleUnitextMuteSync,
    ToggleUnitextIncludeSpeaker,
    ToggleUnitextPreserveOriginal,
    SetDisplayContent(String),
    SetWristHand(String),
    SetOscStrategy(String),
    ToggleRecognitionLanguage {
        group: TargetGroup,
        value: String,
    },
    PrioritizeRecognitionLanguage {
        group: TargetGroup,
        index: usize,
    },
    ClearRecognitionLanguages(TargetGroup),
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
        #[serde(default)]
        coarse: bool,
    },
    AdjustDisplayAppearance {
        kind: DisplayKind,
        field: AppearanceField,
        direction: i32,
        #[serde(default)]
        coarse: bool,
    },
    ResetDisplayPosition(DisplayKind),
    RecenterDialogue,
    AttachDialogue,
    EnterLayoutEdit,
    CommitLayoutEdit,
    CancelLayoutEdit,
    ResetLayoutEdit,
}

#[cfg(any(windows, linux_vr))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DashboardPointerEvent {
    Move {
        x: f32,
        y: f32,
    },
    Down {
        x: f32,
        y: f32,
    },
    Up {
        x: f32,
        y: f32,
    },
    /// Viewport fraction; positive scrolls down.
    Scroll {
        delta: f32,
    },
    Shown,
    Hidden,
}

#[cfg(any(windows, linux_vr, test))]
/// Commands come from the hit map of the currently rendered Dashboard surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum DashboardCommand {
    PlayerAudio {
        request_id: u32,
        action: super::player_audio_ui::Action,
    },
    Page(DashboardPage),
    Picker(DashboardPicker),
    PickerPage(i32),
    Select(usize),
    Position(DisplayKind),
    Recognition(TargetGroup),
    Close,
    Confirm,
    Cancel,
    Action(DashboardAction),
}

#[cfg(any(windows, linux_vr, test))]
impl DashboardCommand {
    pub fn is_repeatable(&self) -> bool {
        matches!(
            self,
            Self::Action(
                DashboardAction::AdjustDisplayPosition { .. }
                    | DashboardAction::AdjustDisplayAppearance { .. }
                    | DashboardAction::HeadsetWidthDown
                    | DashboardAction::HeadsetWidthUp
                    | DashboardAction::HeadsetOpacityDown
                    | DashboardAction::HeadsetOpacityUp
                    | DashboardAction::WristWidthDown
                    | DashboardAction::WristWidthUp
                    | DashboardAction::WristOpacityDown
                    | DashboardAction::WristOpacityUp
            )
        )
    }
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
    Mode,
    Target { group: TargetGroup, index: usize },
    ApplyPreset,
    DeletePreset,
}

#[cfg(any(windows, linux_vr, test))]
pub const PICKER_PAGE_SIZE: usize = 15;

#[cfg(any(windows, linux_vr, test))]
impl DashboardPicker {
    pub(crate) fn choice(self, view: &DashboardViewModel) -> Option<&DashboardChoice> {
        Some(match self {
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
    pub recognition_editor: Option<TargetGroup>,
    pub confirmation: Option<DashboardConfirmation>,
    pub edit_active: bool,
    pub edit_waiting_close: bool,
    pub edit_dirty: bool,
    pub edit_saving: bool,
    pub edit_error: Option<String>,
}

#[derive(Debug)]
pub struct DashboardState {
    pub ui: DashboardUi,
    pub hovered: Option<u8>,
    pub pressed: Option<u8>,
    #[cfg(any(windows, linux_vr, test))]
    interactive: bool,
    language: Option<DashboardLanguage>,
}

impl DashboardState {
    pub fn new() -> Self {
        Self {
            ui: DashboardUi::default(),
            hovered: None,
            pressed: None,
            #[cfg(any(windows, linux_vr, test))]
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
        if self
            .ui
            .recognition_editor
            .is_some_and(|group| view.language.recognition(group).options.is_empty())
        {
            self.ui.recognition_editor = None;
            self.clear_pointer();
        }
        if !view.osc_available && self.ui.page == DashboardPage::Osc {
            self.ui = DashboardUi::default();
            self.clear_pointer();
        }
    }

    #[cfg(any(windows, linux_vr, test))]
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
            DashboardCommand::Recognition(group)
                if self.ui.page == DashboardPage::Language
                    && self.ui.picker.is_none()
                    && self.ui.recognition_editor.is_none() =>
            {
                if !view.language.recognition(group).options.is_empty() {
                    self.ui.recognition_editor = Some(group);
                }
                None
            }
            DashboardCommand::Close => {
                self.ui.picker = None;
                self.ui.display_editor = None;
                self.ui.recognition_editor = None;
                None
            }
            DashboardCommand::Action(action) => {
                if self.ui.picker.is_some()
                    && !matches!(
                        action,
                        DashboardAction::ToggleMaster
                            | DashboardAction::ToggleCapture
                            | DashboardAction::ToggleNetworkCapture
                    )
                {
                    return None;
                }
                self.emit_or_confirm(action, String::new(), view)
            }
            _ => None,
        }
    }

    #[cfg(any(windows, linux_vr, test))]
    pub fn activate_repeat(
        &mut self,
        command: DashboardCommand,
        view: &DashboardViewModel,
    ) -> Option<DashboardAction> {
        if !self.interactive || !command.is_repeatable() {
            return None;
        }
        self.update_view(view);
        if self.ui.confirmation.is_some() || self.ui.picker.is_some() {
            return None;
        }
        let DashboardCommand::Action(mut action) = command else {
            return None;
        };
        if !self.action_allowed(&action, view) {
            return None;
        }
        match &mut action {
            DashboardAction::AdjustDisplayPosition { coarse, .. }
            | DashboardAction::AdjustDisplayAppearance { coarse, .. } => *coarse = true,
            _ => {}
        }
        Some(action)
    }

    #[cfg(any(windows, linux_vr, test))]
    fn emit_or_confirm(
        &mut self,
        action: DashboardAction,
        label: String,
        view: &DashboardViewModel,
    ) -> Option<DashboardAction> {
        if !self.action_allowed(&action, view) {
            return None;
        }
        if matches!(action, DashboardAction::DeleteLanguagePreset(_)) {
            self.ui.confirmation = Some(DashboardConfirmation { action, label });
            None
        } else {
            Some(action)
        }
    }

    #[cfg(any(windows, linux_vr, test))]
    fn action_allowed(&self, action: &DashboardAction, view: &DashboardViewModel) -> bool {
        use DashboardAction::*;
        if matches!(action, CommitLayoutEdit | CancelLayoutEdit | ResetLayoutEdit) {
            return (self.ui.edit_active || self.ui.edit_waiting_close)
                && !matches!(
                    (action, self.ui.edit_active, self.ui.edit_saving),
                    (CommitLayoutEdit | ResetLayoutEdit, false, _)
                        | (CommitLayoutEdit, _, true)
                );
        }
        let page = match action {
            ToggleMaster => return true,
            ToggleCapture => return view.capture.available,
            ToggleNetworkCapture => {
                return view.network_capture.visible && view.network_capture.available;
            }
            ToggleRecognitionLanguage { .. }
            | PrioritizeRecognitionLanguage { .. }
            | ClearRecognitionLanguages(_)
            | SetTranslationMode(_)
            | SetTargetLanguage { .. }
            | AddTarget(_)
            | DeleteTarget { .. }
            | MoveTarget { .. }
            | SaveLanguagePreset(_)
            | ApplyLanguagePreset(_)
            | DeleteLanguagePreset(_) => DashboardPage::Language,
            ToggleOsc
            | ToggleOscOriginal
            | ToggleOscMuteSync
            | SetOscStrategy(_)
            | ToggleUnitext
            | ToggleUnitextIncludeMicrophone
            | ToggleUnitextMuteSync
            | ToggleUnitextIncludeSpeaker
            | ToggleUnitextPreserveOriginal => DashboardPage::Osc,
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
            ToggleUnitext => view.unitext.enabled || !view.unitext.port_conflict,
            ToggleUnitextIncludeMicrophone
            | ToggleUnitextMuteSync
            | ToggleUnitextIncludeSpeaker
            | ToggleUnitextPreserveOriginal => true,
            ToggleRecognitionLanguage { group, value } => {
                let choice = view.language.recognition(*group);
                self.ui.recognition_editor == Some(*group)
                    && (choice.selected.iter().any(|item| item.value == *value)
                        || (choice.selected.len() < choice.max_selected
                            && choice.options.iter().any(|opt| opt.value == *value)))
            }
            PrioritizeRecognitionLanguage { group, index } => {
                let choice = view.language.recognition(*group);
                self.ui.recognition_editor == Some(*group)
                    && *index > 0
                    && choice.selected.get(*index).is_some_and(|item| {
                        item.can_prioritize
                            || choice.options.iter().any(|opt| opt.value == item.value)
                    })
            }
            ClearRecognitionLanguages(group) => {
                let choice = view.language.recognition(*group);
                self.ui.recognition_editor == Some(*group)
                    && !choice.selected.is_empty()
                    && !choice.options.is_empty()
            }
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
                ..
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
            AdjustDisplayAppearance {
                kind,
                field,
                direction,
                ..
            } => {
                let fields = if *kind == DisplayKind::Headset {
                    &view.headset.appearance
                } else {
                    &view.wrist.appearance
                };
                self.ui.display_editor.is_none_or(|editor| editor == *kind)
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
            ToggleWristFollowView => {
                self.ui.display_editor == Some(DisplayKind::Wrist)
                    && view.wrist.anchor_mode == "world"
            }
            EnterLayoutEdit => !self.ui.edit_active && !self.ui.edit_waiting_close,
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
            locale: "en-US".into(),
            player_audio_notice: None,
            caption_notice: None,
            max_visible_players: None,
            caption_visibility: CaptionVisibility::default(),
            labels: DashboardLabels::default(),
            enabled: false,
            capture: DashboardCapture {
                running: false,
                available: true,
            },
            network_capture: DashboardNetworkCapture::default(),
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
                appearance: vec![DashboardAppearanceField {
                    field: AppearanceField::FontSize,
                    label: "Font size".into(),
                    value: "54 px".into(),
                    can_decrease: true,
                    can_increase: false,
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
                appearance: vec![],
            },
            wrist_menu: DashboardWristMenu::default(),
            ocr: DashboardOcr {
                available: true,
                enabled: true,
                backend: "winrt".into(),
                gesture: true,
            },
            vision: DashboardVision::default(),
            language: DashboardLanguage {
                speaker_recognition: DashboardRecognitionChoice {
                    summary: "English".into(),
                    selected: vec![DashboardRecognitionOption {
                        value: "en".into(),
                        label: "English".into(),
                        can_prioritize: false,
                    }],
                    options: choice.options.clone(),
                    max_selected: 4,
                    note: None,
                    note_is_warning: false,
                },
                microphone_recognition: DashboardRecognitionChoice {
                    summary: "Auto".into(),
                    selected: vec![],
                    options: choice.options.clone(),
                    max_selected: 4,
                    note: None,
                    note_is_warning: false,
                },
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
            unitext: DashboardUnitext {
                enabled: false,
                include_microphone: true,
                mute_sync: true,
                include_speaker: true,
                preserve_original: true,
                port_conflict: false,
                endpoint: "127.0.0.1:9000".into(),
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
        view.language.speaker_targets[0].options = (0..40)
            .map(|index| DashboardOption {
                value: format!("language-{index}"),
                label: format!("Language {index}"),
            })
            .collect();
        state.activate(DashboardCommand::Page(DashboardPage::Language), &view);
        state.activate(
            DashboardCommand::Picker(DashboardPicker::Target {
                group: TargetGroup::Speaker,
                index: 0,
            }),
            &view,
        );
        assert_eq!(state.activate(DashboardCommand::Select(15), &view), None);
        state.activate(DashboardCommand::PickerPage(1), &view);
        let selected = state.activate(DashboardCommand::Select(15), &view).unwrap();
        assert_eq!(
            serde_json::to_value(selected).unwrap(),
            serde_json::json!({
                "set_target_language": {
                    "group": "speaker",
                    "index": 0,
                    "value": "language-15"
                }
            })
        );
        assert!(state.ui.picker.is_none());
        assert_eq!(
            action(
                &mut state,
                &view,
                DashboardAction::SetTargetLanguage {
                    group: TargetGroup::Speaker,
                    index: 0,
                    value: "missing".into(),
                }
            ),
            None
        );
    }

    #[test]
    fn recognition_editor_persists_across_updates_and_validates_actions() {
        let mut state = DashboardState::new();
        let mut view = view();
        state.activate(DashboardCommand::Page(DashboardPage::Language), &view);
        assert_eq!(
            state.activate(
                DashboardCommand::Recognition(TargetGroup::Microphone),
                &view
            ),
            None
        );
        assert_eq!(state.ui.recognition_editor, Some(TargetGroup::Microphone));

        let add = DashboardAction::ToggleRecognitionLanguage {
            group: TargetGroup::Microphone,
            value: "ja".into(),
        };
        assert_eq!(
            serde_json::to_value(&add).unwrap(),
            serde_json::json!({
                "toggle_recognition_language": {
                    "group": "microphone",
                    "value": "ja"
                }
            })
        );
        assert_eq!(action(&mut state, &view, add.clone()), Some(add));
        assert_eq!(
            action(
                &mut state,
                &view,
                DashboardAction::ToggleRecognitionLanguage {
                    group: TargetGroup::Speaker,
                    value: "ja".into(),
                }
            ),
            None
        );
        assert_eq!(
            action(
                &mut state,
                &view,
                DashboardAction::ClearRecognitionLanguages(TargetGroup::Microphone)
            ),
            None
        );

        view.language.microphone_recognition.selected = vec![
            DashboardRecognitionOption {
                value: "en".into(),
                label: "English".into(),
                can_prioritize: false,
            },
            DashboardRecognitionOption {
                value: "ja".into(),
                label: "Japanese".into(),
                can_prioritize: true,
            },
        ];
        state.update_view(&view);
        assert_eq!(state.ui.recognition_editor, Some(TargetGroup::Microphone));

        let prioritize = DashboardAction::PrioritizeRecognitionLanguage {
            group: TargetGroup::Microphone,
            index: 1,
        };
        assert_eq!(
            serde_json::to_value(&prioritize).unwrap(),
            serde_json::json!({
                "prioritize_recognition_language": {
                    "group": "microphone",
                    "index": 1
                }
            })
        );
        assert_eq!(
            action(&mut state, &view, prioritize.clone()),
            Some(prioritize)
        );

        let clear = DashboardAction::ClearRecognitionLanguages(TargetGroup::Microphone);
        assert_eq!(
            serde_json::to_value(&clear).unwrap(),
            serde_json::json!({ "clear_recognition_languages": "microphone" })
        );
        assert_eq!(action(&mut state, &view, clear.clone()), Some(clear));

        view.language.microphone_recognition.options.clear();
        state.update_view(&view);
        assert_eq!(state.ui.recognition_editor, None);
    }

    #[test]
    fn unitext_actions_encode_snake_case_and_enforce_port_conflict() {
        let mut state = DashboardState::new();
        let mut view = view();
        state.activate(DashboardCommand::Page(DashboardPage::Osc), &view);

        assert_eq!(
            serde_json::to_value(DashboardAction::ToggleUnitext).unwrap(),
            serde_json::json!("toggle_unitext")
        );
        assert_eq!(
            serde_json::to_value(DashboardAction::ToggleUnitextIncludeMicrophone).unwrap(),
            serde_json::json!("toggle_unitext_include_microphone")
        );
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleUnitext),
            Some(DashboardAction::ToggleUnitext)
        );
        assert_eq!(
            action(
                &mut state,
                &view,
                DashboardAction::ToggleUnitextPreserveOriginal
            ),
            Some(DashboardAction::ToggleUnitextPreserveOriginal)
        );

        view.unitext.port_conflict = true;
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleUnitext),
            None
        );
        view.unitext.enabled = true;
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleUnitext),
            Some(DashboardAction::ToggleUnitext)
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
        assert_eq!(action(&mut state, &view, remove.clone()), Some(remove));
        assert!(state.ui.confirmation.is_none());
        view.language.speaker_targets.remove(1);
        state.update_view(&view);
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

        let delete_preset = DashboardAction::DeleteLanguagePreset("en".into());
        assert_eq!(action(&mut state, &view, delete_preset.clone()), None);
        assert!(state.ui.confirmation.is_some());
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleMaster),
            None
        );
        assert_eq!(
            state.activate(DashboardCommand::Confirm, &view),
            Some(delete_preset.clone())
        );
        action(&mut state, &view, delete_preset);
        view.language
            .presets
            .options
            .retain(|option| option.value != "en");
        state.update_view(&view);
        assert!(state.ui.confirmation.is_none());
        assert_eq!(state.activate(DashboardCommand::Confirm, &view), None);
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
            coarse: false,
        };
        assert_eq!(action(&mut state, &view, adjust(-1)), None);
        assert_eq!(action(&mut state, &view, adjust(1)), Some(adjust(1)));
        assert_eq!(action(&mut state, &view, adjust(2)), None);
        let appearance = |direction| DashboardAction::AdjustDisplayAppearance {
            kind: DisplayKind::Headset,
            field: AppearanceField::FontSize,
            direction,
            coarse: true,
        };
        assert_eq!(
            action(&mut state, &view, appearance(-1)),
            Some(appearance(-1))
        );
        assert_eq!(action(&mut state, &view, appearance(1)), None);
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleWristFollowView),
            None
        );
        state.activate(DashboardCommand::Position(DisplayKind::Wrist), &view);
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleWristFollowView),
            None
        );
        view.wrist.anchor_mode = "world".into();
        assert_eq!(
            action(&mut state, &view, DashboardAction::ToggleWristFollowView),
            Some(DashboardAction::ToggleWristFollowView)
        );
        view.osc_available = false;
        state.activate(DashboardCommand::Page(DashboardPage::Osc), &view);
        assert_eq!(state.ui.page, DashboardPage::Display);
    }

    #[test]
    fn repeatable_commands_stay_active_without_clearing_pointer_and_stop_at_limits() {
        let mut state = DashboardState::new();
        let mut view = view();
        state.activate(DashboardCommand::Position(DisplayKind::Headset), &view);
        state.hovered = Some(4);
        state.pressed = Some(4);
        let cmd = DashboardCommand::Action(DashboardAction::AdjustDisplayPosition {
            kind: DisplayKind::Headset,
            field: PositionField::Horizontal,
            direction: 1,
            coarse: false,
        });
        assert!(cmd.is_repeatable());
        assert_eq!(
            state.activate_repeat(cmd.clone(), &view),
            Some(DashboardAction::AdjustDisplayPosition {
                kind: DisplayKind::Headset,
                field: PositionField::Horizontal,
                direction: 1,
                coarse: true,
            })
        );
        let appearance_cmd = DashboardCommand::Action(DashboardAction::AdjustDisplayAppearance {
            kind: DisplayKind::Headset,
            field: AppearanceField::FontSize,
            direction: -1,
            coarse: false,
        });
        assert_eq!(
            state.activate_repeat(appearance_cmd.clone(), &view),
            Some(DashboardAction::AdjustDisplayAppearance {
                kind: DisplayKind::Headset,
                field: AppearanceField::FontSize,
                direction: -1,
                coarse: true,
            })
        );
        assert_eq!(state.hovered, Some(4));
        assert_eq!(state.pressed, Some(4));
        assert_eq!(
            state.activate(appearance_cmd, &view),
            Some(DashboardAction::AdjustDisplayAppearance {
                kind: DisplayKind::Headset,
                field: AppearanceField::FontSize,
                direction: -1,
                coarse: false,
            })
        );
        view.headset.position[0].can_increase = false;
        assert_eq!(state.activate_repeat(cmd, &view), None);
        assert!(!DashboardCommand::Action(DashboardAction::ToggleMaster).is_repeatable());
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
