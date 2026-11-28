//! Passive VR surfaces: OcrSelection, DashboardThumbnail, OcrProgress, Headset, OcrResult.

#[cfg(windows)]
use std::time::Instant;

use cosmic_text::{Align, Wrap};
use taffy::prelude::*;

use super::{single_line_style, surface_size};
use crate::vr_overlay::caption_status::CaptionLabels;
use crate::vr_overlay::dashboard::CaptionVisibility;
#[cfg(windows)]
use crate::vr_overlay::gpu_ui::SurfaceRuntimeState;
use crate::vr_overlay::gpu_ui::caption::{visible_caption_messages, visible_tail};
use crate::vr_overlay::gpu_ui::draw::DrawList;
#[cfg(windows)]
use crate::vr_overlay::gpu_ui::draw::{Instance, MaskVertex, OcrCardDraw};
use crate::vr_overlay::gpu_ui::icons::IconId;
use crate::vr_overlay::gpu_ui::layout::{BoxPaint, UiBuilder, length, percent};
#[cfg(windows)]
use crate::vr_overlay::gpu_ui::ocr_scene::{layout_ocr_scene, quad_homography};
use crate::vr_overlay::gpu_ui::text::{TextEngine, TextStyle};
#[cfg(windows)]
use crate::vr_overlay::gpu_ui::theme::Color;
use crate::vr_overlay::gpu_ui::theme::Theme;
#[cfg(windows)]
use crate::vr_overlay::ocr_plane::OcrScene;
#[cfg(windows)]
use crate::vr_overlay::ocr_progress::{ProgressTask, ProgressView, Stage};
use crate::vr_overlay::presentation::{MessageSide, WristMessage};
use crate::vr_overlay::surface_protocol::{
    EditSurfaceOverlay, SurfaceId, SurfaceKey, SurfaceLayout, SurfaceState,
};

#[cfg(windows)]
pub fn render_ocr_selection(
    text: &mut TextEngine,
    key: SurfaceKey,
    dark: bool,
) -> (SurfaceLayout, DrawList) {
    let (w, h) = surface_size(SurfaceId::OcrSelection);
    let theme = Theme::of(dark);
    let mut ui = UiBuilder::new(text, theme, w, h);
    let root = ui.root();
    ui.add_box(
        root,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(2.0),
                top: length(2.0),
                right: length(2.0),
                bottom: length(2.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(theme.primary.with_alpha(0.04))
            .border(3.0, theme.primary)
            .radius(8.0),
    );
    ui.compute_layout();
    ui.emit(SurfaceId::OcrSelection, key, 1)
}

#[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
pub fn render_dashboard_thumbnail(
    text: &mut TextEngine,
    key: SurfaceKey,
    dark: bool,
) -> (SurfaceLayout, DrawList) {
    let (w, h) = surface_size(SurfaceId::DashboardThumbnail);
    let theme = Theme::of(dark);
    let mut ui = UiBuilder::new(text, theme, w, h);
    let root = ui.root();
    ui.add_thumbnail(root, w as f32);
    ui.compute_layout();
    ui.emit(SurfaceId::DashboardThumbnail, key, 1)
}

#[cfg(windows)]
pub fn render_ocr_progress(
    text: &mut TextEngine,
    key: SurfaceKey,
    dark: bool,
    progress: Option<&ProgressView>,
    runtime: &mut SurfaceRuntimeState,
    now: Instant,
) -> (SurfaceLayout, DrawList) {
    let (w, h) = surface_size(SurfaceId::OcrProgress);
    let theme = Theme::of(dark);
    let mut ui = UiBuilder::new(text, theme, w, h);
    let Some(progress) = progress else {
        ui.compute_layout();
        return ui.emit(SurfaceId::OcrProgress, key, 1);
    };

    let failed = progress.stages.contains(&Stage::Failed);
    let complete = progress.stages.iter().all(|s| *s == Stage::Complete);
    let status_icon = if failed {
        IconId::X
    } else if complete {
        IconId::Check
    } else if progress.task == ProgressTask::Key {
        IconId::KeyRound
    } else {
        IconId::ScanText
    };
    let icons = match progress.task {
        ProgressTask::Ocr => [
            IconId::Camera,
            IconId::ScanText,
            IconId::Languages,
            IconId::Monitor,
        ],
        ProgressTask::Key => [
            IconId::Shield,
            IconId::Gamepad2,
            IconId::KeyRound,
            IconId::AudioLines,
        ],
    };

    let anim_start = *runtime.anim_start.get_or_insert(now);
    let root = ui.root();
    let panel = ui.add_box(
        root,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(12.0),
                top: length(8.0),
                right: length(12.0),
                bottom: length(16.0),
            },
            flex_direction: FlexDirection::Column,
            padding: Rect {
                left: length(24.0),
                right: length(24.0),
                top: length(18.0),
                bottom: length(18.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(theme.surface)
            .border(2.0, theme.border)
            .radius(32.0),
    );

    let header = ui.add_box(
        panel,
        Style {
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(12.0),
                height: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    let status_color = if failed { theme.error } else { theme.text };
    let icon_color = if failed {
        theme.error
    } else {
        theme.primary_ink
    };
    ui.add_icon(header, status_icon, 30.0, 1.8, icon_color);
    ui.add_text(
        header,
        &progress.status,
        Style {
            flex_grow: 1.0,
            min_size: Size {
                width: length(0.0),
                height: auto(),
            },
            ..Default::default()
        },
        single_line_style(28.0, 650, status_color, Align::Left),
    );

    let stages_row = ui.add_box(
        panel,
        Style {
            flex_direction: FlexDirection::Row,
            gap: Size {
                width: length(16.0),
                height: length(0.0),
            },
            margin: Rect {
                top: length(18.0),
                bottom: length(14.0),
                left: length(0.0),
                right: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );

    for (index, stage) in progress.stages.iter().enumerate() {
        let stage_icon = match stage {
            Stage::Pending | Stage::Active => icons[index],
            Stage::Complete => IconId::Check,
            Stage::Skipped => IconId::Minus,
            Stage::Failed => IconId::X,
        };
        let color = match stage {
            Stage::Active | Stage::Complete => theme.primary_ink,
            Stage::Failed => theme.error,
            Stage::Pending | Stage::Skipped => theme.text_quiet,
        };
        let col = ui.add_box(
            stages_row,
            Style {
                flex_grow: 1.0,
                flex_basis: length(0.0),
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                flex_direction: FlexDirection::Column,
                ..Default::default()
            },
            BoxPaint::new(),
        );
        let lbl_row = ui.add_box(
            col,
            Style {
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
        ui.add_icon(lbl_row, stage_icon, 22.0, 1.8, color);
        ui.add_text(
            lbl_row,
            &progress.stage_labels[index],
            Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                ..Default::default()
            },
            single_line_style(22.0, 400, color, Align::Left),
        );

        let fraction = (index == 2)
            .then_some(progress.translation_fraction)
            .flatten()
            .filter(|&(_, total)| total > 0)
            .map(|(done, total)| (done as f32 / total as f32).clamp(0.0, 1.0));
        let indeterminate = *stage == Stage::Active && fraction.is_none();

        let mut track_paint = BoxPaint::new().radius(4.0);
        if *stage == Stage::Skipped {
            track_paint = track_paint.border(1.0, theme.border_strong);
            track_paint.border_flags = Instance::FLAG_DASHED;
        } else {
            track_paint = track_paint.bg(theme.surface_subtle);
        }
        let track = ui.add_box(
            col,
            Style {
                size: Size {
                    width: auto(),
                    height: length(8.0),
                },
                margin: Rect {
                    top: length(10.0),
                    left: length(0.0),
                    right: length(0.0),
                    bottom: length(0.0),
                },
                ..Default::default()
            },
            track_paint,
        );
        ui.mark_clip(track);

        if indeterminate {
            let phase = (now.saturating_duration_since(anim_start).as_secs_f32() % 1.4) / 1.4;
            let tri = if phase < 0.5 {
                phase * 2.0
            } else {
                (1.0 - phase) * 2.0
            };
            let eased = tri * tri * (3.0 - 2.0 * tri);
            let left_pct = -35.0 + eased * 100.0;
            ui.add_box(
                track,
                Style {
                    position: Position::Absolute,
                    inset: Rect {
                        left: percent(left_pct / 100.0),
                        top: length(0.0),
                        right: auto(),
                        bottom: length(0.0),
                    },
                    size: Size {
                        width: percent(0.35),
                        height: percent(1.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new().bg(color).radius(4.0),
            );
        } else {
            let fill_pct = if *stage == Stage::Complete {
                1.0
            } else {
                fraction.unwrap_or(0.0)
            };
            if fill_pct > 0.0 {
                ui.add_box(
                    track,
                    Style {
                        size: Size {
                            width: percent(fill_pct),
                            height: percent(1.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new().bg(color).radius(4.0),
                );
            }
        }
    }

    ui.add_text(
        panel,
        &progress.detail,
        Style {
            size: Size {
                width: percent(1.0),
                height: auto(),
            },
            ..Default::default()
        },
        single_line_style(22.0, 400, theme.text_secondary, Align::Left),
    );

    ui.compute_layout();
    ui.emit(SurfaceId::OcrProgress, key, 1)
}

#[allow(clippy::too_many_arguments)]
pub fn render_headset(
    text: &mut TextEngine,
    state: &SurfaceState,
    dark: bool,
    messages: &[WristMessage],
    visibility: &CaptionVisibility,
    max_visible_players: Option<usize>,
    notice: Option<&str>,
    notice_error: bool,
    labels: &CaptionLabels,
    lines_per_language: u32,
    font_size: u32,
    opacity: f32,
    edit_mode: Option<&EditSurfaceOverlay>,
) -> (SurfaceLayout, DrawList) {
    let (w, h) = surface_size(SurfaceId::Headset);
    let theme = Theme::of(dark);
    let visible = visible_caption_messages(true, messages, visibility, max_visible_players, 10);
    let has_notice = notice.is_some_and(|n| !n.is_empty());
    if visible.is_empty() && !has_notice && edit_mode.is_none() {
        let mut ui = UiBuilder::new(text, theme, w, h);
        ui.compute_layout();
        return ui.emit(SurfaceId::Headset, state.key, 1);
    }

    let lines = lines_per_language.clamp(1, 4) as usize;
    let base_font = (font_size as f32).clamp(16.0, 68.0);
    let caption_line_h = 1.3_f32;

    // Pre-measure notice height if present.
    let inner_w = w as f32 - 32.0 - 48.0 - 4.0; // outer pad 16*2, panel pad 24*2, border 2*2
    let notice_h = if let Some(n) = notice.filter(|n| !n.is_empty()) {
        let s = text.shape_text(
            n,
            Some(inner_w),
            TextStyle {
                font_size: 26.0,
                line_height: (26.0f32 * 1.35).round(),
                weight: 600,
                align: Align::Center,
                wrap: Wrap::WordOrGlyph,
                ..Default::default()
            },
        );
        s.height.min(120.0) + 16.0
    } else {
        0.0
    };
    let avail_h = (h as f32 - 32.0 - 32.0 - 4.0 - notice_h).max(1.0);
    let rows = visible.len();
    let lang_count: usize = visible.iter().map(|m| m.text.splitn(4, '\n').count()).sum();
    let required_h = base_font
        * (rows as f32 * 0.75 * 1.1 + lang_count as f32 * lines as f32 * caption_line_h)
        + rows as f32 * 14.0
        + (lang_count.saturating_sub(rows)) as f32 * 8.0
        + rows.saturating_sub(1) as f32 * 12.0;
    let scale = ((avail_h - lang_count as f32) / required_h.max(1.0)).clamp(0.01, 1.0);
    let size = base_font * scale;

    // Fit each language line with `visible_tail` matching `HeadsetCaptions`.
    let card_inner_w = (inner_w - 16.0).max(16.0);
    let mut fitted_cards: Vec<(&WristMessage, Vec<String>)> = Vec::with_capacity(visible.len());
    for msg in &visible {
        let mut fitted_langs = Vec::new();
        for lang_text in msg.text.splitn(4, '\n') {
            let line_h = (size * caption_line_h).round().max(1.0);
            let max_h = (line_h * lines as f32).ceil();
            let fitted = visible_tail(lang_text, |cand| {
                if lines == 1 {
                    text.measure_line_width(cand, size, 600) <= card_inner_w - 1.0
                } else {
                    let shaped = text.shape_text(
                        cand,
                        Some(card_inner_w),
                        TextStyle {
                            font_size: size,
                            line_height: line_h,
                            weight: 600,
                            align: Align::Center,
                            wrap: Wrap::WordOrGlyph,
                            ..Default::default()
                        },
                    );
                    shaped.width <= card_inner_w + 1.0 && shaped.height <= max_h + 1.0
                }
            });
            fitted_langs.push(fitted);
        }
        fitted_cards.push((*msg, fitted_langs));
    }

    let pad = if edit_mode.is_some() { 24.0 } else { 16.0 };
    let mut ui = UiBuilder::new(text, theme, w, h);
    ui.set_root_style(Style {
        size: Size {
            width: length(w as f32),
            height: length(h as f32),
        },
        padding: Rect {
            left: length(pad),
            right: length(pad),
            top: length(pad),
            bottom: length(pad),
        },
        align_items: Some(AlignItems::Center),
        justify_content: Some(JustifyContent::Center),
        ..Default::default()
    });

    let root = ui.root();
    let wrapper = ui.add_box(
        root,
        Style {
            size: Size {
                width: percent(1.0_f32),
                height: auto(),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    let panel_border = match edit_mode {
        Some(ov) if ov.selected || ov.active_grab || ov.active_resize => theme.primary,
        Some(_) => theme.primary.with_alpha(0.65),
        None => theme.border,
    };
    let panel = ui.add_box(
        wrapper,
        Style {
            size: Size {
                width: percent(1.0_f32),
                height: auto(),
            },
            min_size: Size {
                width: auto(),
                height: length(if edit_mode.is_some() { 200.0 } else { 0.0 }),
            },
            flex_direction: FlexDirection::Column,
            align_items: Some(AlignItems::Center),
            padding: Rect {
                left: length(24.0),
                right: length(24.0),
                top: length(if edit_mode.is_some() { 76.0 } else { 16.0 }),
                bottom: length(16.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(theme.panel_bg(opacity))
            .border(if edit_mode.is_some() { 3.0 } else { 2.0 }, panel_border)
            .radius(32.0),
    );
    if edit_mode.is_some() {
        ui.mark_panel(panel);
    }
    ui.mark_clip(panel);

    if let Some(n) = notice.filter(|n| !n.is_empty()) {
        let color = if notice_error {
            theme.error
        } else {
            theme.text_secondary
        };
        ui.add_text(
            panel,
            n,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                max_size: Size {
                    width: auto(),
                    height: length(120.0),
                },
                margin: Rect {
                    bottom: length(16.0),
                    left: length(0.0),
                    right: length(0.0),
                    top: length(0.0),
                },
                ..Default::default()
            },
            TextStyle {
                font_size: 26.0,
                line_height: (26.0f32 * 1.35).round(),
                weight: 600,
                align: Align::Center,
                color,
                ..Default::default()
            },
        );
    }

    let grid = ui.add_box(
        panel,
        Style {
            size: Size {
                width: percent(1.0),
                height: auto(),
            },
            flex_direction: FlexDirection::Column,
            gap: Size {
                width: length(0.0),
                height: length(12.0 * scale),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );

    for (msg, langs) in fitted_cards {
        let card = ui.add_box(
            grid,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                flex_direction: FlexDirection::Column,
                align_items: Some(AlignItems::Center),
                padding: Rect {
                    left: length(8.0),
                    right: length(8.0),
                    top: length(4.0 * scale),
                    bottom: length(4.0 * scale),
                },
                ..Default::default()
            },
            BoxPaint::new(),
        );
        let speaker_size = (size * 0.75).max(8.0);
        let speaker_row = ui.add_box(
            card,
            Style {
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                gap: Size {
                    width: length(speaker_size * 0.35),
                    height: length(0.0),
                },
                margin: Rect {
                    bottom: length(6.0 * scale),
                    left: length(0.0),
                    right: length(0.0),
                    top: length(0.0),
                },
                ..Default::default()
            },
            BoxPaint::new(),
        );
        let icon = IconId::from_message_source(msg.original.as_ref().map(|o| o.source.as_str()));
        ui.add_icon(speaker_row, icon, speaker_size, 1.8, theme.primary_ink);
        let speaker_name = msg.speaker.as_deref().unwrap_or(match msg.side {
            MessageSide::Right => &labels.me,
            MessageSide::Left => &labels.other,
        });
        ui.add_text(
            speaker_row,
            speaker_name,
            Style {
                flex_shrink: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                ..Default::default()
            },
            single_line_style(speaker_size, 650, theme.primary_ink, Align::Center),
        );

        for (lang_idx, lang_text) in langs.into_iter().enumerate() {
            let color = if lang_idx == 0 {
                theme.text
            } else {
                theme.text_secondary
            };
            let line_h = (size * caption_line_h).round().max(1.0);
            ui.add_text(
                card,
                lang_text,
                Style {
                    size: Size {
                        width: percent(1.0),
                        height: auto(),
                    },
                    margin: Rect {
                        top: length(if lang_idx > 0 { 8.0 * scale } else { 0.0 }),
                        left: length(0.0),
                        right: length(0.0),
                        bottom: length(0.0),
                    },
                    ..Default::default()
                },
                TextStyle {
                    font_size: size,
                    line_height: line_h,
                    weight: 600,
                    align: Align::Center,
                    wrap: if lines == 1 {
                        Wrap::None
                    } else {
                        Wrap::WordOrGlyph
                    },
                    max_lines: Some(lines),
                    color,
                    ..Default::default()
                },
            );
        }
    }

    if let Some(ov) = edit_mode {
        super::add_edit_chrome(
            &mut ui,
            wrapper,
            state,
            ov,
            1,
            crate::vr_overlay::gpu_ui::layout::RegionPayload::Simple(1, false),
            super::CornerHandleSpec {
                panel_radius: 32.0,
                gap: 10.0,
                stroke_idle: 5.0,
                stroke_active: 7.5,
                start_rad: -std::f32::consts::FRAC_PI_2,
                sweep_rad: std::f32::consts::FRAC_PI_2,
                hit_size: 72.0,
            },
        );
    }

    ui.compute_layout();
    ui.emit(SurfaceId::Headset, state.key, 1)
}

#[cfg(windows)]
pub fn render_ocr_result(
    text: &mut TextEngine,
    key: SurfaceKey,
    dark: bool,
    scene: &OcrScene,
) -> (SurfaceLayout, DrawList) {
    use std::collections::HashSet;

    let (w, h) = surface_size(SurfaceId::OcrResult);
    let theme = Theme::of(dark);
    let scene_layout = layout_ocr_scene(scene, &mut |s, font_size| {
        text.measure_line_width(s, font_size, 600)
    });
    let placed: HashSet<usize> = scene_layout.cards.iter().map(|c| c.id).collect();
    let aspect = scene.aspect;
    let mask_color = theme
        .surface
        .with_alpha(scene.opacity.clamp(0.0, 1.0))
        .premultiplied();
    let card_bg = theme.surface.with_alpha(scene.opacity.clamp(0.0, 1.0));

    let mut draw = DrawList::new(w, h);
    draw.capture_bounds = scene_layout.capture_bounds;
    let root_clip = [0.0, 0.0, w as f32, h as f32];

    for block in scene.blocks.iter().filter(|b| placed.contains(&b.id)) {
        for mask in &block.masks {
            let scaled = mask.map(|[x, y]| [x * 2048.0, y * scene_layout.height * aspect]);
            // Triangulate convex quad [0,1,2,3] -> (0,1,2) and (0,2,3).
            for idx in [0usize, 1, 2, 0, 2, 3] {
                draw.mask_vertices.push(MaskVertex {
                    pos: scaled[idx],
                    color: mask_color,
                });
            }
        }
    }

    for card in &scene_layout.cards {
        if card.connected {
            let center = |q: [[f32; 2]; 4]| -> [f32; 2] {
                [
                    q.iter().map(|p| p[0]).sum::<f32>() / 4.0,
                    q.iter().map(|p| p[1]).sum::<f32>() / 4.0 * aspect,
                ]
            };
            draw.instances.push(Instance::line(
                center(card.source),
                center(card.quad),
                3.0,
                theme.primary,
                root_clip,
            ));
        }

        let aspect_quad = card.quad.map(|[x, y]| [x, y * aspect]);
        if quad_homography(&aspect_quad, card.width, card.height).is_none() {
            continue;
        }
        let local_clip = [0.0, 0.0, card.width, card.height];
        let mut instances = Vec::new();
        // Card box: padding 12px, border-radius 16px, inset shadow 1px var(--border).
        instances.push(Instance::rect(
            [0.0, 0.0, card.width, card.height],
            [16.0; 4],
            card_bg,
            1.0,
            theme.border,
            local_clip,
            16.0,
        ));
        let line_h = (card.font_size * 1.4).round();
        let shaped = text.shape_text(
            &card.lines.join("\n"),
            Some((card.width - 24.0).max(1.0)),
            TextStyle {
                font_size: card.font_size,
                line_height: line_h,
                weight: 600,
                color: theme.text,
                wrap: Wrap::None,
                ..Default::default()
            },
        );
        for g in &shaped.glyphs {
            let rect = [12.0 + g.x, 12.0 + g.y, g.w, g.h];
            if g.is_rgba {
                instances.push(Instance::sprite_rgba8(
                    rect,
                    g.uv,
                    Color::WHITE.with_alpha(g.color.a),
                    local_clip,
                    16.0,
                ));
            } else {
                instances.push(Instance::mask_r8(rect, g.uv, g.color, local_clip, 16.0));
            }
        }
        draw.ocr_cards.push(OcrCardDraw {
            width: card.width,
            height: card.height,
            quad: aspect_quad,
            instances,
        });
    }

    let layout = SurfaceLayout {
        key,
        regions: Vec::new(),
        menu_bounds: None,
        panels: Vec::new(),
        page_count: 1,
        unplaced: scene_layout.unplaced,
        capture_bounds: Some(scene_layout.capture_bounds),
    };
    (layout, draw)
}
