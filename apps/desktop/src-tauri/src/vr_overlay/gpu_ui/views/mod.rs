//! All 10 VR Surface views matching the React/CSS implementation.

pub mod dashboard;
pub mod dialogue;
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
use crate::vr_overlay::surface_protocol::{SurfaceId, SurfaceLayout, SurfaceState, SurfaceView};

pub fn render_surface(
    text: &mut TextEngine,
    state: &SurfaceState,
    runtime: &mut SurfaceRuntimeState,
    now: Instant,
) -> (SurfaceLayout, DrawList) {
    match &state.view {
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
        } => passive::render_headset(
            text,
            state.key,
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
    }
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
