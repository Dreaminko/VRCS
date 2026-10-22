use super::text_raster::{Rect, TextMask};
use windows_sys::Win32::Graphics::Gdi::{DT_NOPREFIX, DT_RIGHT, DT_WORDBREAK};

use super::presentation::MessageSide;
pub(super) fn compact_text_box_size(
    text: &str,
    max_width: u32,
    max_height: u32,
) -> Result<Option<(u32, u32)>, String> {
    if max_width < 12 || max_height < 12 || max_width > 960 || max_height > 720 {
        return Ok(None);
    }
    for font_size in [24, 18, 12] {
        let mask = TextMask::new(1, 1, font_size)?;
        let (width, height) =
            mask.measure_size(text, max_width as i32 - 8, text_flags(MessageSide::Left));
        if width > 0
            && height > 0
            && width + 8 <= max_width as i32
            && height + 8 <= max_height as i32
        {
            return Ok(Some((
                (width as u32 + 8).max(12),
                (height as u32 + 8).max(12),
            )));
        }
    }
    Ok(None)
}

pub(super) fn render_text_box(
    text: &str,
    width: u32,
    height: u32,
    background_opacity: f32,
) -> Result<Option<Vec<u8>>, String> {
    render_text_box_with_colors(text, width, height, background_opacity, [0; 3], [255; 3])
}

pub(super) fn render_text_box_with_colors(
    text: &str,
    width: u32,
    height: u32,
    background_opacity: f32,
    background: [u8; 3],
    foreground: [u8; 3],
) -> Result<Option<Vec<u8>>, String> {
    let padding = (width.min(height) as i32 / 12).clamp(1, 4);
    if width < 12 || height < 12 || width > 4096 || height > 4096 {
        return Ok(None);
    }
    let available_width = width as i32 - padding * 2;
    let available_height = height as i32 - padding * 2;
    let mut low = 12;
    let mut high = (height as i32 - padding * 2).clamp(12, 48);
    let mut best = None;
    while low <= high {
        let size = (low + high) / 2;
        let candidate = TextMask::new(width, height, size)?;
        let (text_width, text_height) =
            candidate.measure_size(text, available_width, text_flags(MessageSide::Left));
        if text_width <= available_width && text_height <= available_height {
            best = Some(candidate);
            low = size + 1;
        } else {
            high = size - 1;
        }
    }
    let Some(mut best) = best else {
        return Ok(None);
    };
    let rect = Rect::new(
        padding,
        padding,
        width as i32 - padding,
        height as i32 - padding,
    );
    best.draw(text, rect, text_flags(MessageSide::Left), true)?;
    let alpha = (background_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    Ok(Some(
        best.pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|pixel| {
                let coverage = pixel[0].max(pixel[1]).max(pixel[2]);
                // Text remains opaque; only the source-covering patch uses the configured opacity.
                let color: [u8; 3] = std::array::from_fn(|channel| {
                    ((foreground[channel] as u32 * coverage as u32
                        + background[channel] as u32 * alpha as u32 * (255 - coverage) as u32
                            / 255)
                        / 255) as u8
                });
                [
                    color[0],
                    color[1],
                    color[2],
                    alpha.saturating_add(((255 - alpha) as u16 * coverage as u16 / 255) as u8),
                ]
            })
            .collect(),
    ))
}

fn text_flags(side: MessageSide) -> u32 {
    let alignment = match side {
        MessageSide::Left => 0,
        MessageSide::Right => DT_RIGHT,
    };
    DT_WORDBREAK | DT_NOPREFIX | alignment
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_ocr_lines_fit_without_reducing_the_minimum_font_size() {
        let mask = TextMask::new(1, 1, 12).unwrap();
        let (text_width, text_height) = mask.measure_size("VR", 100, text_flags(MessageSide::Left));
        let pixels =
            render_text_box("VR", (text_width + 4) as u32, (text_height + 4) as u32, 0.6).unwrap();
        assert!(
            pixels.is_some(),
            "small OCR boxes must not lose eight pixels to fixed padding"
        );
    }

    #[test]
    fn ocr_text_boxes_reject_overflow_at_the_minimum_font_size() {
        assert!(render_text_box("VR", 80, 50, 0.6).unwrap().is_some());
        assert!(
            render_text_box(&"Long translation sentence. ".repeat(12), 80, 30, 0.6)
                .unwrap()
                .is_none()
        );
        assert!(render_text_box(&"W".repeat(100), 80, 80, 0.6)
            .unwrap()
            .is_none());
    }
}
