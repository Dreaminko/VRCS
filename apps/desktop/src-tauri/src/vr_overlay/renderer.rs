#[cfg(not(windows))]
use std::hash::{DefaultHasher, Hash, Hasher};

#[cfg(not(windows))]
use super::presentation::PresentationContent;

#[derive(Debug, Clone)]
pub struct Texture {
    /// Row-major RGBA8 pixels. Upload backends convert to their native format.
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Native BGRA8 upload buffer, separate from RGBA capture and recognition images.
pub struct BgraTexture {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl BgraTexture {
    pub fn from_rgba(source: &Texture) -> Self {
        let mut pixels = source.pixels.clone();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        Self {
            pixels,
            width: source.width,
            height: source.height,
        }
    }

    pub fn into_rgba(mut self) -> Texture {
        for pixel in self.pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        Texture {
            pixels: self.pixels,
            width: self.width,
            height: self.height,
        }
    }
}

#[cfg(not(windows))]
#[derive(Debug, Clone, Copy)]
pub enum Layout {
    Headset { lines_per_language: u32 },
    Wrist,
}

#[cfg(not(windows))]
pub fn content_hash(
    layout: Layout,
    content: &PresentationContent,
    font_size_px: u32,
    background_opacity: f32,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    if let Layout::Headset { lines_per_language } = layout {
        lines_per_language.clamp(1, 4).hash(&mut hasher);
    }
    font_size_px.hash(&mut hasher);
    background_opacity.to_bits().hash(&mut hasher);
    hasher.finish()
}

#[cfg(not(windows))]
pub fn render(
    _layout: Layout,
    _content: &PresentationContent,
    _font_size_px: u32,
    _background_opacity: f32,
) -> Result<Texture, String> {
    Err("VR Overlay rendering is only supported on Windows".into())
}
