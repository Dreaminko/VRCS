use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC, DeleteObject,
    DrawTextW, FillRect, SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CALCRECT,
    DT_END_ELLIPSIS, DT_NOPREFIX, DT_RIGHT, DT_WORDBREAK, FF_DONTCARE, FW_SEMIBOLD,
    OUT_DEFAULT_PRECIS, PROOF_QUALITY, TRANSPARENT,
};

use super::presentation::{MessageSide, WristMessage};

const PANEL_MARGIN: i32 = 16;
const TEXT_MARGIN: i32 = 38;
const TEXT_VERTICAL_MARGIN: i32 = 16;
const MESSAGE_PADDING_Y: i32 = 8;
const MESSAGE_GAP: i32 = 10;
const MIN_FONT_SIZE_PX: i32 = 16;

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
    let mut top = content.top;
    for (message, row_height) in messages.iter().zip(row_heights) {
        if top >= content.bottom {
            break;
        }
        let bottom = (top + row_height).min(content.bottom);
        let text_rect = Rect::new(
            content.left,
            top + MESSAGE_PADDING_Y,
            content.right,
            bottom - MESSAGE_PADDING_Y,
        );
        text_mask.draw(&message.text, text_rect, message.side)?;
        blend_text(&mut pixels, text_mask.pixels(), width, text_rect);
        top = bottom + MESSAGE_GAP;
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
                text_mask.measure(&message.text, text_width, message.side) + MESSAGE_PADDING_Y * 2
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
            text_mask.measure(&message.text, text_width, message.side) + MESSAGE_PADDING_Y * 2
        })
        .collect();
    Ok((text_mask, row_heights))
}

struct TextMask {
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
    fn new(width: u32, height: u32, font_size_px: i32) -> Result<Self, String> {
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

    fn measure(&self, text: &str, width: i32, side: MessageSide) -> i32 {
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
                text_flags(side) | DT_CALCRECT,
            )
        }
    }

    fn draw(&mut self, text: &str, rect: Rect, side: MessageSide) -> Result<(), String> {
        unsafe {
            let full = RECT {
                left: 0,
                top: 0,
                right: self.width as i32,
                bottom: self.height as i32,
            };
            FillRect(self.dc, &full, self.black);
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            let text_height = self.measure(text, rect.right - rect.left, side);
            let mut target = RECT {
                left: rect.left,
                top: rect.top + ((rect.bottom - rect.top - text_height).max(0) / 2),
                right: rect.right,
                bottom: rect.bottom,
            };
            let result = DrawTextW(
                self.dc,
                wide.as_mut_ptr(),
                wide.len() as i32,
                &mut target,
                text_flags(side) | DT_END_ELLIPSIS,
            );
            if result == 0 && !text.is_empty() {
                return Err(last_error("DrawTextW"));
            }
            Ok(())
        }
    }

    fn pixels(&self) -> &[u8] {
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
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl Rect {
    const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
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

fn fill_rounded_rect(
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

fn last_error(operation: &str) -> String {
    format!("{operation} failed: {}", std::io::Error::last_os_error())
}
