use std::hash::{DefaultHasher, Hash, Hasher};

use super::presentation::PresentationContent;

#[derive(Debug, Clone)]
pub struct Texture {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum Layout {
    Headset { lines_per_language: u32 },
    Wrist,
}

pub fn content_hash(
    layout: Layout,
    content: &PresentationContent,
    font_size_px: u32,
    background_opacity: f32,
) -> u64 {
    let (width, height) = dimensions(layout, content);
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    if let Layout::Headset { lines_per_language } = layout {
        lines_per_language.clamp(1, 4).hash(&mut hasher);
    }
    font_size_px.hash(&mut hasher);
    background_opacity.to_bits().hash(&mut hasher);
    (width, height).hash(&mut hasher);
    hasher.finish()
}

pub fn render(
    layout: Layout,
    content: &PresentationContent,
    font_size_px: u32,
    background_opacity: f32,
) -> Result<Texture, String> {
    let (width, height) = dimensions(layout, content);

    #[cfg(windows)]
    let pixels = match (layout, content) {
        (Layout::Headset { lines_per_language }, PresentationContent::Headset(text)) => {
            windows_renderer::render_mask(
                text,
                width,
                height,
                font_size_px,
                background_opacity,
                lines_per_language.clamp(1, 4),
            )?
        }
        (Layout::Wrist, PresentationContent::Wrist(messages)) => super::wrist_renderer::render(
            messages,
            width,
            height,
            font_size_px,
            background_opacity,
        )?,
        _ => return Err("VR Overlay content does not match its layout".into()),
    };
    #[cfg(not(windows))]
    let pixels = {
        let _ = (layout, content, font_size_px, background_opacity);
        return Err("VR Overlay rendering is only supported on Windows".into());
    };

    Ok(Texture {
        pixels,
        width,
        height,
    })
}

fn dimensions(layout: Layout, content: &PresentationContent) -> (u32, u32) {
    match layout {
        Layout::Headset { lines_per_language } => {
            let languages = match content {
                PresentationContent::Headset(text) => text.splitn(4, '\n').count() as u32,
                _ => 1,
            };
            (1024, 28 + 82 * lines_per_language.clamp(1, 4) * languages)
        }
        Layout::Wrist => (768, 768),
    }
}

#[cfg(windows)]
mod windows_renderer {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC,
        DeleteObject, DrawTextW, FillRect, GetTextMetricsW, SelectObject, SetBkMode, SetTextColor,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH,
        DIB_RGB_COLORS, DT_CALCRECT, DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
        DT_WORDBREAK, FF_DONTCARE, FW_SEMIBOLD, OUT_DEFAULT_PRECIS, PROOF_QUALITY, TEXTMETRICW,
        TRANSPARENT,
    };

    use super::super::headset_layout;

    pub fn render_mask(
        text: &str,
        width: u32,
        height: u32,
        font_size_px: u32,
        background_opacity: f32,
        lines_per_language: u32,
    ) -> Result<Vec<u8>, String> {
        unsafe {
            let dc = CreateCompatibleDC(null_mut());
            if dc.is_null() {
                return Err(last_error("CreateCompatibleDC"));
            }

            let mut info: BITMAPINFO = zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..zeroed()
            };
            let mut bits: *mut c_void = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() || bits.is_null() {
                DeleteDC(dc);
                return Err(last_error("CreateDIBSection"));
            }
            let old_bitmap = SelectObject(dc, bitmap);
            let black = CreateSolidBrush(0);
            let full = RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            FillRect(dc, &full, black);
            DeleteObject(black);

            let padding = 28;
            let content_top = padding / 2;
            let content_bottom = height as i32 - padding / 2;
            let lines: Vec<&str> = text.splitn(4, '\n').collect();
            let slot_height = (content_bottom - content_top) / lines.len() as i32;
            let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
            let rendered_font_size = fit_font_size(
                dc,
                &face,
                font_size_px,
                slot_height / lines_per_language as i32,
            );
            let font = CreateFontW(
                -rendered_font_size,
                0,
                0,
                0,
                FW_SEMIBOLD as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET.into(),
                OUT_DEFAULT_PRECIS.into(),
                CLIP_DEFAULT_PRECIS.into(),
                PROOF_QUALITY.into(),
                (DEFAULT_PITCH | FF_DONTCARE).into(),
                face.as_ptr(),
            );
            if font.is_null() {
                SelectObject(dc, old_bitmap);
                DeleteObject(bitmap);
                DeleteDC(dc);
                return Err(last_error("CreateFontW"));
            }
            let old_font = SelectObject(dc, font);
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, 0x00ff_ffff);
            let mut metrics: TEXTMETRICW = zeroed();
            GetTextMetricsW(dc, &mut metrics);
            let line_height = metrics.tmHeight.max(rendered_font_size);

            for (index, line) in lines.iter().enumerate() {
                let mut rect = RECT {
                    left: padding,
                    top: content_top + index as i32 * slot_height,
                    right: width as i32 - padding,
                    bottom: if index + 1 == lines.len() {
                        content_bottom
                    } else {
                        content_top + (index as i32 + 1) * slot_height
                    },
                };
                // Each language has its own bounded, wrapping caption window.
                let flags = DT_CENTER
                    | DT_NOPREFIX
                    | if lines_per_language > 1 {
                        DT_WORDBREAK
                    } else {
                        DT_SINGLELINE
                    };
                let available_width = rect.right - rect.left;
                let available_height =
                    (rect.bottom - rect.top).min(line_height * lines_per_language as i32);
                let fits = |candidate: &str| {
                    let measured = measure_text(dc, candidate, available_width, flags);
                    measured.right <= available_width && measured.bottom <= available_height
                };
                let visible = if index == 0 {
                    headset_layout::visible_caption(line, fits)
                } else {
                    headset_layout::visible_tail(line, fits)
                };
                let mut wide: Vec<u16> = visible.encode_utf16().collect();
                if lines_per_language > 1 {
                    let measured = measure_text(dc, &visible, available_width, flags);
                    rect.top += ((rect.bottom - rect.top - measured.bottom) / 2).max(0);
                }
                DrawTextW(
                    dc,
                    wide.as_mut_ptr(),
                    wide.len() as i32,
                    &mut rect,
                    flags
                        | if lines_per_language > 1 {
                            0
                        } else {
                            DT_VCENTER
                        },
                );
            }

            let mask = std::slice::from_raw_parts(bits.cast::<u8>(), (width * height * 4) as usize);
            let background_alpha = (background_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            let mut pixels = Vec::with_capacity(mask.len());
            for pixel in mask.as_chunks::<4>().0 {
                let coverage = pixel[0].max(pixel[1]).max(pixel[2]);
                let alpha = background_alpha.saturating_add(
                    ((255 - background_alpha) as u16 * coverage as u16 / 255) as u8,
                );
                let channel = coverage;
                pixels.extend_from_slice(&[channel, channel, channel, alpha]);
            }

            SelectObject(dc, old_font);
            SelectObject(dc, old_bitmap);
            DeleteObject(font);
            DeleteObject(bitmap);
            DeleteDC(dc);
            Ok(pixels)
        }
    }

    unsafe fn fit_font_size(
        dc: *mut c_void,
        face: &[u16],
        maximum: u32,
        available_height: i32,
    ) -> i32 {
        headset_layout::font_size(maximum, available_height, |size| {
            let font = CreateFontW(
                -(size as i32),
                0,
                0,
                0,
                FW_SEMIBOLD as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET.into(),
                OUT_DEFAULT_PRECIS.into(),
                CLIP_DEFAULT_PRECIS.into(),
                PROOF_QUALITY.into(),
                (DEFAULT_PITCH | FF_DONTCARE).into(),
                face.as_ptr(),
            );
            if font.is_null() {
                return None;
            }
            let old_font = SelectObject(dc, font);
            let mut metrics: TEXTMETRICW = zeroed();
            let measured = (GetTextMetricsW(dc, &mut metrics) != 0).then_some(metrics.tmHeight);
            SelectObject(dc, old_font);
            DeleteObject(font);
            measured
        }) as i32
    }

    unsafe fn measure_text(dc: *mut c_void, text: &str, width: i32, flags: u32) -> RECT {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut measured = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: 0,
        };
        DrawTextW(
            dc,
            wide.as_mut_ptr(),
            wide.len() as i32,
            &mut measured,
            flags | DT_CALCRECT,
        );
        measured
    }

    fn last_error(operation: &str) -> String {
        format!("{operation} failed: {}", std::io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_LINE: Layout = Layout::Headset {
        lines_per_language: 1,
    };
    const TWO_LINES: Layout = Layout::Headset {
        lines_per_language: 2,
    };

    #[test]
    fn line_limits_change_geometry_and_hash_without_dependence_on_text_length() {
        let short = PresentationContent::Headset("hello\n你好".into());
        let long = PresentationContent::Headset(format!("{0}\n{0}", "很长的字幕".repeat(100)));
        for lines_per_language in 1..=4 {
            let layout = Layout::Headset { lines_per_language };
            assert_eq!(dimensions(layout, &short), dimensions(layout, &long));
            assert_eq!(
                dimensions(layout, &short),
                (1024, 28 + 164 * lines_per_language)
            );
        }
        assert_ne!(
            content_hash(ONE_LINE, &short, 54, 0.5),
            content_hash(TWO_LINES, &short, 54, 0.5)
        );
        let multilingual = PresentationContent::Headset("source\n中文\n日本語\nEnglish".into());
        assert_eq!(dimensions(TWO_LINES, &multilingual), (1024, 684));
        assert_eq!(
            dimensions(
                Layout::Headset {
                    lines_per_language: u32::MAX
                },
                &short
            ),
            dimensions(
                Layout::Headset {
                    lines_per_language: 4
                },
                &short
            )
        );
    }

    #[cfg(windows)]
    #[test]
    fn configured_bilingual_line_limits_wrap_both_languages_at_a_stable_font_size() {
        let content = PresentationContent::Headset(format!("{0}\n{0}", "国".repeat(200)));
        let mut glyph_height = None;
        for lines_per_language in 1..=4 {
            let texture =
                render(Layout::Headset { lines_per_language }, &content, 54, 0.5).unwrap();
            for (from, to) in [
                (0, texture.height / 2),
                (texture.height / 2, texture.height),
            ] {
                let mut bands = Vec::new();
                let mut started = None;
                for y in from..to {
                    let row = (y * texture.width * 4) as usize;
                    let has_ink = texture.pixels[row..row + (texture.width * 4) as usize]
                        .chunks_exact(4)
                        .any(|pixel| pixel[0] != 0);
                    if has_ink {
                        started.get_or_insert(y);
                    } else if let Some(start) = started.take() {
                        bands.push(y - start);
                    }
                }
                assert_eq!(bands.len(), lines_per_language as usize);
                for height in bands {
                    assert!(height > 30);
                    assert_eq!(height, *glyph_height.get_or_insert(height));
                }
            }
        }
    }

    #[test]
    fn content_hash_changes_with_text_or_style() {
        let hello = PresentationContent::Headset("hello".into());
        let world = PresentationContent::Headset("world".into());
        let first_hash = content_hash(ONE_LINE, &hello, 48, 0.5);
        assert_ne!(first_hash, content_hash(ONE_LINE, &world, 48, 0.5));
        assert_ne!(first_hash, content_hash(ONE_LINE, &hello, 54, 0.5));
    }

    #[cfg(windows)]
    #[test]
    fn rendered_mask_matches_texture_dimensions() {
        let content = PresentationContent::Headset("hello".into());
        let texture = render(ONE_LINE, &content, 48, 0.5).unwrap();
        assert_eq!(
            texture.pixels.len(),
            (texture.width * texture.height * 4) as usize
        );
    }

    #[cfg(windows)]
    #[test]
    fn overflowing_headset_lines_follow_new_text() {
        let prefix = "前面的文字很长已经超过字幕区域".repeat(40);
        let first =
            PresentationContent::Headset(format!("{prefix}旧的结尾 OLD 123\n{prefix}old ending"));
        let next = PresentationContent::Headset(format!(
            "{prefix}新的内容继续出现 NEW 456\n{prefix}new words keep arriving"
        ));
        let first = render(ONE_LINE, &first, 54, 0.5).unwrap();
        let next = render(ONE_LINE, &next, 54, 0.5).unwrap();
        let middle = (first.width * first.height / 2 * 4) as usize;
        assert_ne!(&first.pixels[..middle], &next.pixels[..middle]);
        assert_ne!(&first.pixels[middle..], &next.pixels[middle..]);
    }

    #[cfg(windows)]
    #[test]
    fn overflowing_headset_lines_keep_the_tail_in_view() {
        let suffix = "最新文字必须留在画面中 The newest words stay visible".repeat(10);
        let first = PresentationContent::Headset(format!("{}{suffix}", "旧内容".repeat(100)));
        let next = PresentationContent::Headset(format!("{}{suffix}", "不同的旧内容".repeat(150)));
        let first = render(TWO_LINES, &first, 54, 0.5).unwrap();
        let next = render(TWO_LINES, &next, 54, 0.5).unwrap();
        assert_eq!(first.pixels, next.pixels);
    }

    #[cfg(windows)]
    #[test]
    fn growing_bilingual_captions_keep_the_same_glyph_height() {
        let short = PresentationContent::Headset("国\n国".into());
        let long = PresentationContent::Headset(format!("{0}\n{0}", "国".repeat(80)));
        let short = render(ONE_LINE, &short, 54, 0.5).unwrap();
        let long = render(ONE_LINE, &long, 54, 0.5).unwrap();
        let ink_rows = |texture: &Texture, from: u32, to: u32| {
            // Inspect the final glyphs so the leading overflow marker does not
            // affect the measured ink height.
            let right = (0..texture.width)
                .rev()
                .find(|&x| {
                    (from..to).any(|y| texture.pixels[((y * texture.width + x) * 4) as usize] != 0)
                })
                .unwrap();
            let left = right.saturating_sub(128);
            (from..to)
                .filter(|&y| {
                    let row = ((y * texture.width + left) * 4) as usize;
                    texture.pixels[row..row + ((right - left + 1) * 4) as usize]
                        .chunks_exact(4)
                        .any(|pixel| pixel[0] != 0)
                })
                .collect::<Vec<_>>()
        };
        for (from, to) in [(0, short.height / 2), (short.height / 2, short.height)] {
            let short_rows = ink_rows(&short, from, to);
            let long_rows = ink_rows(&long, from, to);
            assert!(!short_rows.is_empty());
            assert_eq!(short_rows.first(), long_rows.first());
            assert_eq!(short_rows.last(), long_rows.last());
        }
    }

    #[cfg(windows)]
    #[test]
    fn monolingual_captions_use_two_lines_without_shrinking() {
        let content = PresentationContent::Headset("国".repeat(80));
        let texture = render(TWO_LINES, &content, 54, 0.5).unwrap();
        let mut bands = Vec::new();
        let mut ink_started = None;
        for y in 0..texture.height {
            let row = (y * texture.width * 4) as usize;
            let has_ink = texture.pixels[row..row + (texture.width * 4) as usize]
                .chunks_exact(4)
                .any(|pixel| pixel[0] != 0);
            if has_ink && ink_started.is_none() {
                ink_started = Some(y);
            } else if !has_ink {
                if let Some(start) = ink_started.take() {
                    bands.push(y - start);
                }
            }
        }
        assert_eq!(bands.len(), 2);
        assert!(bands.iter().all(|&height| height > 30));
    }

    #[cfg(windows)]
    #[test]
    fn overflowing_captions_still_render_different_speaker_numbers() {
        let source = format!("{}最新字幕清晰可读", "旧字幕内容".repeat(100));
        for bilingual in [false, true] {
            let translation = if bilingual {
                format!("\n{}", "Translation ".repeat(40))
            } else {
                String::new()
            };
            let first = PresentationContent::Headset(format!("[1] {source}{translation}"));
            let next = PresentationContent::Headset(format!("[2] {source}{translation}"));
            let first = render(ONE_LINE, &first, 54, 0.5).unwrap();
            let next = render(ONE_LINE, &next, 54, 0.5).unwrap();
            assert_ne!(first.pixels, next.pixels);
            if bilingual {
                let middle = (first.width * first.height / 2 * 4) as usize;
                assert_eq!(&first.pixels[middle..], &next.pixels[middle..]);
            }
        }
    }
}
