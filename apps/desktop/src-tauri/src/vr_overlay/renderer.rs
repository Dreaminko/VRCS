#[derive(Debug, Clone)]
pub struct Texture {
    /// Row-major RGBA8 pixels for capture and recognition.
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[cfg(windows)]
pub fn dashboard_placeholder(message: &str) -> Result<Texture, String> {
    use std::{mem::zeroed, ptr::null_mut};
    use windows_sys::Win32::{Foundation::RECT, Graphics::Gdi::*};

    let width = super::dashboard::DASHBOARD_WIDTH / 2;
    let height = super::dashboard::DASHBOARD_HEIGHT / 2;
    let text: Vec<u16> = message.encode_utf16().collect();
    unsafe {
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return Err("Create dashboard placeholder DC failed".into());
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..zeroed()
            },
            ..zeroed()
        };
        let mut pixels = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0);
        if bitmap.is_null() {
            DeleteDC(dc);
            return Err("Create dashboard placeholder bitmap failed".into());
        }
        let previous_bitmap = SelectObject(dc, bitmap);
        let pixels = std::slice::from_raw_parts_mut(pixels.cast::<u8>(), (width * height * 4) as usize);
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[30, 24, 20, 255]);
        }
        let font = CreateFontW(
            -24,
            0,
            0,
            0,
            FW_NORMAL as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            ANTIALIASED_QUALITY as u32,
            DEFAULT_PITCH as u32,
            windows_sys::core::w!("Segoe UI"),
        );
        let previous_font = SelectObject(
            dc,
            if font.is_null() {
                GetStockObject(DEFAULT_GUI_FONT)
            } else {
                font
            },
        );
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x00eeeeee);
        let mut rect = RECT {
            left: 36,
            top: 96,
            right: width as i32 - 36,
            bottom: height as i32 - 36,
        };
        DrawTextW(
            dc,
            text.as_ptr(),
            text.len() as i32,
            &mut rect,
            DT_CENTER | DT_WORDBREAK | DT_NOPREFIX,
        );
        GdiFlush();
        let mut pixels = pixels.to_vec();
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        SelectObject(dc, previous_font);
        if !font.is_null() {
            DeleteObject(font);
        }
        SelectObject(dc, previous_bitmap);
        DeleteObject(bitmap);
        DeleteDC(dc);
        Ok(Texture {
            pixels,
            width,
            height,
        })
    }
}
