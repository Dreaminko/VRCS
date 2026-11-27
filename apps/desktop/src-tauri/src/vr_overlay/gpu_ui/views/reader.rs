//! Reader (`1024x1024`) and PlayerAudioPrompt (`1024x1024` or embedded inside Dashboard).

use cosmic_text::Align;
#[cfg(windows)]
use cosmic_text::Wrap;
use taffy::prelude::*;

#[cfg(windows)]
use super::add_circle_button;
use super::{single_line_style, surface_size, text_style};
use crate::vr_overlay::dashboard::DashboardCommand;
use crate::vr_overlay::gpu_ui::SurfaceRuntimeState;
#[cfg(windows)]
use crate::vr_overlay::gpu_ui::caption::page_geometry;
use crate::vr_overlay::gpu_ui::draw::DrawList;
use crate::vr_overlay::gpu_ui::icons::IconId;
use crate::vr_overlay::gpu_ui::layout::{BoxPaint, RegionPayload, UiBuilder, length, percent};
#[cfg(windows)]
use crate::vr_overlay::gpu_ui::markdown::{MdBlock, MdSpan, parse_markdown};
use crate::vr_overlay::gpu_ui::text::{TextEngine, TextStyle};
use crate::vr_overlay::gpu_ui::theme::{Color, Theme};
use crate::vr_overlay::player_audio_ui::{self, Prompt};
#[cfg(windows)]
use crate::vr_overlay::surface_protocol::SurfaceView;
use crate::vr_overlay::surface_protocol::{SurfaceId, SurfaceLayout, SurfaceState};

#[cfg(windows)]
pub fn render_reader(
    text: &mut TextEngine,
    state: &SurfaceState,
    runtime: &mut SurfaceRuntimeState,
) -> (SurfaceLayout, DrawList) {
    use crate::vr_overlay::ocr_wrist::ViewMode;

    let SurfaceView::Reader {
        dark,
        labels,
        reader,
        font_size,
        opacity,
    } = &state.view
    else {
        unreachable!();
    };

    let (w, h) = surface_size(SurfaceId::Reader);
    let theme = Theme::of(*dark);
    let font_px = (*font_size as f32).clamp(16.0, 64.0);

    let vision = reader.mode == ViewMode::Vision;
    let busy = vision
        && matches!(
            reader.status.as_str(),
            "listening" | "finalizing" | "requesting"
        );
    let vision_error = vision && reader.status == "visionError";
    let is_error = vision_error
        || (!vision
            && matches!(
                reader.status.as_str(),
                "error" | "translation_failed" | "timed_out"
            ));
    let translation = if busy {
        labels
            .text(if reader.status == "listening" {
                "listeningHint"
            } else {
                "requestingHint"
            })
            .to_owned()
    } else if vision_error {
        format!("{}\n\n{}", labels.text("retryHint"), reader.translation)
    } else {
        reader.translation.clone()
    };

    let mut ui = UiBuilder::new(text, theme, w, h);
    let root = ui.root();

    let panel = ui.add_box(
        root,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(24.0),
                top: length(24.0),
                right: length(24.0),
                bottom: length(16.0),
            },
            flex_direction: FlexDirection::Column,
            padding: Rect {
                left: length(24.0),
                right: length(24.0),
                top: length(24.0),
                bottom: length(24.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(theme.panel_bg(*opacity))
            .border(2.0, theme.border)
            .radius(32.0),
    );
    ui.mark_panel(panel);
    ui.mark_clip(panel);

    // Header.
    let header = ui.add_box(
        panel,
        Style {
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::SpaceBetween),
            gap: Size {
                width: length(24.0),
                height: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    let title_row = ui.add_box(
        header,
        Style {
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(14.0),
                height: length(0.0),
            },
            flex_grow: 1.0,
            min_size: Size {
                width: length(0.0),
                height: auto(),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    ui.add_icon(title_row, IconId::ScanEye, 32.0, 1.8, theme.primary_ink);
    ui.add_text(
        title_row,
        labels.text(&reader.title),
        Style {
            flex_shrink: 1.0,
            min_size: Size {
                width: length(0.0),
                height: auto(),
            },
            ..Default::default()
        },
        single_line_style(30.0, 650, theme.text, Align::Left),
    );
    if !vision && reader.block_count > 0 {
        let meta = format!(
            "{} {} / {}",
            labels.text("block"),
            reader.block_index + 1,
            reader.block_count
        );
        ui.add_text(
            header,
            meta,
            Style::default(),
            single_line_style(22.0, 400, theme.text_secondary, Align::Right),
        );
    }

    // Status row.
    let status_row = ui.add_box(
        panel,
        Style {
            min_size: Size {
                width: length(0.0),
                height: length(48.0),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::SpaceBetween),
            gap: Size {
                width: length(16.0),
                height: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    let status_color = if is_error {
        theme.error
    } else {
        theme.text_secondary
    };
    ui.add_text(
        status_row,
        labels.text(&reader.status),
        Style::default(),
        single_line_style(22.0, 400, status_color, Align::Left),
    );
    if !reader.detail.is_empty() {
        ui.add_text(
            status_row,
            &reader.detail,
            Style {
                flex_shrink: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                ..Default::default()
            },
            single_line_style(22.0, 400, status_color, Align::Right),
        );
    }

    // Viewport.
    let (_, content) = ui.add_scroll_viewport(
        panel,
        Style {
            flex_grow: 1.0,
            min_size: Size {
                width: length(0.0),
                height: length(0.0),
            },
            margin: Rect {
                top: length(12.0),
                bottom: length(20.0),
                left: length(0.0),
                right: length(0.0),
            },
            ..Default::default()
        },
        runtime.scroll_y,
    );
    ui.tree
        .set_style(
            content,
            Style {
                flex_direction: FlexDirection::Column,
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                gap: Size {
                    width: length(0.0),
                    height: length(20.0),
                },
                padding: Rect {
                    right: length(10.0),
                    left: length(0.0),
                    top: length(0.0),
                    bottom: length(0.0),
                },
                flex_shrink: 0.0,
                ..Default::default()
            },
        )
        .expect("reader content");

    let line_h = (font_px * 1.65).round();

    if !reader.source.is_empty() {
        let card = ui.add_box(
            content,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                flex_direction: FlexDirection::Column,
                padding: Rect {
                    left: length(24.0),
                    right: length(24.0),
                    top: length(24.0),
                    bottom: length(24.0),
                },
                ..Default::default()
            },
            BoxPaint::new()
                .bg(theme.surface)
                .border(1.0, theme.border)
                .radius(24.0),
        );
        ui.add_text(
            card,
            labels.text(if vision { "question" } else { "source" }),
            Style {
                margin: Rect {
                    bottom: length(16.0),
                    left: length(0.0),
                    right: length(0.0),
                    top: length(0.0),
                },
                ..Default::default()
            },
            text_style(24.0, 600, theme.text_secondary),
        );
        ui.add_text(
            card,
            &reader.source,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                ..Default::default()
            },
            TextStyle {
                font_size: font_px,
                line_height: line_h,
                color: theme.text,
                wrap: Wrap::WordOrGlyph,
                ..Default::default()
            },
        );
    }

    if !reader.source_only && !translation.is_empty() {
        let card = ui.add_box(
            content,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                flex_direction: FlexDirection::Column,
                padding: Rect {
                    left: length(24.0),
                    right: length(24.0),
                    top: length(24.0),
                    bottom: length(24.0),
                },
                ..Default::default()
            },
            BoxPaint::new()
                .bg(if is_error {
                    theme.error_soft
                } else {
                    theme.surface
                })
                .border(
                    1.0,
                    if is_error {
                        theme.error_border
                    } else {
                        theme.border
                    },
                )
                .radius(24.0),
        );
        if !vision_error {
            let heading = if reader.translation_label.is_empty() {
                labels.text("answer")
            } else {
                reader.translation_label.as_str()
            };
            ui.add_text(
                card,
                heading,
                Style {
                    margin: Rect {
                        bottom: length(16.0),
                        left: length(0.0),
                        right: length(0.0),
                        top: length(0.0),
                    },
                    ..Default::default()
                },
                text_style(
                    24.0,
                    600,
                    if is_error {
                        theme.error
                    } else {
                        theme.text_secondary
                    },
                ),
            );
        }
        if vision && reader.status == "complete" {
            let blocks = parse_markdown(&translation);
            emit_markdown_blocks(&mut ui, card, &blocks, font_px, theme);
        } else {
            ui.add_text(
                card,
                &translation,
                Style {
                    size: Size {
                        width: percent(1.0),
                        height: auto(),
                    },
                    ..Default::default()
                },
                TextStyle {
                    font_size: font_px,
                    line_height: line_h,
                    color: if is_error { theme.error } else { theme.text },
                    wrap: Wrap::WordOrGlyph,
                    ..Default::default()
                },
            );
        }
    }

    // Bottom capsule toolbar.
    let toolbar = ui.add_box(
        panel,
        Style {
            align_self: Some(AlignSelf::Center),
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(8.0),
                height: length(0.0),
            },
            padding: Rect {
                left: length(8.0),
                right: length(8.0),
                top: length(8.0),
                bottom: length(8.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(theme.surface)
            .border(2.0, theme.border)
            .radius(999.0),
    );

    if !vision {
        add_circle_button(
            &mut ui,
            toolbar,
            state,
            0,
            68.0,
            IconId::ChevronLeft,
            40.0,
            1.8,
            false,
            false,
            reader.block_index == 0,
            Some((labels.text("previousBlock"), false)),
            RegionPayload::Simple(0, true),
        );
        add_circle_button(
            &mut ui,
            toolbar,
            state,
            1,
            68.0,
            IconId::ChevronRight,
            40.0,
            1.8,
            false,
            false,
            reader.block_index + 1 >= reader.block_count,
            Some((labels.text("nextBlock"), false)),
            RegionPayload::Simple(1, true),
        );
        if reader.language_count > 1 {
            add_circle_button(
                &mut ui,
                toolbar,
                state,
                4,
                68.0,
                IconId::Languages,
                40.0,
                1.8,
                false,
                false,
                false,
                Some((labels.text("language"), false)),
                RegionPayload::Simple(4, true),
            );
        }
    } else if !reader.source.is_empty() {
        add_circle_button(
            &mut ui,
            toolbar,
            state,
            5,
            68.0,
            IconId::MessageSquare,
            40.0,
            1.8,
            reader.source_only,
            false,
            false,
            Some((
                labels.text(if reader.source_only {
                    "backToAnswer"
                } else {
                    "question"
                }),
                false,
            )),
            RegionPayload::Simple(5, true),
        );
    }

    if !vision || !reader.source.is_empty() {
        ui.add_box(
            toolbar,
            Style {
                size: Size {
                    width: length(2.0),
                    height: length(40.0),
                },
                margin: Rect {
                    left: length(10.0),
                    right: length(10.0),
                    top: length(0.0),
                    bottom: length(0.0),
                },
                ..Default::default()
            },
            BoxPaint::new().bg(theme.border),
        );
    }

    add_circle_button(
        &mut ui,
        toolbar,
        state,
        6,
        68.0,
        if reader.pinned {
            IconId::PinOff
        } else {
            IconId::Pin
        },
        40.0,
        1.8,
        reader.pinned,
        false,
        false,
        Some((
            labels.text(if reader.pinned { "unpin" } else { "pin" }),
            false,
        )),
        RegionPayload::Simple(6, true),
    );
    add_circle_button(
        &mut ui,
        toolbar,
        state,
        7,
        68.0,
        IconId::X,
        40.0,
        1.8,
        false,
        false,
        false,
        Some((labels.text(if busy { "cancel" } else { "close" }), false)),
        RegionPayload::Simple(7, true),
    );

    ui.compute_layout();
    let mut page_count = 1;
    if let Some(m) = ui.scroll_measurement() {
        runtime.client_height = m.client_height;
        runtime.scroll_height = m.scroll_height;
        let geom = page_geometry(m.scroll_height, m.client_height, reader.page, line_h);
        page_count = geom.count;
        let max_scroll = (m.scroll_height - m.client_height).max(0.0);
        // Apply page offset only on initial page presentation, never snapping back manual scrolling.
        runtime.scroll_y = if runtime.reader_needs_page_sync {
            runtime.reader_needs_page_sync = false;
            geom.offset.clamp(0.0, max_scroll)
        } else {
            runtime.scroll_y.clamp(0.0, max_scroll)
        };
        ui.set_scroll_y(runtime.scroll_y);
    }
    ui.emit(SurfaceId::Reader, state.key, page_count)
}

#[cfg(windows)]
fn spans_to_rich(
    spans: &[MdSpan],
    size: f32,
    line_h: f32,
    color: Color,
) -> Vec<(String, TextStyle)> {
    spans
        .iter()
        .map(|s| {
            (
                s.text.clone(),
                TextStyle {
                    font_size: if s.code { size * 0.9 } else { size },
                    line_height: line_h,
                    weight: if s.bold { 700 } else { 400 },
                    italic: s.italic,
                    mono: s.code,
                    color,
                    ..Default::default()
                },
            )
        })
        .collect()
}

#[cfg(windows)]
fn emit_markdown_blocks(
    ui: &mut UiBuilder<'_>,
    parent: NodeId,
    blocks: &[MdBlock],
    font_px: f32,
    theme: Theme,
) {
    let line_h = (font_px * 1.5).round();
    let container = ui.add_box(
        parent,
        Style {
            size: Size {
                width: percent(1.0),
                height: auto(),
            },
            flex_direction: FlexDirection::Column,
            gap: Size {
                width: length(0.0),
                height: length(font_px * 0.75),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );

    for block in blocks {
        match block {
            MdBlock::Heading { level, spans } => {
                let scale = match level {
                    1 => 1.4,
                    2 => 1.25,
                    _ => 1.1,
                };
                let hs = font_px * scale;
                let base = TextStyle {
                    font_size: hs,
                    line_height: (hs * 1.4).round(),
                    weight: 650,
                    color: theme.text,
                    ..Default::default()
                };
                let rich = spans_to_rich(spans, hs, base.line_height, theme.text);
                ui.add_rich_text(
                    container,
                    rich,
                    Style {
                        size: Size {
                            width: percent(1.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    base,
                );
            }
            MdBlock::Paragraph(spans) => {
                let base = TextStyle {
                    font_size: font_px,
                    line_height: line_h,
                    color: theme.text,
                    ..Default::default()
                };
                let rich = spans_to_rich(spans, font_px, line_h, theme.text);
                ui.add_rich_text(
                    container,
                    rich,
                    Style {
                        size: Size {
                            width: percent(1.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    base,
                );
            }
            MdBlock::Blockquote(inner) => {
                let bq = ui.add_box(
                    container,
                    Style {
                        size: Size {
                            width: percent(1.0),
                            height: auto(),
                        },
                        flex_direction: FlexDirection::Row,
                        gap: Size {
                            width: length(font_px * 0.8),
                            height: length(0.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new(),
                );
                ui.add_box(
                    bq,
                    Style {
                        size: Size {
                            width: length(3.0),
                            height: auto(),
                        },
                        flex_shrink: 0.0,
                        ..Default::default()
                    },
                    BoxPaint::new().bg(theme.border_strong),
                );
                let col = ui.add_box(
                    bq,
                    Style {
                        flex_grow: 1.0,
                        min_size: Size {
                            width: length(0.0),
                            height: auto(),
                        },
                        flex_direction: FlexDirection::Column,
                        ..Default::default()
                    },
                    BoxPaint::new(),
                );
                emit_markdown_blocks(ui, col, inner, font_px, theme);
            }
            MdBlock::CodeBlock { code } => {
                let box_node = ui.add_box(
                    container,
                    Style {
                        size: Size {
                            width: percent(1.0),
                            height: auto(),
                        },
                        padding: Rect {
                            left: length(12.0),
                            right: length(12.0),
                            top: length(12.0),
                            bottom: length(12.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new()
                        .bg(theme.surface_subtle)
                        .border(1.0, theme.border)
                        .radius(16.0),
                );
                ui.add_text(
                    box_node,
                    code,
                    Style {
                        size: Size {
                            width: percent(1.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    TextStyle {
                        font_size: font_px * 0.9,
                        line_height: (font_px * 0.9 * 1.45).round(),
                        mono: true,
                        color: theme.text,
                        wrap: Wrap::WordOrGlyph,
                        ..Default::default()
                    },
                );
            }
            MdBlock::List {
                ordered,
                start,
                items,
            } => {
                let list_box = ui.add_box(
                    container,
                    Style {
                        size: Size {
                            width: percent(1.0),
                            height: auto(),
                        },
                        flex_direction: FlexDirection::Column,
                        gap: Size {
                            width: length(0.0),
                            height: length(font_px * 0.3),
                        },
                        padding: Rect {
                            left: length(font_px * 0.6),
                            right: length(0.0),
                            top: length(0.0),
                            bottom: length(0.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new(),
                );
                for (idx, item) in items.iter().enumerate() {
                    let row = ui.add_box(
                        list_box,
                        Style {
                            size: Size {
                                width: percent(1.0),
                                height: auto(),
                            },
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::FlexStart),
                            gap: Size {
                                width: length(font_px * 0.5),
                                height: length(0.0),
                            },
                            ..Default::default()
                        },
                        BoxPaint::new(),
                    );
                    let marker = match item.checked {
                        Some(true) => "☑".to_owned(),
                        Some(false) => "☐".to_owned(),
                        None if *ordered => format!("{}.", start + idx as u64),
                        None => "•".to_owned(),
                    };
                    let marker_color = if item.checked == Some(true) {
                        theme.primary_ink
                    } else {
                        theme.text_secondary
                    };
                    ui.add_text(
                        row,
                        marker,
                        Style {
                            flex_shrink: 0.0,
                            ..Default::default()
                        },
                        TextStyle {
                            font_size: font_px,
                            line_height: line_h,
                            color: marker_color,
                            wrap: Wrap::None,
                            ..Default::default()
                        },
                    );
                    let body = ui.add_box(
                        row,
                        Style {
                            flex_grow: 1.0,
                            min_size: Size {
                                width: length(0.0),
                                height: auto(),
                            },
                            flex_direction: FlexDirection::Column,
                            ..Default::default()
                        },
                        BoxPaint::new(),
                    );
                    emit_markdown_blocks(ui, body, &item.blocks, font_px, theme);
                }
            }
            MdBlock::Rule => {
                ui.add_box(
                    container,
                    Style {
                        size: Size {
                            width: percent(1.0),
                            height: length(1.0),
                        },
                        margin: Rect {
                            top: length(font_px * 0.4),
                            bottom: length(font_px * 0.4),
                            left: length(0.0),
                            right: length(0.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new().bg(theme.border),
                );
            }
            MdBlock::Table { headers, rows } => {
                // VerticalTable card layout matching VisionAnswerMarkdown.tsx.
                let cards = ui.add_box(
                    container,
                    Style {
                        size: Size {
                            width: percent(1.0),
                            height: auto(),
                        },
                        flex_direction: FlexDirection::Column,
                        gap: Size {
                            width: length(0.0),
                            height: length(font_px * 0.8),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new(),
                );
                let data_rows: &[Vec<Vec<MdSpan>>] = if rows.is_empty() && !headers.is_empty() {
                    std::slice::from_ref(headers)
                } else {
                    rows.as_slice()
                };
                let show_labels = !headers.is_empty() && !rows.is_empty();
                for row_cells in data_rows {
                    let row_card = ui.add_box(
                        cards,
                        Style {
                            size: Size {
                                width: percent(1.0),
                                height: auto(),
                            },
                            flex_direction: FlexDirection::Column,
                            ..Default::default()
                        },
                        BoxPaint::new().border(1.0, theme.border).radius(16.0),
                    );
                    for (col_idx, cell_spans) in row_cells.iter().enumerate() {
                        let cell_box = ui.add_box(
                            row_card,
                            Style {
                                size: Size {
                                    width: percent(1.0),
                                    height: auto(),
                                },
                                flex_direction: FlexDirection::Column,
                                gap: Size {
                                    width: length(0.0),
                                    height: length(font_px * 0.2),
                                },
                                padding: Rect {
                                    left: length(font_px * 0.8),
                                    right: length(font_px * 0.8),
                                    top: length(font_px * 0.6),
                                    bottom: length(font_px * 0.6),
                                },
                                ..Default::default()
                            },
                            if col_idx > 0 {
                                BoxPaint::new().border(1.0, theme.border)
                            } else {
                                BoxPaint::new()
                            },
                        );
                        if show_labels
                            && let Some(lbl_spans) = headers.get(col_idx)
                            && !lbl_spans.is_empty()
                        {
                            let lbl_text: String =
                                lbl_spans.iter().map(|s| s.text.as_str()).collect();
                            ui.add_text(
                                cell_box,
                                lbl_text,
                                Style {
                                    size: Size {
                                        width: percent(1.0),
                                        height: auto(),
                                    },
                                    ..Default::default()
                                },
                                text_style(font_px * 0.85, 600, theme.text_secondary),
                            );
                        }
                        let base = TextStyle {
                            font_size: font_px,
                            line_height: line_h,
                            color: theme.text,
                            ..Default::default()
                        };
                        let rich = spans_to_rich(cell_spans, font_px, line_h, theme.text);
                        ui.add_rich_text(
                            cell_box,
                            rich,
                            Style {
                                size: Size {
                                    width: percent(1.0),
                                    height: auto(),
                                },
                                ..Default::default()
                            },
                            base,
                        );
                    }
                }
            }
        }
    }
}

pub fn render_player_audio_standalone(
    text: &mut TextEngine,
    state: &SurfaceState,
    dark: bool,
    notice: &Prompt,
    runtime: &mut SurfaceRuntimeState,
) -> (SurfaceLayout, DrawList) {
    let (w, h) = surface_size(SurfaceId::PlayerAudio);
    let theme = Theme::of(dark);
    let mut ui = UiBuilder::new(text, theme, w, h);
    let root = ui.root();
    build_player_audio_prompt(&mut ui, root, state, notice, false, runtime);
    ui.compute_layout();
    if let Some(m) = ui.scroll_measurement() {
        runtime.client_height = m.client_height;
        runtime.scroll_height = m.scroll_height;
        let max_scroll = (m.scroll_height - m.client_height).max(0.0);
        runtime.scroll_y = runtime.scroll_y.clamp(0.0, max_scroll);
        ui.set_scroll_y(runtime.scroll_y);
    }
    ui.emit(SurfaceId::PlayerAudio, state.key, 1)
}

pub fn build_player_audio_prompt(
    ui: &mut UiBuilder<'_>,
    parent: NodeId,
    state: &SurfaceState,
    notice: &Prompt,
    dashboard: bool,
    runtime: &mut SurfaceRuntimeState,
) {
    let theme = ui.theme;
    let inset_lr = if dashboard { 170.0 } else { 16.0 };
    let inset_tb = if dashboard { 24.0 } else { 16.0 };
    let panel = ui.add_box(
        parent,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(inset_lr),
                right: length(inset_lr),
                top: length(inset_tb),
                bottom: length(inset_tb),
            },
            flex_direction: FlexDirection::Column,
            gap: Size {
                width: length(0.0),
                height: length(16.0),
            },
            padding: Rect {
                left: length(32.0),
                right: length(32.0),
                top: length(32.0),
                bottom: length(32.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(theme.surface)
            .border(2.0, theme.border)
            .radius(28.0),
    );
    if !dashboard {
        ui.mark_panel(panel);
    }
    ui.mark_clip(panel);

    let header = ui.add_box(
        panel,
        Style {
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(16.0),
                height: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    ui.add_icon(header, IconId::FlaskConical, 40.0, 1.8, theme.text);
    ui.add_text(
        header,
        &notice.title,
        Style {
            flex_grow: 1.0,
            min_size: Size {
                width: length(0.0),
                height: auto(),
            },
            ..Default::default()
        },
        TextStyle {
            font_size: 32.0,
            line_height: (32.0f32 * 1.35).round(),
            weight: 650,
            color: theme.text,
            ..Default::default()
        },
    );

    ui.add_text(
        panel,
        &notice.scroll_hint,
        Style {
            size: Size {
                width: percent(1.0),
                height: auto(),
            },
            ..Default::default()
        },
        text_style(22.0, 400, theme.text_secondary),
    );

    let (_, content) = ui.add_scroll_viewport(
        panel,
        Style {
            flex_grow: 1.0,
            min_size: Size {
                width: length(0.0),
                height: length(0.0),
            },
            padding: Rect {
                right: length(18.0),
                left: length(0.0),
                top: length(0.0),
                bottom: length(0.0),
            },
            ..Default::default()
        },
        runtime.scroll_y,
    );
    ui.tree
        .set_style(
            content,
            Style {
                flex_direction: FlexDirection::Column,
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                gap: Size {
                    width: length(0.0),
                    height: length(20.0),
                },
                flex_shrink: 0.0,
                ..Default::default()
            },
        )
        .expect("player audio content");

    ui.add_text(
        content,
        &notice.intro,
        Style {
            size: Size {
                width: percent(1.0),
                height: auto(),
            },
            ..Default::default()
        },
        TextStyle {
            font_size: 28.0,
            line_height: (28.0f32 * 1.5).round(),
            color: theme.text,
            ..Default::default()
        },
    );
    for section in &notice.sections {
        ui.add_text(
            content,
            &section.title,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                margin: Rect {
                    top: length(8.0),
                    left: length(0.0),
                    right: length(0.0),
                    bottom: length(0.0),
                },
                ..Default::default()
            },
            text_style(28.0, 650, theme.text),
        );
        for p in &section.paragraphs {
            ui.add_text(
                content,
                p,
                Style {
                    size: Size {
                        width: percent(1.0),
                        height: auto(),
                    },
                    ..Default::default()
                },
                TextStyle {
                    font_size: 28.0,
                    line_height: (28.0f32 * 1.5).round(),
                    color: theme.text,
                    ..Default::default()
                },
            );
        }
    }

    let footer = ui.add_box(
        panel,
        Style {
            flex_direction: FlexDirection::Column,
            gap: Size {
                width: length(0.0),
                height: length(12.0),
            },
            padding: Rect {
                top: length(16.0),
                left: length(0.0),
                right: length(0.0),
                bottom: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new().border(2.0, theme.border),
    );
    ui.add_text(
        footer,
        &notice.reading_status,
        Style {
            size: Size {
                width: percent(1.0),
                height: auto(),
            },
            ..Default::default()
        },
        text_style(22.0, 400, theme.text_secondary),
    );
    let nav = ui.add_box(
        footer,
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

    let add_btn = |ui: &mut UiBuilder<'_>,
                   id: u8,
                   action: player_audio_ui::Action,
                   label: &str,
                   icon: Option<IconId>,
                   primary: bool,
                   disabled: bool| {
        let hovered = !disabled && state.hovered == Some(id);
        let pressed = !disabled && state.pressed == Some(id);
        let bg = if primary {
            theme.primary
        } else {
            theme.surface_subtle
        };
        let bc = if primary {
            theme.primary
        } else {
            theme.border_strong
        };
        let fg = if primary {
            Color::rgb(0x12, 0x23, 0x2c)
        } else {
            theme.text
        };
        let mut paint = BoxPaint::new().bg(bg).border(2.0, bc).radius(16.0);
        if hovered || pressed {
            paint = paint.outline(3.0, -4.0, theme.primary_ink, 0);
        }
        if disabled {
            paint.opacity = 0.45;
        }
        let btn = ui.add_box(
            nav,
            Style {
                min_size: Size {
                    width: length(72.0),
                    height: length(68.0),
                },
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                padding: Rect {
                    left: length(22.0),
                    right: length(22.0),
                    top: length(12.0),
                    bottom: length(12.0),
                },
                ..Default::default()
            },
            paint,
        );
        if !disabled {
            let payload = if dashboard {
                RegionPayload::Dashboard(
                    id,
                    DashboardCommand::PlayerAudio {
                        request_id: notice.request_id,
                        action,
                    },
                )
            } else {
                RegionPayload::Simple(id, false)
            };
            ui.set_region(btn, payload);
        }
        if let Some(ic) = icon {
            ui.add_icon(btn, ic, 40.0, 1.8, fg);
        } else {
            ui.add_text(
                btn,
                label,
                Style::default(),
                single_line_style(24.0, 400, fg, Align::Center),
            );
        }
    };

    add_btn(
        ui,
        2,
        player_audio_ui::Action::Previous,
        &notice.previous,
        Some(IconId::ChevronLeft),
        false,
        false,
    );
    add_btn(
        ui,
        3,
        player_audio_ui::Action::Next,
        &notice.next,
        Some(IconId::ChevronRight),
        false,
        false,
    );
    ui.add_box(
        nav,
        Style {
            flex_grow: 1.0,
            ..Default::default()
        },
        BoxPaint::new(),
    );
    add_btn(
        ui,
        0,
        player_audio_ui::Action::Cancel,
        &notice.cancel,
        None,
        false,
        notice.remaining_seconds > 0,
    );
    add_btn(
        ui,
        1,
        player_audio_ui::Action::Confirm,
        &notice.confirm,
        None,
        true,
        notice.remaining_seconds > 0,
    );
}
