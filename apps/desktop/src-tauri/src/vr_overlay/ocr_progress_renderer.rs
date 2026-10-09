use super::{
    ocr_progress::{ProgressView, Stage},
    renderer::Texture,
    text_raster::{fill_rounded_rect, Rect, TextMask},
};
use windows_sys::Win32::Graphics::Gdi::{DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE};

const WIDTH: u32 = 768;
const HEIGHT: u32 = 192;
// RGBA colors match styles/base.css and the SteamVR dashboard.
const PRIMARY: [u8; 4] = [116, 214, 255, 255];
const SUBTLE: [u8; 4] = [238, 242, 247, 255];
const ERROR: [u8; 4] = [201, 68, 68, 255];

pub(super) fn render(view: &ProgressView, animation: u8) -> Result<Texture, String> {
    let mut pixels = vec![0; (WIDTH * HEIGHT * 4) as usize];
    fill_rounded_rect(
        &mut pixels,
        WIDTH,
        HEIGHT,
        Rect::new(0, 0, 768, 192),
        20,
        [221, 226, 233, 255],
    );
    fill_rounded_rect(
        &mut pixels,
        WIDTH,
        HEIGHT,
        Rect::new(1, 1, 767, 191),
        19,
        [255; 4],
    );
    let mut mask = TextMask::new(WIDTH, HEIGHT, 26)?;
    text(
        &mut pixels,
        &mut mask,
        &format!("OCR · {}", view.status),
        Rect::new(24, 16, 744, 54),
        [29, 40, 52],
    )?;
    for (index, (label, stage)) in ["Prepare", "Recognize", "Translate", "Display"]
        .into_iter()
        .zip(view.stages)
        .enumerate()
    {
        let left = 24 + index as i32 * 182;
        let color = match stage {
            Stage::Active | Stage::Complete => PRIMARY,
            Stage::Failed => ERROR,
            _ => SUBTLE,
        };
        let marker = match stage {
            Stage::Complete => "✓",
            Stage::Active => "●",
            Stage::Failed => "!",
            Stage::Skipped => "–",
            Stage::Pending => "○",
        };
        text(
            &mut pixels,
            &mut mask,
            &format!("{marker} {label}"),
            Rect::new(left, 63, left + 174, 97),
            if stage == Stage::Failed {
                [201, 68, 68]
            } else {
                [90, 104, 120]
            },
        )?;
        let rect = Rect::new(left, 106, left + 172, 116);
        fill_rounded_rect(&mut pixels, WIDTH, HEIGHT, rect, 5, SUBTLE);
        if stage == Stage::Active {
            if let Some((done, total)) = view.translation_fraction.filter(|_| index == 2) {
                let width = (172 * done.min(total) / total.max(1)) as i32;
                if width > 0 {
                    fill_rounded_rect(
                        &mut pixels,
                        WIDTH,
                        HEIGHT,
                        Rect::new(left, 106, left + width, 116),
                        5,
                        color,
                    );
                }
            } else {
                let offset = i32::from(animation % 4) * 40;
                fill_rounded_rect(
                    &mut pixels,
                    WIDTH,
                    HEIGHT,
                    Rect::new(left + offset, 106, left + offset + 52, 116),
                    5,
                    color,
                );
            }
        } else if matches!(stage, Stage::Complete | Stage::Failed) {
            fill_rounded_rect(&mut pixels, WIDTH, HEIGHT, rect, 5, color);
        }
    }
    text(
        &mut pixels,
        &mut mask,
        &view.detail,
        Rect::new(24, 139, 744, 177),
        [90, 104, 120],
    )?;
    Ok(Texture {
        pixels,
        width: WIDTH,
        height: HEIGHT,
    })
}

fn text(
    pixels: &mut [u8],
    mask: &mut TextMask,
    value: &str,
    rect: Rect,
    color: [u8; 3],
) -> Result<(), String> {
    mask.draw(
        value,
        rect,
        DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        false,
    )?;
    for (pixel, glyph) in pixels
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(mask.pixels().as_chunks::<4>().0)
    {
        let coverage = u16::from(*glyph[..3].iter().max().unwrap());
        for channel in 0..3 {
            pixel[channel] = ((u16::from(color[channel]) * coverage
                + u16::from(pixel[channel]) * (255 - coverage))
                / 255) as u8;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{ocr_progress::Feedback, ocr_status::OcrState};
    use super::*;
    use std::time::Instant;
    use vrcs_core::ocr::{Phase, PipelineProgress};

    #[test]
    fn renders_actual_translation_fraction_and_terminal_error() {
        let now = Instant::now();
        let mut feedback = Feedback::new(1, false, now);
        feedback.apply(
            PipelineProgress {
                scan_id: 1,
                images_total: 2,
                images_done: 2,
                recognition_done: true,
                translation_started: true,
                translation_total: 8,
                translation_completed: 2,
                translation_total_final: true,
                ..Default::default()
            },
            now,
        );
        let view = feedback.view(now).unwrap();
        let texture = render(&view, 0).unwrap();
        assert_eq!((texture.width, texture.height), (768, 192));
        let pixel = |texture: &Texture, x: usize, y: usize| {
            texture.pixels[(y * 768 + x) * 4..(y * 768 + x) * 4 + 4].to_vec()
        };
        assert_eq!(pixel(&texture, 392, 111), vec![116, 214, 255, 255]);
        assert_eq!(pixel(&texture, 480, 111), vec![238, 242, 247, 255]);
        feedback.finish(OcrState::TimedOut, now);
        let failed = render(&feedback.view(now).unwrap(), 0).unwrap();
        assert_ne!(texture.pixels, failed.pixels);
    }

    #[test]
    fn exports_progress_previews_when_requested() {
        let Ok(directory) = std::env::var("VRCS_OCR_PROGRESS_PREVIEW_DIR") else {
            return;
        };
        let now = Instant::now();
        for (name, state, source_only) in [
            ("preparing", None, false),
            ("recognizing", Some(OcrState::Recognizing), false),
            ("translating", Some(OcrState::Translating), false),
            ("complete", Some(OcrState::Visible), false),
            ("source", Some(OcrState::SourceVisible), true),
            ("partial", Some(OcrState::PartialVisible), false),
            ("timeout", Some(OcrState::TimedOut), false),
            ("empty", Some(OcrState::NoText), false),
        ] {
            let mut feedback = Feedback::new(1, source_only, now);
            if matches!(state, Some(OcrState::Recognizing | OcrState::Translating)) {
                let translating = state == Some(OcrState::Translating);
                feedback.apply(
                    PipelineProgress {
                        scan_id: 1,
                        images_total: 2,
                        images_done: usize::from(translating),
                        recognition_phases: vec![Phase::Recognizing],
                        translation_started: translating,
                        translation_total: 8,
                        translation_completed: 3,
                        ..Default::default()
                    },
                    now,
                );
            } else if let Some(state) = state {
                let empty = state == OcrState::NoText;
                let timed_out = state == OcrState::TimedOut;
                feedback.apply(
                    PipelineProgress {
                        scan_id: 1,
                        images_total: 2,
                        images_done: 2,
                        recognition_done: true,
                        translation_started: !empty && !source_only,
                        translation_total: if empty || source_only { 0 } else { 8 },
                        translation_completed: if empty || source_only {
                            0
                        } else if timed_out {
                            3
                        } else {
                            8
                        },
                        translation_failed: usize::from(state == OcrState::PartialVisible),
                        translation_total_final: true,
                        ..Default::default()
                    },
                    now,
                );
                if !empty {
                    feedback.displayed();
                }
                feedback.finish(state, now);
            }
            let texture = render(&feedback.view(now).unwrap(), 1).unwrap();
            std::fs::write(
                std::path::Path::new(&directory).join(format!("{name}.png")),
                super::super::ocr_capture::encode_png(texture).unwrap(),
            )
            .unwrap();
        }
    }
}
