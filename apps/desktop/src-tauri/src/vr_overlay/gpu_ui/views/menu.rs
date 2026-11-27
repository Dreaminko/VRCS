//! Wrist menu capsule & PlayerDisplayMenu (`1280x1280`).

use std::time::Instant;

use cosmic_text::Align;
use taffy::prelude::*;

use super::{single_line_style, surface_size, text_style};
use crate::vr_overlay::control_protocol::ControlAction;
use crate::vr_overlay::gpu_ui::SurfaceRuntimeState;
use crate::vr_overlay::gpu_ui::draw::{DrawList, Instance};
use crate::vr_overlay::gpu_ui::icons::IconId;
use crate::vr_overlay::gpu_ui::layout::{BoxPaint, RegionPayload, UiBuilder, length, percent};
use crate::vr_overlay::gpu_ui::text::{TextEngine, TextStyle};
use crate::vr_overlay::gpu_ui::theme::{Color, Theme};
use crate::vr_overlay::player_display;
use crate::vr_overlay::surface_protocol::{SurfaceId, SurfaceLayout, SurfaceState, SurfaceView};
use crate::vr_overlay::wrist_menu::{
    Command, Group, LANGUAGES_PER_PAGE, PLAYERS_PER_PAGE, Page, Slider, SliderAction,
};

const PROXIMITY_SLIDER: Slider = Slider {
    action: SliderAction::ProximityRadius,
    min: 0.0,
    max: 10.0,
    step: 0.1,
};
const VIEW_ANGLE_SLIDER: Slider = Slider {
    action: SliderAction::ViewHorizontalFov,
    min: 30.0,
    max: 180.0,
    step: 5.0,
};

pub fn render_menu(
    text: &mut TextEngine,
    state: &SurfaceState,
    _runtime: &mut SurfaceRuntimeState,
    _now: Instant,
) -> (SurfaceLayout, DrawList) {
    let SurfaceView::Menu {
        controls,
        players,
        ui: menu_ui,
        hold_progress,
    } = &state.view
    else {
        unreachable!();
    };

    let (w, h) = surface_size(SurfaceId::Menu);
    let theme = Theme::of(controls.dark);
    let mut ui = UiBuilder::new(text, theme, w, h);
    let root = ui.root();

    if menu_ui.page != Page::Home {
        build_player_display_panel(&mut ui, root, state, players, menu_ui);
    }

    // Bottom forearm capsule (`left: 16, right: 16, bottom: 8, height: 240`).
    let capsule = ui.add_box(
        root,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(16.0),
                right: length(16.0),
                bottom: length(8.0),
                top: auto(),
            },
            size: Size {
                width: auto(),
                height: length(240.0),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::SpaceAround),
            padding: Rect {
                left: length(16.0),
                right: length(16.0),
                top: length(16.0),
                bottom: length(16.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(theme.surface)
            .border(2.0, theme.border)
            .radius(120.0),
    );
    ui.mark_menu_capsule(capsule);

    let actions = [
        ControlAction::SetWristEnabled(!controls.items[0].requested),
        ControlAction::SetHeadsetEnabled(!controls.items[1].requested),
        ControlAction::SetOscEnabled(!controls.items[2].requested),
        ControlAction::SetCaptureRunning(!controls.items[3].requested),
        ControlAction::SetOcrGestureEnabled(!controls.items[4].requested),
        ControlAction::SetNetworkCaptureRunning(!controls.items[5].requested),
    ];
    let icons = [
        IconId::MessageSquare,
        IconId::Captions,
        IconId::Radio,
        if controls.items[3].requested {
            IconId::Square
        } else {
            IconId::Mic
        },
        IconId::Crop,
        IconId::Radar,
    ];

    for id in [0u8, 1, 2, 30, 4, 55, 3] {
        if id == 4 {
            let active = menu_ui.page != Page::Home;
            let hovered = state.hovered == Some(4);
            let pressed = state.pressed == Some(4);
            let bg = if active {
                theme.primary_soft
            } else if hovered {
                theme.primary_softer
            } else {
                Color::TRANSPARENT
            };
            let fg = if active || hovered {
                theme.primary_ink
            } else {
                theme.text_secondary
            };
            let btn = ui.add_box(
                capsule,
                Style {
                    size: Size {
                        width: length(160.0),
                        height: length(160.0),
                    },
                    flex_shrink: 1.0,
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::Center),
                    ..Default::default()
                },
                BoxPaint::new().bg(bg).radius(80.0),
            );
            ui.set_region(
                btn,
                RegionPayload::Menu(
                    4,
                    true,
                    Command::Page(if menu_ui.page == Page::Home {
                        Page::Players
                    } else {
                        Page::Home
                    }),
                ),
            );
            if hovered {
                let tip = players
                    .labels
                    .get("title")
                    .filter(|s| !s.is_empty())
                    .map(String::as_str)
                    .unwrap_or("Capture management");
                ui.set_tooltip(btn, tip, false);
            }
            let ic = ui.add_icon(btn, IconId::UsersRound, 80.0, 1.8, fg);
            if pressed && let Some(meta) = ui.tree.get_node_context_mut(ic) {
                meta.paint.scale = 0.95;
            }
            continue;
        }

        let index = match id {
            30 => 4,
            55 => 5,
            _ => id as usize,
        };
        let item = &controls.items[index];
        if item.hidden {
            continue;
        }
        let disabled = !item.available || item.busy;
        let active = item.requested;
        let primary = index == 3;
        let hovered = !disabled && state.hovered == Some(id);
        let pressed = !disabled && state.pressed == Some(id);

        let status = if item.busy {
            "busy"
        } else if item.reason.is_some() || !item.available {
            "unavailable"
        } else if item.requested && !item.effective {
            "waiting"
        } else {
            "idle"
        };
        let detail = if status == "busy" {
            Some(
                controls
                    .labels
                    .get("controlBusy")
                    .filter(|s| !s.is_empty())
                    .map(String::as_str)
                    .unwrap_or("Working"),
            )
        } else if status == "waiting" {
            Some(
                controls
                    .labels
                    .get("waitingContent")
                    .filter(|s| !s.is_empty())
                    .map(String::as_str)
                    .unwrap_or("Enabled, waiting for content"),
            )
        } else {
            item.reason.as_deref()
        };

        let bg = if primary {
            if hovered || pressed {
                theme.primary_hover
            } else {
                theme.primary
            }
        } else if active {
            theme.primary_soft
        } else if hovered {
            theme.primary_softer
        } else {
            Color::TRANSPARENT
        };
        let fg = if primary {
            Color::WHITE
        } else if active || hovered {
            theme.primary_ink
        } else {
            theme.text_secondary
        };

        let mut paint = BoxPaint::new().bg(bg).radius(80.0);
        if disabled {
            paint.opacity = 0.5;
        }
        if status == "waiting" {
            paint = paint.outline(2.0, -3.0, fg, Instance::FLAG_DASHED);
        } else if status == "busy" {
            paint = paint.outline(3.0, 5.0, fg, Instance::FLAG_DOTTED);
        }

        let btn = ui.add_box(
            capsule,
            Style {
                size: Size {
                    width: length(160.0),
                    height: length(160.0),
                },
                flex_shrink: 1.0,
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                ..Default::default()
            },
            paint,
        );
        if !disabled {
            ui.set_region(
                btn,
                RegionPayload::Menu(id, true, Command::Control(actions[index])),
            );
        }
        if hovered {
            let tip = match detail {
                Some(d) => format!("{} · {d}", item.label),
                None => item.label.clone(),
            };
            ui.set_tooltip(btn, tip, false);
        }
        let ic = ui.add_icon(btn, icons[index], 80.0, 1.8, fg);
        if pressed && let Some(meta) = ui.tree.get_node_context_mut(ic) {
            meta.paint.scale = 0.95;
        }
        if status == "unavailable" {
            ui.add_box(
                btn,
                Style {
                    position: Position::Absolute,
                    inset: Rect {
                        top: length(4.0),
                        right: length(4.0),
                        left: auto(),
                        bottom: auto(),
                    },
                    size: Size {
                        width: length(12.0),
                        height: length(12.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new()
                    .bg(theme.error)
                    .border(2.0, theme.surface)
                    .radius(6.0),
            );
        }
        if index == 0
            && state.pressed == Some(0)
            && let Some(prog) = hold_progress
        {
            // Track circle + progress arc.
            ui.add_arc(
                btn,
                160.0,
                0.0,
                std::f32::consts::TAU,
                6.0,
                theme.primary_ink.with_alpha(0.2),
            );
            ui.add_arc(
                btn,
                160.0,
                -std::f32::consts::FRAC_PI_2,
                prog.clamp(0.0, 1.0) * std::f32::consts::TAU,
                6.0,
                theme.primary_ink,
            );
        }
    }

    if controls.labels.contains_key("settings_available") {
        let hovered = state.hovered == Some(31);
        let pressed = state.pressed == Some(31);
        let bg = if hovered {
            theme.primary_softer
        } else {
            Color::TRANSPARENT
        };
        let fg = if hovered {
            theme.primary_ink
        } else {
            theme.text_secondary
        };
        let btn = ui.add_box(
            capsule,
            Style {
                size: Size {
                    width: length(160.0),
                    height: length(160.0),
                },
                flex_shrink: 1.0,
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                ..Default::default()
            },
            BoxPaint::new().bg(bg).radius(80.0),
        );
        ui.set_region(btn, RegionPayload::Menu(31, true, Command::Dashboard));
        if hovered {
            let tip = controls
                .labels
                .get("settings")
                .filter(|s| !s.is_empty())
                .map(String::as_str)
                .unwrap_or("Settings");
            ui.set_tooltip(btn, tip, false);
        }
        let ic = ui.add_icon(btn, IconId::Settings2, 80.0, 1.8, fg);
        if pressed && let Some(meta) = ui.tree.get_node_context_mut(ic) {
            meta.paint.scale = 0.95;
        }
    }

    ui.compute_layout();
    ui.emit(SurfaceId::Menu, state.key, 1)
}

fn build_player_display_panel(
    ui: &mut UiBuilder<'_>,
    root: NodeId,
    state: &SurfaceState,
    view: &player_display::View,
    menu_ui: &crate::vr_overlay::wrist_menu::Ui,
) {
    let t = ui.theme;
    fn label<'a>(
        labels: &'a std::collections::BTreeMap<String, String>,
        k: &str,
        fallback: &'a str,
    ) -> &'a str {
        labels
            .get(k)
            .filter(|v| !v.is_empty())
            .map(String::as_str)
            .unwrap_or(fallback)
    }
    let locked = !view.available;

    let panel = ui.add_box(
        root,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(16.0),
                top: length(16.0),
                right: length(16.0),
                bottom: length(272.0),
            },
            flex_direction: FlexDirection::Column,
            gap: Size {
                width: length(0.0),
                height: length(10.0),
            },
            padding: Rect {
                left: length(20.0),
                right: length(20.0),
                top: length(20.0),
                bottom: length(20.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(t.surface)
            .border(2.0, t.border)
            .radius(32.0),
    );
    ui.mark_panel(panel);
    ui.mark_clip(panel);

    let menu_btn = |ui: &mut UiBuilder<'_>,
                    parent: NodeId,
                    id: u8,
                    cmd: Command,
                    active: bool,
                    disabled: bool,
                    style: Style,
                    radius: f32,
                    extra_bg: Option<Color>,
                    extra_border: Option<Color>|
     -> (NodeId, Color) {
        let hovered = !disabled && state.hovered == Some(id);
        let pressed = !disabled && state.pressed == Some(id);
        let bg = extra_bg.unwrap_or(if active || pressed {
            t.primary_soft
        } else {
            t.surface_subtle
        });
        let border = extra_border.unwrap_or(if active {
            t.primary_ink
        } else {
            t.border_strong
        });
        let fg = if active { t.primary_ink } else { t.text };
        let mut paint = BoxPaint::new().bg(bg).border(2.0, border).radius(radius);
        if hovered || pressed {
            paint = paint.outline(3.0, -5.0, t.primary_ink, 0);
        }
        if disabled {
            paint.opacity = 0.42;
        }
        let node = ui.add_box(parent, style, paint);
        if !disabled {
            ui.set_region(node, RegionPayload::Menu(id, false, cmd));
        }
        (node, fg)
    };

    // Heading.
    let heading = ui.add_box(
        panel,
        Style {
            min_size: Size {
                width: length(0.0),
                height: length(72.0),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::SpaceBetween),
            gap: Size {
                width: length(20.0),
                height: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    let head_copy = ui.add_box(
        heading,
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
    ui.add_text(
        head_copy,
        label(&view.labels, "title", "Capture management"),
        Style::default(),
        single_line_style(32.0, 650, t.text, Align::Left),
    );
    let status_msg = view.error.as_deref().unwrap_or(if !view.available {
        label(
            &view.labels,
            "unavailable",
            "Player settings are unavailable",
        )
    } else {
        ""
    });
    if !status_msg.is_empty() {
        ui.add_text(
            head_copy,
            status_msg,
            Style {
                margin: Rect {
                    top: length(4.0),
                    left: length(0.0),
                    right: length(0.0),
                    bottom: length(0.0),
                },
                ..Default::default()
            },
            TextStyle {
                font_size: 22.0,
                line_height: 26.0,
                max_lines: Some(2),
                color: if view.error.is_some() {
                    t.error
                } else {
                    t.text_secondary
                },
                ..Default::default()
            },
        );
    }
    let (close_btn, close_fg) = menu_btn(
        ui,
        heading,
        5,
        Command::Page(Page::Home),
        false,
        false,
        Style {
            size: Size {
                width: length(72.0),
                height: length(72.0),
            },
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::Center),
            ..Default::default()
        },
        16.0,
        None,
        None,
    );
    ui.add_icon(close_btn, IconId::X, 32.0, 1.8, close_fg);

    // Tabs (`6`: players, `52`: rules, `7`: languages).
    let tabs = ui.add_box(
        panel,
        Style {
            size: Size {
                width: auto(),
                height: length(72.0),
            },
            flex_direction: FlexDirection::Row,
            gap: Size {
                width: length(10.0),
                height: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    let hidden_lang_count = view.languages.iter().filter(|l| l.hidden).count();
    for (id, pg, icon, title, count) in [
        (
            6u8,
            Page::Players,
            IconId::UsersRound,
            label(&view.labels, "playersTab", "People"),
            0usize,
        ),
        (
            52,
            Page::Rules,
            IconId::SlidersHorizontal,
            label(&view.labels, "rulesTab", "Rules"),
            0,
        ),
        (
            7,
            Page::Languages,
            IconId::Languages,
            label(&view.labels, "languagesTab", "Languages"),
            hidden_lang_count,
        ),
    ] {
        let active = menu_ui.page == pg;
        let (btn, fg) = menu_btn(
            ui,
            tabs,
            id,
            Command::Page(pg),
            active,
            false,
            Style {
                flex_grow: 1.0,
                flex_basis: length(0.0),
                min_size: Size {
                    width: length(0.0),
                    height: length(72.0),
                },
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                gap: Size {
                    width: length(10.0),
                    height: length(0.0),
                },
                padding: Rect {
                    left: length(14.0),
                    right: length(14.0),
                    top: length(8.0),
                    bottom: length(8.0),
                },
                ..Default::default()
            },
            36.0,
            None,
            None,
        );
        ui.add_icon(btn, icon, 32.0, 1.8, fg);
        ui.add_text(
            btn,
            title,
            Style {
                flex_shrink: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                ..Default::default()
            },
            single_line_style(24.0, 400, fg, Align::Center),
        );
        if count > 0 {
            let badge = ui.add_box(
                btn,
                Style {
                    min_size: Size {
                        width: length(30.0),
                        height: length(30.0),
                    },
                    padding: Rect {
                        left: length(8.0),
                        right: length(8.0),
                        top: length(0.0),
                        bottom: length(0.0),
                    },
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::Center),
                    ..Default::default()
                },
                BoxPaint::new()
                    .bg(if active {
                        t.primary_ink.with_alpha(0.18)
                    } else {
                        t.text.with_alpha(0.10)
                    })
                    .radius(15.0),
            );
            ui.add_text(
                badge,
                count.to_string(),
                Style::default(),
                single_line_style(
                    18.0,
                    600,
                    if active {
                        t.primary_ink
                    } else {
                        t.text_secondary
                    },
                    Align::Center,
                ),
            );
        }
    }

    let add_source_segments = |ui: &mut UiBuilder<'_>, parent: NodeId, flex_grow: f32| {
        let seg = ui.add_box(
            parent,
            Style {
                flex_grow,
                flex_basis: if flex_grow > 0.0 {
                    length(0.0)
                } else {
                    length(460.0)
                },
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                flex_direction: FlexDirection::Row,
                gap: Size {
                    width: length(6.0),
                    height: length(0.0),
                },
                padding: Rect {
                    left: length(4.0),
                    right: length(4.0),
                    top: length(4.0),
                    bottom: length(4.0),
                },
                ..Default::default()
            },
            BoxPaint::new()
                .bg(t.surface_subtle)
                .border(1.0, t.border)
                .radius(20.0),
        );
        for (idx, (mode_val, key, fb)) in [
            ("everyone", "sourceModeEveryone", "Everyone"),
            ("allowlist", "sourceModeAllowlist", "Allowlist"),
        ]
        .into_iter()
        .enumerate()
        {
            let active = view.source_mode == mode_val;
            let (btn, fg) = menu_btn(
                ui,
                seg,
                48 + idx as u8,
                Command::Display(player_display::Action::SetSourceMode(mode_val.to_owned())),
                active,
                locked,
                Style {
                    flex_grow: 1.0,
                    flex_basis: length(0.0),
                    min_size: Size {
                        width: length(0.0),
                        height: length(72.0),
                    },
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::Center),
                    padding: Rect {
                        left: length(12.0),
                        right: length(12.0),
                        top: length(8.0),
                        bottom: length(8.0),
                    },
                    ..Default::default()
                },
                16.0,
                Some(if active {
                    t.primary_soft
                } else {
                    Color::TRANSPARENT
                }),
                Some(if active {
                    t.primary_ink
                } else {
                    Color::TRANSPARENT
                }),
            );
            let txt_color = if active { fg } else { t.text_secondary };
            ui.add_text(
                btn,
                label(&view.labels, key, fb),
                Style {
                    flex_shrink: 1.0,
                    min_size: Size {
                        width: length(0.0),
                        height: auto(),
                    },
                    ..Default::default()
                },
                single_line_style(24.0, 400, txt_color, Align::Center),
            );
        }
    };

    let add_pagination =
        |ui: &mut UiBuilder<'_>, parent: NodeId, page: usize, count: usize, is_players: bool| {
            let row = ui.add_box(
                parent,
                Style {
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    gap: Size {
                        width: length(10.0),
                        height: length(0.0),
                    },
                    margin: Rect {
                        left: auto(),
                        right: length(0.0),
                        top: length(0.0),
                        bottom: length(0.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new(),
            );
            let mk_cmd = |p: usize| {
                if is_players {
                    Command::PlayerPage(p)
                } else {
                    Command::LanguagePage(p)
                }
            };
            let (prev, pfg) = menu_btn(
                ui,
                row,
                15,
                mk_cmd(page.saturating_sub(1)),
                false,
                page == 0,
                Style {
                    size: Size {
                        width: length(72.0),
                        height: length(72.0),
                    },
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::Center),
                    ..Default::default()
                },
                16.0,
                None,
                None,
            );
            ui.add_icon(prev, IconId::ChevronLeft, 32.0, 1.8, pfg);
            ui.add_text(
                row,
                format!("{} / {}", page + 1, count),
                Style {
                    min_size: Size {
                        width: length(80.0),
                        height: auto(),
                    },
                    ..Default::default()
                },
                single_line_style(24.0, 400, t.text_secondary, Align::Center),
            );
            let (next, nfg) = menu_btn(
                ui,
                row,
                16,
                mk_cmd((page + 1).min(count.saturating_sub(1))),
                false,
                page + 1 >= count,
                Style {
                    size: Size {
                        width: length(72.0),
                        height: length(72.0),
                    },
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::Center),
                    ..Default::default()
                },
                16.0,
                None,
                None,
            );
            ui.add_icon(next, IconId::ChevronRight, 32.0, 1.8, nfg);
        };

    match menu_ui.page {
        Page::Players => {
            let current: Vec<_> = view.players.iter().filter(|p| p.current).collect();
            let history: Vec<_> = view.players.iter().filter(|p| !p.current).collect();
            let active_list = if menu_ui.group == Group::Current {
                &current
            } else {
                &history
            };
            let page_count = (active_list.len().div_ceil(PLAYERS_PER_PAGE)).max(1);
            let page = menu_ui.player_page.min(page_count - 1);
            let slice = &active_list
                [page * PLAYERS_PER_PAGE..((page + 1) * PLAYERS_PER_PAGE).min(active_list.len())];

            let toolbar = ui.add_box(
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
            let group_seg = ui.add_box(
                toolbar,
                Style {
                    flex_grow: 1.35,
                    flex_basis: length(0.0),
                    min_size: Size {
                        width: length(0.0),
                        height: auto(),
                    },
                    flex_direction: FlexDirection::Row,
                    gap: Size {
                        width: length(6.0),
                        height: length(0.0),
                    },
                    padding: Rect {
                        left: length(4.0),
                        right: length(4.0),
                        top: length(4.0),
                        bottom: length(4.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new()
                    .bg(t.surface_subtle)
                    .border(1.0, t.border)
                    .radius(20.0),
            );
            for (id, grp, k, fb, cnt) in [
                (
                    12u8,
                    Group::Current,
                    "current",
                    "Current world",
                    current.len(),
                ),
                (
                    13u8,
                    Group::History,
                    "historySaved",
                    "History",
                    history.len(),
                ),
            ] {
                let active = menu_ui.group == grp;
                let (btn, fg) = menu_btn(
                    ui,
                    group_seg,
                    id,
                    Command::Group(grp),
                    active,
                    false,
                    Style {
                        flex_grow: 1.0,
                        flex_basis: length(0.0),
                        min_size: Size {
                            width: length(0.0),
                            height: length(72.0),
                        },
                        flex_direction: FlexDirection::Row,
                        align_items: Some(AlignItems::Center),
                        justify_content: Some(JustifyContent::Center),
                        gap: Size {
                            width: length(10.0),
                            height: length(0.0),
                        },
                        padding: Rect {
                            left: length(12.0),
                            right: length(12.0),
                            top: length(8.0),
                            bottom: length(8.0),
                        },
                        ..Default::default()
                    },
                    16.0,
                    Some(if active {
                        t.primary_soft
                    } else {
                        Color::TRANSPARENT
                    }),
                    Some(if active {
                        t.primary_ink
                    } else {
                        Color::TRANSPARENT
                    }),
                );
                let txt_c = if active { fg } else { t.text_secondary };
                ui.add_text(
                    btn,
                    label(&view.labels, k, fb),
                    Style {
                        flex_shrink: 1.0,
                        min_size: Size {
                            width: length(0.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    single_line_style(24.0, 400, txt_c, Align::Center),
                );
                let badge = ui.add_box(
                    btn,
                    Style {
                        min_size: Size {
                            width: length(30.0),
                            height: length(30.0),
                        },
                        padding: Rect {
                            left: length(8.0),
                            right: length(8.0),
                            top: length(0.0),
                            bottom: length(0.0),
                        },
                        align_items: Some(AlignItems::Center),
                        justify_content: Some(JustifyContent::Center),
                        ..Default::default()
                    },
                    BoxPaint::new()
                        .bg(if active {
                            t.primary_ink.with_alpha(0.18)
                        } else {
                            t.text.with_alpha(0.10)
                        })
                        .radius(15.0),
                );
                ui.add_text(
                    badge,
                    cnt.to_string(),
                    Style::default(),
                    single_line_style(
                        18.0,
                        600,
                        if active {
                            t.primary_ink
                        } else {
                            t.text_secondary
                        },
                        Align::Center,
                    ),
                );
            }
            add_source_segments(ui, toolbar, 0.9);

            if !slice.is_empty() {
                let cols = ui.add_box(
                    panel,
                    Style {
                        display: Display::Grid,
                        grid_template_columns: vec![
                            minmax(length(0.0), fr(1.0_f32)),
                            length(72.0),
                            length(72.0),
                        ],
                        align_items: Some(AlignItems::Center),
                        gap: Size {
                            width: length(14.0),
                            height: length(0.0),
                        },
                        padding: Rect {
                            left: length(17.0),
                            right: length(17.0),
                            top: length(0.0),
                            bottom: length(0.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new(),
                );
                ui.add_box(cols, Style::default(), BoxPaint::new());
                ui.add_text(
                    cols,
                    label(&view.labels, "chatboxLabel", "Chatbox"),
                    Style::default(),
                    single_line_style(18.0, 400, t.text_quiet, Align::Center),
                );
                ui.add_text(
                    cols,
                    label(&view.labels, "voiceLabel", "Voice"),
                    Style::default(),
                    single_line_style(18.0, 400, t.text_quiet, Align::Center),
                );
            }

            let list = ui.add_box(
                panel,
                Style {
                    flex_grow: 1.0,
                    min_size: Size {
                        width: length(0.0),
                        height: length(0.0),
                    },
                    display: Display::Grid,
                    grid_template_rows: vec![minmax(length(0.0), fr(1.0_f32)); PLAYERS_PER_PAGE],
                    gap: Size {
                        width: length(0.0),
                        height: length(8.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new(),
            );
            if slice.is_empty() {
                let empty_text = if menu_ui.group == Group::Current {
                    label(
                        &view.labels,
                        "currentEmpty",
                        label(
                            &view.labels,
                            "empty",
                            "Waiting for tracks or loading history.",
                        ),
                    )
                } else {
                    label(
                        &view.labels,
                        "historyEmpty",
                        label(
                            &view.labels,
                            "empty",
                            "Waiting for tracks or loading history.",
                        ),
                    )
                };
                ui.add_text(
                    list,
                    empty_text,
                    Style {
                        grid_row: taffy::Line {
                            start: line(1),
                            end: span(PLAYERS_PER_PAGE as u16),
                        },
                        align_self: Some(AlignSelf::Center),
                        size: Size {
                            width: percent(1.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    text_style(24.0, 400, t.text_secondary),
                );
            } else {
                for (index, player) in slice.iter().enumerate() {
                    let row = ui.add_box(
                        list,
                        Style {
                            display: Display::Grid,
                            grid_template_columns: vec![
                                minmax(length(0.0), fr(1.0_f32)),
                                length(72.0),
                                length(72.0),
                            ],
                            align_items: Some(AlignItems::Center),
                            gap: Size {
                                width: length(14.0),
                                height: length(0.0),
                            },
                            padding: Rect {
                                left: length(16.0),
                                right: length(16.0),
                                top: length(6.0),
                                bottom: length(6.0),
                            },
                            ..Default::default()
                        },
                        BoxPaint::new()
                            .bg(if player.hidden {
                                t.surface_subtle
                            } else {
                                t.surface
                            })
                            .border(1.0, t.border)
                            .radius(18.0),
                    );
                    let info = ui.add_box(
                        row,
                        Style {
                            min_size: Size {
                                width: length(0.0),
                                height: auto(),
                            },
                            flex_direction: FlexDirection::Column,
                            ..Default::default()
                        },
                        BoxPaint::new(),
                    );
                    ui.add_text(
                        info,
                        &player.name,
                        Style {
                            size: Size {
                                width: percent(1.0),
                                height: auto(),
                            },
                            ..Default::default()
                        },
                        TextStyle {
                            font_size: 28.0,
                            line_height: 31.0,
                            weight: 600,
                            max_lines: Some(2),
                            color: t.text,
                            ..Default::default()
                        },
                    );
                    let detail = if player.user_id.is_none() {
                        label(
                            &view.labels,
                            "userIdPending",
                            "Waiting for player user ID before selection",
                        )
                        .to_owned()
                    } else if player.voice_locked {
                        format!(
                            "{} · {}",
                            player.detail,
                            label(&view.labels, "voiceLocked", "Hidden record · voice locked")
                        )
                    } else {
                        player.detail.clone()
                    };
                    ui.add_text(
                        info,
                        detail,
                        Style {
                            size: Size {
                                width: percent(1.0),
                                height: auto(),
                            },
                            margin: Rect {
                                top: length(2.0),
                                left: length(0.0),
                                right: length(0.0),
                                bottom: length(0.0),
                            },
                            ..Default::default()
                        },
                        single_line_style(20.0, 400, t.text_secondary, Align::Left),
                    );

                    for (offset, (src, is_disabled, icon)) in [
                        (
                            "vrchat_chatbox",
                            player.chatbox_disabled,
                            if player.chatbox_disabled {
                                IconId::MessageSquareOff
                            } else {
                                IconId::MessageSquare
                            },
                        ),
                        (
                            "speaker",
                            player.voice_disabled,
                            if player.voice_disabled {
                                IconId::VolumeX
                            } else {
                                IconId::Volume2
                            },
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let btn_id = 33 + index as u8 * 3 + offset as u8;
                        let btn_disabled = locked
                            || player.user_id.is_none()
                            || (src == "speaker" && player.voice_locked);
                        let (btn, fg) = menu_btn(
                            ui,
                            row,
                            btn_id,
                            Command::Display(player_display::Action::SetUserSourceDisabled {
                                user_id: player.user_id.clone().unwrap_or_default(),
                                source: src.to_owned(),
                                disabled: !is_disabled,
                            }),
                            !is_disabled,
                            btn_disabled,
                            Style {
                                size: Size {
                                    width: length(72.0),
                                    height: length(72.0),
                                },
                                align_items: Some(AlignItems::Center),
                                justify_content: Some(JustifyContent::Center),
                                ..Default::default()
                            },
                            16.0,
                            is_disabled.then_some(t.surface_subtle),
                            is_disabled.then_some(t.border),
                        );
                        ui.add_icon(
                            btn,
                            icon,
                            32.0,
                            1.8,
                            if is_disabled { t.text_quiet } else { fg },
                        );
                    }
                }
            }

            let footer = ui.add_box(
                panel,
                Style {
                    size: Size {
                        width: auto(),
                        height: length(72.0),
                    },
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::SpaceBetween),
                    gap: Size {
                        width: length(14.0),
                        height: length(0.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new(),
            );
            let mut summary_parts = Vec::new();
            if let Some(prox) = &view.proximity {
                let val = if prox.enabled {
                    format!("{:.1} m", prox.radius_m)
                } else {
                    label(&view.labels, "proximityOff", "Off").to_owned()
                };
                summary_parts.push(format!(
                    "{} {val}",
                    label(&view.labels, "proximityRadius", "Recognition distance")
                ));
            }
            if let Some(vp) = &view.view_priority {
                let val = if vp.enabled {
                    format!("{}°", vp.horizontal_fov_deg.round() as i32)
                } else {
                    label(&view.labels, "proximityOff", "Off").to_owned()
                };
                summary_parts.push(format!(
                    "{} {val}",
                    label(&view.labels, "viewAngle", "Horizontal view")
                ));
            }
            if !summary_parts.is_empty() {
                let (btn, _) = menu_btn(
                    ui,
                    footer,
                    53,
                    Command::Page(Page::Rules),
                    false,
                    false,
                    Style {
                        flex_grow: 1.0,
                        min_size: Size {
                            width: length(0.0),
                            height: length(72.0),
                        },
                        flex_direction: FlexDirection::Row,
                        align_items: Some(AlignItems::Center),
                        justify_content: Some(JustifyContent::SpaceBetween),
                        gap: Size {
                            width: length(12.0),
                            height: length(0.0),
                        },
                        padding: Rect {
                            left: length(18.0),
                            right: length(18.0),
                            top: length(8.0),
                            bottom: length(8.0),
                        },
                        ..Default::default()
                    },
                    16.0,
                    Some(t.surface),
                    Some(t.border),
                );
                ui.add_icon(btn, IconId::SlidersHorizontal, 32.0, 1.8, t.text_secondary);
                ui.add_text(
                    btn,
                    summary_parts.join(" · "),
                    Style {
                        flex_grow: 1.0,
                        min_size: Size {
                            width: length(0.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    single_line_style(22.0, 400, t.text_secondary, Align::Left),
                );
                ui.add_icon(btn, IconId::ChevronRight, 32.0, 1.8, t.text_secondary);
            }
            add_pagination(ui, footer, page, page_count, true);
        }
        Page::Rules => {
            let rules = ui.add_box(
                panel,
                Style {
                    flex_grow: 1.0,
                    min_size: Size {
                        width: length(0.0),
                        height: length(0.0),
                    },
                    flex_direction: FlexDirection::Column,
                    gap: Size {
                        width: length(0.0),
                        height: length(16.0),
                    },
                    padding: Rect {
                        top: length(4.0),
                        left: length(0.0),
                        right: length(0.0),
                        bottom: length(0.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new(),
            );

            let card_border = if t.dark { t.border_strong } else { t.border };
            let inline_card = ui.add_box(
                rules,
                Style {
                    min_size: Size {
                        width: length(0.0),
                        height: length(118.0),
                    },
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::SpaceBetween),
                    gap: Size {
                        width: length(20.0),
                        height: length(0.0),
                    },
                    padding: Rect {
                        left: length(24.0),
                        right: length(24.0),
                        top: length(18.0),
                        bottom: length(18.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new()
                    .bg(t.surface_elevated)
                    .border(1.0, card_border)
                    .radius(22.0),
            );
            let title_row = ui.add_box(
                inline_card,
                Style {
                    flex_grow: 1.0,
                    min_size: Size {
                        width: length(0.0),
                        height: auto(),
                    },
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    gap: Size {
                        width: length(14.0),
                        height: length(0.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new(),
            );
            ui.add_icon(
                title_row,
                IconId::UsersRound,
                28.0,
                1.8,
                if locked { t.text_quiet } else { t.primary_ink },
            );
            ui.add_text(
                title_row,
                label(&view.labels, "sourceMode", "Player sources"),
                Style {
                    flex_shrink: 1.0,
                    min_size: Size {
                        width: length(0.0),
                        height: auto(),
                    },
                    ..Default::default()
                },
                single_line_style(
                    25.0,
                    600,
                    if locked { t.text_secondary } else { t.text },
                    Align::Left,
                ),
            );
            add_source_segments(ui, inline_card, 0.0);

            if view.available && (!view.processing_available || !view.player_mode) {
                let banner = ui.add_box(
                    rules,
                    Style {
                        min_size: Size {
                            width: length(0.0),
                            height: length(52.0),
                        },
                        flex_direction: FlexDirection::Row,
                        align_items: Some(AlignItems::Center),
                        gap: Size {
                            width: length(12.0),
                            height: length(0.0),
                        },
                        padding: Rect {
                            left: length(18.0),
                            right: length(18.0),
                            top: length(10.0),
                            bottom: length(10.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new()
                        .bg(t.warning_soft)
                        .border(1.0, t.warning_border)
                        .radius(16.0),
                );
                ui.add_icon(banner, IconId::TriangleAlert, 24.0, 1.8, t.warning);
                ui.add_text(
                    banner,
                    label(
                        &view.labels,
                        "processingUnavailable",
                        "Distance filtering requires per-player audio or Chatbox capture; view priority requires per-player audio and a connected Core.",
                    ),
                    Style {
                        flex_grow: 1.0,
                        min_size: Size {
                            width: length(0.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    text_style(20.0, 400, t.warning),
                );
            }

            if let Some(prox) = &view.proximity {
                let prox_avail = view.proximity_available || view.player_mode;
                let restore = if prox.radius_m > 0.0 {
                    (prox.radius_m.clamp(0.1, 10.0) * 10.0).round() / 10.0
                } else {
                    5.0
                };
                let val = if prox.enabled { prox.radius_m } else { 0.0 };
                build_rule_slider_card(
                    ui,
                    rules,
                    state,
                    IconId::Radar,
                    label(&view.labels, "proximityRadius", "Recognition distance"),
                    None,
                    val,
                    locked || !prox_avail,
                    locked || !prox_avail,
                    54,
                    Command::Display(player_display::Action::SetProximityRadius(
                        if prox.enabled { 0.0 } else { restore },
                    )),
                    prox.enabled,
                    32,
                    PROXIMITY_SLIDER,
                    [label(&view.labels, "proximityOff", "Off"), "10 m"],
                    &|v| {
                        if v > 0.0 {
                            format!("{v:.1} m")
                        } else {
                            label(&view.labels, "proximityOff", "Off").to_owned()
                        }
                    },
                    locked || !prox_avail || val <= 0.0,
                    None,
                );
            }

            if let Some(vp) = &view.view_priority {
                let note = (view.available
                    && view.player_mode
                    && view.processing_available
                    && vp.enabled
                    && !vp.local_view)
                    .then(|| {
                        label(
                            &view.labels,
                            "viewWaiting",
                            "Waiting for view data; players currently run at normal priority.",
                        )
                    });
                build_rule_slider_card(
                    ui,
                    rules,
                    state,
                    IconId::Eye,
                    label(&view.labels, "viewPriority", "Prioritize players in view"),
                    Some(label(&view.labels, "viewAngle", "Horizontal view")),
                    vp.horizontal_fov_deg,
                    locked || !view.player_mode,
                    locked || !view.player_mode || !vp.enabled,
                    50,
                    Command::Display(player_display::Action::SetViewPriority {
                        enabled: !vp.enabled,
                        horizontal_fov_deg: vp.horizontal_fov_deg,
                    }),
                    vp.enabled,
                    51,
                    VIEW_ANGLE_SLIDER,
                    ["30°", "180°"],
                    &|v| format!("{}°", v.round() as i32),
                    locked || !view.player_mode || !vp.enabled,
                    note,
                );
            }
        }
        Page::Languages => {
            let page_count = (view.languages.len().div_ceil(LANGUAGES_PER_PAGE)).max(1);
            let page = menu_ui.language_page.min(page_count - 1);
            let slice = &view.languages[page * LANGUAGES_PER_PAGE
                ..((page + 1) * LANGUAGES_PER_PAGE).min(view.languages.len())];

            let grid = ui.add_box(
                panel,
                Style {
                    flex_grow: 1.0,
                    min_size: Size {
                        width: length(0.0),
                        height: length(0.0),
                    },
                    display: Display::Grid,
                    grid_template_columns: vec![minmax(length(0.0), fr(1.0_f32)); 2],
                    grid_template_rows: vec![minmax(length(0.0), fr(1.0_f32)); 6],
                    gap: Size {
                        width: length(10.0),
                        height: length(10.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new(),
            );
            if slice.is_empty() {
                ui.add_text(
                    grid,
                    label(&view.labels, "noHiddenLanguages", "No languages hidden"),
                    Style {
                        grid_column: taffy::Line {
                            start: line(1),
                            end: span(2),
                        },
                        grid_row: taffy::Line {
                            start: line(1),
                            end: span(6),
                        },
                        align_self: Some(AlignSelf::Center),
                        size: Size {
                            width: percent(1.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    text_style(24.0, 400, t.text_secondary),
                );
            } else {
                for (index, lang) in slice.iter().enumerate() {
                    let (btn, fg) = menu_btn(
                        ui,
                        grid,
                        17 + index as u8,
                        Command::Display(player_display::Action::SetSourceLanguageHidden {
                            language: lang.code.clone(),
                            hidden: !lang.hidden,
                        }),
                        lang.hidden,
                        locked,
                        Style {
                            min_size: Size {
                                width: length(0.0),
                                height: length(72.0),
                            },
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
                            justify_content: Some(JustifyContent::FlexStart),
                            gap: Size {
                                width: length(12.0),
                                height: length(0.0),
                            },
                            padding: Rect {
                                left: length(16.0),
                                right: length(16.0),
                                top: length(8.0),
                                bottom: length(8.0),
                            },
                            ..Default::default()
                        },
                        16.0,
                        None,
                        None,
                    );
                    let chk = ui.add_box(
                        btn,
                        Style {
                            size: Size {
                                width: length(30.0),
                                height: length(30.0),
                            },
                            flex_shrink: 0.0,
                            align_items: Some(AlignItems::Center),
                            justify_content: Some(JustifyContent::Center),
                            ..Default::default()
                        },
                        BoxPaint::new().border(2.0, fg).radius(7.0),
                    );
                    if lang.hidden {
                        ui.add_icon(chk, IconId::Check, 24.0, 2.0, fg);
                    }
                    ui.add_text(
                        btn,
                        &lang.label,
                        Style {
                            flex_grow: 1.0,
                            min_size: Size {
                                width: length(0.0),
                                height: auto(),
                            },
                            ..Default::default()
                        },
                        single_line_style(26.0, 400, fg, Align::Left),
                    );
                }
            }

            let footer = ui.add_box(
                panel,
                Style {
                    size: Size {
                        width: auto(),
                        height: length(72.0),
                    },
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::SpaceBetween),
                    gap: Size {
                        width: length(14.0),
                        height: length(0.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new(),
            );
            let left = ui.add_box(
                footer,
                Style {
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    gap: Size {
                        width: length(14.0),
                        height: length(0.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new(),
            );
            let (clr, cfg) = menu_btn(
                ui,
                left,
                9,
                Command::Display(player_display::Action::ClearHiddenLanguages(())),
                false,
                locked || hidden_lang_count == 0,
                Style {
                    min_size: Size {
                        width: length(72.0),
                        height: length(72.0),
                    },
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::Center),
                    padding: Rect {
                        left: length(16.0),
                        right: length(16.0),
                        top: length(8.0),
                        bottom: length(8.0),
                    },
                    ..Default::default()
                },
                16.0,
                None,
                None,
            );
            ui.add_text(
                clr,
                label(&view.labels, "noHiddenLanguages", "No languages hidden"),
                Style::default(),
                single_line_style(24.0, 400, cfg, Align::Center),
            );
            if hidden_lang_count > 0 {
                let note = label(&view.labels, "hiddenLanguagesCount", "{{count}} hidden")
                    .replace("{{count}}", &hidden_lang_count.to_string());
                ui.add_text(
                    left,
                    note,
                    Style::default(),
                    single_line_style(22.0, 400, t.text_secondary, Align::Left),
                );
            }
            add_pagination(ui, footer, page, page_count, false);
        }
        Page::Home => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn build_rule_slider_card(
    ui: &mut UiBuilder<'_>,
    parent: NodeId,
    state: &SurfaceState,
    icon: IconId,
    title: &str,
    subtitle: Option<&str>,
    value: f32,
    card_disabled: bool,
    slider_disabled: bool,
    toggle_id: u8,
    toggle_command: Command,
    toggle_checked: bool,
    slider_id: u8,
    slider_command: Slider,
    scale: [&str; 2],
    format_value: &dyn Fn(f32) -> String,
    value_muted: bool,
    note: Option<&str>,
) {
    let t = ui.theme;
    // Live pointer preview while dragging the slider track (`vr-menu-slider-track` is centered inside `1248px` card).
    let shown = if !slider_disabled
        && state.pressed == Some(slider_id)
        && let Some([px, _]) = state.pointer
    {
        // Card inner width = 1280 - 32(panel) - 40(panel pad) - 48(card pad) = 1160; track width = 1120 (`calc(100% - 40px)`).
        let track_left = (1280.0 - 1120.0) * 0.5;
        let fraction = ((px * 1280.0 - track_left) / 1120.0).clamp(0.0, 1.0) as f64;
        let raw = slider_command.min + fraction * (slider_command.max - slider_command.min);
        let q = slider_command.min
            + ((raw - slider_command.min) / slider_command.step).round() * slider_command.step;
        (((q.clamp(slider_command.min, slider_command.max)) * 10.0).round() / 10.0) as f32
    } else {
        value.clamp(slider_command.min as f32, slider_command.max as f32)
    };
    let progress = ((f64::from(shown) - slider_command.min)
        / (slider_command.max - slider_command.min))
        .clamp(0.0, 1.0) as f32;

    let card = ui.add_box(
        parent,
        Style {
            flex_direction: FlexDirection::Column,
            gap: Size {
                width: length(0.0),
                height: length(12.0),
            },
            padding: Rect {
                left: length(24.0),
                right: length(24.0),
                top: length(18.0),
                bottom: length(18.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(t.surface_elevated)
            .border(1.0, if t.dark { t.border_strong } else { t.border })
            .radius(22.0),
    );

    let head = ui.add_box(
        card,
        Style {
            min_size: Size {
                width: length(0.0),
                height: length(72.0),
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
    let title_box = ui.add_box(
        head,
        Style {
            flex_grow: 1.0,
            min_size: Size {
                width: length(0.0),
                height: auto(),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(14.0),
                height: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    ui.add_icon(
        title_box,
        icon,
        28.0,
        1.8,
        if card_disabled {
            t.text_quiet
        } else {
            t.primary_ink
        },
    );
    let title_col = ui.add_box(
        title_box,
        Style {
            flex_grow: 1.0,
            min_size: Size {
                width: length(0.0),
                height: auto(),
            },
            flex_direction: FlexDirection::Column,
            gap: Size {
                width: length(0.0),
                height: length(2.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    ui.add_text(
        title_col,
        title,
        Style::default(),
        single_line_style(
            25.0,
            600,
            if card_disabled {
                t.text_secondary
            } else {
                t.text
            },
            Align::Left,
        ),
    );
    if let Some(sub) = subtitle {
        ui.add_text(
            title_col,
            sub,
            Style::default(),
            single_line_style(19.0, 400, t.text_secondary, Align::Left),
        );
    }

    let ctrls = ui.add_box(
        head,
        Style {
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(14.0),
                height: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );
    let pill = ui.add_box(
        ctrls,
        Style {
            min_size: Size {
                width: length(112.0),
                height: length(42.0),
            },
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::Center),
            padding: Rect {
                left: length(16.0),
                right: length(16.0),
                top: length(0.0),
                bottom: length(0.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(if value_muted {
                t.surface_subtle
            } else {
                t.primary_soft
            })
            .border(
                1.0,
                if value_muted {
                    t.border
                } else {
                    t.primary_ink.with_alpha(0.32)
                },
            )
            .radius(21.0),
    );
    ui.add_text(
        pill,
        format_value(shown),
        Style::default(),
        single_line_style(
            23.0,
            650,
            if value_muted {
                t.text_quiet
            } else {
                t.primary_ink
            },
            Align::Center,
        ),
    );

    let thov = !card_disabled && state.hovered == Some(toggle_id);
    let tpress = !card_disabled && state.pressed == Some(toggle_id);
    let mut tpaint = BoxPaint::new()
        .bg(if tpress {
            t.primary_soft
        } else if thov {
            t.primary_softer
        } else {
            Color::TRANSPARENT
        })
        .border(
            2.0,
            if thov || tpress {
                t.primary_ink
            } else {
                Color::TRANSPARENT
            },
        )
        .radius(36.0);
    if card_disabled {
        tpaint.opacity = 0.42;
    }
    let tbtn = ui.add_box(
        ctrls,
        Style {
            min_size: Size {
                width: length(88.0),
                height: length(72.0),
            },
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::Center),
            padding: Rect {
                left: length(12.0),
                right: length(12.0),
                top: length(8.0),
                bottom: length(8.0),
            },
            ..Default::default()
        },
        tpaint,
    );
    if !card_disabled {
        ui.set_region(tbtn, RegionPayload::Menu(toggle_id, false, toggle_command));
    }
    let track_sw = ui.add_box(
        tbtn,
        Style {
            size: Size {
                width: length(64.0),
                height: length(36.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(if toggle_checked {
                t.primary
            } else {
                t.border_strong
            })
            .radius(18.0),
    );
    ui.add_box(
        track_sw,
        Style {
            position: Position::Absolute,
            inset: Rect {
                top: length(4.0),
                left: length(if toggle_checked { 32.0 } else { 4.0 }),
                right: auto(),
                bottom: auto(),
            },
            size: Size {
                width: length(28.0),
                height: length(28.0),
            },
            ..Default::default()
        },
        BoxPaint::new().bg(Color::WHITE).radius(14.0),
    );

    // Slider row.
    let slider_wrap = ui.add_box(
        card,
        Style {
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::Center),
            ..Default::default()
        },
        BoxPaint::new(),
    );
    let shov = !slider_disabled && state.hovered == Some(slider_id);
    let spress = !slider_disabled && state.pressed == Some(slider_id);
    let mut spaint = BoxPaint::new().radius(16.0);
    if slider_disabled {
        spaint.opacity = 0.42;
    }
    let track = ui.add_box(
        slider_wrap,
        Style {
            size: Size {
                width: percent(0.965),
                height: length(80.0),
            },
            ..Default::default()
        },
        spaint,
    );
    if !slider_disabled {
        ui.set_region(
            track,
            RegionPayload::Menu(slider_id, false, Command::Slider(slider_command)),
        );
    }
    let rail = ui.add_box(
        track,
        Style {
            position: Position::Absolute,
            inset: Rect {
                top: length(24.0),
                left: length(0.0),
                right: length(0.0),
                bottom: auto(),
            },
            size: Size {
                width: auto(),
                height: length(12.0),
            },
            ..Default::default()
        },
        BoxPaint::new().bg(t.border_strong).radius(6.0),
    );
    if progress > 0.0 {
        ui.add_box(
            rail,
            Style {
                size: Size {
                    width: percent(progress),
                    height: percent(1.0),
                },
                ..Default::default()
            },
            BoxPaint::new()
                .bg(if slider_disabled {
                    t.text_quiet
                } else {
                    t.primary_ink
                })
                .radius(6.0),
        );
    }
    let thumb_size = if spress { 42.4 } else { 40.0 };
    let thumb_border = if slider_disabled {
        t.text_quiet
    } else if shown <= 0.0 && !shov && !spress {
        t.text_secondary
    } else {
        t.primary_ink
    };
    let mut thumb_paint = BoxPaint::new()
        .bg(if spress { t.primary_soft } else { t.surface })
        .border(3.0, thumb_border)
        .radius(thumb_size * 0.5);
    if shov || spress {
        thumb_paint = thumb_paint.outline(if spress { 8.0 } else { 6.0 }, 0.0, t.primary_soft, 0);
    }
    ui.add_box(
        track,
        Style {
            position: Position::Absolute,
            inset: Rect {
                top: length(30.0 - thumb_size * 0.5),
                left: percent(progress),
                right: auto(),
                bottom: auto(),
            },
            margin: Rect {
                left: length(-thumb_size * 0.5),
                right: length(0.0),
                top: length(0.0),
                bottom: length(0.0),
            },
            size: Size {
                width: length(thumb_size),
                height: length(thumb_size),
            },
            ..Default::default()
        },
        thumb_paint,
    );
    let scale_row = ui.add_box(
        track,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(0.0),
                right: length(0.0),
                bottom: length(2.0),
                top: auto(),
            },
            flex_direction: FlexDirection::Row,
            justify_content: Some(JustifyContent::SpaceBetween),
            ..Default::default()
        },
        BoxPaint::new(),
    );
    ui.add_text(
        scale_row,
        scale[0],
        Style::default(),
        single_line_style(18.0, 400, t.text_secondary, Align::Left),
    );
    ui.add_text(
        scale_row,
        scale[1],
        Style::default(),
        single_line_style(18.0, 400, t.text_secondary, Align::Right),
    );

    if let Some(n) = note {
        let note_box = ui.add_box(
            card,
            Style {
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: Size {
                    width: length(10.0),
                    height: length(0.0),
                },
                padding: Rect {
                    left: length(14.0),
                    right: length(14.0),
                    top: length(10.0),
                    bottom: length(10.0),
                },
                ..Default::default()
            },
            BoxPaint::new().bg(t.surface_subtle).radius(12.0),
        );
        ui.add_icon(note_box, IconId::Info, 20.0, 1.8, t.text_secondary);
        ui.add_text(
            note_box,
            n,
            Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                ..Default::default()
            },
            text_style(19.0, 400, t.text_secondary),
        );
    }
}
