//! Font discovery, text shaping, and shared GPU glyph/icon atlases via `cosmic-text`.

use std::collections::HashMap;
use std::sync::Arc;

use cosmic_text::{
    Align, Attrs, Buffer, CacheKey, Family, FontSystem, Metrics, Shaping, Style as FontStyle,
    SwashCache, SwashContent, Weight, Wrap,
};

use super::draw::{DrawList, Instance};
use super::icons::{self, IconId};
use super::theme::Color;

pub const ATLAS_R8_SIZE: u32 = 2048;
pub const ATLAS_RGBA_SIZE: u32 = 1024;

#[derive(Debug, Clone, Copy)]
pub struct AtlasRegion {
    pub uv: [f32; 4],
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub is_rgba: bool,
}

#[derive(Default)]
struct ShelfPacker {
    size: u32,
    cursor_x: u32,
    cursor_y: u32,
    shelf_h: u32,
}

impl ShelfPacker {
    fn new(size: u32) -> Self {
        Self {
            size,
            cursor_x: 1,
            cursor_y: 1,
            shelf_h: 0,
        }
    }

    fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w == 0 || h == 0 || w + 2 > self.size || h + 2 > self.size {
            return None;
        }
        if self.cursor_x + w + 1 > self.size {
            self.cursor_x = 1;
            self.cursor_y += self.shelf_h + 1;
            self.shelf_h = 0;
        }
        if self.cursor_y + h + 1 > self.size {
            return None;
        }
        let pos = (self.cursor_x, self.cursor_y);
        self.cursor_x += w + 1;
        self.shelf_h = self.shelf_h.max(h);
        Some(pos)
    }

    fn reset(&mut self) {
        *self = Self::new(self.size);
    }
}

pub struct GlyphAtlas {
    pub r8_data: Vec<u8>,
    pub rgba_data: Vec<u8>,
    pub r8_generation: u64,
    pub rgba_generation: u64,
    pub r8_reset_gen: u64,
    pub rgba_reset_gen: u64,
    pub r8_dirty: Vec<[u32; 4]>,
    pub rgba_dirty: Vec<[u32; 4]>,
    r8_pack: ShelfPacker,
    rgba_pack: ShelfPacker,
    glyphs: HashMap<CacheKey, Option<AtlasRegion>>,
    icons: HashMap<(IconId, u32, u32), AtlasRegion>,
    logo: Option<(u32, AtlasRegion)>,
    #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
    thumbnail: Option<AtlasRegion>,
}

impl GlyphAtlas {
    pub fn new() -> Self {
        Self {
            r8_data: vec![0; (ATLAS_R8_SIZE * ATLAS_R8_SIZE) as usize],
            rgba_data: vec![0; (ATLAS_RGBA_SIZE * ATLAS_RGBA_SIZE * 4) as usize],
            r8_generation: 1,
            rgba_generation: 1,
            r8_reset_gen: 1,
            rgba_reset_gen: 1,
            r8_dirty: Vec::new(),
            rgba_dirty: Vec::new(),
            r8_pack: ShelfPacker::new(ATLAS_R8_SIZE),
            rgba_pack: ShelfPacker::new(ATLAS_RGBA_SIZE),
            glyphs: HashMap::new(),
            icons: HashMap::new(),
            logo: None,
            #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
            thumbnail: None,
        }
    }

    fn reset_r8(&mut self) {
        self.r8_data.fill(0);
        self.r8_pack.reset();
        self.glyphs.retain(|_, v| v.is_some_and(|r| r.is_rgba));
        self.icons.clear();
        self.r8_dirty.clear();
        self.r8_reset_gen = self.r8_reset_gen.wrapping_add(1);
        self.r8_generation = self.r8_generation.wrapping_add(1);
    }

    fn reset_rgba(&mut self) {
        self.rgba_data.fill(0);
        self.rgba_pack.reset();
        self.glyphs.retain(|_, v| v.is_some_and(|r| !r.is_rgba));
        self.logo = None;
        #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
        {
            self.thumbnail = None;
        }
        self.rgba_dirty.clear();
        self.rgba_reset_gen = self.rgba_reset_gen.wrapping_add(1);
        self.rgba_generation = self.rgba_generation.wrapping_add(1);
    }

    fn alloc_r8(&mut self, w: u32, h: u32, pixels: &[u8]) -> Option<AtlasRegion> {
        let pos = match self.r8_pack.alloc(w, h) {
            Some(pos) => pos,
            None => {
                self.reset_r8();
                self.r8_pack.alloc(w, h)?
            }
        };
        let stride = ATLAS_R8_SIZE as usize;
        for row in 0..h as usize {
            let dst = (pos.1 as usize + row) * stride + pos.0 as usize;
            let src = row * w as usize;
            self.r8_data[dst..dst + w as usize].copy_from_slice(&pixels[src..src + w as usize]);
        }
        self.r8_dirty.push([pos.0, pos.1, pos.0 + w, pos.1 + h]);
        self.r8_generation = self.r8_generation.wrapping_add(1);
        let s = ATLAS_R8_SIZE as f32;
        Some(AtlasRegion {
            uv: [
                pos.0 as f32 / s,
                pos.1 as f32 / s,
                (pos.0 + w) as f32 / s,
                (pos.1 + h) as f32 / s,
            ],
            width: w,
            height: h,
            left: 0,
            top: 0,
            is_rgba: false,
        })
    }

    fn alloc_rgba(&mut self, w: u32, h: u32, pixels: &[u8]) -> Option<AtlasRegion> {
        let pos = match self.rgba_pack.alloc(w, h) {
            Some(pos) => pos,
            None => {
                self.reset_rgba();
                self.rgba_pack.alloc(w, h)?
            }
        };
        let stride = ATLAS_RGBA_SIZE as usize * 4;
        let row_bytes = w as usize * 4;
        for row in 0..h as usize {
            let dst = (pos.1 as usize + row) * stride + pos.0 as usize * 4;
            let src = row * row_bytes;
            self.rgba_data[dst..dst + row_bytes].copy_from_slice(&pixels[src..src + row_bytes]);
        }
        self.rgba_dirty.push([pos.0, pos.1, pos.0 + w, pos.1 + h]);
        self.rgba_generation = self.rgba_generation.wrapping_add(1);
        let s = ATLAS_RGBA_SIZE as f32;
        Some(AtlasRegion {
            uv: [
                pos.0 as f32 / s,
                pos.1 as f32 / s,
                (pos.0 + w) as f32 / s,
                (pos.1 + h) as f32 / s,
            ],
            width: w,
            height: h,
            left: 0,
            top: 0,
            is_rgba: true,
        })
    }

    pub fn icon(&mut self, id: IconId, size_px: u32, stroke_width: f32) -> Option<AtlasRegion> {
        let stroke_key = (stroke_width * 10.0).round() as u32;
        if let Some(&region) = self.icons.get(&(id, size_px, stroke_key)) {
            return Some(region);
        }
        let pixels = icons::rasterize_icon_r8(id, size_px, stroke_width);
        let region = self.alloc_r8(size_px, size_px, &pixels)?;
        self.icons.insert((id, size_px, stroke_key), region);
        Some(region)
    }

    pub fn logo(&mut self, size_px: u32) -> Option<AtlasRegion> {
        if let Some((cached, region)) = self.logo
            && cached == size_px
        {
            return Some(region);
        }
        let pixels = icons::rasterize_logo_rgba8(size_px);
        let region = self.alloc_rgba(size_px, size_px, &pixels)?;
        self.logo = Some((size_px, region));
        Some(region)
    }

    #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
    pub fn thumbnail(&mut self) -> Option<AtlasRegion> {
        if let Some(region) = self.thumbnail {
            return Some(region);
        }
        let (w, h, pixels) = icons::thumbnail_rgba8();
        let region = self.alloc_rgba(w, h, &pixels)?;
        self.thumbnail = Some(region);
        Some(region)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TextStyle {
    pub font_size: f32,
    pub line_height: f32,
    pub weight: u16,
    pub italic: bool,
    pub mono: bool,
    pub align: Align,
    pub wrap: Wrap,
    pub max_lines: Option<usize>,
    pub ellipsis: bool,
    pub color: Color,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: 20.0,
            line_height: 27.0,
            weight: 400,
            italic: false,
            mono: false,
            align: Align::Left,
            wrap: Wrap::WordOrGlyph,
            max_lines: None,
            ellipsis: false,
            color: Color::WHITE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PositionedGlyph {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub uv: [f32; 4],
    pub is_rgba: bool,
    pub color: Color,
}

#[derive(Debug, Clone, Default)]
pub struct ShapedText {
    pub width: f32,
    pub height: f32,
    #[allow(dead_code)]
    pub line_count: usize,
    pub glyphs: Vec<PositionedGlyph>,
}

impl ShapedText {
    pub fn emit(
        &self,
        draw: &mut DrawList,
        offset_x: f32,
        offset_y: f32,
        clip_rect: [f32; 4],
        clip_radius: f32,
    ) {
        for g in &self.glyphs {
            let rect = [offset_x + g.x, offset_y + g.y, g.w, g.h];
            if g.is_rgba {
                draw.instances.push(Instance::sprite_rgba8(
                    rect,
                    g.uv,
                    Color::WHITE.with_alpha(g.color.a),
                    clip_rect,
                    clip_radius,
                ));
            } else {
                draw.instances.push(Instance::mask_r8(
                    rect,
                    g.uv,
                    g.color,
                    clip_rect,
                    clip_radius,
                ));
            }
        }
    }

    pub fn emit_colored(
        &self,
        draw: &mut DrawList,
        offset_x: f32,
        offset_y: f32,
        color: Color,
        clip_rect: [f32; 4],
        clip_radius: f32,
    ) {
        for g in &self.glyphs {
            let rect = [offset_x + g.x, offset_y + g.y, g.w, g.h];
            if g.is_rgba {
                draw.instances.push(Instance::sprite_rgba8(
                    rect,
                    g.uv,
                    Color::WHITE.with_alpha(color.a),
                    clip_rect,
                    clip_radius,
                ));
            } else {
                draw.instances.push(Instance::mask_r8(
                    rect,
                    g.uv,
                    color,
                    clip_rect,
                    clip_radius,
                ));
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextSpan<'a> {
    pub text: &'a str,
    pub style: TextStyle,
}

#[derive(Hash, PartialEq, Eq)]
struct ShapeKey {
    locale: String,
    text: String,
    max_width_q: Option<i32>,
    font_size: u32,
    line_height: u32,
    weight: u16,
    italic: bool,
    mono: bool,
    align: u8,
    wrap: u8,
    max_lines: Option<usize>,
    ellipsis: bool,
}

#[derive(Hash, PartialEq, Eq)]
struct MeasureKey {
    locale: String,
    text: String,
    font_size: u32,
    weight: u16,
}

fn align_tag(align: Align) -> u8 {
    match align {
        Align::Left => 0,
        Align::Right => 1,
        Align::Center => 2,
        Align::Justified => 3,
        Align::End => 4,
    }
}

fn wrap_tag(wrap: Wrap) -> u8 {
    match wrap {
        Wrap::None => 0,
        Wrap::Glyph => 1,
        Wrap::Word => 2,
        Wrap::WordOrGlyph => 3,
    }
}

pub struct TextEngine {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub atlas: GlyphAtlas,
    locale: String,
    scratch: Buffer,
    shape_cache: HashMap<ShapeKey, Arc<ShapedText>>,
    measure_cache: HashMap<MeasureKey, f32>,
    cache_reset_gens: (u64, u64),
}

impl TextEngine {
    pub fn new() -> Self {
        let mut font_system = FontSystem::new();
        #[cfg(windows)]
        {
            font_system.db_mut().set_sans_serif_family("Segoe UI");
            font_system.db_mut().set_monospace_family("Cascadia Mono");
        }
        let scratch = Buffer::new(&mut font_system, Metrics::new(20.0, 27.0));
        let atlas = GlyphAtlas::new();
        let cache_reset_gens = (atlas.r8_reset_gen, atlas.rgba_reset_gen);
        Self {
            font_system,
            swash_cache: SwashCache::new(),
            atlas,
            locale: String::new(),
            scratch,
            shape_cache: HashMap::new(),
            measure_cache: HashMap::new(),
            cache_reset_gens,
        }
    }

    fn sync_cache_generation(&mut self) {
        let cur = (self.atlas.r8_reset_gen, self.atlas.rgba_reset_gen);
        if cur != self.cache_reset_gens {
            self.shape_cache.clear();
            self.cache_reset_gens = cur;
        }
    }

    pub fn set_locale(&mut self, locale: &str) {
        if self.locale != locale {
            self.locale = locale.to_owned();
        }
    }

    fn attrs_for(locale: &str, style: &TextStyle) -> Attrs<'static> {
        let family = if style.mono {
            #[cfg(windows)]
            {
                Family::Name("Cascadia Mono")
            }
            #[cfg(not(windows))]
            {
                Family::Monospace
            }
        } else {
            #[cfg(windows)]
            {
                let lower = locale.to_ascii_lowercase();
                if lower.starts_with("zh-hant")
                    || lower.starts_with("zh-tw")
                    || lower.starts_with("zh-hk")
                {
                    Family::Name("Microsoft JhengHei UI")
                } else if lower.starts_with("zh") {
                    Family::Name("Microsoft YaHei UI")
                } else if lower.starts_with("ja") {
                    Family::Name("Yu Gothic UI")
                } else if lower.starts_with("ko") {
                    Family::Name("Malgun Gothic")
                } else {
                    Family::Name("Segoe UI")
                }
            }
            #[cfg(not(windows))]
            {
                let _ = locale;
                Family::SansSerif
            }
        };
        let weight = if style.weight >= 650 {
            700
        } else if style.weight >= 550 {
            600
        } else if style.weight >= 450 {
            500
        } else {
            400
        };
        Attrs::new()
            .family(family)
            .weight(Weight(weight))
            .style(if style.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            })
            .color(cosmic_text::Color::rgba(
                (style.color.r * 255.0).round() as u8,
                (style.color.g * 255.0).round() as u8,
                (style.color.b * 255.0).round() as u8,
                (style.color.a * 255.0).round() as u8,
            ))
    }

    pub fn measure_line_width(&mut self, text: &str, font_size: f32, weight: u16) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let key = MeasureKey {
            locale: self.locale.clone(),
            text: text.to_owned(),
            font_size: font_size.to_bits(),
            weight,
        };
        if let Some(&w) = self.measure_cache.get(&key) {
            return w;
        }
        let style = TextStyle {
            font_size,
            line_height: (font_size * 1.4).max(1.0),
            weight,
            wrap: Wrap::None,
            ..Default::default()
        };
        let attrs = Self::attrs_for(&self.locale, &style);
        self.scratch.set_metrics(
            &mut self.font_system,
            Metrics::new(style.font_size.max(1.0), style.line_height.max(1.0)),
        );
        self.scratch
            .set_wrap(&mut self.font_system, Wrap::None);
        self.scratch
            .set_size(&mut self.font_system, None, None);
        self.scratch
            .set_text(&mut self.font_system, text, &attrs, Shaping::Advanced);
        self.scratch.shape_until_scroll(&mut self.font_system, false);
        let width = self
            .scratch
            .layout_runs()
            .map(|r| r.line_w)
            .fold(0.0f32, f32::max);
        if self.measure_cache.len() > 4096 {
            self.measure_cache.clear();
        }
        self.measure_cache.insert(key, width);
        width
    }

    pub fn shape_text_cached(
        &mut self,
        text: &str,
        max_width: Option<f32>,
        style: TextStyle,
    ) -> Arc<ShapedText> {
        if text.is_empty() {
            return Arc::new(ShapedText::default());
        }
        self.sync_cache_generation();
        let max_width_q = max_width.map(|w| (w.max(1.0) * 4.0).round() as i32);
        let key = ShapeKey {
            locale: self.locale.clone(),
            text: text.to_owned(),
            max_width_q,
            font_size: style.font_size.to_bits(),
            line_height: style.line_height.to_bits(),
            weight: style.weight,
            italic: style.italic,
            mono: style.mono,
            align: align_tag(style.align),
            wrap: wrap_tag(style.wrap),
            max_lines: style.max_lines,
            ellipsis: style.ellipsis,
        };
        if let Some(hit) = self.shape_cache.get(&key) {
            return Arc::clone(hit);
        }
        let shaped = Arc::new(self.shape_text(text, max_width, TextStyle { color: Color::WHITE, ..style }));
        self.sync_cache_generation();
        if self.shape_cache.len() > 4096 {
            self.shape_cache.clear();
        }
        self.shape_cache.insert(key, Arc::clone(&shaped));
        shaped
    }

    pub fn shape_text(
        &mut self,
        text: &str,
        max_width: Option<f32>,
        style: TextStyle,
    ) -> ShapedText {
        let span = [TextSpan { text, style }];
        self.shape_spans(&span, max_width, style)
    }

    pub fn shape_spans(
        &mut self,
        spans: &[TextSpan<'_>],
        max_width: Option<f32>,
        base: TextStyle,
    ) -> ShapedText {
        if spans.is_empty() || spans.iter().all(|s| s.text.is_empty()) {
            return ShapedText::default();
        }
        let metrics = Metrics::new(base.font_size.max(1.0), base.line_height.max(1.0));
        self.scratch.set_metrics(&mut self.font_system, metrics);
        self.scratch.set_wrap(&mut self.font_system, base.wrap);
        let width_limit = max_width.map(|w| w.max(1.0));
        self.scratch
            .set_size(&mut self.font_system, width_limit, None);

        let default_attrs = Self::attrs_for(&self.locale, &base);
        let rich: Vec<(&str, Attrs<'_>)> = spans
            .iter()
            .filter(|s| !s.text.is_empty())
            .map(|s| {
                let attrs = Self::attrs_for(&self.locale, &s.style);
                (s.text, attrs)
            })
            .collect();

        if rich.len() == 1 {
            self.scratch.set_text(
                &mut self.font_system,
                rich[0].0,
                &rich[0].1,
                Shaping::Advanced,
            );
        } else {
            self.scratch.set_rich_text(
                &mut self.font_system,
                rich,
                &default_attrs,
                Shaping::Advanced,
                Some(base.align),
            );
        }
        for line in &mut self.scratch.lines {
            line.set_align(Some(base.align));
        }
        self.scratch.shape_until_scroll(&mut self.font_system, false);

        // If single-line ellipsis is requested and overflows, binary-search a fitting prefix.
        if base.ellipsis
            && base.max_lines == Some(1)
            && let Some(limit) = width_limit
            && spans.len() == 1
        {
            let total_w = self
                .scratch
                .layout_runs()
                .map(|r| r.line_w)
                .fold(0.0f32, f32::max);
            if total_w > limit + 0.5 {
                let raw = spans[0].text;
                let mut bounds: Vec<usize> = raw.char_indices().map(|(i, _)| i).collect();
                bounds.push(raw.len());
                let mut lo = 0usize;
                let mut hi = bounds.len() - 1;
                while lo < hi {
                    let mid = (lo + hi).div_ceil(2);
                    let candidate = format!("{}…", &raw[..bounds[mid]]);
                    if self.measure_line_width(&candidate, base.font_size, base.weight) <= limit {
                        lo = mid;
                    } else {
                        hi = mid - 1;
                    }
                }
                let truncated = if lo == 0 {
                    "…".to_owned()
                } else {
                    format!("{}…", raw[..bounds[lo]].trim_end())
                };
                let mut single = base;
                single.ellipsis = false;
                return self.shape_text(&truncated, max_width, single);
            }
        }

        let mut max_w = 0.0f32;
        let mut total_h = 0.0f32;
        let mut line_count = 0usize;
        let mut glyphs = Vec::new();

        // Collect run items first to avoid holding borrow on `self.scratch`.
        #[derive(Clone)]
        struct PendingGlyph {
            cache_key: CacheKey,
            x: i32,
            y: i32,
            color: Color,
        }
        let mut pending = Vec::new();
        for run in self.scratch.layout_runs() {
            if let Some(limit) = base.max_lines
                && line_count >= limit
            {
                break;
            }
            line_count += 1;
            max_w = max_w.max(run.line_w);
            total_h = run.line_top + run.line_height;
            for glyph in run.glyphs {
                let phys = glyph.physical((0.0, 0.0), 1.0);
                let color = glyph.color_opt.map_or(base.color, |c| {
                    Color::rgba(c.r(), c.g(), c.b(), c.a() as f32 / 255.0)
                });
                pending.push(PendingGlyph {
                    cache_key: phys.cache_key,
                    x: phys.x,
                    y: run.line_y.round() as i32 + phys.y,
                    color,
                });
            }
        }

        for pg in pending {
            let region = self.ensure_glyph(pg.cache_key);
            if let Some(region) = region {
                glyphs.push(PositionedGlyph {
                    x: (pg.x + region.left) as f32,
                    y: (pg.y - region.top) as f32,
                    w: region.width as f32,
                    h: region.height as f32,
                    uv: region.uv,
                    is_rgba: region.is_rgba,
                    color: pg.color,
                });
            }
        }

        ShapedText {
            width: max_w.ceil(),
            height: total_h.ceil(),
            line_count,
            glyphs,
        }
    }

    fn ensure_glyph(&mut self, key: CacheKey) -> Option<AtlasRegion> {
        if let Some(entry) = self.atlas.glyphs.get(&key) {
            return *entry;
        }
        let img = self
            .swash_cache
            .get_image_uncached(&mut self.font_system, key);
        let region = img.and_then(|img| {
            let w = img.placement.width;
            let h = img.placement.height;
            if w == 0 || h == 0 {
                return None;
            }
            let mut region = match img.content {
                SwashContent::Mask => self.atlas.alloc_r8(w, h, &img.data)?,
                SwashContent::Color => {
                    let mut premul = img.data;
                    for px in premul.as_chunks_mut::<4>().0 {
                        let a = u32::from(px[3]);
                        for ch in &mut px[..3] {
                            *ch = ((u32::from(*ch) * a + 127) / 255) as u8;
                        }
                    }
                    self.atlas.alloc_rgba(w, h, &premul)?
                }
                SwashContent::SubpixelMask => {
                    let alpha: Vec<u8> = img
                        .data
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|px| ((u16::from(px[0]) + u16::from(px[1]) + u16::from(px[2])) / 3) as u8)
                        .collect();
                    self.atlas.alloc_r8(w, h, &alpha)?
                }
            };
            region.left = img.placement.left;
            region.top = img.placement.top;
            Some(region)
        });
        self.atlas.glyphs.insert(key, region);
        region
    }
}
