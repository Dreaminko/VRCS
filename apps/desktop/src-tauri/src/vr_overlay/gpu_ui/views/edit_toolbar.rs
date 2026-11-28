//! Floating layout edit toolbar (`960x260`).

use cosmic_text::Align;
use taffy::prelude::*;

use super::{length, single_line_style, surface_size};
use crate::vr_overlay::gpu_ui::SurfaceRuntimeState;
use crate::vr_overlay::gpu_ui::draw::DrawList;
use crate::vr_overlay::gpu_ui::icons::IconId;
use crate::vr_overlay::gpu_ui::layout::{BoxPaint, RegionPayload, UiBuilder};
use crate::vr_overlay::gpu_ui::text::TextEngine;
use crate::vr_overlay::gpu_ui::theme::{Color, Theme};
use crate::vr_overlay::surface_protocol::{EditToolbarView, SurfaceId, SurfaceLayout, SurfaceState};

#[allow(clippy::too_many_arguments)]
fn add_pill_button(
    ui: &mut UiBuilder<'_>,
    parent: NodeId,
    state: &SurfaceState,
    id: u8,
    label: &str,
    icon: Option<IconId>,
    active: bool,
    primary: bool,
    disabled: bool,
    grow: bool,
) {
    let t = ui.theme;
    let hovered = !disabled && state.hovered == Some(id);
    let pressed = !disabled && state.pressed == Some(id);

    let bg = if primary {
        if hovered || pressed {
            t.primary_hover
        } else {
            t.primary
        }
    } else if active {
        t.primary_soft
    } else if hovered {
        t.primary_softer
    } else {
        t.surface_subtle
    };
    let fg = if primary {
        Color::WHITE
    } else if active || hovered {
        t.primary_ink
    } else {
        t.text
    };
    let border = if active && !primary {
        t.primary
    } else {
        t.border
    };

    let mut paint = BoxPaint::new().bg(bg).border(1.5, border).radius(20.0);
    if disabled {
        paint.opacity = 0.5;
    }
    if pressed {
        paint.scale = 0.97;
    }

    let btn = ui.add_box(
        parent,
        Style {
            size: Size {
                width: auto(),
                height: length(52.0),
            },
            flex_grow: if grow { 1.0 } else { 0.0 },
            flex_shrink: 1.0,
            min_size: Size {
                width: length(84.0),
                height: length(52.0),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::Center),
            gap: Size {
                width: length(8.0),
                height: length(0.0),
            },
            padding: Rect {
                left: length(14.0),
                right: length(14.0),
                top: length(0.0),
                bottom: length(0.0),
            },
            ..Default::default()
        },
        paint,
    );
    if !disabled {
        ui.set_region(btn, RegionPayload::Simple(id, false));
    }
    if let Some(ic) = icon {
        ui.add_icon(btn, ic, 22.0, 1.9, fg);
    }
    ui.add_text(
        btn,
        label,
        Style {
            flex_shrink: 1.0,
            min_size: Size {
                width: length(0.0),
                height: auto(),
            },
            ..Default::default()
        },
        single_line_style(21.0, if primary || active { 600 } else { 500 }, fg, Align::Center),
    );
}

pub fn render_edit_toolbar(
    text: &mut TextEngine,
    state: &SurfaceState,
    dark: bool,
    toolbar: &EditToolbarView,
    _runtime: &mut SurfaceRuntimeState,
) -> (SurfaceLayout, DrawList) {
    let (w, h) = surface_size(SurfaceId::EditToolbar);
    let mut ui = UiBuilder::new(text, Theme::of(dark), w, h);
    let theme = ui.theme;
    let root = ui.root();

    let panel = ui.add_box(
        root,
        Style {
            size: Size {
                width: percent(1.0_f32),
                height: percent(1.0_f32),
            },
            flex_direction: FlexDirection::Column,
            justify_content: Some(JustifyContent::SpaceBetween),
            gap: Size {
                width: length(0.0),
                height: length(10.0),
            },
            padding: Rect {
                left: length(16.0),
                right: length(16.0),
                top: length(14.0),
                bottom: length(14.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(theme.surface)
            .border(2.0, theme.primary.with_alpha(0.65))
            .radius(28.0),
    );
    ui.mark_panel(panel);

    // Drag header (`region 0`).
    let header_bg = if state.pressed == Some(0) {
        theme.primary_soft
    } else if state.hovered == Some(0) {
        theme.surface_elevated
    } else {
        theme.surface_subtle
    };
    let header = ui.add_box(
        panel,
        Style {
            size: Size {
                width: percent(1.0_f32),
                height: length(48.0),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::SpaceBetween),
            gap: Size {
                width: length(12.0),
                height: length(0.0),
            },
            padding: Rect {
                left: length(14.0),
                right: length(14.0),
                top: length(0.0),
                bottom: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new().bg(header_bg).radius(18.0),
    );
    ui.set_region(header, RegionPayload::Simple(0, false));

    let left_header = ui.add_box(
        header,
        Style {
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(10.0),
                height: length(0.0),
            },
            flex_shrink: 1.0,
            ..Default::default()
        },
        BoxPaint::new(),
    );
    ui.add_icon(left_header, IconId::GripHorizontal, 24.0, 2.0, theme.primary_ink);
    ui.add_text(
        left_header,
        &toolbar.title,
        Style::default(),
        single_line_style(24.0, 700, theme.text, Align::Left),
    );
    if !toolbar.target_label.is_empty() {
        ui.add_text(
            left_header,
            &toolbar.target_label,
            Style::default(),
            single_line_style(20.0, 500, theme.text_secondary, Align::Left),
        );
    }

    let status_text = toolbar
        .error
        .as_deref()
        .filter(|e| !e.is_empty())
        .unwrap_or(&toolbar.status_label);
    let status_color = if toolbar.error.as_deref().is_some_and(|e| !e.is_empty()) {
        theme.error
    } else if toolbar.dirty {
        theme.primary_ink
    } else {
        theme.text_secondary
    };
    ui.add_text(
        header,
        status_text,
        Style {
            flex_shrink: 1.0,
            min_size: Size {
                width: length(0.0),
                height: auto(),
            },
            ..Default::default()
        },
        single_line_style(20.0, 600, status_color, Align::Right),
    );

    // Row 2: helper & visibility toggles (`regions 1..=5`).
    let row_mid = ui.add_box(
        panel,
        Style {
            size: Size {
                width: percent(1.0_f32),
                height: length(52.0),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(8.0),
                height: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    add_pill_button(
        &mut ui,
        row_mid,
        state,
        1,
        &toolbar.label_auto_level,
        Some(IconId::Level),
        toolbar.auto_level,
        false,
        toolbar.saving,
        true,
    );
    add_pill_button(
        &mut ui,
        row_mid,
        state,
        2,
        &toolbar.label_face_head,
        Some(IconId::Eye),
        toolbar.face_head,
        false,
        toolbar.saving,
        true,
    );
    add_pill_button(
        &mut ui,
        row_mid,
        state,
        3,
        &toolbar.label_headset,
        Some(IconId::Captions),
        toolbar.show_headset,
        false,
        toolbar.saving,
        true,
    );
    add_pill_button(
        &mut ui,
        row_mid,
        state,
        4,
        &toolbar.label_wrist,
        Some(IconId::MessageSquare),
        toolbar.show_wrist,
        false,
        toolbar.saving,
        true,
    );
    add_pill_button(
        &mut ui,
        row_mid,
        state,
        5,
        &toolbar.label_wrist_menu,
        Some(IconId::Hand),
        toolbar.show_wrist_menu,
        false,
        toolbar.saving,
        true,
    );

    // Row 3: reset / cancel / save (`regions 6..=9`).
    let row_bot = ui.add_box(
        panel,
        Style {
            size: Size {
                width: percent(1.0_f32),
                height: length(54.0),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(10.0),
                height: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    add_pill_button(
        &mut ui,
        row_bot,
        state,
        6,
        &toolbar.label_reset_selected,
        Some(IconId::RotateCcw),
        false,
        false,
        toolbar.saving,
        true,
    );
    add_pill_button(
        &mut ui,
        row_bot,
        state,
        7,
        &toolbar.label_reset_all,
        Some(IconId::RotateCcw),
        false,
        false,
        toolbar.saving,
        true,
    );
    add_pill_button(
        &mut ui,
        row_bot,
        state,
        8,
        &toolbar.label_cancel,
        Some(IconId::X),
        false,
        false,
        false,
        true,
    );
    add_pill_button(
        &mut ui,
        row_bot,
        state,
        9,
        &toolbar.label_save,
        Some(IconId::Check),
        false,
        true,
        toolbar.saving,
        true,
    );

    ui.compute_layout();
    ui.emit(SurfaceId::EditToolbar, state.key, 1)
}
