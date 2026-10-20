use super::dashboard::{
    control_rect, picker_option_rect, position_field_rect, DashboardAction, DashboardControl,
    DashboardPage, DashboardSaveState, DashboardState, DashboardViewModel, DisplayKind,
    TargetGroup, DASHBOARD_HEIGHT, DASHBOARD_WIDTH, PICKER_CLOSE_RECT, PICKER_NEXT_RECT,
    PICKER_PAGE_SIZE, PICKER_PREVIOUS_RECT,
};
use super::renderer::{BgraTexture, Texture};
use std::collections::HashMap;

const MAX_TEXT_CACHE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub struct RasterCache {
    text: HashMap<TextKey, Vec<u8>>,
    text_bytes: usize,
    icon: Option<(u32, u32, Vec<u8>)>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct TextKey {
    text: String,
    width: i32,
    height: i32,
    style: TextStyle,
    font_face: &'static str,
}

impl RasterCache {
    fn text_mask(
        &mut self,
        text: &str,
        rect: Rect,
        style: TextStyle,
        font_face: &'static str,
    ) -> Result<&[u8], String> {
        let key = TextKey {
            text: text.into(),
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
            style,
            font_face,
        };
        if !self.text.contains_key(&key) {
            let bgra = render_text_mask(text, rect, style, font_face)?;
            // Store coverage only; the text color and position may change on hover.
            let mask: Vec<u8> = bgra
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| pixel[0])
                .collect();
            if self.text_bytes + mask.len() > MAX_TEXT_CACHE_BYTES {
                self.text.clear();
                self.text_bytes = 0;
            }
            self.text_bytes += mask.len();
            self.text.insert(key.clone(), mask);
        }
        Ok(self.text.get(&key).expect("text mask exists"))
    }

    fn icon(&mut self, width: u32, height: u32) -> Result<&[u8], String> {
        if self
            .icon
            .as_ref()
            .is_none_or(|(w, h, _)| (*w, *h) != (width, height))
        {
            self.icon = Some((width, height, render_app_icon(width, height)?));
        }
        Ok(&self.icon.as_ref().expect("icon was rendered").2)
    }
}

// The GDI canvas stays BGRA through D3D11 upload; raw uploads convert at the boundary.
// Colors match the desktop theme in styles/base.css.
const CANVAS: [u8; 4] = [0xfb, 0xf8, 0xf5, 255];
const SURFACE: [u8; 4] = [255, 255, 255, 255];
const SURFACE_ELEVATED: [u8; 4] = [0xfd, 0xfb, 0xf9, 255];
const SURFACE_SUBTLE: [u8; 4] = [0xf7, 0xf2, 0xee, 255];
const TEXT: [u8; 4] = [0x34, 0x28, 0x1d, 255];
const TEXT_SECONDARY: [u8; 4] = [0x78, 0x68, 0x5a, 255];
const BORDER: [u8; 4] = [0xe9, 0xe2, 0xdd, 255];
const BORDER_STRONG: [u8; 4] = [0xbc, 0xb0, 0xa3, 255];
const PRIMARY: [u8; 4] = [0xff, 0xd6, 0x74, 255];
const PRIMARY_HOVER: [u8; 4] = [0xf5, 0xc8, 0x56, 255];
const PRIMARY_SOFT: [u8; 4] = [0xff, 0xf6, 0xdf, 255];
const PRIMARY_SOFTER: [u8; 4] = [0xff, 0xfb, 0xf0, 255];
const PRIMARY_INK: [u8; 4] = [0x7d, 0x61, 0x17, 255];
const ERROR: [u8; 4] = [0x44, 0x44, 0xc9, 255];
const ERROR_SOFT: [u8; 4] = [0xee, 0xf0, 0xfd, 255];

pub fn render(
    view: &DashboardViewModel,
    state: &DashboardState,
    cache: &mut RasterCache,
) -> Result<BgraTexture, String> {
    let compact = state.page() == DashboardPage::Display
        && state.display_editor().is_none()
        && state.picker().is_none()
        && state.confirmation().is_none();
    let has_footer = view.error.is_some()
        || matches!(
            view.save_state,
            DashboardSaveState::Saving | DashboardSaveState::Saved
        );
    let height = if compact {
        if has_footer {
            872
        } else {
            840
        }
    } else {
        DASHBOARD_HEIGHT
    };
    let footer_top = height as i32 - 40;
    let mut canvas = Canvas::new(DASHBOARD_WIDTH, height, CANVAS);
    canvas.cache = Some(cache);
    canvas.font_face = if view
        .labels
        .title
        .chars()
        .any(|c| matches!(c, '\u{3040}'..='\u{30ff}'))
    {
        "Yu Gothic UI\0"
    } else if view
        .labels
        .title
        .chars()
        .any(|c| matches!(c, '\u{3400}'..='\u{9fff}'))
    {
        "Microsoft YaHei UI\0"
    } else {
        "Segoe UI\0"
    };
    canvas.icon(Rect::new(48, 26, 116, 94))?;
    canvas.heading(&view.labels.title, Rect::new(140, 24, 704, 64), 32)?;
    canvas.text(
        &view.labels.subtitle,
        Rect::new(140, 64, 704, 102),
        20,
        TEXT_SECONDARY,
        Align::Left,
    )?;
    canvas.pill(Rect::new(732, 42, 1040, 86), &view.status, false)?;
    let capture_feedback = visual(state, DashboardControl::Capture);
    canvas.primary_icon_button(
        control_rect(DashboardControl::Capture).into(),
        if view.capture.running {
            ControlIcon::Square
        } else {
            ControlIcon::Mic
        },
        capture_feedback,
        view.capture.available,
    );
    if capture_feedback != ControlVisual::Idle {
        canvas.text(
            if view.capture.running {
                &view.labels.capture_stop
            } else {
                &view.labels.capture_start
            },
            Rect::new(1076, 26, 1258, 102),
            20,
            TEXT_SECONDARY,
            Align::Left,
        )?;
    }

    for (control, page, label) in [
        (
            DashboardControl::DisplayTab,
            DashboardPage::Display,
            &view.labels.display_tab,
        ),
        (
            DashboardControl::LanguageTab,
            DashboardPage::Language,
            &view.labels.language_tab,
        ),
        (
            DashboardControl::OscTab,
            DashboardPage::Osc,
            &view.labels.osc_tab,
        ),
    ] {
        if control == DashboardControl::OscTab && !view.osc_available {
            continue;
        }
        let feedback = visual(state, control);
        canvas.selection_button(
            control_rect(control).into(),
            label,
            feedback,
            state.page() == page,
        )?;
    }
    canvas.outlined_control(
        control_rect(DashboardControl::Master).into(),
        visual(state, DashboardControl::Master),
    );
    canvas.heading(&view.labels.master, Rect::new(1076, 140, 1258, 196), 20)?;
    canvas.toggle(
        Rect::new(1278, 144, 1362, 192),
        view.enabled,
        visual(state, DashboardControl::Master),
    );

    match state.page() {
        DashboardPage::Display => render_display(&mut canvas, view, state)?,
        DashboardPage::Language => render_language(&mut canvas, view, state)?,
        DashboardPage::Osc => render_osc(&mut canvas, view, state)?,
    }
    if state.picker().is_some() {
        render_picker(&mut canvas, view, state)?;
    }
    if let Some((action, label)) = state.confirmation() {
        render_confirmation(&mut canvas, view, state, action, label)?;
    }
    if let Some(error) = view.error.as_deref() {
        canvas.rounded(
            Rect::new(48, footer_top, 1392, footer_top + 32),
            10,
            ERROR_SOFT,
        );
        canvas.text(
            error,
            Rect::new(64, footer_top, 1376, footer_top + 32),
            18,
            ERROR,
            Align::Left,
        )?;
    }
    if view.error.is_none()
        && matches!(
            view.save_state,
            DashboardSaveState::Saving | DashboardSaveState::Saved
        )
    {
        let label = if view.save_state == DashboardSaveState::Saving {
            &view.labels.saving
        } else {
            &view.labels.saved
        };
        canvas.text(
            label,
            Rect::new(48, footer_top, 1392, footer_top + 32),
            20,
            TEXT_SECONDARY,
            Align::Left,
        )?;
    }
    Ok(canvas.into_texture())
}

fn render_display(
    canvas: &mut Canvas,
    view: &DashboardViewModel,
    state: &DashboardState,
) -> Result<(), String> {
    if let Some(kind) = state.display_editor() {
        return render_display_position(canvas, view, state, kind);
    }
    canvas.card(Rect::new(48, 236, 1392, 320));
    canvas.heading(&view.labels.content, Rect::new(78, 242, 540, 278), 24)?;
    canvas.text(
        &view.labels.content_hint,
        Rect::new(78, 278, 540, 310),
        20,
        TEXT_SECONDARY,
        Align::Left,
    )?;
    render_segments(
        canvas,
        state,
        &view.content,
        DashboardControl::DisplayContent,
    )?;

    for (
        kind,
        left,
        title,
        enabled,
        width,
        opacity,
        preview_visible,
        down,
        up,
        opacity_down,
        opacity_up,
        toggle,
        position,
        preview,
    ) in [
        (
            DisplayKind::Headset,
            48,
            &view.labels.headset,
            view.headset.enabled,
            &view.headset.width,
            &view.headset.opacity,
            view.headset.preview_visible,
            DashboardControl::HeadsetWidthDown,
            DashboardControl::HeadsetWidthUp,
            DashboardControl::HeadsetOpacityDown,
            DashboardControl::HeadsetOpacityUp,
            DashboardControl::HeadsetEnabled,
            DashboardControl::HeadsetPosition,
            DashboardControl::HeadsetPreview,
        ),
        (
            DisplayKind::Wrist,
            732,
            &view.labels.wrist,
            view.wrist.enabled,
            &view.wrist.width,
            &view.wrist.opacity,
            view.wrist.preview_visible,
            DashboardControl::WristWidthDown,
            DashboardControl::WristWidthUp,
            DashboardControl::WristOpacityDown,
            DashboardControl::WristOpacityUp,
            DashboardControl::WristEnabled,
            DashboardControl::WristPosition,
            DashboardControl::WristPreview,
        ),
    ] {
        canvas.card(Rect::new(left, 336, left + 660, 678));
        canvas.section_title(
            title,
            Rect::new(left + 30, 348, left + 530, 404),
            if kind == DisplayKind::Headset {
                SectionIcon::Headset
            } else {
                SectionIcon::Wrist
            },
        )?;
        canvas.toggle(
            Rect::new(left + 552, 352, left + 630, 400),
            enabled,
            visual(state, toggle),
        );
        canvas.divider(Rect::new(left + 30, 406, left + 630, 407));
        if kind == DisplayKind::Wrist {
            canvas.text(
                &view.labels.hand,
                Rect::new(762, 416, 910, 472),
                22,
                TEXT_SECONDARY,
                Align::Left,
            )?;
            render_segments(canvas, state, &view.wrist.hand, DashboardControl::WristHand)?;
        } else {
            canvas.text(
                &view.labels.headset_hint,
                Rect::new(78, 416, 678, 472),
                22,
                TEXT_SECONDARY,
                Align::Left,
            )?;
        }
        canvas.stepper_field(
            &view.labels.width,
            width,
            Rect::new(left + 30, 478, left + 318, 578),
            state,
            down,
            up,
        )?;
        canvas.stepper_field(
            &view.labels.opacity,
            opacity,
            Rect::new(left + 342, 478, left + 630, 578),
            state,
            opacity_down,
            opacity_up,
        )?;
        canvas.label_icon_button(
            control_rect(position).into(),
            &format!("{} · {}", view.labels.position, view.labels.rotation),
            (Some(ControlIcon::SlidersHorizontal), None),
            visual(state, position),
            state.control_enabled(position),
        )?;
        canvas.selection_button(
            control_rect(preview).into(),
            if preview_visible {
                &view.labels.hide_preview
            } else {
                &view.labels.preview
            },
            visual(state, preview),
            preview_visible,
        )?;
    }
    if !view.ocr_available && !view.vision_available {
        return Ok(());
    }
    canvas.card(Rect::new(48, 696, 1392, 824));
    canvas.heading(&view.labels.tools, Rect::new(78, 704, 800, 740), 24)?;
    for (left, label, checked, control, visible) in [
        (
            78,
            &view.labels.ocr,
            view.ocr.enabled,
            DashboardControl::OcrEnabled,
            view.ocr_available,
        ),
        (
            418,
            &view.labels.gesture,
            view.ocr.gesture,
            DashboardControl::OcrGesture,
            view.ocr_available,
        ),
        (
            766,
            &view.labels.vision,
            view.vision.enabled,
            DashboardControl::VisionEnabled,
            view.vision_available,
        ),
    ] {
        if !visible {
            continue;
        }
        canvas.text(
            label,
            Rect::new(left, 750, left + 208, 802),
            22,
            TEXT,
            Align::Left,
        )?;
        let rect = control_rect(control);
        canvas.toggle_enabled(
            Rect::new(rect.left as i32 + 8, 752, rect.right as i32 - 10, 800),
            checked,
            visual(state, control),
            state.control_enabled(control),
        );
    }
    canvas.label_icon_button(
        control_rect(DashboardControl::OcrBindings).into(),
        &view.labels.bindings,
        (None, None),
        visual(state, DashboardControl::OcrBindings),
        state.control_enabled(DashboardControl::OcrBindings),
    )?;
    Ok(())
}

fn render_segments(
    canvas: &mut Canvas,
    state: &DashboardState,
    choice: &super::dashboard::DashboardChoice,
    control: fn(usize) -> DashboardControl,
) -> Result<(), String> {
    for (index, option) in choice.options.iter().enumerate() {
        let control = control(index);
        canvas.selection_button(
            control_rect(control).into(),
            &option.label,
            visual(state, control),
            option.label == choice.value,
        )?;
    }
    Ok(())
}

fn render_confirmation(
    canvas: &mut Canvas,
    view: &DashboardViewModel,
    state: &DashboardState,
    action: &DashboardAction,
    label: &str,
) -> Result<(), String> {
    canvas.rounded(Rect::new(48, 124, 1392, 852), 12, SURFACE_SUBTLE);
    canvas.card(Rect::new(344, 350, 1096, 704));
    canvas.heading(
        &view.labels.confirm_delete,
        Rect::new(408, 388, 1032, 436),
        28,
    )?;
    let context = match action {
        DashboardAction::DeleteTarget { group, index } => format!(
            "{} / {} {}",
            match group {
                TargetGroup::Speaker => &view.labels.speaker_language,
                TargetGroup::Microphone => &view.labels.microphone_language,
            },
            view.labels.translation_languages,
            index + 1
        ),
        DashboardAction::DeleteLanguagePreset(_) => view.labels.presets.clone(),
        _ => String::new(),
    };
    canvas.text(
        &context,
        Rect::new(408, 436, 1032, 466),
        18,
        TEXT_SECONDARY,
        Align::Left,
    )?;
    canvas.text(label, Rect::new(408, 466, 1032, 508), 28, TEXT, Align::Left)?;
    canvas.text(
        &view.labels.delete_hint,
        Rect::new(408, 514, 1032, 554),
        22,
        TEXT_SECONDARY,
        Align::Left,
    )?;
    canvas.button(
        control_rect(DashboardControl::CancelDelete).into(),
        &view.labels.cancel,
        visual(state, DashboardControl::CancelDelete),
    )?;
    let rect: Rect = control_rect(DashboardControl::ConfirmDelete).into();
    canvas.rounded(rect, 8, ERROR_SOFT);
    if visual(state, DashboardControl::ConfirmDelete) != ControlVisual::Idle {
        canvas.rounded(rect, 8, ERROR);
    }
    canvas.text(
        &view.labels.delete,
        rect,
        22,
        if visual(state, DashboardControl::ConfirmDelete) == ControlVisual::Idle {
            ERROR
        } else {
            SURFACE
        },
        Align::Center,
    )
}

fn render_display_position(
    canvas: &mut Canvas,
    view: &DashboardViewModel,
    state: &DashboardState,
    kind: DisplayKind,
) -> Result<(), String> {
    let (title, fields) = match kind {
        DisplayKind::Headset => (&view.labels.headset, &view.headset.position),
        DisplayKind::Wrist => (&view.labels.wrist, &view.wrist.position),
    };
    canvas.card(Rect::new(48, 236, 1392, 852));
    canvas.section_title(
        title,
        Rect::new(78, 250, 850, 306),
        if kind == DisplayKind::Headset {
            SectionIcon::Headset
        } else {
            SectionIcon::Wrist
        },
    )?;
    canvas.button(
        control_rect(DashboardControl::PositionClose).into(),
        &view.labels.close,
        visual(state, DashboardControl::PositionClose),
    )?;
    canvas.button(
        control_rect(DashboardControl::PositionPreview).into(),
        if match kind {
            DisplayKind::Headset => view.headset.preview_visible,
            DisplayKind::Wrist => view.wrist.preview_visible,
        } {
            &view.labels.hide_preview
        } else {
            &view.labels.preview
        },
        visual(state, DashboardControl::PositionPreview),
    )?;
    canvas.divider(Rect::new(78, 322, 1362, 323));
    canvas.icon_heading(
        &view.labels.position,
        Rect::new(78, 330, 1362, 364),
        ControlIcon::Move3d,
    )?;
    canvas.icon_heading(
        &view.labels.rotation,
        Rect::new(78, 528, 1362, 562),
        ControlIcon::Rotate3d,
    )?;
    for field in fields {
        canvas.stepper_field(
            &field.label,
            &field.value,
            position_field_rect(field.field).into(),
            state,
            DashboardControl::PositionStep(field.field, -1),
            DashboardControl::PositionStep(field.field, 1),
        )?;
    }
    canvas.divider(Rect::new(78, 740, 1362, 741));
    canvas.centered_icon(
        ControlIcon::Move3d,
        Rect::new(636, 760, 708, 828),
        48,
        PRIMARY_INK,
    );
    canvas.text(
        &view.labels.position_hint,
        Rect::new(744, 752, 1362, 792),
        20,
        TEXT_SECONDARY,
        Align::Left,
    )?;
    canvas.text(
        &view.labels.rotation_hint,
        Rect::new(744, 792, 1362, 832),
        20,
        TEXT_SECONDARY,
        Align::Left,
    )?;
    canvas.label_icon_button(
        control_rect(DashboardControl::PositionReset).into(),
        &view.labels.reset_position,
        (Some(ControlIcon::RotateCcw), None),
        visual(state, DashboardControl::PositionReset),
        state.control_enabled(DashboardControl::PositionReset),
    )
}

fn language_label(view: &DashboardViewModel, control: DashboardControl) -> &str {
    match control {
        DashboardControl::RecognitionLanguage => &view.labels.recognition_language,
        DashboardControl::TranslationMode => &view.labels.translation_mode,
        DashboardControl::TargetLanguage(TargetGroup::Speaker, _) => &view.labels.speaker_language,
        DashboardControl::TargetLanguage(TargetGroup::Microphone, _) => {
            &view.labels.microphone_language
        }
        DashboardControl::PresetApply => &view.labels.apply_preset,
        DashboardControl::PresetDelete => &view.labels.delete,
        _ => "",
    }
}

fn language_button(
    canvas: &mut Canvas,
    state: &DashboardState,
    control: DashboardControl,
    label: &str,
) -> Result<(), String> {
    let rect = control_rect(control).into();
    let icons = match control {
        DashboardControl::TargetAdd(_) => (Some(ControlIcon::Plus), None),
        DashboardControl::PresetApply => (Some(ControlIcon::Play), Some(ControlIcon::ChevronDown)),
        DashboardControl::PresetSave => (Some(ControlIcon::BookmarkPlus), None),
        DashboardControl::PresetDelete => {
            (Some(ControlIcon::Trash), Some(ControlIcon::ChevronDown))
        }
        _ => (None, None),
    };
    canvas.label_icon_button(
        rect,
        label,
        icons,
        visual(state, control),
        state.control_enabled(control),
    )
}

fn language_choice(
    canvas: &mut Canvas,
    state: &DashboardState,
    control: DashboardControl,
    label: &str,
    choice: &super::dashboard::DashboardChoice,
) -> Result<(), String> {
    let rect: Rect = control_rect(control).into();
    if choice.options.is_empty() {
        canvas.rounded(rect, 8, SURFACE_SUBTLE);
        canvas.text(
            label,
            Rect::new(rect.left + 20, rect.top + 6, rect.right - 20, rect.top + 32),
            18,
            TEXT_SECONDARY,
            Align::Left,
        )?;
        canvas.text(
            &choice.value,
            Rect::new(
                rect.left + 20,
                rect.top + 32,
                rect.right - 20,
                rect.bottom - 6,
            ),
            22,
            TEXT_SECONDARY,
            Align::Left,
        )
    } else {
        canvas.choice_field(
            label,
            &choice.value,
            rect,
            visual(state, control),
            ControlIcon::ChevronDown,
        )
    }
}

fn render_language(
    canvas: &mut Canvas,
    view: &DashboardViewModel,
    state: &DashboardState,
) -> Result<(), String> {
    canvas.card(Rect::new(48, 236, 708, 350));
    canvas.card(Rect::new(732, 236, 1392, 350));
    language_choice(
        canvas,
        state,
        DashboardControl::RecognitionLanguage,
        &view.labels.recognition_language,
        &view.language.recognition,
    )?;
    language_choice(
        canvas,
        state,
        DashboardControl::TranslationMode,
        &view.labels.translation_mode,
        &view.language.mode,
    )?;
    for (group, targets, title, left) in [
        (
            TargetGroup::Microphone,
            &view.language.microphone_targets,
            &view.labels.microphone_language,
            48,
        ),
        (
            TargetGroup::Speaker,
            &view.language.speaker_targets,
            &view.labels.speaker_language,
            732,
        ),
    ] {
        canvas.card(Rect::new(left, 366, left + 660, 742));
        canvas.heading(title, Rect::new(left + 30, 376, left + 560, 416), 24)?;
        canvas.text(
            &format!("{}/3", targets.len()),
            Rect::new(left + 560, 376, left + 630, 416),
            20,
            TEXT_SECONDARY,
            Align::Center,
        )?;
        for (index, choice) in targets.iter().enumerate().take(3) {
            language_choice(
                canvas,
                state,
                DashboardControl::TargetLanguage(group, index),
                &format!("{} {}", view.labels.translation_languages, index + 1),
                choice,
            )?;
            for (control, icon) in [
                (
                    DashboardControl::TargetUp(group, index),
                    ControlIcon::ArrowUp,
                ),
                (
                    DashboardControl::TargetDown(group, index),
                    ControlIcon::ArrowDown,
                ),
                (
                    DashboardControl::TargetDelete(group, index),
                    ControlIcon::Trash,
                ),
            ] {
                canvas.icon_button(
                    control_rect(control).into(),
                    icon,
                    visual(state, control),
                    state.control_enabled(control),
                );
            }
        }
        language_button(
            canvas,
            state,
            DashboardControl::TargetAdd(group),
            &view.labels.add_target,
        )?;
    }
    canvas.card(Rect::new(48, 756, 1392, 852));
    canvas.heading(&view.labels.presets, Rect::new(78, 768, 286, 800), 22)?;
    canvas.text(
        &format!("{}/5", view.language.presets.options.len()),
        Rect::new(78, 804, 286, 836),
        18,
        TEXT_SECONDARY,
        Align::Left,
    )?;
    language_button(
        canvas,
        state,
        DashboardControl::PresetApply,
        &view.labels.apply_preset,
    )?;
    language_button(
        canvas,
        state,
        DashboardControl::PresetSave,
        &view.labels.save_preset,
    )?;
    language_button(
        canvas,
        state,
        DashboardControl::PresetDelete,
        &view.labels.delete,
    )
}

fn render_osc(
    canvas: &mut Canvas,
    view: &DashboardViewModel,
    state: &DashboardState,
) -> Result<(), String> {
    canvas.card(Rect::new(48, 236, 1392, 752));
    canvas.text(
        &view.osc.endpoint,
        Rect::new(78, 244, 1362, 274),
        20,
        TEXT_SECONDARY,
        Align::Left,
    )?;
    for (control, label, checked) in [
        (
            DashboardControl::OscEnabled,
            &view.labels.osc_enabled,
            view.osc.enabled,
        ),
        (
            DashboardControl::OscOriginal,
            &view.labels.osc_original,
            view.osc.original,
        ),
        (
            DashboardControl::OscMuteSync,
            &view.labels.osc_mute_sync,
            view.osc.mute_sync,
        ),
    ] {
        let rect: Rect = control_rect(control).into();
        let feedback = visual(state, control);
        if feedback != ControlVisual::Idle {
            canvas.rounded(
                rect,
                8,
                if feedback == ControlVisual::Pressed {
                    PRIMARY_SOFT
                } else {
                    PRIMARY_SOFTER
                },
            );
        }
        canvas.text(
            label,
            Rect::new(rect.left + 20, rect.top, rect.right - 124, rect.bottom),
            24,
            TEXT,
            Align::Left,
        )?;
        canvas.toggle(
            Rect::new(1278, rect.top + 16, 1362, rect.top + 64),
            checked,
            feedback,
        );
        if control != DashboardControl::OscMuteSync {
            canvas.divider(Rect::new(78, rect.bottom + 9, 1362, rect.bottom + 10));
        }
    }
    canvas.heading(&view.labels.osc_strategy, Rect::new(78, 584, 1362, 620), 24)?;
    render_segments(
        canvas,
        state,
        &view.osc.strategy,
        DashboardControl::OscStrategy,
    )?;
    canvas.text(
        &view.labels.osc_hint,
        Rect::new(78, 696, 1362, 736),
        18,
        TEXT_SECONDARY,
        Align::Left,
    )
}

fn render_picker(
    canvas: &mut Canvas,
    view: &DashboardViewModel,
    state: &DashboardState,
) -> Result<(), String> {
    let Some(choice) = state.picker_choice() else {
        return Ok(());
    };
    canvas.card(Rect::new(48, 236, 1392, 852));
    canvas.heading(
        language_label(view, state.picker().unwrap()),
        Rect::new(78, 250, 676, 306),
        26,
    )?;
    canvas.button(
        PICKER_CLOSE_RECT.into(),
        &view.labels.close,
        visual(state, DashboardControl::PickerClose),
    )?;
    canvas.text(
        &format!(
            "{} / {}",
            state.picker_page() + 1,
            choice.options.len().div_ceil(PICKER_PAGE_SIZE).max(1)
        ),
        Rect::new(690, 250, 844, 306),
        22,
        TEXT_SECONDARY,
        Align::Center,
    )?;
    let start = state.picker_page() * PICKER_PAGE_SIZE;
    if state.picker_page() > 0 {
        canvas.icon_button(
            PICKER_PREVIOUS_RECT.into(),
            ControlIcon::ChevronLeft,
            visual(state, DashboardControl::PickerPrevious),
            true,
        );
    }
    if start + PICKER_PAGE_SIZE < choice.options.len() {
        canvas.icon_button(
            PICKER_NEXT_RECT.into(),
            ControlIcon::ChevronRight,
            visual(state, DashboardControl::PickerNext),
            true,
        );
    }
    for (index, option) in choice
        .options
        .iter()
        .enumerate()
        .skip(start)
        .take(PICKER_PAGE_SIZE)
    {
        let feedback = visual(state, DashboardControl::PickerOption(index));
        let selected = option.label == choice.value;
        canvas.selection_button(
            picker_option_rect(index).into(),
            &option.label,
            feedback,
            selected,
        )?;
    }
    Ok(())
}

pub fn render_thumbnail(_title: &str) -> Result<Texture, String> {
    let mut canvas = Canvas::new(256, 256, PRIMARY_SOFTER);
    canvas.icon(Rect::new(26, 26, 230, 230))?;
    Ok(canvas.into_texture().into_rgba())
}

fn visual(state: &DashboardState, control: DashboardControl) -> ControlVisual {
    if state.pressed() == Some(control) {
        ControlVisual::Pressed
    } else if state.hovered() == Some(control) {
        ControlVisual::Hovered
    } else {
        ControlVisual::Idle
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlVisual {
    Idle,
    Hovered,
    Pressed,
}

#[derive(Clone, Copy)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl Rect {
    const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

impl From<super::dashboard::Rect> for Rect {
    fn from(rect: super::dashboard::Rect) -> Self {
        Self::new(
            rect.left as i32,
            rect.top as i32,
            rect.right as i32,
            rect.bottom as i32,
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Align {
    Left,
    Center,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TextStyle {
    size: i32,
    align: Align,
    strong: bool,
}

enum SectionIcon {
    Headset,
    Wrist,
}

#[derive(Clone, Copy)]
enum ControlIcon {
    ArrowUp,
    ArrowDown,
    Trash,
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    Plus,
    Minus,
    Play,
    Mic,
    Square,
    BookmarkPlus,
    SlidersHorizontal,
    Move3d,
    Rotate3d,
    RotateCcw,
}

// Lucide icon geometry (Mic, Square, SlidersHorizontal, Move3d, Rotate3d, RotateCcw):
// ISC License
// Copyright (c) 2026 Lucide Icons and Contributors
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
impl ControlIcon {
    // Lucide-style 24-unit line icons, independent of the localized font.
    // Spatial icons follow the desktop Lucide geometry; arcs are sampled for the native rasterizer.
    fn paths(self) -> &'static [&'static [(f32, f32)]] {
        match self {
            Self::ArrowUp => &[
                &[(12., 19.), (12., 5.)],
                &[(5., 12.), (12., 5.), (19., 12.)],
            ],
            Self::ArrowDown => &[
                &[(12., 5.), (12., 19.)],
                &[(5., 12.), (12., 19.), (19., 12.)],
            ],
            Self::Trash => &[
                &[(3., 6.), (21., 6.)],
                &[
                    (5., 6.),
                    (5., 20.),
                    (5.6, 21.4),
                    (7., 22.),
                    (17., 22.),
                    (18.4, 21.4),
                    (19., 20.),
                    (19., 6.),
                ],
                &[
                    (9., 6.),
                    (9., 4.),
                    (9.6, 2.6),
                    (11., 2.),
                    (13., 2.),
                    (14.4, 2.6),
                    (15., 4.),
                    (15., 6.),
                ],
                &[(10., 11.), (10., 17.)],
                &[(14., 11.), (14., 17.)],
            ],
            Self::ChevronDown => &[&[(6., 9.), (12., 15.), (18., 9.)]],
            Self::ChevronLeft => &[&[(15., 18.), (9., 12.), (15., 6.)]],
            Self::ChevronRight => &[&[(9., 18.), (15., 12.), (9., 6.)]],
            Self::Plus => &[&[(12., 5.), (12., 19.)], &[(5., 12.), (19., 12.)]],
            Self::Minus => &[&[(5., 12.), (19., 12.)]],
            Self::Play => &[&[(6., 3.), (20., 12.), (6., 21.), (6., 3.)]],
            Self::Mic => &[
                &[(12., 19.), (12., 22.)],
                &[
                    (19., 10.),
                    (19., 12.),
                    (18.467, 14.679),
                    (16.95, 16.95),
                    (14.679, 18.467),
                    (12., 19.),
                    (9.321, 18.467),
                    (7.05, 16.95),
                    (5.533, 14.679),
                    (5., 12.),
                    (5., 10.),
                ],
                &[
                    (9., 5.),
                    (9., 12.),
                    (9.228, 13.148),
                    (9.879, 14.121),
                    (10.852, 14.772),
                    (12., 15.),
                    (13.148, 14.772),
                    (14.121, 14.121),
                    (14.772, 13.148),
                    (15., 12.),
                    (15., 5.),
                    (14.772, 3.852),
                    (14.121, 2.879),
                    (13.148, 2.228),
                    (12., 2.),
                    (10.852, 2.228),
                    (9.879, 2.879),
                    (9.228, 3.852),
                    (9., 5.),
                ],
            ],
            Self::Square => &[&[
                (3., 5.),
                (3., 19.),
                (3.152, 19.765),
                (3.586, 20.414),
                (4.235, 20.848),
                (5., 21.),
                (19., 21.),
                (19.765, 20.848),
                (20.414, 20.414),
                (20.848, 19.765),
                (21., 19.),
                (21., 5.),
                (20.848, 4.235),
                (20.414, 3.586),
                (19.765, 3.152),
                (19., 3.),
                (5., 3.),
                (4.235, 3.152),
                (3.586, 3.586),
                (3.152, 4.235),
                (3., 5.),
            ]],
            Self::BookmarkPlus => &[
                &[
                    (19., 21.),
                    (12., 17.),
                    (5., 21.),
                    (5., 5.),
                    (5.6, 3.6),
                    (7., 3.),
                    (17., 3.),
                    (18.4, 3.6),
                    (19., 5.),
                    (19., 21.),
                ],
                &[(12., 7.), (12., 13.)],
                &[(9., 10.), (15., 10.)],
            ],
            Self::SlidersHorizontal => &[
                &[(10., 5.), (3., 5.)],
                &[(12., 19.), (3., 19.)],
                &[(14., 3.), (14., 7.)],
                &[(16., 17.), (16., 21.)],
                &[(21., 12.), (12., 12.)],
                &[(21., 19.), (16., 19.)],
                &[(21., 5.), (14., 5.)],
                &[(8., 10.), (8., 14.)],
                &[(8., 12.), (3., 12.)],
            ],
            Self::Move3d => &[
                &[(5., 3.), (5., 19.), (21., 19.)],
                &[(5., 19.), (11., 13.)],
                &[(2., 6.), (5., 3.), (8., 6.)],
                &[(18., 16.), (21., 19.), (18., 22.)],
            ],
            Self::Rotate3d => &[
                &[(15.194, 13.707), (19.008, 15.567), (17.148, 19.381)],
                &[
                    (16.472, 7.528),
                    (16.119, 6.331),
                    (15.686, 5.244),
                    (15.183, 4.287),
                    (14.617, 3.479),
                    (14.001, 2.836),
                    (13.347, 2.370),
                    (12.666, 2.089),
                    (11.973, 2.000),
                    (11.280, 2.104),
                    (10.601, 2.399),
                    (9.949, 2.880),
                    (9.337, 3.536),
                    (8.776, 4.356),
                    (8.278, 5.324),
                    (7.851, 6.420),
                    (7.504, 7.624),
                    (7.244, 8.913),
                    (7.076, 10.261),
                    (7.003, 11.643),
                    (7.027, 13.032),
                    (7.146, 14.401),
                    (7.359, 15.723),
                    (7.662, 16.974),
                    (8.049, 18.128),
                    (8.512, 19.164),
                    (9.042, 20.062),
                    (9.629, 20.805),
                    (10.262, 21.377),
                    (10.929, 21.768),
                    (11.617, 21.971),
                    (12.311, 21.981),
                    (13.000, 21.798),
                ],
                &[
                    (21.798, 11.000),
                    (21.426, 10.330),
                    (20.872, 9.693),
                    (20.146, 9.100),
                    (19.263, 8.563),
                    (18.240, 8.093),
                    (17.096, 7.698),
                    (15.854, 7.386),
                    (14.537, 7.164),
                    (13.171, 7.034),
                    (11.783, 7.001),
                    (10.398, 7.065),
                    (9.045, 7.223),
                    (7.749, 7.474),
                    (6.535, 7.813),
                    (5.426, 8.232),
                    (4.445, 8.724),
                    (3.609, 9.280),
                    (2.936, 9.888),
                    (2.438, 10.537),
                    (2.124, 11.214),
                    (2.002, 11.906),
                    (2.072, 12.600),
                    (2.335, 13.283),
                    (2.784, 13.940),
                    (3.411, 14.561),
                    (4.204, 15.131),
                    (5.148, 15.642),
                    (6.224, 16.082),
                    (7.412, 16.443),
                    (8.688, 16.718),
                    (10.029, 16.902),
                    (11.407, 16.991),
                    (12.797, 16.984),
                    (14.172, 16.881),
                    (15.504, 16.683),
                    (16.769, 16.395),
                    (17.942, 16.022),
                    (19.000, 15.571),
                ],
            ],
            Self::RotateCcw => &[
                &[
                    (3.000, 12.000),
                    (3.086, 13.243),
                    (3.344, 14.463),
                    (3.767, 15.635),
                    (4.348, 16.738),
                    (5.076, 17.750),
                    (5.937, 18.651),
                    (6.914, 19.425),
                    (7.988, 20.056),
                    (9.140, 20.533),
                    (10.346, 20.847),
                    (11.584, 20.990),
                    (12.830, 20.962),
                    (14.061, 20.761),
                    (15.251, 20.392),
                    (16.379, 19.863),
                    (17.424, 19.182),
                    (18.364, 18.364),
                    (19.182, 17.424),
                    (19.863, 16.379),
                    (20.392, 15.251),
                    (20.761, 14.061),
                    (20.962, 12.830),
                    (20.990, 11.584),
                    (20.847, 10.346),
                    (20.533, 9.140),
                    (20.056, 7.988),
                    (19.425, 6.914),
                    (18.651, 5.937),
                    (17.750, 5.076),
                    (16.738, 4.348),
                    (15.635, 3.767),
                    (14.463, 3.344),
                    (13.243, 3.086),
                    (12.000, 3.000),
                    (10.761, 3.084),
                    (9.543, 3.324),
                    (8.365, 3.718),
                    (7.247, 4.258),
                    (6.206, 4.935),
                    (5.260, 5.740),
                    (3.000, 8.000),
                ],
                &[(3., 3.), (3., 8.), (8., 8.)],
            ],
        }
    }
}

struct Canvas<'a> {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    font_face: &'static str,
    cache: Option<&'a mut RasterCache>,
}

impl Canvas<'_> {
    fn into_texture(self) -> BgraTexture {
        BgraTexture {
            pixels: self.pixels,
            width: self.width,
            height: self.height,
        }
    }

    fn new(width: u32, height: u32, color: [u8; 4]) -> Self {
        let mut pixels = vec![0; (width * height * 4) as usize];
        for pixel in pixels.as_chunks_mut::<4>().0 {
            *pixel = color;
        }
        Self {
            pixels,
            width,
            height,
            font_face: "Segoe UI\0",
            cache: None,
        }
    }

    fn card(&mut self, rect: Rect) {
        self.rounded(rect, 12, BORDER);
        self.rounded(
            Rect::new(rect.left + 1, rect.top + 1, rect.right - 1, rect.bottom - 1),
            11,
            SURFACE,
        );
    }

    fn divider(&mut self, rect: Rect) {
        self.rounded(rect, 0, BORDER);
    }

    fn icon_heading(&mut self, text: &str, rect: Rect, icon: ControlIcon) -> Result<(), String> {
        self.centered_icon(
            icon,
            Rect::new(rect.left, rect.top, rect.left + 24, rect.bottom),
            24,
            PRIMARY_INK,
        );
        self.heading(
            text,
            Rect::new(rect.left + 36, rect.top, rect.right, rect.bottom),
            24,
        )
    }

    fn section_title(&mut self, text: &str, rect: Rect, icon: SectionIcon) -> Result<(), String> {
        let x = rect.left;
        let y = rect.top + (rect.bottom - rect.top - 48) / 2;
        self.rounded(Rect::new(x, y, x + 48, y + 48), 12, PRIMARY_SOFTER);
        match icon {
            SectionIcon::Headset => {
                self.rounded(Rect::new(x + 8, y + 14, x + 40, y + 34), 7, PRIMARY_INK);
                self.rounded(Rect::new(x + 11, y + 17, x + 37, y + 31), 4, PRIMARY_SOFTER);
                self.rounded(Rect::new(x + 21, y + 26, x + 27, y + 35), 3, PRIMARY_INK);
            }
            SectionIcon::Wrist => {
                self.rounded(Rect::new(x + 18, y + 6, x + 30, y + 42), 3, PRIMARY_INK);
                self.rounded(Rect::new(x + 13, y + 13, x + 35, y + 35), 6, PRIMARY_INK);
                self.rounded(Rect::new(x + 16, y + 16, x + 32, y + 32), 3, PRIMARY_SOFTER);
            }
        }
        self.heading(
            text,
            Rect::new(x + 64, rect.top, rect.right, rect.bottom),
            26,
        )
    }

    fn pill(&mut self, rect: Rect, text: &str, error: bool) -> Result<(), String> {
        self.rounded(rect, 22, if error { ERROR_SOFT } else { PRIMARY_SOFT });
        self.text(
            text,
            rect,
            20,
            if error { ERROR } else { PRIMARY_INK },
            Align::Center,
        )
    }

    fn choice_field(
        &mut self,
        label: &str,
        value: &str,
        rect: Rect,
        visual: ControlVisual,
        marker: ControlIcon,
    ) -> Result<(), String> {
        self.outlined_control(rect, visual);
        self.text(
            label,
            Rect::new(rect.left + 20, rect.top + 6, rect.right - 52, rect.top + 32),
            18,
            TEXT_SECONDARY,
            Align::Left,
        )?;
        self.control_icon(
            marker,
            Rect::new(
                rect.right - 42,
                rect.top + 9,
                rect.right - 22,
                rect.top + 29,
            ),
            PRIMARY_INK,
        );
        self.text(
            value,
            Rect::new(
                rect.left + 20,
                rect.top + 32,
                rect.right - 20,
                rect.bottom - 6,
            ),
            22,
            TEXT,
            Align::Left,
        )
    }

    fn stepper_field(
        &mut self,
        label: &str,
        value: &str,
        rect: Rect,
        state: &DashboardState,
        down: DashboardControl,
        up: DashboardControl,
    ) -> Result<(), String> {
        self.text(
            label,
            Rect::new(rect.left, rect.top, rect.right, rect.top + 34),
            20,
            TEXT_SECONDARY,
            Align::Left,
        )?;
        let control = Rect::new(rect.left, rect.top + 44, rect.right, rect.bottom);
        self.rounded(control, 10, BORDER);
        self.rounded(
            Rect::new(
                control.left + 1,
                control.top + 1,
                control.right - 1,
                control.bottom - 1,
            ),
            9,
            SURFACE_ELEVATED,
        );
        self.small_button(
            Rect::new(rect.left, rect.top + 44, rect.left + 62, rect.bottom),
            ControlIcon::Minus,
            visual(state, down),
            state.control_enabled(down),
        );
        self.text(
            value,
            Rect::new(rect.left + 62, rect.top + 44, rect.right - 62, rect.bottom),
            24,
            TEXT,
            Align::Center,
        )?;
        self.small_button(
            Rect::new(rect.right - 62, rect.top + 44, rect.right, rect.bottom),
            ControlIcon::Plus,
            visual(state, up),
            state.control_enabled(up),
        );
        Ok(())
    }

    fn button(&mut self, rect: Rect, text: &str, visual: ControlVisual) -> Result<(), String> {
        let rect = self.button_surface(rect, visual);
        self.text(text, rect, 22, PRIMARY_INK, Align::Center)
    }

    fn button_surface(&mut self, rect: Rect, visual: ControlVisual) -> Rect {
        let rect = if visual == ControlVisual::Pressed {
            Rect::new(rect.left + 2, rect.top + 2, rect.right - 2, rect.bottom - 2)
        } else {
            rect
        };
        self.rounded(
            rect,
            8,
            if visual == ControlVisual::Idle {
                BORDER
            } else {
                PRIMARY
            },
        );
        let inset = if visual == ControlVisual::Pressed {
            2
        } else {
            1
        };
        self.rounded(
            Rect::new(
                rect.left + inset,
                rect.top + inset,
                rect.right - inset,
                rect.bottom - inset,
            ),
            7,
            match visual {
                ControlVisual::Idle => SURFACE,
                ControlVisual::Hovered => PRIMARY_SOFT,
                ControlVisual::Pressed => PRIMARY,
            },
        );
        rect
    }

    fn icon_button(&mut self, rect: Rect, icon: ControlIcon, visual: ControlVisual, enabled: bool) {
        let (rect, color) = if enabled {
            (self.button_surface(rect, visual), PRIMARY_INK)
        } else {
            self.rounded(rect, 8, SURFACE_SUBTLE);
            (rect, BORDER_STRONG)
        };
        let size = if visual == ControlVisual::Pressed && enabled {
            26
        } else {
            28
        };
        self.centered_icon(icon, rect, size, color);
    }

    fn primary_icon_button(
        &mut self,
        mut rect: Rect,
        icon: ControlIcon,
        visual: ControlVisual,
        enabled: bool,
    ) {
        let mut fill = if enabled && visual != ControlVisual::Idle {
            PRIMARY_HOVER
        } else {
            PRIMARY
        };
        let mut ink = SURFACE;
        if !enabled {
            for channel in 0..3 {
                fill[channel] = ((fill[channel] as u16 + CANVAS[channel] as u16) / 2) as u8;
                ink[channel] = ((ink[channel] as u16 + CANVAS[channel] as u16) / 2) as u8;
            }
        }
        let pressed = enabled && visual == ControlVisual::Pressed;
        if pressed {
            rect = Rect::new(rect.left + 2, rect.top + 2, rect.right - 2, rect.bottom - 2);
        }
        self.rounded(rect, (rect.bottom - rect.top) / 2, fill);
        self.centered_icon(icon, rect, if pressed { 36 } else { 38 }, ink);
    }

    fn label_icon_button(
        &mut self,
        rect: Rect,
        label: &str,
        icons: (Option<ControlIcon>, Option<ControlIcon>),
        visual: ControlVisual,
        enabled: bool,
    ) -> Result<(), String> {
        let (rect, color) = if enabled {
            (self.button_surface(rect, visual), PRIMARY_INK)
        } else {
            self.rounded(rect, 8, SURFACE_SUBTLE);
            (rect, TEXT_SECONDARY)
        };
        let mut text_rect = rect;
        if let Some(icon) = icons.0 {
            self.centered_icon(
                icon,
                Rect::new(rect.left + 16, rect.top, rect.left + 44, rect.bottom),
                24,
                color,
            );
            text_rect.left += 54;
        }
        if let Some(icon) = icons.1 {
            self.centered_icon(
                icon,
                Rect::new(rect.right - 40, rect.top, rect.right - 16, rect.bottom),
                20,
                color,
            );
            text_rect.right -= 48;
        } else {
            text_rect.right -= 16;
        }
        self.text(label, text_rect, 22, color, Align::Center)
    }

    fn selection_button(
        &mut self,
        rect: Rect,
        label: &str,
        visual: ControlVisual,
        selected: bool,
    ) -> Result<(), String> {
        if selected {
            self.rounded(rect, 8, PRIMARY_INK);
            if visual == ControlVisual::Hovered {
                self.rounded(rect, 8, TEXT);
            }
            self.text(
                label,
                Rect::new(rect.left + 12, rect.top, rect.right - 12, rect.bottom),
                22,
                SURFACE,
                Align::Center,
            )
        } else {
            self.button(rect, label, visual)
        }
    }

    fn toggle_enabled(&mut self, rect: Rect, checked: bool, visual: ControlVisual, enabled: bool) {
        if enabled {
            self.toggle(rect, checked, visual);
        } else {
            self.rounded(rect, (rect.bottom - rect.top) / 2, SURFACE_SUBTLE);
            let diameter = rect.bottom - rect.top - 10;
            let left = if checked {
                rect.right - diameter - 5
            } else {
                rect.left + 5
            };
            self.rounded(
                Rect::new(left, rect.top + 5, left + diameter, rect.bottom - 5),
                diameter / 2,
                BORDER_STRONG,
            );
        }
    }

    fn small_button(
        &mut self,
        rect: Rect,
        icon: ControlIcon,
        visual: ControlVisual,
        enabled: bool,
    ) {
        self.rounded(
            rect,
            9,
            match if enabled { visual } else { ControlVisual::Idle } {
                ControlVisual::Idle => SURFACE_SUBTLE,
                ControlVisual::Hovered => PRIMARY_SOFT,
                ControlVisual::Pressed => PRIMARY,
            },
        );
        let text_rect = if enabled && visual == ControlVisual::Pressed {
            Rect::new(rect.left, rect.top + 2, rect.right, rect.bottom + 2)
        } else {
            rect
        };
        self.centered_icon(
            icon,
            text_rect,
            24,
            if enabled { PRIMARY_INK } else { BORDER_STRONG },
        );
    }

    fn centered_icon(&mut self, icon: ControlIcon, rect: Rect, size: i32, color: [u8; 4]) {
        let left = (rect.left + rect.right - size) / 2;
        let top = (rect.top + rect.bottom - size) / 2;
        self.control_icon(icon, Rect::new(left, top, left + size, top + size), color);
    }

    fn control_icon(&mut self, icon: ControlIcon, rect: Rect, color: [u8; 4]) {
        let scale = (rect.right - rect.left).min(rect.bottom - rect.top) as f32 / 24.;
        if scale <= 0. {
            return;
        }
        let radius = scale
            * if matches!(icon, ControlIcon::Mic | ControlIcon::Square) {
                0.9
            } else {
                1.0
            };
        let paths = icon.paths();
        for y in rect.top.max(0)..rect.bottom.min(self.height as i32) {
            for x in rect.left.max(0)..rect.right.min(self.width as i32) {
                let px = (x - rect.left) as f32 + 0.5;
                let py = (y - rect.top) as f32 + 0.5;
                let mut distance = f32::INFINITY;
                for path in paths {
                    for segment in path.windows(2) {
                        let (ax, ay) = (segment[0].0 * scale, segment[0].1 * scale);
                        let (bx, by) = (segment[1].0 * scale, segment[1].1 * scale);
                        let (dx, dy) = (bx - ax, by - ay);
                        let t =
                            (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0., 1.);
                        distance = distance.min((px - ax - t * dx).hypot(py - ay - t * dy));
                    }
                }
                // One coverage value for the whole path keeps joins and caps even.
                let alpha = ((radius + 0.5 - distance).clamp(0., 1.) * 255.).round() as u16;
                if alpha == 0 {
                    continue;
                }
                let offset = ((y as u32 * self.width + x as u32) * 4) as usize;
                for (channel, value) in color.iter().take(3).enumerate() {
                    let background = self.pixels[offset + channel] as u16;
                    self.pixels[offset + channel] =
                        ((*value as u16 * alpha + background * (255 - alpha)) / 255) as u8;
                }
            }
        }
    }

    fn toggle(&mut self, rect: Rect, checked: bool, visual: ControlVisual) {
        self.rounded(
            rect,
            (rect.bottom - rect.top) / 2,
            if checked {
                if visual == ControlVisual::Idle {
                    PRIMARY
                } else {
                    PRIMARY_HOVER
                }
            } else if visual != ControlVisual::Idle {
                TEXT_SECONDARY
            } else {
                BORDER_STRONG
            },
        );
        let diameter = rect.bottom
            - rect.top
            - if visual == ControlVisual::Pressed {
                14
            } else {
                10
            };
        let left = if checked {
            rect.right - diameter - (rect.bottom - rect.top - diameter) / 2
        } else {
            rect.left + (rect.bottom - rect.top - diameter) / 2
        };
        self.rounded(
            Rect::new(
                left,
                rect.top + (rect.bottom - rect.top - diameter) / 2,
                left + diameter,
                rect.bottom - (rect.bottom - rect.top - diameter) / 2,
            ),
            diameter / 2,
            SURFACE,
        );
    }

    fn outlined_control(&mut self, rect: Rect, visual: ControlVisual) {
        self.rounded(
            rect,
            8,
            if visual == ControlVisual::Idle {
                BORDER
            } else {
                PRIMARY
            },
        );
        self.rounded(
            Rect::new(rect.left + 1, rect.top + 1, rect.right - 1, rect.bottom - 1),
            7,
            match visual {
                ControlVisual::Idle => SURFACE,
                ControlVisual::Hovered => PRIMARY_SOFTER,
                ControlVisual::Pressed => PRIMARY_SOFT,
            },
        );
    }

    fn icon(&mut self, rect: Rect) -> Result<(), String> {
        let width = (rect.right - rect.left).max(0) as u32;
        let height = (rect.bottom - rect.top).max(0) as u32;
        let uncached;
        let icon = if let Some(cache) = self.cache.as_deref_mut() {
            cache.icon(width, height)?
        } else {
            uncached = render_app_icon(width, height)?;
            &uncached
        };
        for y in 0..height {
            for x in 0..width {
                let source_offset = ((y * width + x) * 4) as usize;
                let target_offset =
                    ((((rect.top as u32 + y) * self.width) + rect.left as u32 + x) * 4) as usize;
                let source = &icon[source_offset..source_offset + 4];
                let alpha = if source[3] == 0 && source[..3] != [0, 0, 0] {
                    255
                } else {
                    source[3]
                } as u16;
                if alpha == 0 {
                    continue;
                }
                for (channel, value) in source.iter().take(3).enumerate() {
                    self.pixels[target_offset + channel] = (((*value as u16 * alpha)
                        + (self.pixels[target_offset + channel] as u16 * (255 - alpha)))
                        / 255) as u8;
                }
            }
        }
        Ok(())
    }

    fn rounded(&mut self, rect: Rect, radius: i32, color: [u8; 4]) {
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return;
        }
        let radius = radius
            .max(0)
            .min((rect.right - rect.left) / 2)
            .min((rect.bottom - rect.top) / 2);
        let left = rect.left.clamp(0, self.width as i32);
        let top = rect.top.clamp(0, self.height as i32);
        let right = rect.right.clamp(left, self.width as i32);
        let bottom = rect.bottom.clamp(top, self.height as i32);
        for y in top..bottom {
            for x in left..right {
                let coverage = if radius == 0
                    || (x >= rect.left + radius && x < rect.right - radius)
                    || (y >= rect.top + radius && y < rect.bottom - radius)
                {
                    255
                } else {
                    let cx = if x < rect.left + radius {
                        rect.left + radius
                    } else {
                        rect.right - radius
                    } as f32;
                    let cy = if y < rect.top + radius {
                        rect.top + radius
                    } else {
                        rect.bottom - radius
                    } as f32;
                    // Blend a one-pixel edge from the pixel center's distance to the curve.
                    let distance = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy);
                    ((radius as f32 + 0.5 - distance).clamp(0., 1.) * 255.).round() as u16
                };
                if coverage == 0 {
                    continue;
                }
                let offset = ((y as u32 * self.width + x as u32) * 4) as usize;
                if coverage == 255 {
                    self.pixels[offset..offset + 4].copy_from_slice(&color);
                } else {
                    for (channel, value) in color.iter().enumerate() {
                        let background = self.pixels[offset + channel] as u16;
                        self.pixels[offset + channel] = ((*value as u16 * coverage
                            + background * (255 - coverage)
                            + 127)
                            / 255) as u8;
                    }
                }
            }
        }
    }

    fn text(
        &mut self,
        text: &str,
        rect: Rect,
        size: i32,
        color: [u8; 4],
        align: Align,
    ) -> Result<(), String> {
        self.styled_text(
            text,
            rect,
            color,
            TextStyle {
                size,
                align,
                strong: false,
            },
        )
    }

    fn heading(&mut self, text: &str, rect: Rect, size: i32) -> Result<(), String> {
        self.styled_text(
            text,
            rect,
            TEXT,
            TextStyle {
                size,
                align: Align::Left,
                strong: true,
            },
        )
    }

    fn styled_text(
        &mut self,
        text: &str,
        rect: Rect,
        color: [u8; 4],
        style: TextStyle,
    ) -> Result<(), String> {
        if text.is_empty() || rect.right <= rect.left || rect.bottom <= rect.top {
            return Ok(());
        }
        let uncached;
        let mask = if let Some(cache) = self.cache.as_deref_mut() {
            cache.text_mask(text, rect, style, self.font_face)?
        } else {
            let bgra = render_text_mask(text, rect, style, self.font_face)?;
            uncached = bgra
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| pixel[0])
                .collect::<Vec<_>>();
            &uncached
        };
        let mask_width = (rect.right - rect.left) as usize;
        for y in rect.top.max(0)..rect.bottom.min(self.height as i32) {
            for x in rect.left.max(0)..rect.right.min(self.width as i32) {
                let source = (y - rect.top) as usize * mask_width + (x - rect.left) as usize;
                let alpha = mask[source] as u16;
                if alpha == 0 {
                    continue;
                }
                let offset = ((y as u32 * self.width + x as u32) * 4) as usize;
                for (channel, value) in color.iter().take(3).enumerate() {
                    self.pixels[offset + channel] = (((*value as u16 * alpha)
                        + (self.pixels[offset + channel] as u16 * (255 - alpha)))
                        / 255) as u8;
                }
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
fn render_app_icon(width: u32, height: u32) -> Result<Vec<u8>, String> {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::ptr::null_mut;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, DestroyIcon, DrawIconEx, DI_NORMAL, LR_DEFAULTCOLOR,
    };

    const ICON: &[u8] = include_bytes!("../../icons/icon.ico");
    let image = largest_ico_image(ICON)?;
    unsafe {
        let icon = CreateIconFromResourceEx(
            image.as_ptr(),
            image.len() as u32,
            1,
            0x0003_0000,
            width as i32,
            height as i32,
            LR_DEFAULTCOLOR,
        );
        if icon.is_null() {
            return Err(last_error("CreateIconFromResourceEx"));
        }
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            DestroyIcon(icon);
            return Err(last_error("CreateCompatibleDC"));
        }
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..zeroed()
        };
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(dc);
            DestroyIcon(icon);
            return Err(last_error("CreateDIBSection"));
        }
        let old_bitmap = SelectObject(dc, bitmap);
        let drawn = DrawIconEx(
            dc,
            0,
            0,
            icon,
            width as i32,
            height as i32,
            0,
            null_mut(),
            DI_NORMAL,
        );
        let pixels =
            std::slice::from_raw_parts(bits.cast::<u8>(), (width * height * 4) as usize).to_vec();
        SelectObject(dc, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(dc);
        DestroyIcon(icon);
        if drawn == 0 {
            Err(last_error("DrawIconEx"))
        } else {
            Ok(pixels)
        }
    }
}

#[cfg(windows)]
fn largest_ico_image(icon: &[u8]) -> Result<&[u8], String> {
    if icon.len() < 6 || icon[2..4] != [1, 0] {
        return Err("Invalid VRCS icon".into());
    }
    let count = u16::from_le_bytes([icon[4], icon[5]]) as usize;
    let mut best: Option<(u32, usize, usize)> = None;
    for index in 0..count {
        let entry = 6 + index * 16;
        if entry + 16 > icon.len() {
            break;
        }
        let width = if icon[entry] == 0 {
            256
        } else {
            icon[entry] as u32
        };
        let height = if icon[entry + 1] == 0 {
            256
        } else {
            icon[entry + 1] as u32
        };
        let size = u32::from_le_bytes(icon[entry + 8..entry + 12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(icon[entry + 12..entry + 16].try_into().unwrap()) as usize;
        if offset
            .checked_add(size)
            .is_some_and(|end| end <= icon.len())
        {
            let area = width * height;
            if best.is_none_or(|(best_area, _, _)| area > best_area) {
                best = Some((area, offset, size));
            }
        }
    }
    best.map(|(_, offset, size)| &icon[offset..offset + size])
        .ok_or_else(|| "VRCS icon contains no usable image".into())
}

#[cfg(not(windows))]
fn render_app_icon(width: u32, height: u32) -> Result<Vec<u8>, String> {
    Ok(vec![0; (width * height * 4) as usize])
}

#[cfg(windows)]
fn render_text_mask(
    text: &str,
    rect: Rect,
    style: TextStyle,
    font_face: &str,
) -> Result<Vec<u8>, String> {
    let width = (rect.right - rect.left).max(0) as u32;
    let height = (rect.bottom - rect.top).max(0) as u32;
    if width == 0 || height == 0 {
        return Ok(Vec::new());
    }
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
        SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER,
        DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL,
        FW_SEMIBOLD, OUT_DEFAULT_PRECIS, PROOF_QUALITY, TRANSPARENT,
    };

    unsafe {
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return Err(last_error("CreateCompatibleDC"));
        }
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..zeroed()
        };
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(dc);
            return Err(last_error("CreateDIBSection"));
        }
        let old_bitmap = SelectObject(dc, bitmap);
        // Match the desktop UI's CJK font families instead of GDI's legacy fallback.
        let face: Vec<u16> = font_face.encode_utf16().collect();
        let font = CreateFontW(
            -style.size,
            0,
            0,
            0,
            if style.strong { FW_SEMIBOLD } else { FW_NORMAL } as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.into(),
            OUT_DEFAULT_PRECIS.into(),
            CLIP_DEFAULT_PRECIS.into(),
            PROOF_QUALITY.into(),
            (DEFAULT_PITCH | FF_DONTCARE).into(),
            face.as_ptr(),
        );
        if font.is_null() {
            SelectObject(dc, old_bitmap);
            DeleteObject(bitmap);
            DeleteDC(dc);
            return Err(last_error("CreateFontW"));
        }
        let old_font = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x00ff_ffff);
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut target = RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        let alignment = match style.align {
            Align::Left => DT_LEFT,
            Align::Center => DT_CENTER,
        };
        let result = DrawTextW(
            dc,
            wide.as_mut_ptr(),
            wide.len() as i32,
            &mut target,
            alignment | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        let pixels =
            std::slice::from_raw_parts(bits.cast::<u8>(), (width * height * 4) as usize).to_vec();
        SelectObject(dc, old_font);
        SelectObject(dc, old_bitmap);
        DeleteObject(font);
        DeleteObject(bitmap);
        DeleteDC(dc);
        if result == 0 {
            Err(last_error("DrawTextW"))
        } else {
            Ok(pixels)
        }
    }
}

#[cfg(not(windows))]
fn render_text_mask(_: &str, rect: Rect, _: TextStyle, _: &str) -> Result<Vec<u8>, String> {
    let width = (rect.right - rect.left).max(0) as u32;
    let height = (rect.bottom - rect.top).max(0) as u32;
    Ok(vec![0; (width * height * 4) as usize])
}

#[cfg(windows)]
fn last_error(operation: &str) -> String {
    format!(
        "{operation} failed with Windows error {}",
        std::io::Error::last_os_error()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_overlay::dashboard::{
        DashboardHeadset, DashboardLabels, DashboardOcr, DashboardSaveState, DashboardState,
        DashboardViewModel, DashboardWrist, DASHBOARD_HEIGHT, DASHBOARD_WIDTH,
    };

    fn render(view: &DashboardViewModel, state: &DashboardState) -> Result<Texture, String> {
        super::render(view, state, &mut RasterCache::default()).map(BgraTexture::into_rgba)
    }

    #[test]
    fn rounded_edges_have_partial_coverage_and_stay_symmetric() {
        let mut canvas = Canvas::new(64, 48, [255; 4]);
        canvas.rounded(Rect::new(8, 8, 56, 40), 10, [0, 0, 0, 255]);
        let pixels = canvas.pixels.as_chunks::<4>().0;
        assert_eq!(pixels[8 * 64 + 8], [255; 4]);
        assert_eq!(pixels[24 * 64 + 32], [0, 0, 0, 255]);
        assert!(
            pixels.iter().any(|pixel| (1..255).contains(&pixel[0])),
            "Curved edges must include partially covered pixels"
        );
        for y in 0..48 {
            for x in 0..64 {
                let pixel = pixels[y * 64 + x];
                assert_eq!(pixel, pixels[y * 64 + 63 - x]);
                assert_eq!(pixel, pixels[(47 - y) * 64 + x]);
                assert_eq!(pixel[3], 255);
            }
        }
    }

    #[test]
    fn rounded_shapes_preserve_geometry_when_clipped() {
        let mut clipped = Canvas::new(48, 48, CANVAS);
        clipped.rounded(Rect::new(-4, -6, 36, 34), 12, PRIMARY);
        let mut reference = Canvas::new(80, 80, CANVAS);
        reference.rounded(Rect::new(12, 10, 52, 50), 12, PRIMARY);
        for y in 0..48 {
            for x in 0..48 {
                let actual = (y * 48 + x) * 4;
                let expected = ((y + 16) * 80 + x + 16) * 4;
                assert_eq!(
                    clipped.pixels[actual..actual + 4],
                    reference.pixels[expected..expected + 4]
                );
            }
        }
    }

    #[test]
    fn rounded_rectangles_keep_straight_edges_crisp() {
        let mut canvas = Canvas::new(32, 24, CANVAS);
        canvas.rounded(Rect::new(4, 6, 28, 18), 0, PRIMARY);
        let before = canvas.pixels.clone();
        canvas.rounded(Rect::new(8, 8, 8, 20), 4, TEXT);
        canvas.rounded(Rect::new(20, 20, 8, 8), 4, TEXT);
        assert_eq!(canvas.pixels, before);
        for y in 0..24 {
            for x in 0..32 {
                let offset = (y * 32 + x) * 4;
                assert_eq!(
                    canvas.pixels[offset..offset + 4],
                    if (4..28).contains(&x) && (6..18).contains(&y) {
                        PRIMARY
                    } else {
                        CANVAS
                    }
                );
            }
        }
    }

    fn view(error: Option<String>) -> DashboardViewModel {
        DashboardViewModel {
            ocr_available: true,
            vision_available: false,
            vision: super::super::dashboard::DashboardVision::default(),
            osc_available: true,
            labels: DashboardLabels {
                title: "VRCS".into(),
                subtitle: "SteamVR quick settings".into(),
                master: "VR Overlay".into(),
                headset: "Headset".into(),
                wrist: "Wrist".into(),
                ocr: "OCR".into(),
                content: "Content".into(),
                hand: "Hand".into(),
                width: "Width".into(),
                opacity: "Opacity".into(),
                gesture: "Gesture".into(),
                preview: "Preview".into(),
                bindings: "Bindings".into(),
                saving: "Saving".into(),
                saved: "Saved".into(),
                ..DashboardLabels::default()
            },
            enabled: true,
            capture: super::super::dashboard::DashboardCapture::default(),
            content: super::super::dashboard::DashboardChoice::default(),
            headset: DashboardHeadset {
                enabled: true,
                preview_visible: false,
                width: "1.20 m".into(),
                opacity: "92%".into(),
                position: Vec::new(),
            },
            wrist: DashboardWrist {
                enabled: true,
                hand: super::super::dashboard::DashboardChoice::default(),
                preview_visible: false,
                width: "0.32 m".into(),
                opacity: "94%".into(),
                position: Vec::new(),
            },
            ocr: DashboardOcr {
                available: true,
                enabled: false,
                backend: "Cloud".into(),
                gesture: true,
            },
            language: super::super::dashboard::DashboardLanguage::default(),
            osc: super::super::dashboard::DashboardOsc {
                enabled: true,
                original: true,
                mute_sync: true,
                strategy: super::super::dashboard::DashboardChoice::default(),
                endpoint: "127.0.0.1:9000".into(),
            },
            status: "Ready".into(),
            save_state: DashboardSaveState::Idle,
            error,
        }
    }

    #[test]
    fn capture_button_is_available_on_every_page_and_respects_runtime_availability() {
        let mut serialized = serde_json::to_value(view(None)).unwrap();
        serialized["capture"] = serde_json::json!({ "running": false, "available": true });
        let mut state = DashboardState::default();
        state.update_view(&serde_json::from_value(serialized.clone()).unwrap());
        let rect = control_rect(DashboardControl::Capture);
        let x = (rect.left + rect.right) / 2.;
        let y = (rect.top + rect.bottom) / 2.;
        for tab_x in [200., 500., 800.] {
            state.pointer_down(tab_x, 168.);
            state.pointer_up(tab_x, 168.);
            state.pointer_down(x, y);
            assert_eq!(
                state
                    .pointer_up(x, y)
                    .map(|action| serde_json::to_value(action).unwrap()),
                Some(serde_json::json!("toggle_capture"))
            );
        }
        serialized["capture"]["available"] = serde_json::json!(false);
        state.update_view(&serde_json::from_value(serialized).unwrap());
        state.pointer_down(x, y);
        assert_eq!(state.pointer_up(x, y), None);
        state.pointer_down(1200., 168.);
        assert_eq!(
            state.pointer_up(1200., 168.),
            Some(DashboardAction::ToggleMaster)
        );
    }

    #[test]
    fn capture_button_displays_the_actual_running_state() {
        let mut serialized = serde_json::to_value(view(None)).unwrap();
        serialized["capture"] = serde_json::json!({ "running": false, "available": true });
        serialized["labels"]["capture_start"] = serde_json::json!("Start transcription");
        serialized["labels"]["capture_stop"] = serde_json::json!("Stop transcription");
        let stopped: DashboardViewModel = serde_json::from_value(serialized.clone()).unwrap();
        serialized["capture"]["running"] = serde_json::json!(true);
        let running: DashboardViewModel = serde_json::from_value(serialized).unwrap();
        let mut state = DashboardState::default();
        state.update_view(&stopped);
        let stopped_texture = render(&stopped, &state).unwrap();
        state.update_view(&running);
        let running_texture = render(&running, &state).unwrap();
        let rect = control_rect(DashboardControl::Capture);
        assert!((rect.top as u32..rect.bottom as u32).any(|y| {
            let start = ((y * DASHBOARD_WIDTH + rect.left as u32) * 4) as usize;
            let end = ((y * DASHBOARD_WIDTH + rect.right as u32) * 4) as usize;
            stopped_texture.pixels[start..end] != running_texture.pixels[start..end]
        }));
    }

    #[test]
    fn dashboard_canvas_keeps_native_bgra_pixels_for_upload() {
        let canvas = Canvas::new(2, 1, CANVAS);
        let buffer = canvas.pixels.as_ptr();
        let texture = canvas.into_texture();
        assert_eq!(texture.pixels, [0xfb, 0xf8, 0xf5, 255].repeat(2));
        assert_eq!(texture.pixels.as_ptr(), buffer);
    }

    #[test]
    fn cached_rendering_preserves_pixels_across_interaction_and_label_changes() {
        let mut view = view(None);
        let mut state = DashboardState::default();
        let mut cache = RasterCache::default();
        for step in 0..5 {
            match step {
                1 => {
                    state.pointer_move(650., 510.);
                }
                2 => {
                    state.pointer_down(650., 510.);
                }
                3 => {
                    view.labels.title = "VRCS 快捷设置".into();
                }
                4 => {
                    view.headset.width = "1.30 m".into();
                }
                _ => {}
            }
            let cached = super::render(&view, &state, &mut cache).unwrap();
            let fresh = super::render(&view, &state, &mut RasterCache::default()).unwrap();
            assert_eq!(cached.pixels, fresh.pixels);
        }
    }

    #[test]
    fn text_cache_eviction_preserves_the_requested_mask() {
        let mut cache = RasterCache::default();
        let rect = Rect::new(0, 0, 600, 64);
        let style = TextStyle {
            size: 24,
            align: Align::Left,
            strong: false,
        };
        for index in 0..160 {
            let text = format!("Width {index}");
            let actual = cache.text_mask(&text, rect, style, "Segoe UI\0").unwrap();
            let expected = render_text_mask(&text, rect, style, "Segoe UI\0").unwrap();
            assert!(actual.iter().copied().eq(expected
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| pixel[0])));
            assert!(cache.text_bytes <= MAX_TEXT_CACHE_BYTES);
        }
    }

    #[test]
    fn empty_text_rectangles_leave_the_canvas_unchanged() {
        let mut canvas = Canvas::new(64, 32, CANVAS);
        let before = canvas.pixels.clone();
        for rect in [Rect::new(4, 4, 4, 20), Rect::new(20, 20, 4, 4)] {
            canvas.text("VRCS", rect, 24, TEXT, Align::Left).unwrap();
        }
        assert_eq!(canvas.pixels, before);
    }

    #[cfg(windows)]
    #[test]
    fn cropped_text_preserves_alignment_and_clips_to_the_canvas() {
        for align in [Align::Left, Align::Center] {
            let mut reference = Canvas::new(300, 40, CANVAS);
            reference.font_face = "Microsoft YaHei UI\0";
            reference
                .text(
                    "VRCS 快捷设置 · 言語",
                    Rect::new(0, 0, 300, 40),
                    24,
                    TEXT,
                    align,
                )
                .unwrap();
            assert!(reference
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| *pixel != CANVAS));

            let mut clipped = Canvas::new(280, 80, CANVAS);
            clipped.font_face = reference.font_face;
            clipped
                .text(
                    "VRCS 快捷设置 · 言語",
                    Rect::new(-10, 25, 290, 65),
                    24,
                    TEXT,
                    align,
                )
                .unwrap();
            for y in 0..80 {
                for x in 0..280 {
                    let actual = ((y * 280 + x) * 4) as usize;
                    let expected = if (25..65).contains(&y) {
                        let offset = (((y - 25) * 300 + x + 10) * 4) as usize;
                        &reference.pixels[offset..offset + 4]
                    } else {
                        &CANVAS
                    };
                    assert_eq!(&clipped.pixels[actual..actual + 4], expected);
                }
            }
        }
    }

    #[test]
    fn text_masks_only_allocate_the_text_rectangle() {
        let rect = Rect::new(20, 12, 220, 52);
        let mask = render_text_mask(
            "VRCS",
            rect,
            TextStyle {
                size: 24,
                align: Align::Left,
                strong: false,
            },
            "Segoe UI\0",
        )
        .unwrap();
        assert_eq!(mask.len(), 200 * 40 * 4);
    }

    #[test]
    fn saving_keeps_menu_navigation_and_adjustments_interactive() {
        let mut saving = view(None);
        saving.save_state = DashboardSaveState::Saving;
        let mut state = DashboardState::default();
        state.update_view(&saving);
        state.pointer_down(500., 168.);
        state.pointer_up(500., 168.);
        assert_eq!(state.page(), DashboardPage::Language);
        state.pointer_down(100., 168.);
        state.pointer_up(100., 168.);
        state.pointer_down(650., 550.);
        assert_eq!(
            state.pointer_up(650., 550.),
            Some(super::super::dashboard::DashboardAction::HeadsetOpacityUp)
        );
    }

    #[test]
    fn dashboard_texture_uses_desktop_rgba_colors() {
        let texture = render(&view(None), &DashboardState::default()).unwrap();
        assert_eq!(&texture.pixels[..4], &[0xf5, 0xf8, 0xfb, 255]);
        let toggle_offset = ((168 * texture.width + 1290) * 4) as usize;
        assert_eq!(
            &texture.pixels[toggle_offset..toggle_offset + 4],
            &[0x74, 0xd6, 0xff, 255]
        );

        let error = render(
            &view(Some("Save failed".into())),
            &DashboardState::default(),
        )
        .unwrap();
        let error_offset = (((error.height - 24) * error.width + 1380) * 4) as usize;
        assert_eq!(
            &error.pixels[error_offset..error_offset + 4],
            &[0xfd, 0xf0, 0xee, 255]
        );
    }

    #[test]
    fn dashboard_thumbnail_uses_desktop_rgba_background() {
        let texture = render_thumbnail("VRCS").unwrap();
        assert_eq!(&texture.pixels[..4], &[0xf0, 0xfb, 0xff, 255]);
    }

    #[cfg(windows)]
    #[test]
    fn dashboard_thumbnail_converts_gdi_icon_colors_to_rgba() {
        let icon = render_app_icon(204, 204).unwrap();
        let texture = render_thumbnail("VRCS").unwrap();
        let mut colored_pixels = 0;
        for (index, bgra) in icon.as_chunks::<4>().0.iter().enumerate() {
            if bgra[3] != 255 || bgra[0] == bgra[2] {
                continue;
            }
            let x = index as u32 % 204 + 26;
            let y = index as u32 / 204 + 26;
            let offset = ((y * texture.width + x) * 4) as usize;
            assert_eq!(
                &texture.pixels[offset..offset + 4],
                &[bgra[2], bgra[1], bgra[0], 255]
            );
            colored_pixels += 1;
        }
        assert!(
            colored_pixels > 0,
            "The icon must contain opaque colored pixels"
        );
    }

    #[test]
    fn position_updates_preserve_the_editor_and_cancel_stale_pointer_presses() {
        use super::super::dashboard::{DashboardNumberField, PositionField};
        let mut view = view(None);
        view.headset.position = vec![DashboardNumberField {
            field: PositionField::Horizontal,
            label: "Horizontal".into(),
            value: "0.00 m".into(),
            can_decrease: true,
            can_increase: true,
        }];
        let mut state = DashboardState::default();
        state.update_view(&view);
        state.pointer_down(200., 630.);
        state.pointer_up(200., 630.);
        state.pointer_down(450., 450.);
        view.headset.position[0].value = "0.01 m".into();
        state.update_view(&view);
        assert_eq!(state.display_editor(), Some(DisplayKind::Headset));
        assert_eq!(state.pointer_up(450., 450.), None);
        let texture = render(&view, &state).unwrap();
        assert_eq!(
            (texture.width, texture.height),
            (DASHBOARD_WIDTH, DASHBOARD_HEIGHT)
        );
        assert!(texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn display_page_trims_unused_bottom_space() {
        let texture = render(&view(None), &DashboardState::default()).unwrap();

        assert_eq!((texture.width, texture.height), (DASHBOARD_WIDTH, 840));
        assert_eq!(
            texture.pixels.len(),
            (texture.width * texture.height * 4) as usize
        );
        assert!(texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn compact_display_reserves_space_for_errors_and_save_feedback() {
        let texture = render(
            &view(Some("The settings service is unavailable".into())),
            &DashboardState::default(),
        )
        .unwrap();

        assert_eq!((texture.width, texture.height), (DASHBOARD_WIDTH, 872));
        for save_state in [DashboardSaveState::Saving, DashboardSaveState::Saved] {
            let mut view = view(None);
            view.save_state = save_state;
            let texture = render(&view, &DashboardState::default()).unwrap();
            assert_eq!((texture.width, texture.height), (DASHBOARD_WIDTH, 872));
        }
    }

    #[test]
    fn other_pages_keep_the_full_content_height() {
        let view = view(None);
        for x in [500., 800.] {
            let mut state = DashboardState::default();
            state.update_view(&view);
            state.pointer_down(x, 168.);
            state.pointer_up(x, 168.);
            assert_ne!(state.page(), DashboardPage::Display);
            let texture = render(&view, &state).unwrap();
            assert_eq!(
                (texture.width, texture.height),
                (DASHBOARD_WIDTH, DASHBOARD_HEIGHT)
            );
        }
    }

    #[test]
    fn renders_a_square_dashboard_thumbnail() {
        let texture = render_thumbnail("VRCS").unwrap();

        assert_eq!((texture.width, texture.height), (256, 256));
        assert!(texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255));
    }

    #[cfg(windows)]
    #[test]
    fn dashboard_thumbnail_preserves_the_logo_sky_blue_in_rgba() {
        let texture = render_thumbnail("VRCS").unwrap();
        let pixels = texture.pixels.as_chunks::<4>().0;
        assert!(pixels.contains(&[0x74, 0xd6, 0xff, 255]));
        assert!(!pixels.contains(&[0xff, 0xd6, 0x74, 255]));
    }

    #[cfg(windows)]
    #[test]
    fn dashboard_thumbnail_contains_the_existing_white_logo_detail() {
        let texture = render_thumbnail("VRCS").unwrap();

        assert!(texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3].iter().all(|channel| *channel >= 245)));
    }
}
