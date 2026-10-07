use windows_sys::Win32::Graphics::Gdi::{
    DT_CENTER, DT_END_ELLIPSIS, DT_EXPANDTABS, DT_NOPREFIX, DT_SINGLELINE,
};

use super::ocr_wrist::{Action, View};
use super::ocr_wrist_layout::Pages;
use super::renderer::Texture;
use super::text_raster::{fill_rounded_rect, Rect, TextMask};

const SIZE: u32 = 1024;
const TEXT_FLAGS: u32 = DT_SINGLELINE | DT_NOPREFIX | DT_EXPANDTABS;
const SOURCE_CARD: Rect = Rect::new(48, 164, 976, 488);
const TRANSLATION_CARD: Rect = Rect::new(48, 504, 976, 828);
const FULL_CARD: Rect = Rect::new(48, 164, 976, 828);

pub struct Button {
    pub action: Action,
    pub bounds: [f32; 4],
}

pub struct Rendered {
    pub texture: Texture,
    pub page_count: usize,
    pub buttons: Vec<Button>,
}

pub fn render(view: &View, font_size: u32, background_opacity: f32) -> Result<Rendered, String> {
    let alpha = (background_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut pixels = vec![0; (SIZE * SIZE * 4) as usize];
    fill_rounded_rect(
        &mut pixels,
        SIZE,
        SIZE,
        Rect::new(32, 32, 992, 992),
        24,
        [22, 24, 28, alpha],
    );
    let mut body = TextMask::new(SIZE, SIZE, font_size.max(1) as i32)?;
    let mut labels = TextMask::new(SIZE, SIZE, 24)?;
    let line_height = (body.line_height() * 135 + 99) / 100;
    let source_rect = if view.source_only {
        FULL_CARD
    } else {
        SOURCE_CARD
    };
    let text_rect = |card: Rect| {
        Rect::new(
            card.left + 24,
            card.top + 60,
            card.right - 24,
            card.bottom - 24,
        )
    };
    let source_content = text_rect(source_rect);
    let source = Pages::new(
        &view.source,
        source_content.right - source_content.left,
        source_content.bottom - source_content.top,
        line_height,
        |text| body.measure_size(text, 32767, TEXT_FLAGS).0,
    );
    let translated_content = text_rect(TRANSLATION_CARD);
    let translation = Pages::new(
        &view.translation,
        translated_content.right - translated_content.left,
        translated_content.bottom - translated_content.top,
        line_height,
        |text| body.measure_size(text, 32767, TEXT_FLAGS).0,
    );
    let page_count = source.lines.len().max(if view.source_only {
        1
    } else {
        translation.lines.len()
    });
    let page = view.page.min(page_count - 1);

    draw_text(
        &mut pixels,
        &mut labels,
        &view.title,
        Rect::new(56, 56, 700, 92),
        [255; 3],
        false,
    )?;
    if view.block_count > 0 {
        draw_text(
            &mut pixels,
            &mut labels,
            &format!("{} / {}", view.block_index + 1, view.block_count),
            Rect::new(744, 56, 968, 92),
            [200, 210, 220],
            true,
        )?;
        draw_text(
            &mut pixels,
            &mut labels,
            &view.status,
            Rect::new(56, 108, 968, 140),
            [180, 195, 210],
            false,
        )?;
    } else {
        draw_text(
            &mut pixels,
            &mut body,
            &view.status,
            FULL_CARD,
            [210, 217, 225],
            true,
        )?;
    }

    for (card, content, pages, label, color) in [
        (
            source_rect,
            source_content,
            &source,
            "Source",
            [210, 217, 225],
        ),
        (
            TRANSLATION_CARD,
            translated_content,
            &translation,
            view.translation_label.as_str(),
            [255; 3],
        ),
    ]
    .into_iter()
    .take(if view.block_count == 0 {
        0
    } else if view.source_only {
        1
    } else {
        2
    }) {
        fill_rounded_rect(&mut pixels, SIZE, SIZE, card, 16, [34, 38, 44, alpha]);
        draw_text(
            &mut pixels,
            &mut labels,
            label,
            Rect::new(
                card.left + 24,
                card.top + 16,
                card.right - 120,
                card.top + 48,
            ),
            [140, 193, 225],
            false,
        )?;
        let (lines, ended) = pages.at(page);
        if ended {
            draw_text(
                &mut pixels,
                &mut labels,
                "End",
                Rect::new(
                    card.right - 100,
                    card.top + 16,
                    card.right - 24,
                    card.top + 48,
                ),
                [180, 195, 210],
                true,
            )?;
        }
        for (index, line) in lines.iter().enumerate() {
            let top = content.top + index as i32 * line_height;
            draw_text(
                &mut pixels,
                &mut body,
                line,
                Rect::new(content.left, top, content.right, top + line_height),
                color,
                false,
            )?;
        }
    }

    let mut buttons = Vec::new();
    let controls = [
        (
            Action::PreviousBlock,
            "Prev block",
            view.block_count > 1 && view.block_index > 0,
            Rect::new(48, 848, 262, 910),
        ),
        (
            Action::NextBlock,
            "Next block",
            view.block_count > 1 && view.block_index + 1 < view.block_count,
            Rect::new(278, 848, 492, 910),
        ),
        (
            Action::PreviousPage,
            "Prev page",
            page > 0,
            Rect::new(508, 848, 722, 910),
        ),
        (
            Action::NextPage,
            "Next page",
            page + 1 < page_count,
            Rect::new(738, 848, 976, 910),
        ),
        (
            Action::NextLanguage,
            "Language",
            !view.source_only && view.language_count > 1,
            Rect::new(48, 924, 288, 980),
        ),
        (
            Action::TogglePin,
            if view.pinned { "Unpin" } else { "Pin" },
            true,
            Rect::new(544, 924, 748, 980),
        ),
        (Action::Close, "Close", true, Rect::new(764, 924, 976, 980)),
    ];
    for (action, label, enabled, rect) in controls {
        if view.block_count == 0 && !matches!(action, Action::TogglePin | Action::Close) {
            continue;
        }
        let hovered = enabled && view.hovered == Some(action);
        let color = if hovered {
            [82, 133, 157, alpha]
        } else if enabled {
            [49, 61, 73, alpha]
        } else {
            [30, 34, 40, alpha]
        };
        fill_rounded_rect(&mut pixels, SIZE, SIZE, rect, 10, color);
        draw_text(
            &mut pixels,
            &mut labels,
            label,
            rect,
            if enabled { [255; 3] } else { [96, 108, 120] },
            true,
        )?;
        if enabled {
            buttons.push(Button {
                action,
                bounds: [
                    rect.left as f32 / SIZE as f32,
                    rect.top as f32 / SIZE as f32,
                    rect.right as f32 / SIZE as f32,
                    rect.bottom as f32 / SIZE as f32,
                ],
            });
        }
    }
    if view.block_count > 0 {
        draw_text(
            &mut pixels,
            &mut labels,
            &format!("{} / {}", page + 1, page_count),
            Rect::new(304, 924, 528, 980),
            [200, 210, 220],
            true,
        )?;
    }
    Ok(Rendered {
        texture: Texture {
            pixels,
            width: SIZE,
            height: SIZE,
        },
        page_count,
        buttons,
    })
}

fn draw_text(
    pixels: &mut [u8],
    mask: &mut TextMask,
    text: &str,
    rect: Rect,
    color: [u8; 3],
    center: bool,
) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    mask.draw(
        text,
        rect,
        TEXT_FLAGS
            | if center {
                DT_CENTER | DT_END_ELLIPSIS
            } else {
                DT_END_ELLIPSIS
            },
        center,
    )?;
    for y in rect.top.max(0)..rect.bottom.min(SIZE as i32) {
        for x in rect.left.max(0)..rect.right.min(SIZE as i32) {
            let offset = ((y as u32 * SIZE + x as u32) * 4) as usize;
            let coverage = mask.pixels()[offset..offset + 3]
                .iter()
                .copied()
                .max()
                .unwrap_or(0) as u16;
            if coverage == 0 {
                continue;
            }
            for channel in 0..3 {
                pixels[offset + channel] = ((color[channel] as u16 * coverage
                    + pixels[offset + channel] as u16 * (255 - coverage))
                    / 255) as u8;
            }
            pixels[offset + 3] = pixels[offset + 3]
                .saturating_add(((255 - pixels[offset + 3]) as u16 * coverage / 255) as u8);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view() -> View {
        View {
            source: "日本語の原文 English words 中文原文\n".repeat(30),
            translation: "Translated sentence for comparison.\n".repeat(30),
            translation_label: "English".into(),
            title: "OCR".into(),
            status: "Ready".into(),
            block_index: 0,
            block_count: 2,
            page: 0,
            pinned: false,
            source_only: false,
            language_count: 2,
            hovered: None,
        }
    }
    #[test]
    fn empty_results_show_status_without_result_cards_or_reading_controls() {
        let view = super::super::ocr_wrist::Reader::default().view(
            super::super::ocr_status::OcrState::NoText,
            false,
            false,
        );
        let rendered = render(&view, 32, 1.).unwrap();
        assert_eq!(rendered.buttons.len(), 2);
        assert!(rendered
            .buttons
            .iter()
            .all(|button| { matches!(button.action, Action::TogglePin | Action::Close) }));
        for (x, y) in [(60, 170), (60, 510), (750, 60), (50, 850)] {
            let offset = (y * SIZE as usize + x) * 4;
            assert_eq!(
                &rendered.texture.pixels[offset..offset + 4],
                &[22, 24, 28, 255]
            );
        }
    }

    #[test]
    fn renders_bilingual_pages_with_enabled_navigation_only() {
        let mut view = view();
        let first = render(&view, 32, 0.94).unwrap();
        assert_eq!((first.texture.width, first.texture.height), (1024, 1024));
        assert!(first.page_count > 1);
        assert!(!first
            .buttons
            .iter()
            .any(|b| b.action == Action::PreviousPage || b.action == Action::PreviousBlock));
        assert!(first.buttons.iter().any(|b| b.action == Action::NextPage));
        if let Ok(path) = std::env::var("VRCS_OCR_PREVIEW_PATH") {
            std::fs::write(
                path,
                super::super::ocr_capture::encode_png(&first.texture).unwrap(),
            )
            .unwrap();
        }
        view.page = first.page_count - 1;
        let last = render(&view, 32, 0.94).unwrap();
        assert_ne!(first.texture.pixels, last.texture.pixels);
        assert!(!last.buttons.iter().any(|b| b.action == Action::NextPage));
    }
    #[test]
    fn original_only_uses_full_card_and_hover_is_visible() {
        let mut view = view();
        let bilingual = render(&view, 32, 0.94).unwrap();
        view.source_only = true;
        let original = render(&view, 32, 0.94).unwrap();
        assert!(original.page_count < bilingual.page_count);
        assert!(!original
            .buttons
            .iter()
            .any(|b| b.action == Action::NextLanguage));
        view.hovered = Some(Action::Close);
        assert_ne!(
            original.texture.pixels,
            render(&view, 32, 0.94).unwrap().texture.pixels
        );
    }
}
