//! Taffy-based layout builder emitting `(SurfaceLayout, DrawList)` in one pass.

use std::sync::Arc;

use taffy::Overflow;
use taffy::prelude::*;

pub(crate) fn length<T: taffy::style_helpers::FromLength>(v: f32) -> T {
    taffy::prelude::length(v)
}

pub(crate) fn percent<T: taffy::style_helpers::FromPercent>(v: f32) -> T {
    taffy::prelude::percent(v)
}

use super::draw::{DrawList, Instance};
use super::icons::IconId;
#[cfg(windows)]
use super::text::TextSpan;
use super::text::{ShapedText, TextEngine, TextStyle};
use super::theme::{Color, Theme};
use crate::vr_overlay::dashboard::DashboardCommand;
use crate::vr_overlay::surface_protocol::{Panel, Region, SurfaceId, SurfaceKey, SurfaceLayout};
use crate::vr_overlay::wrist_menu;

#[derive(Debug, Clone, Default)]
pub struct BoxPaint {
    pub bg: Option<Color>,
    pub border_width: f32,
    pub border_color: Color,
    pub border_flags: u32,
    pub radii: [f32; 4],
    pub outline_width: f32,
    pub outline_offset: f32,
    pub outline_color: Color,
    pub outline_flags: u32,
    pub shadow_color: Option<Color>,
    pub shadow_expand: f32,
    pub opacity: f32,
    pub scale: f32,
}

impl BoxPaint {
    pub fn new() -> Self {
        Self {
            opacity: 1.0,
            scale: 1.0,
            ..Default::default()
        }
    }

    pub fn radius(mut self, r: f32) -> Self {
        self.radii = [r; 4];
        self
    }

    pub fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    pub fn border(mut self, width: f32, color: Color) -> Self {
        self.border_width = width;
        self.border_color = color;
        self
    }

    pub fn outline(mut self, width: f32, offset: f32, color: Color, flags: u32) -> Self {
        self.outline_width = width;
        self.outline_offset = offset;
        self.outline_color = color;
        self.outline_flags = flags;
        self
    }
}

#[derive(Clone)]
pub enum RegionPayload {
    Simple(u8, bool),
    Dashboard(u8, DashboardCommand),
    Menu(u8, bool, wrist_menu::Command),
}

enum NodeContent {
    None,
    Text {
        text: String,
        style: TextStyle,
        shaped: Option<Arc<ShapedText>>,
    },
    #[cfg(windows)]
    RichText {
        spans: Vec<(String, TextStyle)>,
        base: TextStyle,
        shaped: Option<ShapedText>,
    },
    Icon {
        id: IconId,
        size: f32,
        stroke: f32,
        color: Color,
    },
    Logo {
        size: f32,
    },
    #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
    Thumbnail,
    Arc {
        start_rad: f32,
        sweep_rad: f32,
        stroke: f32,
        color: Color,
    },
    Dot {
        color: Color,
    },
    ScrollViewport {
        scroll_y: f32,
        thumb_color: Color,
    },
}

pub(crate) struct NodeMeta {
    pub(crate) paint: BoxPaint,
    content: NodeContent,
    panel: bool,
    menu_capsule: bool,
    region: Option<RegionPayload>,
    clip_children: bool,
    tooltip: Option<(String, bool)>,
}

impl Default for NodeMeta {
    fn default() -> Self {
        Self {
            paint: BoxPaint::new(),
            content: NodeContent::None,
            panel: false,
            menu_capsule: false,
            region: None,
            clip_children: false,
            tooltip: None,
        }
    }
}

pub struct ScrollMeasurement {
    pub client_height: f32,
    pub scroll_height: f32,
}

pub struct UiBuilder<'a> {
    pub tree: TaffyTree<NodeMeta>,
    pub text: &'a mut TextEngine,
    pub theme: Theme,
    pub width: u32,
    pub height: u32,
    root: NodeId,
    viewport_node: Option<NodeId>,
    content_node: Option<NodeId>,
}

impl<'a> UiBuilder<'a> {
    pub fn new(text: &'a mut TextEngine, theme: Theme, width: u32, height: u32) -> Self {
        let mut tree = TaffyTree::new();
        let root = tree
            .new_leaf_with_context(
                Style {
                    size: Size {
                        width: length(width as f32),
                        height: length(height as f32),
                    },
                    flex_direction: FlexDirection::Column,
                    ..Default::default()
                },
                NodeMeta::default(),
            )
            .expect("taffy root");
        Self {
            tree,
            text,
            theme,
            width,
            height,
            root,
            viewport_node: None,
            content_node: None,
        }
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn set_root_style(&mut self, style: Style) {
        self.tree
            .set_style(self.root, style)
            .expect("set root style");
    }

    pub fn add_box(&mut self, parent: NodeId, style: Style, paint: BoxPaint) -> NodeId {
        let id = self
            .tree
            .new_leaf_with_context(
                style,
                NodeMeta {
                    paint,
                    ..Default::default()
                },
            )
            .expect("taffy box");
        self.tree.add_child(parent, id).expect("add child");
        id
    }

    pub fn mark_panel(&mut self, node: NodeId) {
        if let Some(meta) = self.tree.get_node_context_mut(node) {
            meta.panel = true;
        }
    }

    pub fn mark_menu_capsule(&mut self, node: NodeId) {
        if let Some(meta) = self.tree.get_node_context_mut(node) {
            meta.menu_capsule = true;
        }
    }

    pub fn mark_clip(&mut self, node: NodeId) {
        if let Some(meta) = self.tree.get_node_context_mut(node) {
            meta.clip_children = true;
        }
    }

    pub fn set_region(&mut self, node: NodeId, payload: RegionPayload) {
        if let Some(meta) = self.tree.get_node_context_mut(node) {
            meta.region = Some(payload);
        }
    }

    pub fn set_tooltip(&mut self, node: NodeId, text: impl Into<String>, below: bool) {
        if let Some(meta) = self.tree.get_node_context_mut(node) {
            meta.tooltip = Some((text.into(), below));
        }
    }

    pub fn add_scroll_viewport(
        &mut self,
        parent: NodeId,
        style: Style,
        scroll_y: f32,
    ) -> (NodeId, NodeId) {
        let viewport = self
            .tree
            .new_leaf_with_context(
                Style {
                    overflow: taffy::Point {
                        x: Overflow::Hidden,
                        y: Overflow::Scroll,
                    },
                    flex_direction: FlexDirection::Column,
                    ..style
                },
                NodeMeta {
                    content: NodeContent::ScrollViewport {
                        scroll_y,
                        thumb_color: self.theme.border_strong.with_alpha(0.75),
                    },
                    clip_children: true,
                    ..Default::default()
                },
            )
            .expect("taffy viewport");
        self.tree.add_child(parent, viewport).expect("add viewport");
        let content = self.add_box(
            viewport,
            Style {
                flex_direction: FlexDirection::Column,
                size: Size {
                    width: percent(1.0),
                    height: auto(),
                },
                flex_shrink: 0.0,
                ..Default::default()
            },
            BoxPaint::new(),
        );
        self.viewport_node = Some(viewport);
        self.content_node = Some(content);
        (viewport, content)
    }

    pub fn add_text(
        &mut self,
        parent: NodeId,
        text: impl Into<String>,
        style: Style,
        text_style: TextStyle,
    ) -> NodeId {
        let id = self
            .tree
            .new_leaf_with_context(
                style,
                NodeMeta {
                    content: NodeContent::Text {
                        text: text.into(),
                        style: text_style,
                        shaped: None,
                    },
                    ..Default::default()
                },
            )
            .expect("taffy text");
        self.tree.add_child(parent, id).expect("add text");
        id
    }

    #[cfg(windows)]
    pub fn add_rich_text(
        &mut self,
        parent: NodeId,
        spans: Vec<(String, TextStyle)>,
        style: Style,
        base: TextStyle,
    ) -> NodeId {
        let id = self
            .tree
            .new_leaf_with_context(
                style,
                NodeMeta {
                    content: NodeContent::RichText {
                        spans,
                        base,
                        shaped: None,
                    },
                    ..Default::default()
                },
            )
            .expect("taffy rich text");
        self.tree.add_child(parent, id).expect("add rich text");
        id
    }

    pub fn add_icon(
        &mut self,
        parent: NodeId,
        id: IconId,
        size_px: f32,
        stroke: f32,
        color: Color,
    ) -> NodeId {
        let node = self
            .tree
            .new_leaf_with_context(
                Style {
                    size: Size {
                        width: length(size_px),
                        height: length(size_px),
                    },
                    flex_shrink: 0.0,
                    ..Default::default()
                },
                NodeMeta {
                    content: NodeContent::Icon {
                        id,
                        size: size_px,
                        stroke,
                        color,
                    },
                    ..Default::default()
                },
            )
            .expect("taffy icon");
        self.tree.add_child(parent, node).expect("add icon");
        node
    }

    pub fn add_logo(&mut self, parent: NodeId, size_px: f32) -> NodeId {
        let node = self
            .tree
            .new_leaf_with_context(
                Style {
                    size: Size {
                        width: length(size_px),
                        height: length(size_px),
                    },
                    flex_shrink: 0.0,
                    ..Default::default()
                },
                NodeMeta {
                    content: NodeContent::Logo { size: size_px },
                    ..Default::default()
                },
            )
            .expect("taffy logo");
        self.tree.add_child(parent, node).expect("add logo");
        node
    }

    #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
    pub fn add_thumbnail(&mut self, parent: NodeId, size_px: f32) -> NodeId {
        let node = self
            .tree
            .new_leaf_with_context(
                Style {
                    size: Size {
                        width: length(size_px),
                        height: length(size_px),
                    },
                    flex_shrink: 0.0,
                    ..Default::default()
                },
                NodeMeta {
                    content: NodeContent::Thumbnail,
                    ..Default::default()
                },
            )
            .expect("taffy thumb");
        self.tree.add_child(parent, node).expect("add thumb");
        node
    }

    pub fn add_arc(
        &mut self,
        parent: NodeId,
        size_px: f32,
        start_rad: f32,
        sweep_rad: f32,
        stroke: f32,
        color: Color,
    ) -> NodeId {
        let node = self
            .tree
            .new_leaf_with_context(
                Style {
                    position: Position::Absolute,
                    inset: Rect {
                        left: length(0.0),
                        top: length(0.0),
                        right: auto(),
                        bottom: auto(),
                    },
                    size: Size {
                        width: length(size_px),
                        height: length(size_px),
                    },
                    ..Default::default()
                },
                NodeMeta {
                    content: NodeContent::Arc {
                        start_rad,
                        sweep_rad,
                        stroke,
                        color,
                    },
                    ..Default::default()
                },
            )
            .expect("taffy arc");
        self.tree.add_child(parent, node).expect("add arc");
        node
    }

    pub fn add_dot(&mut self, parent: NodeId, style: Style, color: Color) -> NodeId {
        let node = self
            .tree
            .new_leaf_with_context(
                style,
                NodeMeta {
                    content: NodeContent::Dot { color },
                    ..Default::default()
                },
            )
            .expect("taffy dot");
        self.tree.add_child(parent, node).expect("add dot");
        node
    }

    pub fn compute_layout(&mut self) {
        let text = &mut self.text;
        let root = self.root;
        let avail = Size {
            width: AvailableSpace::Definite(self.width as f32),
            height: AvailableSpace::Definite(self.height as f32),
        };
        self.tree
            .compute_layout_with_measure(root, avail, |known, available, _node_id, ctx, _style| {
                let Some(meta) = ctx else {
                    return Size::ZERO;
                };
                let max_w = known.width.or(match available.width {
                    AvailableSpace::Definite(w) => Some(w),
                    AvailableSpace::MinContent => Some(1.0),
                    AvailableSpace::MaxContent => None,
                });
                match &mut meta.content {
                    NodeContent::Text {
                        text: raw,
                        style,
                        shaped,
                    } => {
                        let measure_style = TextStyle {
                            align: cosmic_text::Align::Left,
                            ..*style
                        };
                        let s = text.shape_text_cached(raw, max_w, measure_style);
                        let w = known.width.unwrap_or(s.width);
                        let h = known.height.unwrap_or(s.height);
                        *shaped = Some(s);
                        Size {
                            width: w,
                            height: h,
                        }
                    }
                    #[cfg(windows)]
                    NodeContent::RichText {
                        spans,
                        base,
                        shaped,
                    } => {
                        let borrowed: Vec<TextSpan<'_>> = spans
                            .iter()
                            .map(|(t, st)| TextSpan {
                                text: t.as_str(),
                                style: *st,
                            })
                            .collect();
                        let measure_base = TextStyle {
                            align: cosmic_text::Align::Left,
                            ..*base
                        };
                        let s = text.shape_spans(&borrowed, max_w, measure_base);
                        let w = known.width.unwrap_or(s.width);
                        let h = known.height.unwrap_or(s.height);
                        *shaped = Some(s);
                        Size {
                            width: w,
                            height: h,
                        }
                    }
                    _ => Size::ZERO,
                }
            })
            .expect("taffy compute layout");
    }

    pub fn scroll_measurement(&self) -> Option<ScrollMeasurement> {
        let vp = self.viewport_node?;
        let ct = self.content_node?;
        let vp_layout = self.tree.layout(vp).ok()?;
        let ct_layout = self.tree.layout(ct).ok()?;
        Some(ScrollMeasurement {
            client_height: vp_layout.size.height,
            scroll_height: ct_layout.size.height,
        })
    }

    pub fn set_scroll_y(&mut self, scroll_y: f32) {
        if let Some(vp) = self.viewport_node
            && let Some(meta) = self.tree.get_node_context_mut(vp)
            && let NodeContent::ScrollViewport { scroll_y: slot, .. } = &mut meta.content
        {
            *slot = scroll_y;
        }
    }

    pub fn emit(
        &mut self,
        surface: SurfaceId,
        key: SurfaceKey,
        page_count: usize,
    ) -> (SurfaceLayout, DrawList) {
        let mut draw = DrawList::new(self.width, self.height);
        let mut regions = Vec::new();
        let mut panels = Vec::new();
        let mut menu_bounds = None;
        let mut tooltips: Vec<([f32; 4], String, bool)> = Vec::new();

        let root_clip = [0.0, 0.0, self.width as f32, self.height as f32];
        {
            let mut ctx = EmitCtx {
                surface,
                w: self.width as f32,
                h: self.height as f32,
                draw: &mut draw,
                regions: &mut regions,
                panels: &mut panels,
                menu_bounds: &mut menu_bounds,
                tooltips: &mut tooltips,
                text: self.text,
            };
            emit_node(
                &self.tree, self.root, 0.0, 0.0, root_clip, 0.0, 1.0, &mut ctx,
            );
        }

        // Emit deferred tooltips in top layer so they never get clipped by capsules.
        for (anchor, label, below) in tooltips {
            let max_w: f32 = match surface {
                SurfaceId::Menu => 230.0,
                SurfaceId::Reader => 220.0,
                _ => 350.0,
            };
            let font_size = if surface == SurfaceId::Menu {
                20.0
            } else {
                22.0
            };
            let pad_x = 14.0;
            let pad_y = 8.0;
            let max_inner_w = (max_w - pad_x * 2.0).max(1.0);
            let mut tip_style = TextStyle {
                font_size,
                line_height: (font_size * 1.4).round(),
                color: self.theme.text,
                align: cosmic_text::Align::Left,
                ..Default::default()
            };
            let measured = self
                .text
                .shape_text_cached(&label, Some(max_inner_w), tip_style);
            let inner_w = measured.width.min(max_inner_w).max(1.0);
            let shaped = if measured.line_count > 1 {
                tip_style.align = cosmic_text::Align::Center;
                self.text
                    .shape_text_cached(&label, Some(inner_w), tip_style)
            } else {
                measured
            };
            let box_w = inner_w + pad_x * 2.0;
            let box_h = shaped.height + pad_y * 2.0;
            let cx = anchor[0] + anchor[2] * 0.5;
            let bx = (cx - box_w * 0.5).clamp(4.0, (self.width as f32 - box_w - 4.0).max(4.0));
            let gap = if surface == SurfaceId::Menu {
                6.0
            } else {
                12.0
            };
            let by = if below {
                (anchor[1] + anchor[3] + gap).min(self.height as f32 - box_h - 4.0)
            } else {
                (anchor[1] - box_h - gap).max(4.0)
            };
            draw.instances.push(Instance::rect(
                [bx, by, box_w, box_h],
                [12.0; 4],
                self.theme.surface,
                1.0,
                self.theme.border,
                root_clip,
                0.0,
            ));
            shaped.emit_colored(
                &mut draw,
                bx + pad_x,
                by + pad_y,
                self.theme.text,
                root_clip,
                0.0,
            );
        }

        let layout = SurfaceLayout {
            key,
            regions,
            menu_bounds,
            panels,
            page_count: page_count.max(1),
            unplaced: Vec::new(),
            capture_bounds: None,
        };
        (layout, draw)
    }
}

struct EmitCtx<'a> {
    surface: SurfaceId,
    w: f32,
    h: f32,
    draw: &'a mut DrawList,
    regions: &'a mut Vec<Region>,
    panels: &'a mut Vec<Panel>,
    menu_bounds: &'a mut Option<[f32; 4]>,
    tooltips: &'a mut Vec<([f32; 4], String, bool)>,
    text: &'a mut TextEngine,
}

#[allow(clippy::too_many_arguments)]
fn emit_node(
    tree: &TaffyTree<NodeMeta>,
    node: NodeId,
    parent_x: f32,
    parent_y: f32,
    clip_rect: [f32; 4],
    clip_radius: f32,
    inherited_opacity: f32,
    ctx: &mut EmitCtx<'_>,
) {
    let Ok(layout) = tree.layout(node) else {
        return;
    };
    let x = parent_x + layout.location.x;
    let y = parent_y + layout.location.y;
    let w = layout.size.width;
    let h = layout.size.height;
    let Some(meta) = tree.get_node_context(node) else {
        return;
    };
    let alpha = inherited_opacity * meta.paint.opacity;
    let scale = meta.paint.scale;
    let (draw_x, draw_y, draw_w, draw_h) = if (scale - 1.0).abs() > 1e-4 {
        let sw = w * scale;
        let sh = h * scale;
        (x + (w - sw) * 0.5, y + (h - sh) * 0.5, sw, sh)
    } else {
        (x, y, w, h)
    };

    let norm = [x / ctx.w, y / ctx.h, (x + w) / ctx.w, (y + h) / ctx.h];
    let clamped_norm = [
        norm[0].clamp(0.0, 1.0),
        norm[1].clamp(0.0, 1.0),
        norm[2].clamp(0.0, 1.0),
        norm[3].clamp(0.0, 1.0),
    ];

    if meta.menu_capsule && w > 0.0 && h > 0.0 {
        *ctx.menu_bounds = Some(clamped_norm);
    }
    if meta.panel && w > 0.0 && h > 0.0 {
        let r = meta.paint.radii[0].min(w * 0.5).min(h * 0.5);
        let max_rx = (clamped_norm[2] - clamped_norm[0]) * 0.5;
        let max_ry = (clamped_norm[3] - clamped_norm[1]) * 0.5;
        ctx.panels.push(Panel {
            bounds: clamped_norm,
            radius: [(r / ctx.w).min(max_rx), (r / ctx.h).min(max_ry)],
        });
    }
    if let Some(payload) = &meta.region
        && w > 0.0
        && h > 0.0
        && clamped_norm[0] < clamped_norm[2]
        && clamped_norm[1] < clamped_norm[3]
    {
        let region = match payload {
            RegionPayload::Simple(id, circle) => Region {
                id: *id,
                bounds: clamped_norm,
                circle: *circle,
                dashboard: None,
                menu: None,
            },
            RegionPayload::Dashboard(id, cmd) => Region {
                id: *id,
                bounds: clamped_norm,
                circle: false,
                dashboard: Some(cmd.clone()),
                menu: None,
            },
            RegionPayload::Menu(id, circle, cmd) => Region {
                id: *id,
                bounds: clamped_norm,
                circle: *circle,
                dashboard: None,
                menu: Some(cmd.clone()),
            },
        };
        ctx.regions.push(region);
    }
    if let Some((label, below)) = &meta.tooltip {
        ctx.tooltips.push(([x, y, w, h], label.clone(), *below));
    }

    if let Some(shadow) = meta.paint.shadow_color
        && w > 0.0
        && h > 0.0
    {
        let exp = meta.paint.shadow_expand;
        ctx.draw.instances.push(Instance::rect(
            [
                draw_x - exp,
                draw_y - exp,
                draw_w + exp * 2.0,
                draw_h + exp * 2.0,
            ],
            meta.paint
                .radii
                .map(|r| if r > 0.0 { r + exp } else { 0.0 }),
            shadow.scale_alpha(alpha),
            0.0,
            Color::TRANSPARENT,
            clip_rect,
            clip_radius,
        ));
    }

    if (meta.paint.bg.is_some() || meta.paint.border_width > 0.0) && w > 0.0 && h > 0.0 {
        let bg = meta
            .paint
            .bg
            .unwrap_or(Color::TRANSPARENT)
            .scale_alpha(alpha);
        let bc = meta.paint.border_color.scale_alpha(alpha);
        let mut inst = Instance::rect(
            [draw_x, draw_y, draw_w, draw_h],
            meta.paint.radii,
            bg,
            meta.paint.border_width,
            bc,
            clip_rect,
            clip_radius,
        );
        inst.flags = meta.paint.border_flags;
        ctx.draw.instances.push(inst);
    }

    if meta.paint.outline_width > 0.0 && w > 0.0 && h > 0.0 {
        let off = meta.paint.outline_offset;
        let ow = meta.paint.outline_width;
        let ox = draw_x - off - ow;
        let oy = draw_y - off - ow;
        let ow_box = draw_w + (off + ow) * 2.0;
        let oh_box = draw_h + (off + ow) * 2.0;
        let mut inst = Instance::rect(
            [ox, oy, ow_box, oh_box],
            meta.paint.radii.map(|r| (r + off + ow).max(0.0)),
            Color::TRANSPARENT,
            ow,
            meta.paint.outline_color.scale_alpha(alpha),
            clip_rect,
            clip_radius,
        );
        inst.flags = meta.paint.outline_flags;
        ctx.draw.instances.push(inst);
    }

    match &meta.content {
        NodeContent::None => {}
        NodeContent::Text {
            text: raw,
            style,
            shaped: Some(shaped),
        } => {
            let content_x = draw_x + layout.padding.left + layout.border.left;
            let content_y = draw_y + layout.padding.top + layout.border.top;
            let content_w = (draw_w
                - layout.padding.left
                - layout.padding.right
                - layout.border.left
                - layout.border.right)
                .max(1.0);
            let reshaper;
            let final_shaped = if style.align != cosmic_text::Align::Left
                || (style.ellipsis && shaped.width > content_w + 1.0)
            {
                reshaper = ctx.text.shape_text_cached(raw, Some(content_w), *style);
                &reshaper
            } else {
                shaped
            };
            final_shaped.emit_colored(
                ctx.draw,
                content_x,
                content_y,
                style.color.scale_alpha(alpha),
                clip_rect,
                clip_radius,
            );
        }
        #[cfg(windows)]
        NodeContent::RichText {
            spans,
            base,
            shaped: Some(shaped),
        } => {
            let content_x = draw_x + layout.padding.left + layout.border.left;
            let content_y = draw_y + layout.padding.top + layout.border.top;
            let content_w = (draw_w
                - layout.padding.left
                - layout.padding.right
                - layout.border.left
                - layout.border.right)
                .max(1.0);
            let reshaper;
            let final_shaped = if base.align != cosmic_text::Align::Left {
                let borrowed: Vec<TextSpan<'_>> = spans
                    .iter()
                    .map(|(t, st)| TextSpan {
                        text: t.as_str(),
                        style: *st,
                    })
                    .collect();
                reshaper = ctx.text.shape_spans(&borrowed, Some(content_w), *base);
                &reshaper
            } else {
                shaped
            };
            if alpha < 0.999 {
                let mut cloned = final_shaped.clone();
                for g in &mut cloned.glyphs {
                    g.color = g.color.scale_alpha(alpha);
                }
                cloned.emit(ctx.draw, content_x, content_y, clip_rect, clip_radius);
            } else {
                final_shaped.emit(ctx.draw, content_x, content_y, clip_rect, clip_radius);
            }
        }
        NodeContent::Text { shaped: None, .. } => {}
        #[cfg(windows)]
        NodeContent::RichText { shaped: None, .. } => {}
        NodeContent::Icon {
            id,
            size,
            stroke,
            color,
        } => {
            let px = (*size * scale).round().max(1.0) as u32;
            if let Some(region) = ctx.text.atlas.icon(*id, px, *stroke) {
                ctx.draw.instances.push(Instance::mask_r8(
                    [draw_x, draw_y, draw_w, draw_h],
                    region.uv,
                    color.scale_alpha(alpha),
                    clip_rect,
                    clip_radius,
                ));
            }
        }
        NodeContent::Logo { size } => {
            let px = size.round().max(1.0) as u32;
            if let Some(region) = ctx.text.atlas.logo(px) {
                ctx.draw.instances.push(Instance::sprite_rgba8(
                    [draw_x, draw_y, draw_w, draw_h],
                    region.uv,
                    Color::WHITE.with_alpha(alpha),
                    clip_rect,
                    clip_radius,
                ));
            }
        }
        #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
        NodeContent::Thumbnail => {
            if let Some(region) = ctx.text.atlas.thumbnail() {
                ctx.draw.instances.push(Instance::sprite_rgba8(
                    [draw_x, draw_y, draw_w, draw_h],
                    region.uv,
                    Color::WHITE.with_alpha(alpha),
                    clip_rect,
                    clip_radius,
                ));
            }
        }
        NodeContent::Arc {
            start_rad,
            sweep_rad,
            stroke,
            color,
        } => {
            ctx.draw.instances.push(Instance::arc(
                [draw_x, draw_y, draw_w, draw_h],
                *start_rad,
                *sweep_rad,
                *stroke,
                color.scale_alpha(alpha),
                clip_rect,
            ));
        }
        NodeContent::Dot { color } => {
            let r = w.min(h) * 0.5;
            ctx.draw.instances.push(Instance::rect(
                [draw_x, draw_y, draw_w, draw_h],
                [r; 4],
                color.scale_alpha(alpha),
                0.0,
                Color::TRANSPARENT,
                clip_rect,
                clip_radius,
            ));
        }
        NodeContent::ScrollViewport { .. } => {}
    }

    let (child_clip, child_clip_radius, scroll_y) =
        if let NodeContent::ScrollViewport { scroll_y, .. } = meta.content {
            (
                [
                    clip_rect[0].max(x),
                    clip_rect[1].max(y),
                    clip_rect[2].min(x + w),
                    clip_rect[3].min(y + h),
                ],
                0.0,
                scroll_y,
            )
        } else if meta.clip_children {
            (
                [
                    clip_rect[0].max(x),
                    clip_rect[1].max(y),
                    clip_rect[2].min(x + w),
                    clip_rect[3].min(y + h),
                ],
                meta.paint.radii[0],
                0.0,
            )
        } else {
            (clip_rect, clip_radius, 0.0)
        };

    if let Ok(children) = tree.children(node) {
        for child in children {
            emit_node(
                tree,
                child,
                x,
                y - scroll_y,
                child_clip,
                child_clip_radius,
                alpha,
                ctx,
            );
        }
    }

    // Draw thin scrollbar indicator if scrollable content overflows.
    if let NodeContent::ScrollViewport {
        scroll_y,
        thumb_color,
        ..
    } = meta.content
        && let Ok(children) = tree.children(node)
        && let Some(&first_child) = children.first()
        && let Ok(child_layout) = tree.layout(first_child)
    {
        let content_h = child_layout.size.height;
        if content_h > h + 1.0 && h > 16.0 {
            let max_scroll = (content_h - h).max(1.0);
            let frac = (scroll_y / max_scroll).clamp(0.0, 1.0);
            let thumb_h = ((h / content_h) * h).clamp(24.0, h);
            let thumb_y = y + frac * (h - thumb_h);
            ctx.draw.instances.push(Instance::rect(
                [x + w - 6.0, thumb_y, 4.0, thumb_h],
                [2.0; 4],
                thumb_color.scale_alpha(alpha),
                0.0,
                Color::TRANSPARENT,
                clip_rect,
                0.0,
            ));
        }
    }
    let _ = ctx.surface;
}
