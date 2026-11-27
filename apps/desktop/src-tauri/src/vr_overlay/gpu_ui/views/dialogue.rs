//! Dialogue wrist/world panel (`768x768`).

use cosmic_text::{Align, Wrap};
use taffy::prelude::*;

use super::{add_circle_button, single_line_style, surface_size, text_style};
use crate::vr_overlay::gpu_ui::SurfaceRuntimeState;
use crate::vr_overlay::gpu_ui::caption::visible_caption_messages;
use crate::vr_overlay::gpu_ui::draw::DrawList;
use crate::vr_overlay::gpu_ui::icons::IconId;
use crate::vr_overlay::gpu_ui::layout::{BoxPaint, RegionPayload, UiBuilder, length, percent};
use crate::vr_overlay::gpu_ui::text::{TextEngine, TextStyle};
use crate::vr_overlay::gpu_ui::theme::Color;
use crate::vr_overlay::gpu_ui::theme::Theme;
use crate::vr_overlay::placement::Mode;
use crate::vr_overlay::presentation::MessageSide;
use crate::vr_overlay::surface_protocol::{SurfaceId, SurfaceLayout, SurfaceState, SurfaceView};

pub fn render_dialogue(
    text: &mut TextEngine,
    state: &SurfaceState,
    runtime: &mut SurfaceRuntimeState,
) -> (SurfaceLayout, DrawList) {
    let SurfaceView::Dialogue {
        visibility,
        max_visible_players,
        max_entries,
        controls,
        messages,
        notice,
        notice_error,
        labels,
        mode,
        follow_view,
        busy,
        error,
        font_size,
        opacity,
    } = &state.view
    else {
        unreachable!();
    };

    let (w, h) = surface_size(SurfaceId::Dialogue);
    let theme = Theme::of(controls.dark);
    let font_px = (*font_size as f32).clamp(16.0, 64.0);
    let visible = visible_caption_messages(
        false,
        messages,
        visibility,
        *max_visible_players,
        *max_entries,
    );
    if visible.is_empty() {
        runtime.dialogue_following = true;
    }

    let mut ui = UiBuilder::new(text, theme, w, h);
    let root = ui.root();

    // Top capsule toolbar (`top: 8, left: 8, right: 8, height: 88`).
    let toolbar = ui.add_box(
        root,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(8.0),
                top: length(8.0),
                right: length(8.0),
                bottom: auto(),
            },
            size: Size {
                width: auto(),
                height: length(88.0),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(8.0),
                height: length(0.0),
            },
            padding: Rect {
                left: length(10.0),
                right: length(10.0),
                top: length(6.0),
                bottom: length(6.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(theme.surface)
            .border(2.0, theme.border)
            .radius(44.0),
    );
    ui.mark_panel(toolbar);

    fn lbl<'a>(
        labels: &'a std::collections::BTreeMap<String, String>,
        key: &str,
        fallback: &'a str,
    ) -> &'a str {
        labels
            .get(key)
            .filter(|v| !v.is_empty())
            .map(String::as_str)
            .unwrap_or(fallback)
    }

    // Grip button (id: 0).
    let grip_hover = !busy && state.hovered == Some(0);
    let grip_press = !busy && state.pressed == Some(0);
    let grip_bg = if grip_hover {
        theme.primary_softer
    } else {
        Color::TRANSPARENT
    };
    let grip_fg = if grip_hover || grip_press {
        theme.primary_ink
    } else {
        theme.text_secondary
    };
    let mut grip_paint = BoxPaint::new().bg(grip_bg).radius(34.0);
    if *busy {
        grip_paint.opacity = 0.5;
    }
    let grip = ui.add_box(
        toolbar,
        Style {
            flex_grow: 1.0,
            min_size: Size {
                width: length(0.0),
                height: length(68.0),
            },
            size: Size {
                width: auto(),
                height: length(68.0),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::Center),
            gap: Size {
                width: length(16.0),
                height: length(0.0),
            },
            padding: Rect {
                left: length(20.0),
                right: length(20.0),
                top: length(0.0),
                bottom: length(0.0),
            },
            ..Default::default()
        },
        grip_paint,
    );
    if !*busy {
        ui.set_region(grip, RegionPayload::Simple(0, false));
    }
    if grip_hover {
        ui.set_tooltip(
            grip,
            lbl(
                &controls.labels,
                "dragHint",
                "Point at the handle and hold grip to drag",
            ),
            true,
        );
    }
    ui.add_icon(grip, IconId::GripHorizontal, 40.0, 1.8, grip_fg);
    let grip_label = if *busy {
        lbl(&controls.labels, "controlBusy", "Working")
    } else {
        controls
            .labels
            .get("dialogueTitle")
            .filter(|v| !v.is_empty())
            .map(String::as_str)
            .unwrap_or_else(|| controls.items[0].label.as_str())
    };
    ui.add_text(
        grip,
        grip_label,
        Style {
            flex_shrink: 1.0,
            min_size: Size {
                width: length(0.0),
                height: auto(),
            },
            ..Default::default()
        },
        single_line_style(26.0, 400, grip_fg, Align::Center),
    );

    // Divider.
    ui.add_box(
        toolbar,
        Style {
            size: Size {
                width: length(2.0),
                height: length(40.0),
            },
            flex_shrink: 0.0,
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

    add_circle_button(
        &mut ui,
        toolbar,
        state,
        1,
        72.0,
        IconId::Hand,
        40.0,
        1.8,
        *mode == Mode::Wrist,
        false,
        *busy,
        Some((lbl(&controls.labels, "anchorWrist", "Wrist"), true)),
        RegionPayload::Simple(1, true),
    );
    add_circle_button(
        &mut ui,
        toolbar,
        state,
        3,
        72.0,
        IconId::Eye,
        40.0,
        1.8,
        *mode == Mode::World && *follow_view,
        false,
        *busy || *mode != Mode::World,
        Some((lbl(&controls.labels, "followView", "Follow view"), true)),
        RegionPayload::Simple(3, true),
    );
    add_circle_button(
        &mut ui,
        toolbar,
        state,
        2,
        72.0,
        IconId::Pin,
        40.0,
        1.8,
        *mode == Mode::World,
        false,
        *busy,
        Some((lbl(&controls.labels, "anchorWorld", "World"), true)),
        RegionPayload::Simple(2, true),
    );

    // Body panel (`top: 112, right: 16, bottom: 16, left: 16`).
    let body = ui.add_box(
        root,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(16.0),
                top: length(112.0),
                right: length(16.0),
                bottom: length(16.0),
            },
            flex_direction: FlexDirection::Column,
            justify_content: Some(JustifyContent::FlexEnd),
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
    ui.mark_panel(body);
    ui.mark_clip(body);

    if let Some(n) = notice.as_deref().filter(|n| !n.is_empty()) {
        let color = if *notice_error {
            theme.error
        } else {
            theme.text_secondary
        };
        ui.add_text(
            body,
            n,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
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
                color,
                ..Default::default()
            },
        );
    }

    let (_, content) = ui.add_scroll_viewport(
        body,
        Style {
            flex_grow: 1.0,
            min_size: Size {
                width: length(0.0),
                height: length(0.0),
            },
            size: Size {
                width: percent(1.0),
                height: auto(),
            },
            ..Default::default()
        },
        runtime.scroll_y,
    );
    if let Some(meta) = ui.tree.get_node_context_mut(content) {
        let _ = meta;
    }
    ui.tree
        .set_style(
            content,
            Style {
                flex_direction: FlexDirection::Column,
                justify_content: Some(JustifyContent::FlexEnd),
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                min_size: Size {
                    width: length(0.0),
                    height: percent(1.0),
                },
                gap: Size {
                    width: length(0.0),
                    height: length(18.0),
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
        .expect("dialogue content style");

    if visible.is_empty() {
        if notice.as_deref().is_none_or(str::is_empty) {
            ui.add_text(
                content,
                lbl(&controls.labels, "emptyDialogue", "No recent dialogue"),
                Style {
                    size: Size {
                        width: percent(1.0),
                        height: auto(),
                    },
                    margin: Rect {
                        top: auto(),
                        bottom: auto(),
                        left: auto(),
                        right: auto(),
                    },
                    ..Default::default()
                },
                TextStyle {
                    font_size: font_px,
                    line_height: (font_px * 1.65).round(),
                    align: Align::Center,
                    color: theme.text_secondary,
                    ..Default::default()
                },
            );
        }
    } else {
        for msg in visible {
            let right = msg.side == MessageSide::Right;
            let mut paint = BoxPaint::new()
                .bg(if right {
                    theme.primary_soft
                } else {
                    theme.surface
                })
                .border(
                    1.0,
                    if right {
                        theme.border_strong
                    } else {
                        theme.border
                    },
                );
            paint.radii = if right {
                [28.0, 6.0, 28.0, 28.0]
            } else {
                [6.0, 28.0, 28.0, 28.0]
            };
            let bubble = ui.add_box(
                content,
                Style {
                    flex_shrink: 0.0,
                    max_size: Size {
                        width: percent(0.92),
                        height: auto(),
                    },
                    align_self: Some(if right {
                        AlignSelf::FlexEnd
                    } else {
                        AlignSelf::FlexStart
                    }),
                    flex_direction: FlexDirection::Column,
                    padding: Rect {
                        left: length(24.0),
                        right: length(24.0),
                        top: length(16.0),
                        bottom: length(16.0),
                    },
                    ..Default::default()
                },
                paint,
            );
            let speaker_row = ui.add_box(
                bubble,
                Style {
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    gap: Size {
                        width: length(font_px * 0.35),
                        height: length(0.0),
                    },
                    margin: Rect {
                        bottom: length(6.0),
                        left: length(0.0),
                        right: length(0.0),
                        top: length(0.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new(),
            );
            let icon =
                IconId::from_message_source(msg.original.as_ref().map(|o| o.source.as_str()));
            ui.add_icon(speaker_row, icon, font_px, 1.8, theme.primary_ink);
            let speaker_name =
                msg.speaker
                    .as_deref()
                    .unwrap_or(if right { &labels.me } else { &labels.other });
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
                single_line_style(font_px, 650, theme.primary_ink, Align::Left),
            );
            ui.add_text(
                bubble,
                &msg.text,
                Style {
                    size: Size {
                        width: percent(1.0),
                        height: auto(),
                    },
                    ..Default::default()
                },
                TextStyle {
                    font_size: font_px,
                    line_height: (font_px * 1.65).round(),
                    color: theme.text,
                    wrap: Wrap::WordOrGlyph,
                    ..Default::default()
                },
            );
        }
    }

    if let Some(err) = error.as_deref().filter(|e| !e.is_empty()) {
        ui.add_text(
            body,
            err,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                margin: Rect {
                    top: length(12.0),
                    left: length(0.0),
                    right: length(0.0),
                    bottom: length(0.0),
                },
                ..Default::default()
            },
            text_style(22.0, 400, theme.error),
        );
    }

    ui.compute_layout();
    if let Some(m) = ui.scroll_measurement() {
        runtime.client_height = m.client_height;
        runtime.scroll_height = m.scroll_height;
        let max_scroll = (m.scroll_height - m.client_height).max(0.0);
        if runtime.dialogue_following {
            runtime.scroll_y = max_scroll;
        } else {
            runtime.scroll_y = runtime.scroll_y.clamp(0.0, max_scroll);
        }
        ui.set_scroll_y(runtime.scroll_y);
    }
    ui.emit(SurfaceId::Dialogue, key_for(state), 1)
}

fn key_for(state: &SurfaceState) -> crate::vr_overlay::surface_protocol::SurfaceKey {
    state.key
}
