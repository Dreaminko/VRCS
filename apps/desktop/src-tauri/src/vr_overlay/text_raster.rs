use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC, DeleteObject,
    DrawTextW, FillRect, SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CALCRECT,
    FF_DONTCARE, FW_SEMIBOLD, OUT_DEFAULT_PRECIS, PROOF_QUALITY, TRANSPARENT,
};

pub(super) struct TextMask {
    dc: *mut c_void,
    bitmap: *mut c_void,
    old_bitmap: *mut c_void,
    font: *mut c_void,
    old_font: *mut c_void,
    bits: *mut c_void,
    width: u32,
    height: u32,
    black: *mut c_void,
}

impl TextMask {
    pub fn new(width: u32, height: u32, font_size_px: i32) -> Result<Self, String> {
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
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() || bits.is_null() {
                DeleteDC(dc);
                return Err(last_error("CreateDIBSection"));
            }
            let old_bitmap = SelectObject(dc, bitmap);

            let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
            let font = CreateFontW(
                -font_size_px,
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
            let black = CreateSolidBrush(0);
            if black.is_null() {
                SelectObject(dc, old_font);
                SelectObject(dc, old_bitmap);
                DeleteObject(font);
                DeleteObject(bitmap);
                DeleteDC(dc);
                return Err(last_error("CreateSolidBrush"));
            }
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, 0x00ff_ffff);

            Ok(Self {
                dc,
                bitmap,
                old_bitmap,
                font,
                old_font,
                bits,
                width,
                height,
                black,
            })
        }
    }

    pub fn measure(&self, text: &str, width: i32, flags: u32) -> i32 {
        self.measure_size(text, width, flags).1
    }

    pub fn measure_size(&self, text: &str, width: i32, flags: u32) -> (i32, i32) {
        unsafe {
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            let mut measured = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: 0,
            };
            DrawTextW(
                self.dc,
                wide.as_mut_ptr(),
                wide.len() as i32,
                &mut measured,
                flags | DT_CALCRECT,
            );
            (
                measured.right - measured.left,
                measured.bottom - measured.top,
            )
        }
    }

    pub fn draw(&mut self, text: &str, rect: Rect, flags: u32, center: bool) -> Result<(), String> {
        unsafe {
            let full = RECT {
                left: 0,
                top: 0,
                right: self.width as i32,
                bottom: self.height as i32,
            };
            FillRect(self.dc, &full, self.black);
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            let text_height = self.measure(text, rect.right - rect.left, flags);
            let mut target = RECT {
                left: rect.left,
                top: rect.top
                    + if center {
                        (rect.bottom - rect.top - text_height).max(0) / 2
                    } else {
                        0
                    },
                right: rect.right,
                bottom: rect.bottom,
            };
            let result = DrawTextW(
                self.dc,
                wide.as_mut_ptr(),
                wide.len() as i32,
                &mut target,
                flags,
            );
            if result == 0 && !text.is_empty() {
                return Err(last_error("DrawTextW"));
            }
            Ok(())
        }
    }

    pub fn line_height(&self) -> i32 {
        let mut metrics: windows_sys::Win32::Graphics::Gdi::TEXTMETRICW = unsafe { zeroed() };
        unsafe {
            windows_sys::Win32::Graphics::Gdi::GetTextMetricsW(self.dc, &mut metrics);
        }
        metrics.tmHeight.max(1)
    }

    pub fn pixels(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self.bits.cast::<u8>(),
                (self.width * self.height * 4) as usize,
            )
        }
    }
}

impl Drop for TextMask {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old_font);
            SelectObject(self.dc, self.old_bitmap);
            DeleteObject(self.black);
            DeleteObject(self.font);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

impl From<Rect> for RECT {
    fn from(value: Rect) -> Self {
        Self {
            left: value.left,
            top: value.top,
            right: value.right,
            bottom: value.bottom,
        }
    }
}

pub(super) fn fill_rounded_rect(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    rect: Rect,
    radius: i32,
    color: [u8; 4],
) {
    let left = rect.left.clamp(0, width as i32);
    let right = rect.right.clamp(0, width as i32);
    let top = rect.top.clamp(0, height as i32);
    let bottom = rect.bottom.clamp(0, height as i32);
    let radius = radius
        .max(0)
        .min((right - left) / 2)
        .min((bottom - top) / 2);

    for y in top..bottom {
        for x in left..right {
            if !inside_rounded_rect(x, y, Rect::new(left, top, right, bottom), radius) {
                continue;
            }
            let offset = ((y as u32 * width + x as u32) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(&color);
        }
    }
}

fn inside_rounded_rect(x: i32, y: i32, rect: Rect, radius: i32) -> bool {
    if radius == 0
        || (x >= rect.left + radius && x < rect.right - radius)
        || (y >= rect.top + radius && y < rect.bottom - radius)
    {
        return true;
    }
    let center_x = if x < rect.left + radius {
        rect.left + radius
    } else {
        rect.right - radius - 1
    };
    let center_y = if y < rect.top + radius {
        rect.top + radius
    } else {
        rect.bottom - radius - 1
    };
    let dx = x - center_x;
    let dy = y - center_y;
    dx * dx + dy * dy <= radius * radius
}

fn last_error(operation: &str) -> String {
    format!("{operation} failed: {}", std::io::Error::last_os_error())
}
