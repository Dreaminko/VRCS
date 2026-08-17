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
    Headset,
    Wrist,
}

pub fn content_hash(
    layout: Layout,
    content: &PresentationContent,
    font_size_px: u32,
    background_opacity: f32,
) -> u64 {
    let (width, height) = dimensions(layout);
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
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
    let (width, height) = dimensions(layout);

    #[cfg(windows)]
    let pixels = match (layout, content) {
        (Layout::Headset, PresentationContent::Headset(text)) => {
            windows_renderer::render_mask(text, width, height, font_size_px, background_opacity)?
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

fn dimensions(layout: Layout) -> (u32, u32) {
    match layout {
        Layout::Headset => (1024, 192),
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
        DeleteObject, DrawTextW, FillRect, SelectObject, SetBkMode, SetTextColor, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH,
        DIB_RGB_COLORS, DT_CALCRECT, DT_CENTER, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE,
        DT_VCENTER, FF_DONTCARE, FW_SEMIBOLD, OUT_DEFAULT_PRECIS, PROOF_QUALITY, TRANSPARENT,
    };

    pub fn render_mask(
        text: &str,
        width: u32,
        height: u32,
        font_size_px: u32,
        background_opacity: f32,
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
            let lines: Vec<&str> = text.splitn(2, '\n').collect();
            let slot_height = (content_bottom - content_top) / lines.len() as i32;
            let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
            let rendered_font_size = fit_font_size(
                dc,
                &face,
                &lines,
                font_size_px,
                width as i32 - padding * 2,
                slot_height,
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

            let flags = DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX;
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
                let mut wide: Vec<u16> = line.encode_utf16().collect();
                DrawTextW(dc, wide.as_mut_ptr(), wide.len() as i32, &mut rect, flags);
            }

            let mask = std::slice::from_raw_parts(bits.cast::<u8>(), (width * height * 4) as usize);
            let background_alpha = (background_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            let mut pixels = Vec::with_capacity(mask.len());
            for pixel in mask.chunks_exact(4) {
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
        lines: &[&str],
        maximum: u32,
        available_width: i32,
        available_height: i32,
    ) -> i32 {
        let mut low = 16;
        let mut high = maximum.max(16) as i32;
        let mut best = 16;

        while low <= high {
            let size = (low + high) / 2;
            let font = CreateFontW(
                -size,
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
                high = size - 1;
                continue;
            }
            let old_font = SelectObject(dc, font);
            let fits = lines.iter().all(|line| {
                let mut wide: Vec<u16> = line.encode_utf16().collect();
                let mut measured = RECT {
                    left: 0,
                    top: 0,
                    right: available_width,
                    bottom: 0,
                };
                DrawTextW(
                    dc,
                    wide.as_mut_ptr(),
                    wide.len() as i32,
                    &mut measured,
                    DT_SINGLELINE | DT_NOPREFIX | DT_CALCRECT,
                );
                measured.right <= available_width && measured.bottom <= available_height
            });
            SelectObject(dc, old_font);
            DeleteObject(font);

            if fits {
                best = size;
                low = size + 1;
            } else {
                high = size - 1;
            }
        }

        best
    }

    fn last_error(operation: &str) -> String {
        format!("{operation} failed: {}", std::io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_changes_with_text_or_style() {
        let hello = PresentationContent::Headset("hello".into());
        let world = PresentationContent::Headset("world".into());
        let first = render(Layout::Headset, &hello, 48, 0.5).unwrap();
        let first_hash = content_hash(Layout::Headset, &hello, 48, 0.5);
        assert_ne!(first_hash, content_hash(Layout::Headset, &world, 48, 0.5));
        assert_ne!(first_hash, content_hash(Layout::Headset, &hello, 54, 0.5));
        assert_eq!(
            first.pixels.len(),
            (first.width * first.height * 4) as usize
        );
    }
}
