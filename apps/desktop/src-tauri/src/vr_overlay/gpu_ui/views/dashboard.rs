//! SteamVR / OpenXR Dashboard settings surface (`1440x900`).

use cosmic_text::Align;
use taffy::prelude::*;

use super::reader::build_player_audio_prompt;
use super::{single_line_style, surface_size, text_style};
use crate::vr_overlay::dashboard::{
    AppearanceField, DashboardAction, DashboardChoice, DashboardCommand, DashboardPage,
    DashboardPicker, DashboardSaveState, DashboardUi, DashboardViewModel, DisplayKind,
    PICKER_PAGE_SIZE, PositionField, TargetGroup,
};
use crate::vr_overlay::gpu_ui::SurfaceRuntimeState;
use crate::vr_overlay::gpu_ui::draw::DrawList;
use crate::vr_overlay::gpu_ui::icons::IconId;
use crate::vr_overlay::gpu_ui::layout::{BoxPaint, RegionPayload, UiBuilder, length, percent};
use crate::vr_overlay::gpu_ui::text::{TextEngine, TextStyle};
use crate::vr_overlay::gpu_ui::theme::{Color, Theme};
use crate::vr_overlay::surface_protocol::{SurfaceId, SurfaceLayout, SurfaceState};

#[derive(Default, Clone, Copy)]
struct BtnOpts {
    active: bool,
    disabled: bool,
    icon: bool,
    primary: bool,
    danger: bool,
    recording: bool,
    pill: bool,
    transparent: bool,
    borderless: bool,
}

pub fn render_dashboard(
    text: &mut TextEngine,
    state: &SurfaceState,
    dark: bool,
    view: &DashboardViewModel,
    dash_ui: &DashboardUi,
    runtime: &mut SurfaceRuntimeState,
) -> (SurfaceLayout, DrawList) {
    let (w, h) = surface_size(SurfaceId::Dashboard);
    let theme = Theme::of(dark);
    let mut ui_buf = UiBuilder::new(text, theme, w, h);
    let ui = &mut ui_buf;
    let root = ui.root();

    if let Some(notice) = &view.player_audio_notice {
        build_player_audio_prompt(ui, root, state, notice, true, runtime);
        ui.compute_layout();
        if let Some(m) = ui.scroll_measurement() {
            runtime.client_height = m.client_height;
            runtime.scroll_height = m.scroll_height;
            let max_scroll = (m.scroll_height - m.client_height).max(0.0);
            runtime.scroll_y = runtime.scroll_y.clamp(0.0, max_scroll);
            ui.set_scroll_y(runtime.scroll_y);
        }
        return ui.emit(SurfaceId::Dashboard, state.key, 1);
    }

    let labels = &view.labels;
    let mut region_seq = 0u8;

    let panel = ui.add_box(
        root,
        Style {
            position: Position::Absolute,
            inset: Rect {
                left: length(16.0),
                right: length(16.0),
                top: length(14.0),
                bottom: length(14.0),
            },
            flex_direction: FlexDirection::Column,
            gap: Size {
                width: length(0.0),
                height: length(14.0),
            },
            padding: Rect {
                left: length(24.0),
                right: length(24.0),
                top: length(20.0),
                bottom: length(20.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(theme.vr_shell)
            .border(2.0, theme.border)
            .radius(26.0),
    );
    ui.mark_clip(panel);

    let add_btn = |ui: &mut UiBuilder<'_>,
                   parent: NodeId,
                   cmd: DashboardCommand,
                   style: Style,
                   opts: BtnOpts,
                   region_seq: &mut u8|
     -> (NodeId, Color) {
        let id = *region_seq;
        *region_seq = region_seq.saturating_add(1);
        let hovered = !opts.disabled && state.hovered == Some(id);
        let pressed = !opts.disabled && state.pressed == Some(id);
        let radius = if opts.pill {
            999.0
        } else if opts.icon {
            12.0
        } else {
            14.0
        };
        let (bg, border, fg) = if opts.primary {
            (
                if hovered || pressed {
                    theme.primary_hover
                } else {
                    theme.primary
                },
                if hovered || pressed {
                    theme.primary_ink
                } else {
                    theme.primary
                },
                theme.on_primary,
            )
        } else if opts.recording {
            (
                if hovered || pressed {
                    theme.error.mix(theme.surface, 0.22)
                } else {
                    theme.error_soft
                },
                theme.error,
                theme.error,
            )
        } else if opts.danger {
            (
                if hovered || pressed {
                    theme.error_soft
                } else {
                    theme.surface_elevated
                },
                if hovered || pressed {
                    theme.error
                } else {
                    theme.error.mix(theme.border_strong, 0.45)
                },
                theme.error,
            )
        } else if opts.active {
            (
                theme.primary_soft,
                if hovered || pressed {
                    theme.primary_ink
                } else {
                    theme.primary_ink.mix(theme.border_strong, 0.50)
                },
                theme.primary_ink,
            )
        } else if opts.transparent {
            (
                if pressed {
                    theme.primary_soft
                } else if hovered {
                    theme.primary_softer
                } else {
                    Color::TRANSPARENT
                },
                if hovered || pressed {
                    theme.primary_ink
                } else {
                    Color::TRANSPARENT
                },
                if hovered || pressed {
                    theme.primary_ink
                } else {
                    theme.text_secondary
                },
            )
        } else {
            (
                if pressed {
                    theme.primary_soft
                } else if hovered {
                    theme.primary_softer
                } else {
                    theme.surface_elevated
                },
                if hovered || pressed {
                    theme.primary_ink
                } else if opts.borderless {
                    Color::TRANSPARENT
                } else {
                    theme.border_strong
                },
                theme.text,
            )
        };
        let mut paint = BoxPaint::new().bg(bg).border(2.0, border).radius(radius);
        if hovered || pressed {
            let oc = if opts.recording || opts.danger {
                theme.error
            } else {
                theme.primary_ink
            };
            paint = paint.outline(3.0, -3.0, oc, 0);
        }
        if opts.disabled {
            paint.opacity = 0.42;
        }
        if pressed && !opts.disabled {
            paint.scale = 0.985;
        }
        let node = ui.add_box(parent, style, paint);
        if !opts.disabled {
            ui.set_region(node, RegionPayload::Dashboard(id, cmd));
        }
        (node, fg)
    };

    let add_toggle_pill = |ui: &mut UiBuilder<'_>, parent: NodeId, enabled: bool| {
        let track = ui.add_box(
            parent,
            Style {
                size: Size {
                    width: length(52.0),
                    height: length(30.0),
                },
                flex_shrink: 0.0,
                ..Default::default()
            },
            BoxPaint::new()
                .bg(if enabled {
                    theme.primary
                } else {
                    theme.border_strong
                })
                .radius(15.0),
        );
        ui.add_box(
            track,
            Style {
                position: Position::Absolute,
                inset: Rect {
                    top: length(4.0),
                    left: length(if enabled { 26.0 } else { 4.0 }),
                    right: auto(),
                    bottom: auto(),
                },
                size: Size {
                    width: length(22.0),
                    height: length(22.0),
                },
                ..Default::default()
            },
            BoxPaint::new().bg(Color::WHITE).radius(11.0),
        );
    };

    let add_toggle_row = |ui: &mut UiBuilder<'_>,
                          parent: NodeId,
                          action: DashboardAction,
                          label: &str,
                          enabled: bool,
                          disabled: bool,
                          sublabel: Option<&str>,
                          region_seq: &mut u8| {
        let switch_only = label.is_empty();
        let (btn, fg) = add_btn(
            ui,
            parent,
            DashboardCommand::Action(action),
            Style {
                min_size: Size {
                    width: length(if switch_only { 64.0 } else { 0.0 }),
                    height: length(if switch_only { 44.0 } else { 52.0 }),
                },
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                justify_content: if switch_only {
                    Some(JustifyContent::Center)
                } else {
                    Some(JustifyContent::SpaceBetween)
                },
                gap: Size {
                    width: length(10.0),
                    height: length(0.0),
                },
                padding: Rect {
                    left: length(if switch_only { 6.0 } else { 18.0 }),
                    right: length(if switch_only { 6.0 } else { 18.0 }),
                    top: length(if switch_only { 6.0 } else { 8.0 }),
                    bottom: length(if switch_only { 6.0 } else { 8.0 }),
                },
                ..Default::default()
            },
            BtnOpts {
                active: !switch_only && enabled,
                disabled,
                pill: switch_only,
                transparent: switch_only,
                ..Default::default()
            },
            region_seq,
        );
        if !switch_only {
            let copy = ui.add_box(
                btn,
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
                copy,
                label,
                Style::default(),
                single_line_style(20.0, 400, fg, Align::Left),
            );
            if let Some(sub) = sublabel.filter(|s| !s.is_empty()) {
                ui.add_text(
                    copy,
                    sub,
                    Style::default(),
                    single_line_style(15.0, 400, theme.text_secondary, Align::Left),
                );
            }
        }
        add_toggle_pill(ui, btn, enabled);
    };

    let add_segments = |ui: &mut UiBuilder<'_>,
                        parent: NodeId,
                        choice: &DashboardChoice,
                        to_action: &dyn Fn(&str) -> DashboardAction,
                        disabled: bool,
                        style: Style,
                        seg_bg: Color,
                        region_seq: &mut u8| {
        let seg = ui.add_box(
            parent,
            Style {
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
                ..style
            },
            BoxPaint::new()
                .bg(seg_bg)
                .border(1.0, theme.border)
                .radius(999.0),
        );
        for opt in &choice.options {
            let active = choice.value == opt.value || choice.value == opt.label;
            let (btn, fg) = add_btn(
                ui,
                seg,
                DashboardCommand::Action(to_action(&opt.value)),
                Style {
                    flex_grow: 1.0,
                    flex_basis: length(0.0),
                    min_size: Size {
                        width: length(0.0),
                        height: length(44.0),
                    },
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::Center),
                    padding: Rect {
                        left: length(12.0),
                        right: length(12.0),
                        top: length(6.0),
                        bottom: length(6.0),
                    },
                    ..Default::default()
                },
                BtnOpts {
                    active,
                    disabled,
                    pill: true,
                    transparent: !active,
                    ..Default::default()
                },
                region_seq,
            );
            ui.add_text(
                btn,
                &opt.label,
                Style {
                    flex_shrink: 1.0,
                    min_size: Size {
                        width: length(0.0),
                        height: auto(),
                    },
                    ..Default::default()
                },
                single_line_style(18.0, if active { 600 } else { 400 }, fg, Align::Center),
            );
        }
    };

    let add_stepper = |ui: &mut UiBuilder<'_>,
                       parent: NodeId,
                       title: &str,
                       value: &str,
                       down: DashboardAction,
                       up: DashboardAction,
                       can_dec: bool,
                       can_inc: bool,
                       icon_size: f32,
                       region_seq: &mut u8| {
        let box_node = ui.add_box(
            parent,
            Style {
                flex_grow: 1.0,
                flex_basis: length(0.0),
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                flex_direction: FlexDirection::Column,
                padding: Rect {
                    left: length(10.0),
                    right: length(10.0),
                    top: length(8.0),
                    bottom: length(8.0),
                },
                ..Default::default()
            },
            BoxPaint::new()
                .bg(theme.surface_subtle)
                .border(1.0, theme.border)
                .radius(14.0),
        );
        ui.add_text(
            box_node,
            title,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                margin: Rect {
                    bottom: length(6.0),
                    left: length(0.0),
                    right: length(0.0),
                    top: length(0.0),
                },
                ..Default::default()
            },
            single_line_style(18.0, 400, theme.text_secondary, Align::Left),
        );
        let row = ui.add_box(
            box_node,
            Style {
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: Size {
                    width: length(4.0),
                    height: length(0.0),
                },
                ..Default::default()
            },
            BoxPaint::new(),
        );
        let (db, dfg) = add_btn(
            ui,
            row,
            DashboardCommand::Action(down),
            Style {
                size: Size {
                    width: length(icon_size),
                    height: length(icon_size),
                },
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                ..Default::default()
            },
            BtnOpts {
                icon: true,
                disabled: !can_dec,
                ..Default::default()
            },
            region_seq,
        );
        ui.add_icon(db, IconId::Minus, 22.0, 1.9, dfg);
        ui.add_text(
            row,
            value,
            Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                ..Default::default()
            },
            single_line_style(18.0, 600, theme.text, Align::Center),
        );
        let (ub, ufg) = add_btn(
            ui,
            row,
            DashboardCommand::Action(up),
            Style {
                size: Size {
                    width: length(icon_size),
                    height: length(icon_size),
                },
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                ..Default::default()
            },
            BtnOpts {
                icon: true,
                disabled: !can_inc,
                ..Default::default()
            },
            region_seq,
        );
        ui.add_icon(ub, IconId::Plus, 22.0, 1.9, ufg);
    };

    // Header.
    let header = ui.add_box(
        panel,
        Style {
            min_size: Size {
                width: length(0.0),
                height: length(58.0),
            },
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
    ui.add_logo(header, 44.0);
    let title_box = ui.add_box(
        header,
        Style {
            flex_grow: 1.0,
            min_size: Size {
                width: length(0.0),
                height: auto(),
            },
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
    ui.add_text(
        title_box,
        &labels.title,
        Style {
            flex_shrink: 1.0,
            min_size: Size {
                width: length(0.0),
                height: auto(),
            },
            ..Default::default()
        },
        single_line_style(28.0, 650, theme.text, Align::Left),
    );
    let (st_bg, st_bc, st_fg, st_dot) = if view.error.is_some() {
        (
            theme.error_soft,
            theme.error_border,
            theme.error,
            theme.error,
        )
    } else if view.enabled {
        (
            theme.primary_softer,
            theme.primary_ink.mix(theme.border, 0.35),
            theme.primary_ink,
            theme.primary_ink,
        )
    } else {
        (
            theme.surface,
            theme.border_strong,
            theme.text_secondary,
            theme.text_quiet,
        )
    };
    let status_pill = ui.add_box(
        title_box,
        Style {
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(8.0),
                height: length(0.0),
            },
            padding: Rect {
                left: length(14.0),
                right: length(14.0),
                top: length(5.0),
                bottom: length(5.0),
            },
            ..Default::default()
        },
        BoxPaint::new().bg(st_bg).border(1.0, st_bc).radius(999.0),
    );
    ui.add_dot(
        status_pill,
        Style {
            size: Size {
                width: length(9.0),
                height: length(9.0),
            },
            ..Default::default()
        },
        st_dot,
    );
    ui.add_text(
        status_pill,
        &view.status,
        Style::default(),
        single_line_style(15.0, 600, st_fg, Align::Left),
    );
    if matches!(
        view.save_state,
        DashboardSaveState::Saving | DashboardSaveState::Saved
    ) {
        let saving = view.save_state == DashboardSaveState::Saving;
        let sb = ui.add_box(
            title_box,
            Style {
                padding: Rect {
                    left: length(12.0),
                    right: length(12.0),
                    top: length(5.0),
                    bottom: length(5.0),
                },
                ..Default::default()
            },
            BoxPaint::new()
                .bg(if saving {
                    theme.primary_softer
                } else {
                    theme.primary_soft
                })
                .radius(999.0),
        );
        ui.add_text(
            sb,
            if saving {
                &labels.saving
            } else {
                &labels.saved
            },
            Style::default(),
            single_line_style(15.0, 600, theme.primary_ink, Align::Left),
        );
    }

    let global_bar = ui.add_box(
        header,
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
    let confirming = dash_ui.confirmation.is_some();
    let (mbtn, mfg) = add_btn(
        ui,
        global_bar,
        DashboardCommand::Action(DashboardAction::ToggleMaster),
        Style {
            min_size: Size {
                width: length(0.0),
                height: length(52.0),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(10.0),
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
        BtnOpts {
            active: view.enabled,
            disabled: confirming,
            pill: true,
            ..Default::default()
        },
        &mut region_seq,
    );
    ui.add_icon(mbtn, IconId::Power, 22.0, 1.9, mfg);
    ui.add_text(
        mbtn,
        &labels.master,
        Style::default(),
        single_line_style(20.0, 400, mfg, Align::Center),
    );
    add_toggle_pill(ui, mbtn, view.enabled);

    if view.network_capture.visible {
        let running = view.network_capture.running;
        let (nbtn, nfg) = add_btn(
            ui,
            global_bar,
            DashboardCommand::Action(DashboardAction::ToggleNetworkCapture),
            Style {
                min_size: Size {
                    width: length(0.0),
                    height: length(52.0),
                },
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: Size {
                    width: length(10.0),
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
            BtnOpts {
                active: running,
                disabled: !view.network_capture.available || confirming,
                pill: true,
                ..Default::default()
            },
            &mut region_seq,
        );
        ui.add_icon(nbtn, IconId::Radar, 22.0, 1.9, nfg);
        let lbl = if running {
            if labels.network_capture_stop.is_empty() {
                &labels.capture_stop
            } else {
                &labels.network_capture_stop
            }
        } else if labels.network_capture_start.is_empty() {
            &labels.capture_start
        } else {
            &labels.network_capture_start
        };
        ui.add_text(
            nbtn,
            lbl,
            Style::default(),
            single_line_style(20.0, 400, nfg, Align::Center),
        );
    }

    let cap_run = view.capture.running;
    let (cbtn, cfg) = add_btn(
        ui,
        global_bar,
        DashboardCommand::Action(DashboardAction::ToggleCapture),
        Style {
            min_size: Size {
                width: length(0.0),
                height: length(52.0),
            },
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(10.0),
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
        BtnOpts {
            primary: !cap_run,
            recording: cap_run,
            active: cap_run,
            disabled: !view.capture.available || confirming,
            pill: true,
            ..Default::default()
        },
        &mut region_seq,
    );
    ui.add_icon(
        cbtn,
        if cap_run { IconId::Square } else { IconId::Mic },
        22.0,
        1.9,
        cfg,
    );
    ui.add_text(
        cbtn,
        if cap_run {
            &labels.capture_stop
        } else {
            &labels.capture_start
        },
        Style::default(),
        single_line_style(20.0, 600, cfg, Align::Center),
    );

    let (xbtn, xfg) = add_btn(
        ui,
        global_bar,
        DashboardCommand::Close,
        Style {
            size: Size {
                width: length(52.0),
                height: length(52.0),
            },
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::Center),
            ..Default::default()
        },
        BtnOpts {
            icon: true,
            ..Default::default()
        },
        &mut region_seq,
    );
    ui.add_icon(xbtn, IconId::X, 22.0, 1.9, xfg);

    // Nav capsule.
    let nav = ui.add_box(
        panel,
        Style {
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            gap: Size {
                width: length(8.0),
                height: length(0.0),
            },
            padding: Rect {
                left: length(6.0),
                right: length(6.0),
                top: length(6.0),
                bottom: length(6.0),
            },
            ..Default::default()
        },
        BoxPaint::new()
            .bg(theme.surface)
            .border(2.0, theme.border)
            .radius(999.0),
    );
    for (pg, icon, lbl, shown) in [
        (
            DashboardPage::Display,
            IconId::Monitor,
            &labels.display_tab,
            true,
        ),
        (
            DashboardPage::Language,
            IconId::Languages,
            &labels.language_tab,
            true,
        ),
        (
            DashboardPage::Osc,
            IconId::Radio,
            &labels.osc_tab,
            view.osc_available,
        ),
    ] {
        if !shown {
            continue;
        }
        let active = dash_ui.page == pg;
        let (tbtn, tfg) = add_btn(
            ui,
            nav,
            DashboardCommand::Page(pg),
            Style {
                flex_grow: 1.0,
                flex_basis: length(0.0),
                min_size: Size {
                    width: length(0.0),
                    height: length(48.0),
                },
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                gap: Size {
                    width: length(10.0),
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
            BtnOpts {
                active,
                disabled: confirming,
                pill: true,
                transparent: !active,
                ..Default::default()
            },
            &mut region_seq,
        );
        ui.add_icon(tbtn, icon, 22.0, 1.9, tfg);
        ui.add_text(
            tbtn,
            lbl,
            Style::default(),
            single_line_style(20.0, 600, tfg, Align::Center),
        );
    }

    // Main body.
    let main = ui.add_box(
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
                height: length(14.0),
            },
            ..Default::default()
        },
        BoxPaint::new(),
    );

    if let Some(err) = view.error.as_deref().filter(|s| !s.is_empty()) {
        let b = ui.add_box(
            main,
            Style {
                min_size: Size {
                    width: length(0.0),
                    height: length(44.0),
                },
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
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
            BoxPaint::new()
                .bg(theme.error_soft)
                .border(1.0, theme.error_border)
                .radius(14.0),
        );
        ui.add_icon(b, IconId::TriangleAlert, 22.0, 1.9, theme.error);
        ui.add_text(
            b,
            err,
            Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                ..Default::default()
            },
            single_line_style(18.0, 400, theme.error, Align::Left),
        );
    }

    let show_master_off = view.error.is_none()
        && dash_ui.page == DashboardPage::Display
        && !view.enabled
        && dash_ui.display_editor.is_none()
        && dash_ui.picker.is_none()
        && dash_ui.confirmation.is_none();
    if show_master_off {
        let b = ui.add_box(
            main,
            Style {
                min_size: Size {
                    width: length(0.0),
                    height: length(44.0),
                },
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
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
            BoxPaint::new()
                .bg(theme.warning_soft)
                .border(1.0, theme.warning_border)
                .radius(14.0),
        );
        ui.add_icon(b, IconId::TriangleAlert, 22.0, 1.9, theme.warning);
        let msg = if labels.master_off_banner.is_empty() {
            &labels.subtitle
        } else {
            &labels.master_off_banner
        };
        ui.add_text(
            b,
            msg,
            Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                ..Default::default()
            },
            single_line_style(18.0, 400, theme.warning, Align::Left),
        );
    }

    if let Some(confirm) = &dash_ui.confirmation {
        let card = ui.add_box(
            main,
            Style {
                size: Size {
                    width: length(620.0),
                    height: auto(),
                },
                margin: Rect {
                    top: auto(),
                    bottom: auto(),
                    left: auto(),
                    right: auto(),
                },
                flex_direction: FlexDirection::Column,
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                gap: Size {
                    width: length(0.0),
                    height: length(16.0),
                },
                padding: Rect {
                    left: length(40.0),
                    right: length(40.0),
                    top: length(36.0),
                    bottom: length(36.0),
                },
                ..Default::default()
            },
            BoxPaint::new()
                .bg(theme.surface)
                .border(2.0, theme.border)
                .radius(20.0),
        );
        let icon_circle = ui.add_box(
            card,
            Style {
                size: Size {
                    width: length(72.0),
                    height: length(72.0),
                },
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                ..Default::default()
            },
            BoxPaint::new().bg(theme.error_soft).radius(36.0),
        );
        ui.add_icon(icon_circle, IconId::Trash2, 34.0, 1.9, theme.error);
        let title = if matches!(confirm.action, DashboardAction::DeleteLanguagePreset(_))
            && !labels.delete_preset_title.is_empty()
        {
            &labels.delete_preset_title
        } else {
            &labels.confirm_delete
        };
        ui.add_text(
            card,
            title,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                ..Default::default()
            },
            text_style(22.0, 600, theme.text),
        );
        ui.add_text(
            card,
            &confirm.label,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                ..Default::default()
            },
            single_line_style(28.0, 700, theme.text, Align::Center),
        );
        ui.add_text(
            card,
            &labels.delete_hint,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                ..Default::default()
            },
            TextStyle {
                font_size: 18.0,
                line_height: 24.0,
                align: Align::Center,
                color: theme.text_secondary,
                ..Default::default()
            },
        );
        let actions = ui.add_box(
            card,
            Style {
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                flex_direction: FlexDirection::Row,
                gap: Size {
                    width: length(18.0),
                    height: length(0.0),
                },
                margin: Rect {
                    top: length(12.0),
                    left: length(0.0),
                    right: length(0.0),
                    bottom: length(0.0),
                },
                ..Default::default()
            },
            BoxPaint::new(),
        );
        let (cbtn, cfg) = add_btn(
            ui,
            actions,
            DashboardCommand::Cancel,
            Style {
                flex_grow: 1.0,
                flex_basis: length(0.0),
                min_size: Size {
                    width: length(0.0),
                    height: length(56.0),
                },
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                ..Default::default()
            },
            BtnOpts::default(),
            &mut region_seq,
        );
        ui.add_text(
            cbtn,
            &labels.cancel,
            Style::default(),
            single_line_style(20.0, 400, cfg, Align::Center),
        );
        let (dbtn, dfg) = add_btn(
            ui,
            actions,
            DashboardCommand::Confirm,
            Style {
                flex_grow: 1.0,
                flex_basis: length(0.0),
                min_size: Size {
                    width: length(0.0),
                    height: length(56.0),
                },
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                ..Default::default()
            },
            BtnOpts {
                danger: true,
                ..Default::default()
            },
            &mut region_seq,
        );
        ui.add_text(
            dbtn,
            &labels.delete,
            Style::default(),
            single_line_style(20.0, 400, dfg, Align::Center),
        );
    } else if let Some(picker) = dash_ui.picker.as_ref()
        && let Some(choice) = picker.choice(view)
    {
        let card = ui.add_box(
            main,
            Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: length(0.0),
                },
                flex_direction: FlexDirection::Column,
                padding: Rect {
                    left: length(22.0),
                    right: length(22.0),
                    top: length(18.0),
                    bottom: length(18.0),
                },
                ..Default::default()
            },
            BoxPaint::new()
                .bg(theme.surface)
                .border(2.0, theme.border)
                .radius(20.0),
        );
        let phead = ui.add_box(
            card,
            Style {
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: Size {
                    width: length(12.0),
                    height: length(0.0),
                },
                margin: Rect {
                    bottom: length(16.0),
                    left: length(0.0),
                    right: length(0.0),
                    top: length(0.0),
                },
                ..Default::default()
            },
            BoxPaint::new(),
        );
        let (bbtn, bfg) = add_btn(
            ui,
            phead,
            DashboardCommand::Close,
            Style {
                min_size: Size {
                    width: length(0.0),
                    height: length(52.0),
                },
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: Size {
                    width: length(10.0),
                    height: length(0.0),
                },
                padding: Rect {
                    left: length(14.0),
                    right: length(18.0),
                    top: length(8.0),
                    bottom: length(8.0),
                },
                ..Default::default()
            },
            BtnOpts::default(),
            &mut region_seq,
        );
        ui.add_icon(bbtn, IconId::ChevronLeft, 22.0, 1.9, bfg);
        ui.add_text(
            bbtn,
            if labels.back.is_empty() {
                &labels.close
            } else {
                &labels.back
            },
            Style::default(),
            single_line_style(20.0, 400, bfg, Align::Center),
        );
        let ptitle_box = ui.add_box(
            phead,
            Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
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
        let picon = match picker {
            DashboardPicker::DeletePreset => IconId::Trash2,
            DashboardPicker::ApplyPreset => IconId::Bookmark,
            DashboardPicker::Mode => IconId::SlidersHorizontal,
            _ => IconId::Languages,
        };
        ui.add_icon(
            ptitle_box,
            picon,
            24.0,
            1.9,
            if matches!(picker, DashboardPicker::DeletePreset) {
                theme.error
            } else {
                theme.primary_ink
            },
        );
        let ptitle = match picker {
            DashboardPicker::Mode => labels.translation_mode.clone(),
            DashboardPicker::Target { group, index } => format!(
                "{} · #{}",
                match group {
                    TargetGroup::Microphone => &labels.microphone_language,
                    TargetGroup::Speaker => &labels.speaker_language,
                },
                index + 1
            ),
            DashboardPicker::DeletePreset => {
                if labels.delete_preset_title.is_empty() {
                    labels.delete.clone()
                } else {
                    labels.delete_preset_title.clone()
                }
            }
            DashboardPicker::ApplyPreset => labels.apply_preset.clone(),
        };
        ui.add_text(
            ptitle_box,
            ptitle,
            Style {
                flex_shrink: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                ..Default::default()
            },
            single_line_style(22.0, 600, theme.text, Align::Left),
        );

        let total_pages = choice.options.len().div_ceil(PICKER_PAGE_SIZE).max(1);
        let page = dash_ui.picker_page.min(total_pages - 1);
        if total_pages > 1 {
            let pager = ui.add_box(
                phead,
                Style {
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
            ui.add_text(
                pager,
                format!("{} / {}", page + 1, total_pages),
                Style {
                    min_size: Size {
                        width: length(72.0),
                        height: auto(),
                    },
                    ..Default::default()
                },
                single_line_style(20.0, 400, theme.text_secondary, Align::Center),
            );
            let (pb, pfg) = add_btn(
                ui,
                pager,
                DashboardCommand::PickerPage(-1),
                Style {
                    size: Size {
                        width: length(52.0),
                        height: length(52.0),
                    },
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::Center),
                    ..Default::default()
                },
                BtnOpts {
                    icon: true,
                    disabled: page == 0,
                    ..Default::default()
                },
                &mut region_seq,
            );
            ui.add_icon(pb, IconId::ChevronLeft, 22.0, 1.9, pfg);
            let (nb, nfg) = add_btn(
                ui,
                pager,
                DashboardCommand::PickerPage(1),
                Style {
                    size: Size {
                        width: length(52.0),
                        height: length(52.0),
                    },
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::Center),
                    ..Default::default()
                },
                BtnOpts {
                    icon: true,
                    disabled: page + 1 >= total_pages,
                    ..Default::default()
                },
                &mut region_seq,
            );
            ui.add_icon(nb, IconId::ChevronRight, 22.0, 1.9, nfg);
        }

        let compact = choice.options.len() <= 6;
        let single_col = compact && choice.options.len() <= 3;
        let cols = if single_col {
            1
        } else if compact {
            2
        } else {
            3
        };
        let grid_style = if compact {
            Style {
                flex_grow: 1.0,
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                max_size: Size {
                    width: length(if single_col { 620.0 } else { 920.0 }),
                    height: auto(),
                },
                align_self: Some(AlignSelf::Center),
                justify_content: Some(JustifyContent::Center),
                display: Display::Grid,
                grid_template_columns: vec![minmax(length(0.0), fr(1.0_f32)); cols],
                gap: Size {
                    width: length(16.0),
                    height: length(12.0),
                },
                ..Default::default()
            }
        } else {
            Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: length(0.0),
                },
                display: Display::Grid,
                grid_template_columns: vec![minmax(length(0.0), fr(1.0_f32)); 3],
                grid_template_rows: vec![minmax(length(0.0), fr(1.0_f32)); 5],
                gap: Size {
                    width: length(16.0),
                    height: length(12.0),
                },
                ..Default::default()
            }
        };
        let grid = ui.add_box(card, grid_style, BoxPaint::new());
        let start = page * PICKER_PAGE_SIZE;
        let end = (start + PICKER_PAGE_SIZE).min(choice.options.len());
        let deleting = matches!(picker, DashboardPicker::DeletePreset);
        for (idx, opt) in choice.options[start..end].iter().enumerate() {
            let selected = !deleting && (choice.value == opt.value || choice.value == opt.label);
            let (btn, fg) = add_btn(
                ui,
                grid,
                DashboardCommand::Select(start + idx),
                Style {
                    min_size: Size {
                        width: length(0.0),
                        height: length(if compact { 76.0 } else { 0.0 }),
                    },
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::SpaceBetween),
                    padding: Rect {
                        left: length(18.0),
                        right: length(18.0),
                        top: length(10.0),
                        bottom: length(10.0),
                    },
                    ..Default::default()
                },
                BtnOpts {
                    active: selected,
                    danger: deleting,
                    ..Default::default()
                },
                &mut region_seq,
            );
            let fsz = if compact { 22.0 } else { 20.0 };
            ui.add_text(
                btn,
                &opt.label,
                Style {
                    flex_grow: 1.0,
                    min_size: Size {
                        width: length(0.0),
                        height: auto(),
                    },
                    ..Default::default()
                },
                TextStyle {
                    font_size: fsz,
                    line_height: (fsz * 1.25f32).round(),
                    max_lines: Some(2),
                    color: fg,
                    ..Default::default()
                },
            );
            if selected {
                ui.add_icon(btn, IconId::Check, 22.0, 1.9, fg);
            }
        }
    } else if let Some(group) = dash_ui.recognition_editor {
        let choice = view.language.recognition(group);
        let card = ui.add_box(
            main,
            Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: length(0.0),
                },
                flex_direction: FlexDirection::Column,
                gap: Size {
                    width: length(0.0),
                    height: length(12.0),
                },
                padding: Rect {
                    left: length(22.0),
                    right: length(22.0),
                    top: length(18.0),
                    bottom: length(18.0),
                },
                ..Default::default()
            },
            BoxPaint::new()
                .bg(theme.surface)
                .border(2.0, theme.border)
                .radius(20.0),
        );
        let rhead = ui.add_box(
            card,
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
        let (bbtn, bfg) = add_btn(
            ui,
            rhead,
            DashboardCommand::Close,
            Style {
                min_size: Size {
                    width: length(0.0),
                    height: length(52.0),
                },
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: Size {
                    width: length(10.0),
                    height: length(0.0),
                },
                padding: Rect {
                    left: length(14.0),
                    right: length(18.0),
                    top: length(8.0),
                    bottom: length(8.0),
                },
                ..Default::default()
            },
            BtnOpts::default(),
            &mut region_seq,
        );
        ui.add_icon(bbtn, IconId::ChevronLeft, 22.0, 1.9, bfg);
        ui.add_text(
            bbtn,
            if labels.back.is_empty() {
                &labels.close
            } else {
                &labels.back
            },
            Style::default(),
            single_line_style(20.0, 400, bfg, Align::Center),
        );
        let rtitle_box = ui.add_box(
            rhead,
            Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
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
        ui.add_icon(
            rtitle_box,
            if group == TargetGroup::Microphone {
                IconId::Mic
            } else {
                IconId::Volume2
            },
            24.0,
            1.9,
            theme.primary_ink,
        );
        let rtitle = match group {
            TargetGroup::Microphone if !labels.microphone_recognition_language.is_empty() => {
                &labels.microphone_recognition_language
            }
            TargetGroup::Speaker if !labels.speaker_recognition_language.is_empty() => {
                &labels.speaker_recognition_language
            }
            _ => &labels.recognition_language,
        };
        ui.add_text(
            rtitle_box,
            rtitle,
            Style {
                flex_shrink: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                ..Default::default()
            },
            single_line_style(22.0, 600, theme.text, Align::Left),
        );
        let count_pill = ui.add_box(
            rtitle_box,
            Style {
                padding: Rect {
                    left: length(12.0),
                    right: length(12.0),
                    top: length(4.0),
                    bottom: length(4.0),
                },
                ..Default::default()
            },
            BoxPaint::new()
                .bg(theme.surface_subtle)
                .border(1.0, theme.border)
                .radius(999.0),
        );
        ui.add_text(
            count_pill,
            format!("{} / {}", choice.selected.len(), choice.max_selected),
            Style::default(),
            single_line_style(15.0, 600, theme.text_secondary, Align::Center),
        );
        let auto_lbl = if labels.recognition_auto.is_empty() {
            "Auto"
        } else {
            &labels.recognition_auto
        };
        let (abtn, afg) = add_btn(
            ui,
            rhead,
            DashboardCommand::Action(DashboardAction::ClearRecognitionLanguages(group)),
            Style {
                min_size: Size {
                    width: length(0.0),
                    height: length(52.0),
                },
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: Size {
                    width: length(8.0),
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
            BtnOpts {
                active: choice.selected.is_empty(),
                disabled: choice.selected.is_empty() || choice.options.is_empty(),
                ..Default::default()
            },
            &mut region_seq,
        );
        ui.add_icon(abtn, IconId::RotateCcw, 20.0, 1.9, afg);
        ui.add_text(
            abtn,
            auto_lbl,
            Style::default(),
            single_line_style(18.0, 600, afg, Align::Center),
        );

        let selected_strip = ui.add_box(
            card,
            Style {
                min_size: Size {
                    width: length(0.0),
                    height: length(64.0),
                },
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                padding: Rect {
                    left: length(12.0),
                    right: length(12.0),
                    top: length(8.0),
                    bottom: length(8.0),
                },
                ..Default::default()
            },
            BoxPaint::new()
                .bg(theme.surface_subtle)
                .border(1.0, theme.border)
                .radius(14.0),
        );
        if choice.selected.is_empty() {
            ui.add_text(
                selected_strip,
                auto_lbl,
                Style {
                    flex_grow: 1.0,
                    ..Default::default()
                },
                single_line_style(18.0, 400, theme.text_secondary, Align::Left),
            );
        } else {
            let chip_grid = ui.add_box(
                selected_strip,
                Style {
                    flex_grow: 1.0,
                    display: Display::Grid,
                    grid_template_columns: vec![minmax(length(0.0), fr(1.0_f32)); 4],
                    gap: Size {
                        width: length(10.0),
                        height: length(0.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new(),
            );
            for (index, item) in choice.selected.iter().enumerate() {
                let chip = ui.add_box(
                    chip_grid,
                    Style {
                        min_size: Size {
                            width: length(0.0),
                            height: length(46.0),
                        },
                        flex_direction: FlexDirection::Row,
                        align_items: Some(AlignItems::Center),
                        gap: Size {
                            width: length(6.0),
                            height: length(0.0),
                        },
                        padding: Rect {
                            left: length(8.0),
                            right: length(6.0),
                            top: length(4.0),
                            bottom: length(4.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new()
                        .bg(theme.surface)
                        .border(1.0, theme.border)
                        .radius(12.0),
                );
                let rank = ui.add_box(
                    chip,
                    Style {
                        min_size: Size {
                            width: length(24.0),
                            height: auto(),
                        },
                        padding: Rect {
                            left: length(7.0),
                            right: length(7.0),
                            top: length(2.0),
                            bottom: length(2.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new()
                        .bg(if index == 0 {
                            theme.primary_soft
                        } else {
                            theme.surface_subtle
                        })
                        .radius(999.0),
                );
                ui.add_text(
                    rank,
                    (index + 1).to_string(),
                    Style::default(),
                    single_line_style(
                        14.0,
                        600,
                        if index == 0 {
                            theme.primary_ink
                        } else {
                            theme.text_secondary
                        },
                        Align::Center,
                    ),
                );
                ui.add_text(
                    chip,
                    &item.label,
                    Style {
                        flex_grow: 1.0,
                        min_size: Size {
                            width: length(0.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    single_line_style(17.0, 600, theme.text, Align::Left),
                );
                if index > 0 {
                    let can_prio = item.can_prioritize
                        || choice.options.iter().any(|opt| opt.value == item.value);
                    let (pb, pfg) = add_btn(
                        ui,
                        chip,
                        DashboardCommand::Action(DashboardAction::PrioritizeRecognitionLanguage {
                            group,
                            index,
                        }),
                        Style {
                            size: Size {
                                width: length(34.0),
                                height: length(34.0),
                            },
                            align_items: Some(AlignItems::Center),
                            justify_content: Some(JustifyContent::Center),
                            ..Default::default()
                        },
                        BtnOpts {
                            icon: true,
                            disabled: !can_prio,
                            ..Default::default()
                        },
                        &mut region_seq,
                    );
                    ui.add_icon(pb, IconId::ArrowUp, 18.0, 1.9, pfg);
                }
                let (rb, rfg) = add_btn(
                    ui,
                    chip,
                    DashboardCommand::Action(DashboardAction::ToggleRecognitionLanguage {
                        group,
                        value: item.value.clone(),
                    }),
                    Style {
                        size: Size {
                            width: length(34.0),
                            height: length(34.0),
                        },
                        align_items: Some(AlignItems::Center),
                        justify_content: Some(JustifyContent::Center),
                        ..Default::default()
                    },
                    BtnOpts {
                        icon: true,
                        ..Default::default()
                    },
                    &mut region_seq,
                );
                ui.add_icon(rb, IconId::X, 18.0, 1.9, rfg);
            }
        }

        if let Some(note) = choice.note.as_deref().filter(|s| !s.is_empty()) {
            let warn = choice.note_is_warning;
            let nbox = ui.add_box(
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
                        top: length(8.0),
                        bottom: length(8.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new()
                    .bg(if warn {
                        theme.error_soft
                    } else {
                        theme.surface_subtle
                    })
                    .border(
                        1.0,
                        if warn {
                            theme.error_border
                        } else {
                            theme.border
                        },
                    )
                    .radius(14.0),
            );
            ui.add_icon(
                nbox,
                if warn {
                    IconId::TriangleAlert
                } else {
                    IconId::Info
                },
                20.0,
                1.9,
                if warn {
                    theme.error
                } else {
                    theme.text_secondary
                },
            );
            ui.add_text(
                nbox,
                note,
                Style {
                    flex_grow: 1.0,
                    min_size: Size {
                        width: length(0.0),
                        height: auto(),
                    },
                    ..Default::default()
                },
                text_style(
                    16.0,
                    400,
                    if warn {
                        theme.error
                    } else {
                        theme.text_secondary
                    },
                ),
            );
        }

        let compact = choice.options.len() <= 6;
        let single_col = compact && choice.options.len() <= 3;
        let cols = if single_col {
            1
        } else if compact {
            2
        } else {
            3
        };
        let grid_style = if compact {
            Style {
                flex_grow: 1.0,
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                max_size: Size {
                    width: length(if single_col { 620.0 } else { 920.0 }),
                    height: auto(),
                },
                align_self: Some(AlignSelf::Center),
                justify_content: Some(JustifyContent::Center),
                display: Display::Grid,
                grid_template_columns: vec![minmax(length(0.0), fr(1.0_f32)); cols],
                gap: Size {
                    width: length(16.0),
                    height: length(12.0),
                },
                ..Default::default()
            }
        } else {
            let rows = choice.options.len().div_ceil(3).max(1);
            Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: length(0.0),
                },
                display: Display::Grid,
                grid_template_columns: vec![minmax(length(0.0), fr(1.0_f32)); 3],
                grid_template_rows: vec![minmax(length(0.0), fr(1.0_f32)); rows],
                gap: Size {
                    width: length(16.0),
                    height: length(10.0),
                },
                ..Default::default()
            }
        };
        let grid = ui.add_box(card, grid_style, BoxPaint::new());
        let at_limit = choice.selected.len() >= choice.max_selected;
        for opt in &choice.options {
            let selected_pos = choice.selected.iter().position(|s| s.value == opt.value);
            let selected = selected_pos.is_some();
            let (btn, fg) = add_btn(
                ui,
                grid,
                DashboardCommand::Action(DashboardAction::ToggleRecognitionLanguage {
                    group,
                    value: opt.value.clone(),
                }),
                Style {
                    min_size: Size {
                        width: length(0.0),
                        height: length(if compact { 76.0 } else { 0.0 }),
                    },
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    gap: Size {
                        width: length(10.0),
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
                BtnOpts {
                    active: selected,
                    disabled: !selected && at_limit,
                    ..Default::default()
                },
                &mut region_seq,
            );
            if let Some(pos) = selected_pos {
                let badge = ui.add_box(
                    btn,
                    Style {
                        min_size: Size {
                            width: length(26.0),
                            height: auto(),
                        },
                        padding: Rect {
                            left: length(8.0),
                            right: length(8.0),
                            top: length(2.0),
                            bottom: length(2.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new().bg(theme.surface).radius(999.0),
                );
                ui.add_text(
                    badge,
                    (pos + 1).to_string(),
                    Style::default(),
                    single_line_style(14.0, 600, theme.primary_ink, Align::Center),
                );
            }
            let fsz = if compact { 22.0 } else { 20.0 };
            ui.add_text(
                btn,
                &opt.label,
                Style {
                    flex_grow: 1.0,
                    min_size: Size {
                        width: length(0.0),
                        height: auto(),
                    },
                    ..Default::default()
                },
                TextStyle {
                    font_size: fsz,
                    line_height: (fsz * 1.25f32).round(),
                    max_lines: Some(2),
                    color: fg,
                    ..Default::default()
                },
            );
            if selected {
                ui.add_icon(btn, IconId::Check, 22.0, 1.9, fg);
            }
        }
    } else if let Some(kind) = dash_ui.display_editor {
        let card = ui.add_box(
            main,
            Style {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                gap: Size {
                    width: length(0.0),
                    height: length(14.0),
                },
                padding: Rect {
                    left: length(22.0),
                    right: length(22.0),
                    top: length(18.0),
                    bottom: length(18.0),
                },
                ..Default::default()
            },
            BoxPaint::new()
                .bg(theme.surface)
                .border(2.0, theme.border)
                .radius(20.0),
        );
        let ehead = ui.add_box(
            card,
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
        let (bbtn, bfg) = add_btn(
            ui,
            ehead,
            DashboardCommand::Close,
            Style {
                min_size: Size {
                    width: length(0.0),
                    height: length(52.0),
                },
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: Size {
                    width: length(10.0),
                    height: length(0.0),
                },
                padding: Rect {
                    left: length(14.0),
                    right: length(18.0),
                    top: length(8.0),
                    bottom: length(8.0),
                },
                ..Default::default()
            },
            BtnOpts::default(),
            &mut region_seq,
        );
        ui.add_icon(bbtn, IconId::ChevronLeft, 22.0, 1.9, bfg);
        ui.add_text(
            bbtn,
            if labels.back.is_empty() {
                &labels.close
            } else {
                &labels.back
            },
            Style::default(),
            single_line_style(20.0, 400, bfg, Align::Center),
        );
        let stitle = ui.add_box(
            ehead,
            Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
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
        ui.add_icon(
            stitle,
            if kind == DisplayKind::Wrist {
                IconId::Hand
            } else {
                IconId::Monitor
            },
            24.0,
            1.9,
            theme.primary_ink,
        );
        ui.add_text(
            stitle,
            format!(
                "{} · {}",
                if kind == DisplayKind::Wrist {
                    &labels.wrist
                } else {
                    &labels.headset
                },
                labels.position
            ),
            Style {
                flex_shrink: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: auto(),
                },
                ..Default::default()
            },
            single_line_style(22.0, 600, theme.text, Align::Left),
        );
        let (preview_visible, pos_fields) = match kind {
            DisplayKind::Headset => (view.headset.preview_visible, &view.headset.position),
            DisplayKind::Wrist => (view.wrist.preview_visible, &view.wrist.position),
        };
        let enabled_disp = match kind {
            DisplayKind::Headset => view.headset.enabled,
            DisplayKind::Wrist => view.wrist.enabled,
        };
        let (pbtn, pfg) = add_btn(
            ui,
            ehead,
            DashboardCommand::Action(if kind == DisplayKind::Wrist {
                DashboardAction::PreviewWrist
            } else {
                DashboardAction::PreviewHeadset
            }),
            Style {
                min_size: Size {
                    width: length(0.0),
                    height: length(52.0),
                },
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: Size {
                    width: length(10.0),
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
            BtnOpts {
                active: preview_visible,
                disabled: !view.enabled || !enabled_disp,
                ..Default::default()
            },
            &mut region_seq,
        );
        ui.add_icon(pbtn, IconId::Eye, 22.0, 1.9, pfg);
        ui.add_text(
            pbtn,
            if preview_visible {
                &labels.hide_preview
            } else {
                &labels.preview
            },
            Style::default(),
            single_line_style(20.0, 400, pfg, Align::Center),
        );

        let is_rot = |f: PositionField| {
            matches!(
                f,
                PositionField::Pitch | PositionField::Yaw | PositionField::Roll
            )
        };
        for (rotation, heading, hint) in [
            (false, &labels.position, &labels.position_hint),
            (true, &labels.rotation, &labels.rotation_hint),
        ] {
            let sec = ui.add_box(
                card,
                Style {
                    flex_direction: FlexDirection::Column,
                    gap: Size {
                        width: length(0.0),
                        height: length(6.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new(),
            );
            let hrow = ui.add_box(
                sec,
                Style {
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    justify_content: Some(JustifyContent::SpaceBetween),
                    ..Default::default()
                },
                BoxPaint::new(),
            );
            ui.add_text(
                hrow,
                heading,
                Style::default(),
                single_line_style(20.0, 600, theme.text, Align::Left),
            );
            ui.add_text(
                hrow,
                hint,
                Style::default(),
                single_line_style(18.0, 400, theme.text_secondary, Align::Right),
            );
            let grid = ui.add_box(
                sec,
                Style {
                    display: Display::Grid,
                    grid_template_columns: vec![minmax(length(0.0), fr(1.0_f32)); 3],
                    gap: Size {
                        width: length(12.0),
                        height: length(12.0),
                    },
                    ..Default::default()
                },
                BoxPaint::new(),
            );
            for item in pos_fields.iter().filter(|i| is_rot(i.field) == rotation) {
                add_stepper(
                    ui,
                    grid,
                    &item.label,
                    &item.value,
                    DashboardAction::AdjustDisplayPosition {
                        kind,
                        field: item.field,
                        direction: -1,
                        coarse: false,
                    },
                    DashboardAction::AdjustDisplayPosition {
                        kind,
                        field: item.field,
                        direction: 1,
                        coarse: false,
                    },
                    item.can_decrease,
                    item.can_increase,
                    44.0,
                    &mut region_seq,
                );
            }
        }

        if kind == DisplayKind::Wrist {
            let wrist_world = view.wrist.anchor_mode == "world";
            let bar = ui.add_box(
                card,
                Style {
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    gap: Size {
                        width: length(12.0),
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
                BoxPaint::new()
                    .bg(theme.surface_subtle)
                    .border(1.0, theme.border)
                    .radius(14.0),
            );
            ui.add_text(
                bar,
                if labels.anchor_mode.is_empty() {
                    &labels.position
                } else {
                    &labels.anchor_mode
                },
                Style::default(),
                single_line_style(18.0, 400, theme.text_secondary, Align::Left),
            );
            let seg = ui.add_box(
                bar,
                Style {
                    flex_grow: 1.0,
                    max_size: Size {
                        width: length(420.0),
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
                    .bg(theme.surface)
                    .border(1.0, theme.border)
                    .radius(999.0),
            );
            for (act, lbl, active) in [
                (
                    DashboardAction::AttachDialogue,
                    labels.attach_dialogue.as_str(),
                    !wrist_world,
                ),
                (
                    DashboardAction::RecenterDialogue,
                    if labels.anchor_world.is_empty() {
                        "World"
                    } else {
                        labels.anchor_world.as_str()
                    },
                    wrist_world,
                ),
            ] {
                let (sb, sfg) = add_btn(
                    ui,
                    seg,
                    DashboardCommand::Action(act),
                    Style {
                        flex_grow: 1.0,
                        flex_basis: length(0.0),
                        min_size: Size {
                            width: length(0.0),
                            height: length(44.0),
                        },
                        align_items: Some(AlignItems::Center),
                        justify_content: Some(JustifyContent::Center),
                        padding: Rect {
                            left: length(12.0),
                            right: length(12.0),
                            top: length(6.0),
                            bottom: length(6.0),
                        },
                        ..Default::default()
                    },
                    BtnOpts {
                        active,
                        pill: true,
                        transparent: !active,
                        ..Default::default()
                    },
                    &mut region_seq,
                );
                ui.add_text(
                    sb,
                    lbl,
                    Style::default(),
                    single_line_style(18.0, if active { 600 } else { 400 }, sfg, Align::Center),
                );
            }
            add_toggle_row(
                ui,
                bar,
                DashboardAction::ToggleWristFollowView,
                if labels.follow_view.is_empty() {
                    "Follow view"
                } else {
                    &labels.follow_view
                },
                view.wrist.follow_view,
                !wrist_world,
                None,
                &mut region_seq,
            );
            let (rcbtn, rcfg) = add_btn(
                ui,
                bar,
                DashboardCommand::Action(DashboardAction::RecenterDialogue),
                Style {
                    min_size: Size {
                        width: length(0.0),
                        height: length(44.0),
                    },
                    flex_direction: FlexDirection::Row,
                    align_items: Some(AlignItems::Center),
                    gap: Size {
                        width: length(8.0),
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
                BtnOpts {
                    disabled: !wrist_world,
                    ..Default::default()
                },
                &mut region_seq,
            );
            ui.add_icon(rcbtn, IconId::Pin, 22.0, 1.9, rcfg);
            ui.add_text(
                rcbtn,
                &labels.recenter_dialogue,
                Style::default(),
                single_line_style(18.0, 400, rcfg, Align::Center),
            );
        }

        ui.add_box(
            card,
            Style {
                flex_grow: 1.0,
                ..Default::default()
            },
            BoxPaint::new(),
        );
        let rrow = ui.add_box(
            card,
            Style {
                justify_content: Some(JustifyContent::Center),
                ..Default::default()
            },
            BoxPaint::new(),
        );
        let (rbtn, rfg) = add_btn(
            ui,
            rrow,
            DashboardCommand::Action(DashboardAction::ResetDisplayPosition(kind)),
            Style {
                min_size: Size {
                    width: length(320.0),
                    height: length(52.0),
                },
                max_size: Size {
                    width: length(420.0),
                    height: auto(),
                },
                flex_grow: 1.0,
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                justify_content: Some(JustifyContent::Center),
                gap: Size {
                    width: length(10.0),
                    height: length(0.0),
                },
                ..Default::default()
            },
            BtnOpts::default(),
            &mut region_seq,
        );
        ui.add_icon(rbtn, IconId::RotateCcw, 22.0, 1.9, rfg);
        ui.add_text(
            rbtn,
            &labels.reset_position,
            Style::default(),
            single_line_style(20.0, 400, rfg, Align::Center),
        );
    } else {
        match dash_ui.page {
            DashboardPage::Display => {
                let cbox = ui.add_box(
                    main,
                    Style {
                        flex_direction: FlexDirection::Row,
                        align_items: Some(AlignItems::Center),
                        gap: Size {
                            width: length(16.0),
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
                    BoxPaint::new()
                        .bg(theme.surface)
                        .border(2.0, theme.border)
                        .radius(20.0),
                );
                ui.add_text(
                    cbox,
                    &labels.content,
                    Style::default(),
                    single_line_style(20.0, 600, theme.text, Align::Left),
                );
                add_segments(
                    ui,
                    cbox,
                    &view.content,
                    &|v| DashboardAction::SetDisplayContent(v.to_owned()),
                    false,
                    Style {
                        flex_grow: 1000.0,
                        max_size: Size {
                            width: length(660.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    theme.surface_subtle,
                    &mut region_seq,
                );
                ui.add_box(
                    cbox,
                    Style {
                        flex_grow: 1.0,
                        ..Default::default()
                    },
                    BoxPaint::new(),
                );
                ui.add_text(
                    cbox,
                    &labels.content_hint,
                    Style {
                        max_size: Size {
                            width: length(340.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    single_line_style(18.0, 400, theme.text_secondary, Align::Right),
                );

                let cols = ui.add_box(
                    main,
                    Style {
                        display: Display::Grid,
                        grid_template_columns: vec![minmax(length(0.0), fr(1.0_f32)); 2],
                        gap: Size {
                            width: length(16.0),
                            height: length(0.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new(),
                );
                for kind in [DisplayKind::Headset, DisplayKind::Wrist] {
                    let wrist = kind == DisplayKind::Wrist;
                    let (disp_en, disp_w, disp_op, disp_prev, appearance) = if wrist {
                        (
                            view.wrist.enabled,
                            &view.wrist.width,
                            &view.wrist.opacity,
                            view.wrist.preview_visible,
                            &view.wrist.appearance,
                        )
                    } else {
                        (
                            view.headset.enabled,
                            &view.headset.width,
                            &view.headset.opacity,
                            view.headset.preview_visible,
                            &view.headset.appearance,
                        )
                    };
                    let mut card_paint = BoxPaint::new()
                        .bg(theme.surface)
                        .border(2.0, theme.border)
                        .radius(20.0);
                    let card = ui.add_box(
                        cols,
                        Style {
                            flex_direction: FlexDirection::Column,
                            padding: Rect {
                                left: length(20.0),
                                right: length(20.0),
                                top: length(18.0),
                                bottom: length(18.0),
                            },
                            ..Default::default()
                        },
                        card_paint.clone(),
                    );
                    let chead = ui.add_box(
                        card,
                        Style {
                            min_size: Size {
                                width: length(0.0),
                                height: length(44.0),
                            },
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
                            gap: Size {
                                width: length(12.0),
                                height: length(0.0),
                            },
                            margin: Rect {
                                bottom: length(10.0),
                                left: length(0.0),
                                right: length(0.0),
                                top: length(0.0),
                            },
                            ..Default::default()
                        },
                        BoxPaint::new(),
                    );
                    ui.add_icon(
                        chead,
                        if wrist { IconId::Hand } else { IconId::Monitor },
                        24.0,
                        1.9,
                        theme.primary_ink,
                    );
                    ui.add_text(
                        chead,
                        if wrist {
                            &labels.wrist
                        } else {
                            &labels.headset
                        },
                        Style {
                            flex_grow: 1.0,
                            min_size: Size {
                                width: length(0.0),
                                height: auto(),
                            },
                            ..Default::default()
                        },
                        single_line_style(22.0, 600, theme.text, Align::Left),
                    );
                    if wrist && view.wrist.anchor_mode == "world" {
                        let p = ui.add_box(
                            chead,
                            Style {
                                padding: Rect {
                                    left: length(12.0),
                                    right: length(12.0),
                                    top: length(4.0),
                                    bottom: length(4.0),
                                },
                                ..Default::default()
                            },
                            BoxPaint::new()
                                .bg(theme.surface_subtle)
                                .border(1.0, theme.border)
                                .radius(999.0),
                        );
                        ui.add_text(
                            p,
                            if labels.anchor_world.is_empty() {
                                "World"
                            } else {
                                &labels.anchor_world
                            },
                            Style::default(),
                            single_line_style(15.0, 600, theme.text_secondary, Align::Center),
                        );
                    }
                    add_toggle_row(
                        ui,
                        chead,
                        if wrist {
                            DashboardAction::ToggleWrist
                        } else {
                            DashboardAction::ToggleHeadset
                        },
                        "",
                        disp_en,
                        false,
                        None,
                        &mut region_seq,
                    );

                    card_paint.opacity = if !view.enabled || !disp_en { 0.68 } else { 1.0 };
                    let body = ui.add_box(
                        card,
                        Style {
                            flex_direction: FlexDirection::Column,
                            ..Default::default()
                        },
                        BoxPaint {
                            opacity: card_paint.opacity,
                            ..BoxPaint::new()
                        },
                    );
                    let opts_row = ui.add_box(
                        body,
                        Style {
                            min_size: Size {
                                width: length(0.0),
                                height: length(52.0),
                            },
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
                            gap: Size {
                                width: length(14.0),
                                height: length(0.0),
                            },
                            margin: Rect {
                                bottom: length(10.0),
                                left: length(0.0),
                                right: length(0.0),
                                top: length(0.0),
                            },
                            ..Default::default()
                        },
                        BoxPaint::new(),
                    );
                    if wrist {
                        ui.add_text(
                            opts_row,
                            &labels.hand,
                            Style::default(),
                            single_line_style(18.0, 400, theme.text_secondary, Align::Left),
                        );
                        add_segments(
                            ui,
                            opts_row,
                            &view.wrist.hand,
                            &|v| DashboardAction::SetWristHand(v.to_owned()),
                            false,
                            Style {
                                flex_grow: 1.0,
                                ..Default::default()
                            },
                            theme.surface_subtle,
                            &mut region_seq,
                        );
                    } else {
                        ui.add_text(
                            opts_row,
                            &labels.headset_hint,
                            Style {
                                flex_grow: 1.0,
                                ..Default::default()
                            },
                            text_style(18.0, 400, theme.text_secondary),
                        );
                    }

                    let steppers = ui.add_box(
                        body,
                        Style {
                            flex_direction: FlexDirection::Row,
                            gap: Size {
                                width: length(10.0),
                                height: length(0.0),
                            },
                            margin: Rect {
                                bottom: length(12.0),
                                left: length(0.0),
                                right: length(0.0),
                                top: length(0.0),
                            },
                            ..Default::default()
                        },
                        BoxPaint::new(),
                    );
                    let find_app = |f: AppearanceField| appearance.iter().find(|a| a.field == f);
                    if let Some(wf) = find_app(AppearanceField::Width) {
                        add_stepper(
                            ui,
                            steppers,
                            &labels.width,
                            &wf.value,
                            DashboardAction::AdjustDisplayAppearance {
                                kind,
                                field: AppearanceField::Width,
                                direction: -1,
                                coarse: false,
                            },
                            DashboardAction::AdjustDisplayAppearance {
                                kind,
                                field: AppearanceField::Width,
                                direction: 1,
                                coarse: false,
                            },
                            wf.can_decrease,
                            wf.can_increase,
                            42.0,
                            &mut region_seq,
                        );
                    } else {
                        add_stepper(
                            ui,
                            steppers,
                            &labels.width,
                            disp_w,
                            if wrist {
                                DashboardAction::WristWidthDown
                            } else {
                                DashboardAction::HeadsetWidthDown
                            },
                            if wrist {
                                DashboardAction::WristWidthUp
                            } else {
                                DashboardAction::HeadsetWidthUp
                            },
                            true,
                            true,
                            42.0,
                            &mut region_seq,
                        );
                    }
                    if let Some(of) = find_app(AppearanceField::Opacity) {
                        add_stepper(
                            ui,
                            steppers,
                            &labels.opacity,
                            &of.value,
                            DashboardAction::AdjustDisplayAppearance {
                                kind,
                                field: AppearanceField::Opacity,
                                direction: -1,
                                coarse: false,
                            },
                            DashboardAction::AdjustDisplayAppearance {
                                kind,
                                field: AppearanceField::Opacity,
                                direction: 1,
                                coarse: false,
                            },
                            of.can_decrease,
                            of.can_increase,
                            42.0,
                            &mut region_seq,
                        );
                    } else {
                        add_stepper(
                            ui,
                            steppers,
                            &labels.opacity,
                            disp_op,
                            if wrist {
                                DashboardAction::WristOpacityDown
                            } else {
                                DashboardAction::HeadsetOpacityDown
                            },
                            if wrist {
                                DashboardAction::WristOpacityUp
                            } else {
                                DashboardAction::HeadsetOpacityUp
                            },
                            true,
                            true,
                            42.0,
                            &mut region_seq,
                        );
                    }
                    if let Some(ff) = find_app(AppearanceField::FontSize) {
                        add_stepper(
                            ui,
                            steppers,
                            &ff.label,
                            &ff.value,
                            DashboardAction::AdjustDisplayAppearance {
                                kind,
                                field: AppearanceField::FontSize,
                                direction: -1,
                                coarse: false,
                            },
                            DashboardAction::AdjustDisplayAppearance {
                                kind,
                                field: AppearanceField::FontSize,
                                direction: 1,
                                coarse: false,
                            },
                            ff.can_decrease,
                            ff.can_increase,
                            42.0,
                            &mut region_seq,
                        );
                    }

                    let actions = ui.add_box(
                        body,
                        Style {
                            flex_direction: FlexDirection::Row,
                            gap: Size {
                                width: length(12.0),
                                height: length(0.0),
                            },
                            ..Default::default()
                        },
                        BoxPaint::new(),
                    );
                    let (pos_btn, pos_fg) = add_btn(
                        ui,
                        actions,
                        DashboardCommand::Position(kind),
                        Style {
                            flex_grow: 1.0,
                            flex_basis: length(0.0),
                            min_size: Size {
                                width: length(0.0),
                                height: length(52.0),
                            },
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
                            justify_content: Some(JustifyContent::Center),
                            gap: Size {
                                width: length(10.0),
                                height: length(0.0),
                            },
                            ..Default::default()
                        },
                        BtnOpts::default(),
                        &mut region_seq,
                    );
                    ui.add_icon(pos_btn, IconId::Settings2, 22.0, 1.9, pos_fg);
                    ui.add_text(
                        pos_btn,
                        &labels.position,
                        Style::default(),
                        single_line_style(20.0, 400, pos_fg, Align::Center),
                    );
                    let (prev_btn, prev_fg) = add_btn(
                        ui,
                        actions,
                        DashboardCommand::Action(if wrist {
                            DashboardAction::PreviewWrist
                        } else {
                            DashboardAction::PreviewHeadset
                        }),
                        Style {
                            flex_grow: 1.0,
                            flex_basis: length(0.0),
                            min_size: Size {
                                width: length(0.0),
                                height: length(52.0),
                            },
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
                            justify_content: Some(JustifyContent::Center),
                            gap: Size {
                                width: length(10.0),
                                height: length(0.0),
                            },
                            ..Default::default()
                        },
                        BtnOpts {
                            active: disp_prev,
                            disabled: !view.enabled || !disp_en,
                            ..Default::default()
                        },
                        &mut region_seq,
                    );
                    ui.add_icon(prev_btn, IconId::Eye, 22.0, 1.9, prev_fg);
                    ui.add_text(
                        prev_btn,
                        if disp_prev {
                            &labels.hide_preview
                        } else {
                            &labels.preview
                        },
                        Style::default(),
                        single_line_style(20.0, 400, prev_fg, Align::Center),
                    );
                }

                let wm_card = ui.add_box(
                    main,
                    Style {
                        flex_direction: FlexDirection::Row,
                        align_items: Some(AlignItems::Center),
                        gap: Size {
                            width: length(12.0),
                            height: length(0.0),
                        },
                        padding: Rect {
                            left: length(20.0),
                            right: length(20.0),
                            top: length(10.0),
                            bottom: length(10.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new()
                        .bg(theme.surface)
                        .border(2.0, theme.border)
                        .radius(20.0),
                );
                ui.add_icon(
                    wm_card,
                    IconId::SlidersHorizontal,
                    24.0,
                    1.9,
                    theme.primary_ink,
                );
                ui.add_text(
                    wm_card,
                    if labels.wrist_menu.is_empty() {
                        "Wrist quick menu"
                    } else {
                        &labels.wrist_menu
                    },
                    Style {
                        flex_grow: 1.0,
                        min_size: Size {
                            width: length(0.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    single_line_style(22.0, 600, theme.text, Align::Left),
                );
                add_toggle_row(
                    ui,
                    wm_card,
                    DashboardAction::ToggleWristMenu,
                    "",
                    view.wrist_menu.enabled,
                    false,
                    None,
                    &mut region_seq,
                );

                if view.ocr_available || view.vision_available {
                    let tools = ui.add_box(
                        main,
                        Style {
                            flex_direction: FlexDirection::Column,
                            padding: Rect {
                                left: length(20.0),
                                right: length(20.0),
                                top: length(16.0),
                                bottom: length(16.0),
                            },
                            ..Default::default()
                        },
                        BoxPaint::new()
                            .bg(theme.surface)
                            .border(2.0, theme.border)
                            .radius(20.0),
                    );
                    let thead = ui.add_box(
                        tools,
                        Style {
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
                            gap: Size {
                                width: length(12.0),
                                height: length(0.0),
                            },
                            margin: Rect {
                                bottom: length(10.0),
                                left: length(0.0),
                                right: length(0.0),
                                top: length(0.0),
                            },
                            ..Default::default()
                        },
                        BoxPaint::new(),
                    );
                    ui.add_icon(thead, IconId::ScanEye, 24.0, 1.9, theme.primary_ink);
                    ui.add_text(
                        thead,
                        &labels.tools,
                        Style::default(),
                        single_line_style(22.0, 600, theme.text, Align::Left),
                    );
                    let tgrid = ui.add_box(
                        tools,
                        Style {
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
                    if view.ocr_available {
                        add_toggle_row(
                            ui,
                            tgrid,
                            DashboardAction::ToggleOcr,
                            &labels.ocr,
                            view.ocr.enabled,
                            !view.ocr.available,
                            (!view.ocr.available).then_some(labels.ocr_unavailable.as_str()),
                            &mut region_seq,
                        );
                        add_toggle_row(
                            ui,
                            tgrid,
                            DashboardAction::ToggleOcrGesture,
                            &labels.gesture,
                            view.ocr.gesture,
                            !view.ocr.available,
                            None,
                            &mut region_seq,
                        );
                    }
                    if view.vision_available {
                        let sub = (!view.vision.enabled && !view.vision.configured)
                            .then_some(labels.vision_unconfigured.as_str());
                        add_toggle_row(
                            ui,
                            tgrid,
                            DashboardAction::ToggleVision,
                            &labels.vision,
                            view.vision.enabled,
                            !view.vision.enabled && !view.vision.configured,
                            sub,
                            &mut region_seq,
                        );
                    }
                    ui.add_box(
                        tgrid,
                        Style {
                            flex_grow: 1.0,
                            ..Default::default()
                        },
                        BoxPaint::new(),
                    );
                    let (bbtn, bfg) = add_btn(
                        ui,
                        tgrid,
                        DashboardCommand::Action(DashboardAction::OpenOcrBindings),
                        Style {
                            size: Size {
                                width: auto(),
                                height: length(52.0),
                            },
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
                            justify_content: Some(JustifyContent::Center),
                            gap: Size {
                                width: length(8.0),
                                height: length(0.0),
                            },
                            padding: Rect {
                                left: length(16.0),
                                right: length(14.0),
                                top: length(8.0),
                                bottom: length(8.0),
                            },
                            ..Default::default()
                        },
                        BtnOpts {
                            disabled: !view.vision_available
                                && (!view.ocr_available || !view.ocr.available),
                            ..Default::default()
                        },
                        &mut region_seq,
                    );
                    ui.add_icon(bbtn, IconId::Settings2, 20.0, 1.9, bfg);
                    ui.add_text(
                        bbtn,
                        &labels.bindings,
                        Style::default(),
                        single_line_style(18.0, 400, bfg, Align::Left),
                    );
                    ui.add_icon(bbtn, IconId::ChevronRight, 20.0, 1.9, bfg);
                }
            }
            DashboardPage::Language => {
                let lhead = ui.add_box(
                    main,
                    Style {
                        display: Display::Grid,
                        grid_template_columns: vec![minmax(length(0.0), fr(1.0_f32)); 3],
                        gap: Size {
                            width: length(14.0),
                            height: length(0.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new(),
                );
                let locked_note = (!labels.recognition_locked.is_empty())
                    .then_some(labels.recognition_locked.as_str());
                let mic_rec_lbl = if labels.microphone_recognition_language.is_empty() {
                    &labels.recognition_language
                } else {
                    &labels.microphone_recognition_language
                };
                let spk_rec_lbl = if labels.speaker_recognition_language.is_empty() {
                    &labels.recognition_language
                } else {
                    &labels.speaker_recognition_language
                };
                for (cmd, icon, lbl, val, disabled, note) in [
                    (
                        DashboardCommand::Recognition(TargetGroup::Microphone),
                        IconId::Mic,
                        mic_rec_lbl,
                        &view.language.microphone_recognition.summary,
                        view.language.microphone_recognition.options.is_empty(),
                        view.language
                            .microphone_recognition
                            .options
                            .is_empty()
                            .then_some(locked_note)
                            .flatten(),
                    ),
                    (
                        DashboardCommand::Recognition(TargetGroup::Speaker),
                        IconId::Volume2,
                        spk_rec_lbl,
                        &view.language.speaker_recognition.summary,
                        view.language.speaker_recognition.options.is_empty(),
                        view.language
                            .speaker_recognition
                            .options
                            .is_empty()
                            .then_some(locked_note)
                            .flatten(),
                    ),
                    (
                        DashboardCommand::Picker(DashboardPicker::Mode),
                        IconId::SlidersHorizontal,
                        &labels.translation_mode,
                        &view.language.mode.value,
                        view.language.mode.options.is_empty(),
                        None,
                    ),
                ] {
                    let (btn, fg) = add_btn(
                        ui,
                        lhead,
                        cmd,
                        Style {
                            min_size: Size {
                                width: length(0.0),
                                height: length(74.0),
                            },
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
                            gap: Size {
                                width: length(12.0),
                                height: length(0.0),
                            },
                            padding: Rect {
                                left: length(16.0),
                                right: length(16.0),
                                top: length(10.0),
                                bottom: length(10.0),
                            },
                            ..Default::default()
                        },
                        BtnOpts {
                            disabled,
                            ..Default::default()
                        },
                        &mut region_seq,
                    );
                    ui.add_icon(btn, icon, 22.0, 1.9, theme.primary_ink);
                    let copy = ui.add_box(
                        btn,
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
                        copy,
                        lbl,
                        Style::default(),
                        single_line_style(15.0, 400, theme.text_secondary, Align::Left),
                    );
                    ui.add_text(
                        copy,
                        val,
                        Style::default(),
                        single_line_style(19.0, 600, fg, Align::Left),
                    );
                    if let Some(n) = note {
                        ui.add_text(
                            copy,
                            n,
                            Style::default(),
                            single_line_style(13.0, 400, theme.text_secondary, Align::Left),
                        );
                    }
                    ui.add_icon(btn, IconId::ChevronRight, 20.0, 1.9, fg);
                }

                let trans_off = view.language.mode.options.iter().any(|o| {
                    o.value == "disabled"
                        && (view.language.mode.value == o.label
                            || view.language.mode.value == o.value)
                });
                if trans_off {
                    let b = ui.add_box(
                        main,
                        Style {
                            min_size: Size {
                                width: length(0.0),
                                height: length(44.0),
                            },
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
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
                        BoxPaint::new()
                            .bg(theme.surface)
                            .border(1.0, theme.border)
                            .radius(14.0),
                    );
                    ui.add_icon(b, IconId::TriangleAlert, 22.0, 1.9, theme.text_secondary);
                    ui.add_text(
                        b,
                        &labels.translation_off_hint,
                        Style {
                            flex_grow: 1.0,
                            min_size: Size {
                                width: length(0.0),
                                height: auto(),
                            },
                            ..Default::default()
                        },
                        single_line_style(18.0, 400, theme.text_secondary, Align::Left),
                    );
                }

                let cols = ui.add_box(
                    main,
                    Style {
                        flex_grow: 1.0,
                        min_size: Size {
                            width: length(0.0),
                            height: length(0.0),
                        },
                        display: Display::Grid,
                        grid_template_columns: vec![minmax(length(0.0), fr(1.0_f32)); 2],
                        gap: Size {
                            width: length(16.0),
                            height: length(0.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new(),
                );
                for (group, icon, heading, targets) in [
                    (
                        TargetGroup::Microphone,
                        IconId::Mic,
                        &labels.microphone_language,
                        &view.language.microphone_targets,
                    ),
                    (
                        TargetGroup::Speaker,
                        IconId::Volume2,
                        &labels.speaker_language,
                        &view.language.speaker_targets,
                    ),
                ] {
                    let card = ui.add_box(
                        cols,
                        Style {
                            flex_direction: FlexDirection::Column,
                            gap: Size {
                                width: length(0.0),
                                height: length(12.0),
                            },
                            padding: Rect {
                                left: length(20.0),
                                right: length(20.0),
                                top: length(18.0),
                                bottom: length(18.0),
                            },
                            ..Default::default()
                        },
                        BoxPaint::new()
                            .bg(theme.surface)
                            .border(2.0, theme.border)
                            .radius(20.0)
                            .tap_opacity(if trans_off { 0.68 } else { 1.0 }),
                    );
                    let chead = ui.add_box(
                        card,
                        Style {
                            min_size: Size {
                                width: length(0.0),
                                height: length(44.0),
                            },
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
                    ui.add_icon(chead, icon, 24.0, 1.9, theme.primary_ink);
                    ui.add_text(
                        chead,
                        heading,
                        Style {
                            flex_grow: 1.0,
                            min_size: Size {
                                width: length(0.0),
                                height: auto(),
                            },
                            ..Default::default()
                        },
                        single_line_style(22.0, 600, theme.text, Align::Left),
                    );
                    let list = ui.add_box(
                        card,
                        Style {
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            gap: Size {
                                width: length(0.0),
                                height: length(12.0),
                            },
                            ..Default::default()
                        },
                        BoxPaint::new(),
                    );
                    for (index, choice) in targets.iter().enumerate() {
                        let trow = ui.add_box(
                            list,
                            Style {
                                flex_direction: FlexDirection::Row,
                                gap: Size {
                                    width: length(8.0),
                                    height: length(0.0),
                                },
                                ..Default::default()
                            },
                            BoxPaint::new(),
                        );
                        let (tbtn, tfg) = add_btn(
                            ui,
                            trow,
                            DashboardCommand::Picker(DashboardPicker::Target { group, index }),
                            Style {
                                flex_grow: 1.0,
                                min_size: Size {
                                    width: length(0.0),
                                    height: length(52.0),
                                },
                                flex_direction: FlexDirection::Row,
                                align_items: Some(AlignItems::Center),
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
                            BtnOpts {
                                disabled: choice.options.is_empty(),
                                ..Default::default()
                            },
                            &mut region_seq,
                        );
                        if index == 0 {
                            let badge = ui.add_box(
                                tbtn,
                                Style {
                                    padding: Rect {
                                        left: length(10.0),
                                        right: length(10.0),
                                        top: length(3.0),
                                        bottom: length(3.0),
                                    },
                                    ..Default::default()
                                },
                                BoxPaint::new().bg(theme.primary_soft).radius(999.0),
                            );
                            ui.add_text(
                                badge,
                                if labels.preferred.is_empty() {
                                    "Primary"
                                } else {
                                    &labels.preferred
                                },
                                Style::default(),
                                single_line_style(15.0, 600, theme.primary_ink, Align::Center),
                            );
                        } else {
                            let badge = ui.add_box(
                                tbtn,
                                Style {
                                    min_size: Size {
                                        width: length(32.0),
                                        height: auto(),
                                    },
                                    padding: Rect {
                                        left: length(10.0),
                                        right: length(10.0),
                                        top: length(3.0),
                                        bottom: length(3.0),
                                    },
                                    ..Default::default()
                                },
                                BoxPaint::new()
                                    .bg(theme.surface_subtle)
                                    .border(1.0, theme.border)
                                    .radius(999.0),
                            );
                            ui.add_text(
                                badge,
                                (index + 1).to_string(),
                                Style::default(),
                                single_line_style(15.0, 600, theme.text_secondary, Align::Center),
                            );
                        }
                        ui.add_text(
                            tbtn,
                            &choice.value,
                            Style {
                                flex_grow: 1.0,
                                min_size: Size {
                                    width: length(0.0),
                                    height: auto(),
                                },
                                ..Default::default()
                            },
                            single_line_style(20.0, 600, tfg, Align::Right),
                        );
                        ui.add_icon(tbtn, IconId::ChevronRight, 22.0, 1.9, tfg);

                        if targets.len() > 1 {
                            let (ub, ufg) = add_btn(
                                ui,
                                trow,
                                DashboardCommand::Action(DashboardAction::MoveTarget {
                                    group,
                                    index,
                                    offset: -1,
                                }),
                                Style {
                                    size: Size {
                                        width: length(52.0),
                                        height: length(52.0),
                                    },
                                    align_items: Some(AlignItems::Center),
                                    justify_content: Some(JustifyContent::Center),
                                    ..Default::default()
                                },
                                BtnOpts {
                                    icon: true,
                                    disabled: index == 0,
                                    ..Default::default()
                                },
                                &mut region_seq,
                            );
                            ui.add_icon(ub, IconId::ArrowUp, 22.0, 1.9, ufg);
                            let (db, dfg) = add_btn(
                                ui,
                                trow,
                                DashboardCommand::Action(DashboardAction::MoveTarget {
                                    group,
                                    index,
                                    offset: 1,
                                }),
                                Style {
                                    size: Size {
                                        width: length(52.0),
                                        height: length(52.0),
                                    },
                                    align_items: Some(AlignItems::Center),
                                    justify_content: Some(JustifyContent::Center),
                                    ..Default::default()
                                },
                                BtnOpts {
                                    icon: true,
                                    disabled: index + 1 >= targets.len(),
                                    ..Default::default()
                                },
                                &mut region_seq,
                            );
                            ui.add_icon(db, IconId::ArrowDown, 22.0, 1.9, dfg);
                            let (delb, delfg) = add_btn(
                                ui,
                                trow,
                                DashboardCommand::Action(DashboardAction::DeleteTarget {
                                    group,
                                    index,
                                }),
                                Style {
                                    size: Size {
                                        width: length(52.0),
                                        height: length(52.0),
                                    },
                                    align_items: Some(AlignItems::Center),
                                    justify_content: Some(JustifyContent::Center),
                                    ..Default::default()
                                },
                                BtnOpts {
                                    icon: true,
                                    danger: true,
                                    ..Default::default()
                                },
                                &mut region_seq,
                            );
                            ui.add_icon(delb, IconId::Trash2, 22.0, 1.9, delfg);
                        }
                    }
                    let (abtn, afg) = add_btn(
                        ui,
                        card,
                        DashboardCommand::Action(DashboardAction::AddTarget(group)),
                        Style {
                            min_size: Size {
                                width: length(0.0),
                                height: length(52.0),
                            },
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
                            justify_content: Some(JustifyContent::Center),
                            gap: Size {
                                width: length(10.0),
                                height: length(0.0),
                            },
                            ..Default::default()
                        },
                        BtnOpts {
                            disabled: !view.language.can_add || targets.len() >= 3,
                            ..Default::default()
                        },
                        &mut region_seq,
                    );
                    ui.add_icon(abtn, IconId::Plus, 22.0, 1.9, afg);
                    ui.add_text(
                        abtn,
                        &labels.add_target,
                        Style::default(),
                        single_line_style(20.0, 400, afg, Align::Center),
                    );
                }

                // Presets bar.
                let presets = ui.add_box(
                    main,
                    Style {
                        flex_direction: FlexDirection::Row,
                        align_items: Some(AlignItems::Center),
                        gap: Size {
                            width: length(12.0),
                            height: length(0.0),
                        },
                        padding: Rect {
                            left: length(20.0),
                            right: length(20.0),
                            top: length(14.0),
                            bottom: length(14.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new()
                        .bg(theme.surface)
                        .border(2.0, theme.border)
                        .radius(20.0),
                );
                ui.add_icon(presets, IconId::Bookmark, 24.0, 1.9, theme.primary_ink);
                ui.add_text(
                    presets,
                    &labels.presets,
                    Style::default(),
                    single_line_style(22.0, 600, theme.text, Align::Left),
                );
                let pill = ui.add_box(
                    presets,
                    Style {
                        padding: Rect {
                            left: length(12.0),
                            right: length(12.0),
                            top: length(4.0),
                            bottom: length(4.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new()
                        .bg(theme.surface_subtle)
                        .border(1.0, theme.border)
                        .radius(999.0),
                );
                ui.add_text(
                    pill,
                    format!("{} / 5", view.language.presets.options.len()),
                    Style::default(),
                    single_line_style(15.0, 600, theme.text_secondary, Align::Center),
                );
                ui.add_box(
                    presets,
                    Style {
                        flex_grow: 1.0,
                        ..Default::default()
                    },
                    BoxPaint::new(),
                );
                if view.language.presets.options.is_empty() {
                    ui.add_text(
                        presets,
                        &labels.no_presets,
                        Style::default(),
                        single_line_style(18.0, 400, theme.text_secondary, Align::Left),
                    );
                } else {
                    let (abtn, afg) = add_btn(
                        ui,
                        presets,
                        DashboardCommand::Picker(DashboardPicker::ApplyPreset),
                        Style {
                            size: Size {
                                width: auto(),
                                height: length(52.0),
                            },
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
                            gap: Size {
                                width: length(10.0),
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
                        BtnOpts::default(),
                        &mut region_seq,
                    );
                    ui.add_icon(abtn, IconId::Check, 22.0, 1.9, afg);
                    ui.add_text(
                        abtn,
                        &labels.apply_preset,
                        Style::default(),
                        single_line_style(20.0, 400, afg, Align::Center),
                    );
                }
                let (sbtn, sfg) = add_btn(
                    ui,
                    presets,
                    DashboardCommand::Action(DashboardAction::SaveLanguagePreset(String::new())),
                    Style {
                        size: Size {
                            width: auto(),
                            height: length(52.0),
                        },
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Row,
                        align_items: Some(AlignItems::Center),
                        gap: Size {
                            width: length(10.0),
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
                    BtnOpts {
                        disabled: !view.language.can_save_preset,
                        ..Default::default()
                    },
                    &mut region_seq,
                );
                ui.add_icon(sbtn, IconId::Plus, 22.0, 1.9, sfg);
                ui.add_text(
                    sbtn,
                    &labels.save_preset,
                    Style::default(),
                    single_line_style(20.0, 400, sfg, Align::Center),
                );
                if !view.language.presets.options.is_empty() {
                    let (dbtn, dfg) = add_btn(
                        ui,
                        presets,
                        DashboardCommand::Picker(DashboardPicker::DeletePreset),
                        Style {
                            size: Size {
                                width: auto(),
                                height: length(52.0),
                            },
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
                            gap: Size {
                                width: length(10.0),
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
                        BtnOpts {
                            danger: true,
                            ..Default::default()
                        },
                        &mut region_seq,
                    );
                    ui.add_icon(dbtn, IconId::Trash2, 22.0, 1.9, dfg);
                    ui.add_text(
                        dbtn,
                        &labels.delete,
                        Style::default(),
                        single_line_style(20.0, 400, dfg, Align::Center),
                    );
                }
            }
            DashboardPage::Osc => {
                let card = ui.add_box(
                    main,
                    Style {
                        flex_direction: FlexDirection::Column,
                        gap: Size {
                            width: length(0.0),
                            height: length(10.0),
                        },
                        padding: Rect {
                            left: length(22.0),
                            right: length(22.0),
                            top: length(14.0),
                            bottom: length(14.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new()
                        .bg(theme.surface)
                        .border(2.0, theme.border)
                        .radius(20.0),
                );
                let chead = ui.add_box(
                    card,
                    Style {
                        min_size: Size {
                            width: length(0.0),
                            height: length(44.0),
                        },
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
                ui.add_icon(chead, IconId::Radio, 24.0, 1.9, theme.primary_ink);
                ui.add_text(
                    chead,
                    if labels.osc_title.is_empty() {
                        &labels.osc_tab
                    } else {
                        &labels.osc_title
                    },
                    Style {
                        flex_grow: 1.0,
                        min_size: Size {
                            width: length(0.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    single_line_style(22.0, 600, theme.text, Align::Left),
                );
                let ep = ui.add_box(
                    chead,
                    Style {
                        padding: Rect {
                            left: length(12.0),
                            right: length(12.0),
                            top: length(4.0),
                            bottom: length(4.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new()
                        .bg(theme.surface_subtle)
                        .border(1.0, theme.border)
                        .radius(999.0),
                );
                ui.add_text(
                    ep,
                    &view.osc.endpoint,
                    Style::default(),
                    crate::vr_overlay::gpu_ui::text::TextStyle {
                        mono: true,
                        ..single_line_style(15.0, 600, theme.text_secondary, Align::Center)
                    },
                );
                add_toggle_row(
                    ui,
                    chead,
                    DashboardAction::ToggleOsc,
                    "",
                    view.osc.enabled,
                    false,
                    None,
                    &mut region_seq,
                );

                let obody = ui.add_box(
                    card,
                    Style {
                        flex_direction: FlexDirection::Column,
                        gap: Size {
                            width: length(0.0),
                            height: length(10.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new().tap_opacity(if view.osc.enabled { 1.0 } else { 0.68 }),
                );
                let otoggles = ui.add_box(
                    obody,
                    Style {
                        display: Display::Grid,
                        grid_template_columns: vec![minmax(length(0.0), fr(1.0_f32)); 2],
                        gap: Size {
                            width: length(14.0),
                            height: length(0.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new(),
                );
                add_toggle_row(
                    ui,
                    otoggles,
                    DashboardAction::ToggleOscOriginal,
                    &labels.osc_original,
                    view.osc.original,
                    false,
                    None,
                    &mut region_seq,
                );
                add_toggle_row(
                    ui,
                    otoggles,
                    DashboardAction::ToggleOscMuteSync,
                    &labels.osc_mute_sync,
                    view.osc.mute_sync,
                    false,
                    None,
                    &mut region_seq,
                );
                ui.add_text(
                    obody,
                    &labels.osc_strategy,
                    Style::default(),
                    single_line_style(18.0, 600, theme.text, Align::Left),
                );
                add_segments(
                    ui,
                    obody,
                    &view.osc.strategy,
                    &|v| DashboardAction::SetOscStrategy(v.to_owned()),
                    false,
                    Style::default(),
                    theme.surface_subtle,
                    &mut region_seq,
                );
                let hint = ui.add_box(
                    obody,
                    Style {
                        padding: Rect {
                            left: length(14.0),
                            right: length(14.0),
                            top: length(8.0),
                            bottom: length(8.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new()
                        .bg(theme.surface_subtle)
                        .border(1.0, theme.border)
                        .radius(14.0),
                );
                ui.add_text(
                    hint,
                    &labels.osc_hint,
                    Style {
                        size: Size {
                            width: percent(1.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    text_style(16.0, 400, theme.text_secondary),
                );

                // UniText card.
                let ucard = ui.add_box(
                    main,
                    Style {
                        flex_direction: FlexDirection::Column,
                        gap: Size {
                            width: length(0.0),
                            height: length(10.0),
                        },
                        padding: Rect {
                            left: length(22.0),
                            right: length(22.0),
                            top: length(14.0),
                            bottom: length(14.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new()
                        .bg(theme.surface)
                        .border(2.0, theme.border)
                        .radius(20.0),
                );
                let uhead = ui.add_box(
                    ucard,
                    Style {
                        min_size: Size {
                            width: length(0.0),
                            height: length(44.0),
                        },
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
                ui.add_icon(uhead, IconId::Monitor, 24.0, 1.9, theme.primary_ink);
                ui.add_text(
                    uhead,
                    if labels.unitext_title.is_empty() {
                        "UniText"
                    } else {
                        &labels.unitext_title
                    },
                    Style {
                        flex_grow: 1.0,
                        min_size: Size {
                            width: length(0.0),
                            height: auto(),
                        },
                        ..Default::default()
                    },
                    single_line_style(22.0, 600, theme.text, Align::Left),
                );
                if !view.unitext.endpoint.is_empty() {
                    let uep = ui.add_box(
                        uhead,
                        Style {
                            padding: Rect {
                                left: length(12.0),
                                right: length(12.0),
                                top: length(4.0),
                                bottom: length(4.0),
                            },
                            ..Default::default()
                        },
                        BoxPaint::new()
                            .bg(theme.surface_subtle)
                            .border(1.0, theme.border)
                            .radius(999.0),
                    );
                    ui.add_text(
                        uep,
                        &view.unitext.endpoint,
                        Style::default(),
                        crate::vr_overlay::gpu_ui::text::TextStyle {
                            mono: true,
                            ..single_line_style(15.0, 600, theme.text_secondary, Align::Center)
                        },
                    );
                }
                add_toggle_row(
                    ui,
                    uhead,
                    DashboardAction::ToggleUnitext,
                    "",
                    view.unitext.enabled,
                    !view.unitext.enabled && view.unitext.port_conflict,
                    None,
                    &mut region_seq,
                );

                if view.unitext.port_conflict && !labels.unitext_port_conflict.is_empty() {
                    let conflict = ui.add_box(
                        ucard,
                        Style {
                            min_size: Size {
                                width: length(0.0),
                                height: length(40.0),
                            },
                            flex_direction: FlexDirection::Row,
                            align_items: Some(AlignItems::Center),
                            gap: Size {
                                width: length(10.0),
                                height: length(0.0),
                            },
                            padding: Rect {
                                left: length(14.0),
                                right: length(14.0),
                                top: length(6.0),
                                bottom: length(6.0),
                            },
                            ..Default::default()
                        },
                        BoxPaint::new()
                            .bg(theme.error_soft)
                            .border(1.0, theme.error_border)
                            .radius(14.0),
                    );
                    ui.add_icon(conflict, IconId::TriangleAlert, 20.0, 1.9, theme.error);
                    ui.add_text(
                        conflict,
                        &labels.unitext_port_conflict,
                        Style {
                            flex_grow: 1.0,
                            min_size: Size {
                                width: length(0.0),
                                height: auto(),
                            },
                            ..Default::default()
                        },
                        single_line_style(16.0, 400, theme.error, Align::Left),
                    );
                }

                let ubody = ui.add_box(
                    ucard,
                    Style {
                        flex_direction: FlexDirection::Column,
                        ..Default::default()
                    },
                    BoxPaint::new().tap_opacity(if view.unitext.enabled { 1.0 } else { 0.68 }),
                );
                let utoggles = ui.add_box(
                    ubody,
                    Style {
                        display: Display::Grid,
                        grid_template_columns: vec![minmax(length(0.0), fr(1.0_f32)); 2],
                        gap: Size {
                            width: length(14.0),
                            height: length(10.0),
                        },
                        ..Default::default()
                    },
                    BoxPaint::new(),
                );
                add_toggle_row(
                    ui,
                    utoggles,
                    DashboardAction::ToggleUnitextIncludeMicrophone,
                    if labels.unitext_include_microphone.is_empty() {
                        "Send my microphone subtitles"
                    } else {
                        &labels.unitext_include_microphone
                    },
                    view.unitext.include_microphone,
                    false,
                    None,
                    &mut region_seq,
                );
                add_toggle_row(
                    ui,
                    utoggles,
                    DashboardAction::ToggleUnitextMuteSync,
                    if labels.unitext_mute_sync.is_empty() {
                        "Sync VRChat microphone mute"
                    } else {
                        &labels.unitext_mute_sync
                    },
                    view.unitext.mute_sync,
                    false,
                    None,
                    &mut region_seq,
                );
                add_toggle_row(
                    ui,
                    utoggles,
                    DashboardAction::ToggleUnitextIncludeSpeaker,
                    if labels.unitext_include_speaker.is_empty() {
                        "Share others' subtitles"
                    } else {
                        &labels.unitext_include_speaker
                    },
                    view.unitext.include_speaker,
                    false,
                    None,
                    &mut region_seq,
                );
                add_toggle_row(
                    ui,
                    utoggles,
                    DashboardAction::ToggleUnitextPreserveOriginal,
                    if labels.unitext_preserve_original.is_empty() {
                        "Keep original text in output"
                    } else {
                        &labels.unitext_preserve_original
                    },
                    view.unitext.preserve_original,
                    false,
                    None,
                    &mut region_seq,
                );
            }
        }
    }

    ui.compute_layout();
    ui.emit(SurfaceId::Dashboard, state.key, 1)
}

trait BoxPaintExt {
    fn tap_opacity(self, o: f32) -> Self;
}

impl BoxPaintExt for BoxPaint {
    fn tap_opacity(mut self, o: f32) -> Self {
        self.opacity = o;
        self
    }
}
