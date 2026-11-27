//! Platform-agnostic GPU draw list primitives.

use super::theme::Color;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceKind {
    /// SDF rounded rectangle with 4 independent corner radii and solid/dashed/dotted border.
    Rect = 0,
    /// R8 atlas mask quad (grayscale glyph or monochrome icon) tinted by `color`.
    MaskR8 = 1,
    /// RGBA8 atlas sprite quad (color emoji, logo, thumbnail) tinted by `color`.
    SpriteRgba8 = 2,
    /// Circular progress arc (`uv.x` = start angle rad, `uv.y` = sweep rad, `uv.z` = stroke px).
    Arc = 3,
    /// Anti-aliased round-cap line segment (`rect` = [x0, y0, x1, y1], `border_width` = thickness).
    #[cfg(windows)]
    Line = 4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Instance {
    /// `[x, y, w, h]` for quads/arcs, or `[x0, y0, x1, y1]` for `Line`.
    pub rect: [f32; 4],
    /// `[u0, v0, u1, v1]` for atlas textures, or `[start_rad, sweep_rad, stroke_px, 0.0]` for `Arc`.
    pub uv: [f32; 4],
    /// Premultiplied RGBA color.
    pub color: [f32; 4],
    /// Premultiplied RGBA border color.
    pub border_color: [f32; 4],
    /// `[top_left, top_right, bottom_right, bottom_left]` corner radii in px.
    pub radii: [f32; 4],
    /// `[left, top, right, bottom]` scissor clip box in px.
    pub clip_rect: [f32; 4],
    /// Uniform corner radius for `clip_rect` in px.
    pub clip_radius: f32,
    /// Border thickness in px (or line thickness for `Line`).
    pub border_width: f32,
    /// `InstanceKind as u32`.
    pub kind: u32,
    /// Bit 0 = dashed border, Bit 1 = dotted border.
    pub flags: u32,
}

impl Instance {
    pub const FLAG_DASHED: u32 = 1;
    pub const FLAG_DOTTED: u32 = 2;

    pub fn rect(
        rect: [f32; 4],
        radii: [f32; 4],
        color: Color,
        border_width: f32,
        border_color: Color,
        clip_rect: [f32; 4],
        clip_radius: f32,
    ) -> Self {
        Self {
            rect,
            uv: [0.0; 4],
            color: color.premultiplied(),
            border_color: border_color.premultiplied(),
            radii,
            clip_rect,
            clip_radius,
            border_width,
            kind: InstanceKind::Rect as u32,
            flags: 0,
        }
    }

    pub fn mask_r8(
        rect: [f32; 4],
        uv: [f32; 4],
        color: Color,
        clip_rect: [f32; 4],
        clip_radius: f32,
    ) -> Self {
        Self {
            rect,
            uv,
            color: color.premultiplied(),
            border_color: [0.0; 4],
            radii: [0.0; 4],
            clip_rect,
            clip_radius,
            border_width: 0.0,
            kind: InstanceKind::MaskR8 as u32,
            flags: 0,
        }
    }

    pub fn sprite_rgba8(
        rect: [f32; 4],
        uv: [f32; 4],
        tint: Color,
        clip_rect: [f32; 4],
        clip_radius: f32,
    ) -> Self {
        Self {
            rect,
            uv,
            color: tint.premultiplied(),
            border_color: [0.0; 4],
            radii: [0.0; 4],
            clip_rect,
            clip_radius,
            border_width: 0.0,
            kind: InstanceKind::SpriteRgba8 as u32,
            flags: 0,
        }
    }

    pub fn arc(
        rect: [f32; 4],
        start_rad: f32,
        sweep_rad: f32,
        stroke_px: f32,
        color: Color,
        clip_rect: [f32; 4],
    ) -> Self {
        Self {
            rect,
            uv: [start_rad, sweep_rad, stroke_px, 0.0],
            color: color.premultiplied(),
            border_color: [0.0; 4],
            radii: [0.0; 4],
            clip_rect,
            clip_radius: 0.0,
            border_width: stroke_px,
            kind: InstanceKind::Arc as u32,
            flags: 0,
        }
    }

    #[cfg(windows)]
    pub fn line(
        p0: [f32; 2],
        p1: [f32; 2],
        thickness: f32,
        color: Color,
        clip_rect: [f32; 4],
    ) -> Self {
        Self {
            rect: [p0[0], p0[1], p1[0], p1[1]],
            uv: [0.0; 4],
            color: color.premultiplied(),
            border_color: [0.0; 4],
            radii: [0.0; 4],
            clip_rect,
            clip_radius: 0.0,
            border_width: thickness,
            kind: InstanceKind::Line as u32,
            flags: 0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MaskVertex {
    pub pos: [f32; 2],
    pub color: [f32; 4],
}

#[derive(Debug, Clone)]
pub struct OcrCardDraw {
    pub width: f32,
    pub height: f32,
    /// Destination quadrilateral in `[0..2048, 0..2048]` surface pixel coordinates (after `aspect`).
    pub quad: [[f32; 2]; 4],
    /// Instances drawn inside `[0..width, 0..height]` local space before homography warp.
    pub instances: Vec<Instance>,
}

#[derive(Debug, Default, Clone)]
pub struct DrawList {
    pub width: u32,
    pub height: u32,
    pub instances: Vec<Instance>,
    /// Triangulated convex mask quads (used by `OcrResult`).
    pub mask_vertices: Vec<MaskVertex>,
    /// Perspective-warped cards (used by `OcrResult`).
    pub ocr_cards: Vec<OcrCardDraw>,
    /// Cropped output bounds `[l, t, r, b]` inside `(width, height)`.
    pub capture_bounds: [u32; 4],
}

impl DrawList {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            instances: Vec::new(),
            mask_vertices: Vec::new(),
            ocr_cards: Vec::new(),
            capture_bounds: [0, 0, width, height],
        }
    }
}
