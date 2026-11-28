//! Font discovery, text shaping, and shared GPU glyph/icon atlases via `cosmic-text`.

use std::collections::HashMap;
use std::sync::Arc;

use cosmic_text::{
    Align, Attrs, Buffer, CacheKey, CacheKeyFlags, Family, FontSystem, Metrics, Shaping, Stretch,
    Style as FontStyle, SwashCache, SwashContent, Weight, Wrap, fontdb,
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
    #[cfg_attr(not(windows), allow(dead_code))]
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
    #[cfg(windows)]
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
                draw.instances
                    .push(Instance::mask_r8(rect, g.uv, color, clip_rect, clip_radius));
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

fn canonical_fallback_locale(locale: &str) -> &'static str {
    let lower = locale.trim().to_ascii_lowercase();
    if lower.starts_with("zh-hk") || lower.starts_with("zh-mo") {
        "zh-HK"
    } else if lower.starts_with("zh-hant") || lower.starts_with("zh-tw") {
        "zh-TW"
    } else if lower.starts_with("ja") {
        "ja"
    } else if lower.starts_with("ko") {
        "ko"
    } else {
        "zh-CN"
    }
}

#[cfg(windows)]
fn font_families_for_locale(
    _db: &fontdb::Database,
    locale: &str,
) -> (Family<'static>, bool, Option<&'static str>) {
    let lower = locale.trim().to_ascii_lowercase();
    if lower.starts_with("zh-hant")
        || lower.starts_with("zh-tw")
        || lower.starts_with("zh-hk")
        || lower.starts_with("zh-mo")
    {
        (
            Family::Name("Microsoft JhengHei UI"),
            true,
            Some("Microsoft JhengHei UI"),
        )
    } else if lower.starts_with("zh") {
        (
            Family::Name("Microsoft YaHei UI"),
            true,
            Some("Microsoft YaHei UI"),
        )
    } else if lower.starts_with("ja") {
        (Family::Name("Yu Gothic UI"), true, Some("Yu Gothic"))
    } else if lower.starts_with("ko") {
        (Family::Name("Malgun Gothic"), true, Some("Malgun Gothic"))
    } else {
        (Family::Name("Segoe UI"), false, Some("Microsoft YaHei UI"))
    }
}

#[cfg(not(windows))]
fn first_installed_family(
    db: &fontdb::Database,
    candidates: &[&'static str],
) -> Option<&'static str> {
    candidates.iter().copied().find(|candidate| {
        db.faces()
            .any(|face| face.families.iter().any(|(name, _)| name == candidate))
    })
}

#[cfg(not(windows))]
fn font_families_for_locale(
    db: &fontdb::Database,
    locale: &str,
) -> (Family<'static>, bool, Option<&'static str>) {
    let lower = locale.trim().to_ascii_lowercase();
    let (is_cjk, candidates): (bool, &[&'static str]) =
        if lower.starts_with("zh-hk") || lower.starts_with("zh-mo") {
            (
                true,
                &[
                    "Noto Sans CJK HK",
                    "Noto Sans HK",
                    "Source Han Sans HK",
                    "Noto Sans CJK TC",
                ],
            )
        } else if lower.starts_with("zh-hant") || lower.starts_with("zh-tw") {
            (
                true,
                &[
                    "Noto Sans CJK TC",
                    "Noto Sans TC",
                    "Source Han Sans TC",
                    "Noto Sans CJK SC",
                ],
            )
        } else if lower.starts_with("ja") {
            (
                true,
                &["Noto Sans CJK JP", "Noto Sans JP", "Source Han Sans JP"],
            )
        } else if lower.starts_with("ko") {
            (
                true,
                &["Noto Sans CJK KR", "Noto Sans KR", "Source Han Sans KR"],
            )
        } else {
            (
                lower.starts_with("zh"),
                &[
                    "Noto Sans CJK SC",
                    "Noto Sans SC",
                    "Source Han Sans SC",
                    "WenQuanYi Micro Hei",
                ],
            )
        };
    let han = first_installed_family(db, candidates);
    match (is_cjk, han) {
        (true, Some(name)) => (Family::Name(name), true, Some(name)),
        _ => (Family::SansSerif, false, han),
    }
}

fn collect_family_normal_weights(db: &fontdb::Database, family_name: &str) -> Vec<u16> {
    let mut weights: Vec<u16> = db
        .faces()
        .filter(|face| {
            face.stretch == Stretch::Normal
                && face.style == FontStyle::Normal
                && face.families.iter().any(|(name, _)| name == family_name)
        })
        .map(|face| face.weight.0)
        .collect();
    weights.sort_unstable();
    weights.dedup();
    weights
}

fn family_has_italic(db: &fontdb::Database, family_name: &str) -> bool {
    db.faces().any(|face| {
        face.stretch == Stretch::Normal
            && face.style == FontStyle::Italic
            && face.families.iter().any(|(name, _)| name == family_name)
    })
}

fn snap_weight(available: &[u16], requested: u16) -> u16 {
    if available.is_empty() {
        return if requested >= 550 { 700 } else { 400 };
    }
    let prefer_heavy = requested >= 550;
    *available
        .iter()
        .min_by_key(|&&w| {
            let diff = w.abs_diff(requested);
            let tie = if prefer_heavy {
                u16::from(w < requested)
            } else {
                u16::from(w > requested)
            };
            (diff, tie, w)
        })
        .expect("non-empty weights")
}

pub struct TextEngine {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub atlas: GlyphAtlas,
    locale: String,
    primary_sans: Family<'static>,
    primary_sans_is_cjk: bool,
    primary_weights: Vec<u16>,
    han_weights: Vec<u16>,
    can_use_native_italic: bool,
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
        let sys_locale = font_system.locale().to_owned();
        let fallback = canonical_fallback_locale(&sys_locale);
        if font_system.locale() != fallback {
            let (_, db) = font_system.into_locale_and_db();
            font_system = FontSystem::new_with_locale_and_db(fallback.to_owned(), db);
        }
        let scratch = Buffer::new(&mut font_system, Metrics::new(20.0, 27.0));
        let atlas = GlyphAtlas::new();
        let cache_reset_gens = (atlas.r8_reset_gen, atlas.rgba_reset_gen);
        let mut engine = Self {
            font_system,
            swash_cache: SwashCache::new(),
            atlas,
            locale: sys_locale,
            primary_sans: Family::SansSerif,
            primary_sans_is_cjk: false,
            primary_weights: Vec::new(),
            han_weights: Vec::new(),
            can_use_native_italic: false,
            scratch,
            shape_cache: HashMap::new(),
            measure_cache: HashMap::new(),
            cache_reset_gens,
        };
        engine.refresh_font_profile();
        engine
    }

    fn refresh_font_profile(&mut self) {
        let db = self.font_system.db();
        let (primary_sans, primary_sans_is_cjk, han_family) =
            font_families_for_locale(db, &self.locale);
        let primary_weights = match primary_sans {
            Family::Name(name) => collect_family_normal_weights(db, name),
            _ => Vec::new(),
        };
        let han_weights = han_family
            .map(|name| collect_family_normal_weights(db, name))
            .unwrap_or_default();
        let primary_has_italic = match primary_sans {
            Family::Name(name) => family_has_italic(db, name),
            _ => true,
        };
        let han_has_italic = han_family.is_some_and(|name| family_has_italic(db, name));

        self.primary_sans = primary_sans;
        self.primary_sans_is_cjk = primary_sans_is_cjk;
        self.primary_weights = primary_weights;
        self.han_weights = han_weights;
        self.can_use_native_italic = primary_has_italic && han_has_italic;
    }

    fn sync_cache_generation(&mut self) {
        let cur = (self.atlas.r8_reset_gen, self.atlas.rgba_reset_gen);
        if cur != self.cache_reset_gens {
            self.shape_cache.clear();
            self.cache_reset_gens = cur;
        }
    }

    pub fn set_locale(&mut self, locale: &str) -> bool {
        let trimmed = locale.trim();
        if trimmed.is_empty() || self.locale == trimmed {
            return false;
        }
        self.locale = trimmed.to_owned();
        let fallback = canonical_fallback_locale(&self.locale);
        if self.font_system.locale() != fallback {
            let (_, db) = std::mem::replace(
                &mut self.font_system,
                FontSystem::new_with_locale_and_db(String::new(), fontdb::Database::new()),
            )
            .into_locale_and_db();
            self.font_system = FontSystem::new_with_locale_and_db(fallback.to_owned(), db);
        }
        self.refresh_font_profile();
        true
    }

    fn attrs_for(&self, style: &TextStyle) -> Attrs<'static> {
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
            self.primary_sans
        };
        let weight = if !style.mono && self.primary_sans_is_cjk {
            snap_weight(&self.primary_weights, style.weight)
        } else {
            snap_weight(&self.han_weights, style.weight)
        };
        let mut attrs =
            Attrs::new()
                .family(family)
                .weight(Weight(weight))
                .color(cosmic_text::Color::rgba(
                    (style.color.r * 255.0).round() as u8,
                    (style.color.g * 255.0).round() as u8,
                    (style.color.b * 255.0).round() as u8,
                    (style.color.a * 255.0).round() as u8,
                ));
        if style.italic {
            if self.can_use_native_italic {
                attrs = attrs.style(FontStyle::Italic);
            } else {
                attrs = attrs.cache_key_flags(CacheKeyFlags::FAKE_ITALIC);
            }
        }
        attrs
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
        let attrs = self.attrs_for(&style);
        self.scratch.set_metrics(
            &mut self.font_system,
            Metrics::new(style.font_size.max(1.0), style.line_height.max(1.0)),
        );
        self.scratch.set_wrap(&mut self.font_system, Wrap::None);
        self.scratch.set_size(&mut self.font_system, None, None);
        self.scratch
            .set_text(&mut self.font_system, text, &attrs, Shaping::Advanced);
        self.scratch
            .shape_until_scroll(&mut self.font_system, false);
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
        let shaped = Arc::new(self.shape_text(
            text,
            max_width,
            TextStyle {
                color: Color::WHITE,
                ..style
            },
        ));
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

        let default_attrs = self.attrs_for(&base);
        let rich: Vec<(&str, Attrs<'_>)> = spans
            .iter()
            .filter(|s| !s.text.is_empty())
            .map(|s| {
                let attrs = self.attrs_for(&s.style);
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
        self.scratch
            .shape_until_scroll(&mut self.font_system, false);

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
                        .map(|px| {
                            ((u16::from(px[0]) + u16::from(px[1]) + u16::from(px[2])) / 3) as u8
                        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_locale_normalizes_fallback_and_ignores_empty() {
        let mut engine = TextEngine::new();
        assert!(!engine.set_locale(""));
        assert!(!engine.set_locale("   "));

        let _ = engine.set_locale("zh-TW");
        assert_eq!(engine.font_system.locale(), "zh-TW");
        let _ = engine.set_locale("ja-JP");
        assert_eq!(engine.font_system.locale(), "ja");
        let _ = engine.set_locale("zh-CN");
        assert_eq!(engine.font_system.locale(), "zh-CN");
    }

    #[cfg(windows)]
    #[test]
    fn zh_cn_han_glyphs_stay_on_single_sc_family() {
        let mut engine = TextEngine::new();
        let sample = "头显字幕设置开关：测试中文简体字形不统一，他们说这里很好";

        for locale in ["zh-CN", "en-US"] {
            let _ = engine.set_locale(locale);
            for (weight, italic, mono) in [
                (400, false, false),
                (500, false, false),
                (600, false, false),
                (700, false, false),
                (400, true, false),
                (600, true, false),
                (600, false, true),
            ] {
                let style = TextStyle {
                    font_size: 24.0,
                    line_height: 32.0,
                    weight,
                    italic,
                    mono,
                    ..Default::default()
                };
                let attrs = engine.attrs_for(&style);
                engine
                    .scratch
                    .set_metrics(&mut engine.font_system, Metrics::new(24.0, 32.0));
                engine
                    .scratch
                    .set_wrap(&mut engine.font_system, Wrap::WordOrGlyph);
                engine
                    .scratch
                    .set_size(&mut engine.font_system, Some(800.0), None);
                engine
                    .scratch
                    .set_text(&mut engine.font_system, sample, &attrs, Shaping::Advanced);
                engine
                    .scratch
                    .shape_until_scroll(&mut engine.font_system, false);

                let families: Vec<(usize, String)> = engine
                    .scratch
                    .layout_runs()
                    .flat_map(|run| {
                        run.glyphs.iter().filter_map(move |g| {
                            let slice = &run.text[g.start..g.end];
                            if !slice.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)) {
                                return None;
                            }
                            let phys = g.physical((0.0, 0.0), 1.0);
                            Some((phys.cache_key.font_id, g.glyph_id))
                        })
                    })
                    .map(|(font_id, glyph_id)| {
                        assert_ne!(
                            glyph_id, 0,
                            "missing Han glyph for locale={locale}, weight={weight}, italic={italic}, mono={mono}"
                        );
                        let name = engine
                            .font_system
                            .db()
                            .face(font_id)
                            .and_then(|f| f.families.first())
                            .map(|(n, _)| n.clone())
                            .unwrap_or_default();
                        (glyph_id as usize, name)
                    })
                    .collect();

                assert!(!families.is_empty());
                for (_, fam) in &families {
                    assert_eq!(
                        fam, "Microsoft YaHei UI",
                        "expected Microsoft YaHei UI for locale={locale}, weight={weight}, italic={italic}, mono={mono}, got {fam}"
                    );
                }
            }
        }
    }
}
