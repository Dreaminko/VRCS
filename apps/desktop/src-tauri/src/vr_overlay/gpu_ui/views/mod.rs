//! All 10 VR Surface views matching the React/CSS implementation.

pub mod dashboard;
pub mod dialogue;
pub mod edit_toolbar;
pub mod menu;
pub mod passive;
pub mod reader;

use std::time::Instant;

use cosmic_text::{Align, Wrap};
use taffy::prelude::*;

use super::SurfaceRuntimeState;
use super::draw::DrawList;
use super::icons::IconId;
pub(super) use super::layout::length;
use super::layout::{BoxPaint, RegionPayload, UiBuilder};
use super::text::{TextEngine, TextStyle};
use super::theme::Color;
use crate::vr_overlay::surface_protocol::{
    EditSurfaceOverlay, SurfaceId, SurfaceLayout, SurfaceState, SurfaceView,
};

pub fn render_surface(
    text: &mut TextEngine,
    state: &SurfaceState,
    runtime: &mut SurfaceRuntimeState,
    now: Instant,
) -> (SurfaceLayout, DrawList) {
    match &state.view {
        #[cfg(windows)]
        SurfaceView::OcrSelection { dark } => passive::render_ocr_selection(text, state.key, *dark),
        #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
        SurfaceView::DashboardThumbnail { dark } => {
            passive::render_dashboard_thumbnail(text, state.key, *dark)
        }
        #[cfg(windows)]
        SurfaceView::OcrProgress { dark, progress } => {
            passive::render_ocr_progress(text, state.key, *dark, progress.as_ref(), runtime, now)
        }
        SurfaceView::Headset {
            visibility,
            max_visible_players,
            dark,
            messages,
            notice,
            notice_error,
            labels,
            lines_per_language,
            font_size,
            opacity,
            edit_mode,
        } => passive::render_headset(
            text,
            state,
            *dark,
            messages,
            visibility,
            *max_visible_players,
            notice.as_deref(),
            *notice_error,
            labels,
            *lines_per_language,
            *font_size,
            *opacity,
            edit_mode.as_ref(),
        ),
        #[cfg(windows)]
        SurfaceView::OcrResult { dark, scene } => {
            passive::render_ocr_result(text, state.key, *dark, scene)
        }
        SurfaceView::Dialogue { .. } => dialogue::render_dialogue(text, state, runtime),
        #[cfg(windows)]
        SurfaceView::Reader { .. } => reader::render_reader(text, state, runtime),
        SurfaceView::PlayerAudio { dark, notice } => {
            reader::render_player_audio_standalone(text, state, *dark, notice, runtime)
        }
        SurfaceView::Menu { .. } => menu::render_menu(text, state, runtime, now),
        SurfaceView::Dashboard {
            dark,
            dashboard,
            ui,
        } => dashboard::render_dashboard(text, state, *dark, dashboard, ui, runtime),
        SurfaceView::EditToolbar { dark, toolbar } => {
            edit_toolbar::render_edit_toolbar(text, state, *dark, toolbar, runtime)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CornerHandleSpec {
    pub panel_radius: f32,
    pub gap: f32,
    pub stroke_idle: f32,
    pub stroke_active: f32,
    pub start_rad: f32,
    pub sweep_rad: f32,
    pub hit_size: f32,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn add_edit_chrome(
    ui: &mut UiBuilder<'_>,
    parent: NodeId,
    state: &SurfaceState,
    ov: &EditSurfaceOverlay,
    resize_id: u8,
    resize_payload: RegionPayload,
    corner: CornerHandleSpec,
) {
    let t = ui.theme;
    let badge_bg = if ov.selected || ov.active_grab {
        t.primary_soft
    } else {
        t.surface_elevated
    };
    let badge_border = if ov.selected || ov.active_grab || ov.active_resize {
        t.primary
    } else {
        t.border
    };
    let badge = ui.add_box(
        parent,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(12.0),
                top: length(10.0),
                right: auto(),
                bottom: auto(),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(8.0),
                height: length(0.0),
            },
            padding: Rect {
                left: length(12.0),
                right: length(12.0),
                top: length(6.0),
                bottom: length(6.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(badge_bg)
            .border(1.5, badge_border)
            .radius(16.0),
    );
    ui.add_text(
        badge,
        &ov.title,
        Style::default(),
        single_line_style(20.0, 700, t.primary_ink, Align::Left),
    );
    if !ov.scale_label.is_empty() {
        ui.add_text(
            badge,
            &ov.scale_label,
            Style::default(),
            single_line_style(18.0, 600, t.text_secondary, Align::Left),
        );
    }
    if let Some(hint) = ov.shared_hint.as_deref().filter(|s| !s.is_empty()) {
        ui.add_text(
            badge,
            hint,
            Style::default(),
            single_line_style(17.0, 500, t.text_quiet, Align::Left),
        );
    }

    let hovered = state.hovered == Some(resize_id);
    let pressed = state.pressed == Some(resize_id);
    let highlighted = hovered || pressed || ov.active_resize;
    let stroke = if highlighted {
        corner.stroke_active
    } else {
        corner.stroke_idle
    };
    let color = if pressed || ov.active_resize {
        t.primary
    } else if hovered {
        t.primary_hover
    } else if ov.selected {
        t.primary_ink.with_alpha(0.88)
    } else {
        t.text_secondary.with_alpha(0.85)
    };
    let r_mid = corner.panel_radius + corner.gap + corner.stroke_idle * 0.5;
    let arc_box = 2.0 * (r_mid + stroke * 0.5);
    let arc_inset = corner.panel_radius - arc_box * 0.5;
    let arc_wrap = ui.add_box(
        parent,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: auto(),
                top: length(arc_inset),
                right: length(arc_inset),
                bottom: auto(),
            },
            size: Size {
                width: length(arc_box),
                height: length(arc_box),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    ui.add_arc(
        arc_wrap,
        arc_box,
        corner.start_rad,
        corner.sweep_rad,
        stroke,
        color,
    );

    let hit_outset = -(corner.gap + corner.stroke_active);
    let hit = ui.add_box(
        parent,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: auto(),
                top: length(hit_outset),
                right: length(hit_outset),
                bottom: auto(),
            },
            size: Size {
                width: length(corner.hit_size),
                height: length(corner.hit_size),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    ui.set_region(hit, resize_payload);
}

pub(super) fn surface_size(surface: SurfaceId) -> (u32, u32) {
    surface.size()
}

pub(super) fn text_style(size: f32, weight: u16, color: Color) -> TextStyle {
    TextStyle {
        font_size: size,
        line_height: (size * 1.35).round(),
        weight,
        color,
        ..Default::default()
    }
}

pub(super) fn single_line_style(size: f32, weight: u16, color: Color, align: Align) -> TextStyle {
    TextStyle {
        font_size: size,
        line_height: (size * 1.25).round(),
        weight,
        color,
        align,
        wrap: Wrap::None,
        max_lines: Some(1),
        ellipsis: true,
        ..Default::default()
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn add_circle_button(
    ui: &mut UiBuilder<'_>,
    parent: NodeId,
    state: &SurfaceState,
    id: u8,
    size: f32,
    icon: IconId,
    icon_size: f32,
    icon_stroke: f32,
    active: bool,
    primary: bool,
    disabled: bool,
    tooltip: Option<(&str, bool)>,
    payload: RegionPayload,
) -> NodeId {
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
        Color::TRANSPARENT
    };
    let fg = if primary {
        Color::WHITE
    } else if active || hovered {
        t.primary_ink
    } else {
        t.text_secondary
    };

    let mut paint = BoxPaint::new().bg(bg).radius(size * 0.5);
    if disabled {
        paint.opacity = 0.5;
    }
    let btn = ui.add_box(
        parent,
        Style {
            size: Size {
                width: length(size),
                height: length(size),
            },
            flex_shrink: 0.0,
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::Center),
            ..Default::default()
        },
        paint,
    );
    if !disabled {
        ui.set_region(btn, payload);
    }
    if hovered
        && let Some((tip, below)) = tooltip
        && !tip.is_empty()
    {
        ui.set_tooltip(btn, tip, below);
    }

    let icon_node = ui.add_icon(btn, icon, icon_size, icon_stroke, fg);
    if pressed && let Some(meta) = ui.tree.get_node_context_mut(icon_node) {
        meta.paint.scale = 0.95;
    }
    btn
}
