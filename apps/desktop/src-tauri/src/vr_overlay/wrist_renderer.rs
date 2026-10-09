use super::text_raster::{fill_rounded_rect, Rect, TextMask};
use windows_sys::Win32::Graphics::Gdi::{DT_NOPREFIX, DT_RIGHT, DT_WORDBREAK};

use super::presentation::{MessageSide, WristMessage};
use super::wrist_layout::visible_rows;

const PANEL_MARGIN: i32 = 16;
const TEXT_MARGIN: i32 = 38;
const TEXT_VERTICAL_MARGIN: i32 = 16;
const MESSAGE_PADDING_Y: i32 = 8;
const MESSAGE_GAP: i32 = 10;
const MIN_FONT_SIZE_PX: i32 = 16;

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

pub fn render(
    messages: &[WristMessage],
    width: u32,
    height: u32,
    font_size_px: u32,
    background_opacity: f32,
) -> Result<Vec<u8>, String> {
    let alpha = (background_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut pixels = vec![0; (width * height * 4) as usize];
    fill_rounded_rect(
        &mut pixels,
        width,
        height,
        Rect::new(
            PANEL_MARGIN,
            PANEL_MARGIN,
            width as i32 - PANEL_MARGIN,
            height as i32 - PANEL_MARGIN,
        ),
        26,
        [42, 42, 42, alpha],
    );

    if messages.is_empty() {
        return Ok(pixels);
    }

    let content = Rect::new(
        PANEL_MARGIN + TEXT_MARGIN,
        PANEL_MARGIN + TEXT_VERTICAL_MARGIN,
        width as i32 - PANEL_MARGIN - TEXT_MARGIN,
        height as i32 - PANEL_MARGIN - TEXT_VERTICAL_MARGIN,
    );
    let (mut text_mask, row_heights) = fit_text(
        messages,
        width,
        height,
        font_size_px as i32,
        content.right - content.left,
        content.bottom - content.top,
    )?;
    for row in visible_rows(&row_heights, content.top, content.bottom, MESSAGE_GAP) {
        let message = &messages[row.index];
        let text_rect = Rect::new(
            content.left,
            row.top + MESSAGE_PADDING_Y,
            content.right,
            row.bottom - MESSAGE_PADDING_Y,
        );
        text_mask.draw(&message.text, text_rect, text_flags(message.side), true)?;
        let visible_rect = Rect::new(
            text_rect.left,
            text_rect.top.max(content.top),
            text_rect.right,
            text_rect.bottom.min(content.bottom),
        );
        blend_text(&mut pixels, text_mask.pixels(), width, visible_rect);
    }

    Ok(pixels)
}

fn fit_text(
    messages: &[WristMessage],
    width: u32,
    height: u32,
    maximum_font_size: i32,
    text_width: i32,
    available_height: i32,
) -> Result<(TextMask, Vec<i32>), String> {
    let maximum_font_size = maximum_font_size.max(MIN_FONT_SIZE_PX);
    let mut low = MIN_FONT_SIZE_PX;
    let mut high = maximum_font_size;
    let mut best = None;

    while low <= high {
        let font_size = (low + high) / 2;
        let text_mask = TextMask::new(width, height, font_size)?;
        let row_heights = messages
            .iter()
            .map(|message| {
                text_mask.measure(&message.text, text_width, text_flags(message.side))
                    + MESSAGE_PADDING_Y * 2
            })
            .collect::<Vec<_>>();
        let required_height =
            row_heights.iter().sum::<i32>() + MESSAGE_GAP * messages.len().saturating_sub(1) as i32;

        if required_height <= available_height {
            best = Some((text_mask, row_heights));
            low = font_size + 1;
        } else {
            high = font_size - 1;
        }
    }

    if let Some(layout) = best {
        return Ok(layout);
    }

    let text_mask = TextMask::new(width, height, MIN_FONT_SIZE_PX)?;
    let row_heights = messages
        .iter()
        .map(|message| {
            text_mask.measure(&message.text, text_width, text_flags(message.side))
                + MESSAGE_PADDING_Y * 2
        })
        .collect();
    Ok((text_mask, row_heights))
}

fn text_flags(side: MessageSide) -> u32 {
    let alignment = match side {
        MessageSide::Left => 0,
        MessageSide::Right => DT_RIGHT,
    };
    DT_WORDBREAK | DT_NOPREFIX | alignment
}

fn blend_text(pixels: &mut [u8], mask: &[u8], width: u32, rect: Rect) {
    let height = pixels.len() as u32 / width / 4;
    for y in rect.top.max(0)..rect.bottom.min(height as i32) {
        for x in rect.left.max(0)..rect.right.min(width as i32) {
            let offset = ((y as u32 * width + x as u32) * 4) as usize;
            let coverage = mask[offset].max(mask[offset + 1]).max(mask[offset + 2]);
            if coverage == 0 {
                continue;
            }
            for channel in &mut pixels[offset..offset + 3] {
                *channel =
                    channel.saturating_add(((255 - *channel) as u16 * coverage as u16 / 255) as u8);
            }
            pixels[offset + 3] = pixels[offset + 3].max(coverage);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_messages_are_drawn_after_an_oversized_translation() {
        let mut messages = vec![
            WristMessage {
                text: "日本語の原文\n很长的翻译 older translated line\n".repeat(150),
                side: MessageSide::Left,
            },
            WristMessage {
                text: "LATEST message A".into(),
                side: MessageSide::Right,
            },
        ];
        let first = render(&messages, 768, 768, 36, 0.5).unwrap();
        messages[1].text = "LATEST message B".into();
        let next = render(&messages, 768, 768, 36, 0.5).unwrap();
        assert_ne!(first, next);
    }

    #[test]
    fn a_growing_translation_keeps_its_latest_text_visible() {
        let mut messages = vec![WristMessage {
            text: format!(
                "{}LATEST translation A",
                "older translated line\n".repeat(150)
            ),
            side: MessageSide::Left,
        }];
        let first = render(&messages, 768, 768, 36, 0.5).unwrap();
        messages[0].text = format!(
            "{}LATEST translation B",
            "older translated line\n".repeat(150)
        );
        let next = render(&messages, 768, 768, 36, 0.5).unwrap();
        assert_ne!(first, next);
    }

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
