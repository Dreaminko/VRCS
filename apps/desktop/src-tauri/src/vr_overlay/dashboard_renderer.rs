use super::dashboard::{
    DashboardControl, DashboardSaveState, DashboardState, DashboardViewModel, DASHBOARD_HEIGHT,
    DASHBOARD_WIDTH,
};
use super::renderer::Texture;

const CANVAS: [u8; 4] = [0xfb, 0xf8, 0xf5, 255];
const SURFACE: [u8; 4] = [255, 255, 255, 255];
const SURFACE_ELEVATED: [u8; 4] = [0xfd, 0xfb, 0xf9, 255];
const TEXT: [u8; 4] = [0x34, 0x28, 0x1d, 255];
const TEXT_SECONDARY: [u8; 4] = [0x78, 0x68, 0x5a, 255];
const TEXT_QUIET: [u8; 4] = [0xa7, 0x98, 0x8b, 255];
const BORDER: [u8; 4] = [0xe9, 0xe2, 0xdd, 255];
const BORDER_STRONG: [u8; 4] = [0xdd, 0xd4, 0xcd, 255];
const PRIMARY: [u8; 4] = [0xff, 0xd6, 0x74, 255];
const PRIMARY_SOFT: [u8; 4] = [0xff, 0xf6, 0xdf, 255];
const PRIMARY_SOFTER: [u8; 4] = [0xff, 0xfb, 0xf0, 255];
const PRIMARY_INK: [u8; 4] = [0x7d, 0x61, 0x17, 255];
const ERROR: [u8; 4] = [0x44, 0x44, 0xc9, 255];
const ERROR_SOFT: [u8; 4] = [0xee, 0xf0, 0xfd, 255];

pub fn render(view: &DashboardViewModel, state: &DashboardState) -> Result<Texture, String> {
    let mut canvas = Canvas::new(DASHBOARD_WIDTH, DASHBOARD_HEIGHT, CANVAS);
    canvas.icon(Rect::new(48, 26, 116, 94))?;
    canvas.text(
        &view.labels.title,
        Rect::new(140, 24, 820, 64),
        30,
        TEXT,
        Align::Left,
    )?;
    canvas.text(
        &view.labels.subtitle,
        Rect::new(140, 64, 900, 102),
        17,
        TEXT_QUIET,
        Align::Left,
    )?;
    let save = match view.save_state {
        DashboardSaveState::Saving => view.labels.saving.as_str(),
        DashboardSaveState::Saved => view.labels.saved.as_str(),
        DashboardSaveState::Error => view.error.as_deref().unwrap_or(&view.status),
        DashboardSaveState::Idle => view.status.as_str(),
    };
    canvas.pill(
        Rect::new(1112, 42, 1392, 88),
        save,
        view.save_state == DashboardSaveState::Error,
    )?;

    canvas.card(Rect::new(48, 124, 1392, 212));
    canvas.text(
        &view.labels.master,
        Rect::new(78, 140, 900, 196),
        22,
        TEXT,
        Align::Left,
    )?;
    canvas.toggle(
        Rect::new(1278, 144, 1362, 192),
        view.enabled,
        visual(state, DashboardControl::Master),
    );

    canvas.card(Rect::new(48, 236, 708, 700));
    canvas.section_title(&view.labels.headset, Rect::new(78, 254, 560, 320))?;
    canvas.toggle(
        Rect::new(600, 264, 678, 312),
        view.headset.enabled,
        visual(state, DashboardControl::HeadsetEnabled),
    );
    canvas.choice_row(
        &view.labels.content,
        &view.headset.content,
        Rect::new(78, 338, 678, 410),
        visual(state, DashboardControl::HeadsetContent),
    )?;
    canvas.stepper_field(
        &view.labels.width,
        &view.headset.width,
        Rect::new(78, 438, 366, 538),
        state,
        DashboardControl::HeadsetWidthDown,
        DashboardControl::HeadsetWidthUp,
    )?;
    canvas.stepper_field(
        &view.labels.opacity,
        &view.headset.opacity,
        Rect::new(390, 438, 678, 538),
        state,
        DashboardControl::HeadsetOpacityDown,
        DashboardControl::HeadsetOpacityUp,
    )?;
    canvas.button(
        Rect::new(78, 616, 678, 672),
        &view.labels.preview,
        visual(state, DashboardControl::HeadsetPreview),
    )?;

    canvas.card(Rect::new(732, 236, 1392, 700));
    canvas.section_title(&view.labels.wrist, Rect::new(762, 254, 1240, 320))?;
    canvas.toggle(
        Rect::new(1284, 264, 1362, 312),
        view.wrist.enabled,
        visual(state, DashboardControl::WristEnabled),
    );
    canvas.choice_row(
        &view.labels.hand,
        &view.wrist.hand,
        Rect::new(762, 338, 1050, 410),
        visual(state, DashboardControl::WristHand),
    )?;
    canvas.choice_row(
        &view.labels.content,
        &view.wrist.content,
        Rect::new(1074, 338, 1362, 410),
        visual(state, DashboardControl::WristContent),
    )?;
    canvas.stepper_field(
        &view.labels.width,
        &view.wrist.width,
        Rect::new(762, 438, 1050, 538),
        state,
        DashboardControl::WristWidthDown,
        DashboardControl::WristWidthUp,
    )?;
    canvas.stepper_field(
        &view.labels.opacity,
        &view.wrist.opacity,
        Rect::new(1074, 438, 1362, 538),
        state,
        DashboardControl::WristOpacityDown,
        DashboardControl::WristOpacityUp,
    )?;
    canvas.button(
        Rect::new(762, 616, 1362, 672),
        &view.labels.preview,
        visual(state, DashboardControl::WristPreview),
    )?;

    canvas.card(Rect::new(48, 724, 1392, 852));
    canvas.section_title(&view.labels.ocr, Rect::new(78, 738, 300, 790))?;
    canvas.text(
        &view.ocr.backend,
        Rect::new(78, 788, 300, 830),
        15,
        TEXT_SECONDARY,
        Align::Left,
    )?;
    canvas.toggle(
        Rect::new(340, 760, 418, 808),
        view.ocr.enabled,
        visual(state, DashboardControl::OcrEnabled),
    );
    canvas.text(
        &view.labels.gesture,
        Rect::new(510, 752, 716, 816),
        18,
        TEXT,
        Align::Left,
    )?;
    canvas.toggle(
        Rect::new(740, 760, 818, 808),
        view.ocr.gesture,
        visual(state, DashboardControl::OcrGesture),
    );
    canvas.button(
        Rect::new(1040, 752, 1362, 824),
        &view.labels.bindings,
        visual(state, DashboardControl::OcrBindings),
    )?;

    if let Some(error) = view.error.as_deref() {
        canvas.rounded(Rect::new(48, 860, 1392, 892), 10, ERROR_SOFT);
        canvas.text(error, Rect::new(64, 860, 1376, 892), 14, ERROR, Align::Left)?;
    }
    Ok(Texture {
        pixels: canvas.pixels,
        width: DASHBOARD_WIDTH,
        height: DASHBOARD_HEIGHT,
    })
}

pub fn render_thumbnail(_title: &str) -> Result<Texture, String> {
    let mut canvas = Canvas::new(256, 256, PRIMARY_SOFTER);
    canvas.icon(Rect::new(26, 26, 230, 230))?;
    Ok(Texture {
        pixels: canvas.pixels,
        width: 256,
        height: 256,
    })
}

fn visual(state: &DashboardState, control: DashboardControl) -> ControlVisual {
    if state.pressed() == Some(control) {
        ControlVisual::Pressed
    } else if state.hovered() == Some(control) {
        ControlVisual::Hovered
    } else {
        ControlVisual::Idle
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlVisual {
    Idle,
    Hovered,
    Pressed,
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

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
}

struct Canvas {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

impl Canvas {
    fn new(width: u32, height: u32, color: [u8; 4]) -> Self {
        let mut pixels = vec![0; (width * height * 4) as usize];
        for pixel in pixels.as_chunks_mut::<4>().0 {
            *pixel = color;
        }
        Self {
            pixels,
            width,
            height,
        }
    }

    fn card(&mut self, rect: Rect) {
        self.rounded(rect, 12, BORDER);
        self.rounded(
            Rect::new(rect.left + 1, rect.top + 1, rect.right - 1, rect.bottom - 1),
            11,
            SURFACE,
        );
    }

    fn section_title(&mut self, text: &str, rect: Rect) -> Result<(), String> {
        self.text(text, rect, 22, PRIMARY_INK, Align::Left)
    }

    fn pill(&mut self, rect: Rect, text: &str, error: bool) -> Result<(), String> {
        self.rounded(rect, 22, if error { ERROR_SOFT } else { PRIMARY_SOFT });
        self.text(
            text,
            rect,
            15,
            if error { ERROR } else { PRIMARY_INK },
            Align::Center,
        )
    }

    fn choice_row(
        &mut self,
        label: &str,
        value: &str,
        rect: Rect,
        visual: ControlVisual,
    ) -> Result<(), String> {
        self.outlined_control(rect, visual);
        self.text(
            label,
            Rect::new(rect.left + 20, rect.top, rect.left + 122, rect.bottom),
            14,
            TEXT_SECONDARY,
            Align::Left,
        )?;
        self.text(
            value,
            Rect::new(rect.left + 118, rect.top, rect.right - 20, rect.bottom),
            17,
            PRIMARY_INK,
            Align::Center,
        )
    }

    fn stepper_field(
        &mut self,
        label: &str,
        value: &str,
        rect: Rect,
        state: &DashboardState,
        down: DashboardControl,
        up: DashboardControl,
    ) -> Result<(), String> {
        self.text(
            label,
            Rect::new(rect.left, rect.top, rect.right, rect.top + 34),
            14,
            TEXT_QUIET,
            Align::Left,
        )?;
        let control = Rect::new(rect.left, rect.top + 44, rect.right, rect.bottom);
        self.rounded(control, 10, BORDER);
        self.rounded(
            Rect::new(
                control.left + 1,
                control.top + 1,
                control.right - 1,
                control.bottom - 1,
            ),
            9,
            SURFACE_ELEVATED,
        );
        self.small_button(
            Rect::new(rect.left, rect.top + 44, rect.left + 62, rect.bottom),
            "−",
            visual(state, down),
        )?;
        self.text(
            value,
            Rect::new(rect.left + 62, rect.top + 44, rect.right - 62, rect.bottom),
            16,
            TEXT,
            Align::Center,
        )?;
        self.small_button(
            Rect::new(rect.right - 62, rect.top + 44, rect.right, rect.bottom),
            "+",
            visual(state, up),
        )
    }

    fn button(&mut self, rect: Rect, text: &str, visual: ControlVisual) -> Result<(), String> {
        self.rounded(rect, 10, BORDER_STRONG);
        let inset = if visual == ControlVisual::Pressed {
            2
        } else {
            1
        };
        self.rounded(
            Rect::new(
                rect.left + inset,
                rect.top + inset,
                rect.right - inset,
                rect.bottom - inset,
            ),
            9,
            match visual {
                ControlVisual::Idle => PRIMARY_SOFTER,
                ControlVisual::Hovered => PRIMARY_SOFT,
                ControlVisual::Pressed => PRIMARY,
            },
        );
        self.text(text, rect, 16, PRIMARY_INK, Align::Center)
    }

    fn small_button(
        &mut self,
        rect: Rect,
        text: &str,
        visual: ControlVisual,
    ) -> Result<(), String> {
        self.rounded(
            rect,
            9,
            match visual {
                ControlVisual::Idle => PRIMARY_SOFTER,
                ControlVisual::Hovered => PRIMARY_SOFT,
                ControlVisual::Pressed => PRIMARY,
            },
        );
        self.text(text, rect, 22, PRIMARY_INK, Align::Center)
    }

    fn toggle(&mut self, rect: Rect, checked: bool, visual: ControlVisual) {
        self.rounded(
            rect,
            (rect.bottom - rect.top) / 2,
            if checked {
                PRIMARY
            } else if visual != ControlVisual::Idle {
                TEXT_SECONDARY
            } else {
                BORDER_STRONG
            },
        );
        let diameter = rect.bottom - rect.top - 10;
        let left = if checked {
            rect.right - diameter - 5
        } else {
            rect.left + 5
        };
        self.rounded(
            Rect::new(left, rect.top + 5, left + diameter, rect.bottom - 5),
            diameter / 2,
            SURFACE,
        );
    }

    fn outlined_control(&mut self, rect: Rect, visual: ControlVisual) {
        self.rounded(rect, 10, BORDER);
        self.rounded(
            Rect::new(rect.left + 1, rect.top + 1, rect.right - 1, rect.bottom - 1),
            9,
            match visual {
                ControlVisual::Idle => SURFACE_ELEVATED,
                ControlVisual::Hovered => PRIMARY_SOFTER,
                ControlVisual::Pressed => PRIMARY_SOFT,
            },
        );
    }

    fn icon(&mut self, rect: Rect) -> Result<(), String> {
        let width = (rect.right - rect.left).max(0) as u32;
        let height = (rect.bottom - rect.top).max(0) as u32;
        let icon = render_app_icon(width, height)?;
        for y in 0..height {
            for x in 0..width {
                let source_offset = ((y * width + x) * 4) as usize;
                let target_offset =
                    ((((rect.top as u32 + y) * self.width) + rect.left as u32 + x) * 4) as usize;
                let source = &icon[source_offset..source_offset + 4];
                let alpha = if source[3] == 0 && source[..3] != [0, 0, 0] {
                    255
                } else {
                    source[3]
                } as u16;
                if alpha == 0 {
                    continue;
                }
                for channel in 0..3 {
                    self.pixels[target_offset + channel] = (((source[channel] as u16 * alpha)
                        + (self.pixels[target_offset + channel] as u16 * (255 - alpha)))
                        / 255) as u8;
                }
            }
        }
        Ok(())
    }

    fn rounded(&mut self, rect: Rect, radius: i32, color: [u8; 4]) {
        let left = rect.left.clamp(0, self.width as i32);
        let top = rect.top.clamp(0, self.height as i32);
        let right = rect.right.clamp(left, self.width as i32);
        let bottom = rect.bottom.clamp(top, self.height as i32);
        let radius = radius
            .max(0)
            .min((right - left) / 2)
            .min((bottom - top) / 2);
        for y in top..bottom {
            for x in left..right {
                let inside = radius == 0
                    || (x >= left + radius && x < right - radius)
                    || (y >= top + radius && y < bottom - radius)
                    || {
                        let cx = if x < left + radius {
                            left + radius
                        } else {
                            right - radius - 1
                        };
                        let cy = if y < top + radius {
                            top + radius
                        } else {
                            bottom - radius - 1
                        };
                        let dx = x - cx;
                        let dy = y - cy;
                        dx * dx + dy * dy <= radius * radius
                    };
                if inside {
                    let offset = ((y as u32 * self.width + x as u32) * 4) as usize;
                    self.pixels[offset..offset + 4].copy_from_slice(&color);
                }
            }
        }
    }

    fn text(
        &mut self,
        text: &str,
        rect: Rect,
        size: i32,
        color: [u8; 4],
        align: Align,
    ) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }
        let mask = render_text_mask(text, self.width, self.height, rect, size, align)?;
        for (target, coverage) in self
            .pixels
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(mask.as_chunks::<4>().0)
        {
            let alpha = coverage[0] as u16;
            if alpha == 0 {
                continue;
            }
            for channel in 0..3 {
                target[channel] = (((color[channel] as u16 * alpha)
                    + (target[channel] as u16 * (255 - alpha)))
                    / 255) as u8;
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
fn render_app_icon(width: u32, height: u32) -> Result<Vec<u8>, String> {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::ptr::null_mut;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, DestroyIcon, DrawIconEx, DI_NORMAL, LR_DEFAULTCOLOR,
    };

    const ICON: &[u8] = include_bytes!("../../icons/icon.ico");
    let image = largest_ico_image(ICON)?;
    unsafe {
        let icon = CreateIconFromResourceEx(
            image.as_ptr(),
            image.len() as u32,
            1,
            0x0003_0000,
            width as i32,
            height as i32,
            LR_DEFAULTCOLOR,
        );
        if icon.is_null() {
            return Err(last_error("CreateIconFromResourceEx"));
        }
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            DestroyIcon(icon);
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
            DestroyIcon(icon);
            return Err(last_error("CreateDIBSection"));
        }
        let old_bitmap = SelectObject(dc, bitmap);
        let drawn = DrawIconEx(
            dc,
            0,
            0,
            icon,
            width as i32,
            height as i32,
            0,
            null_mut(),
            DI_NORMAL,
        );
        let pixels =
            std::slice::from_raw_parts(bits.cast::<u8>(), (width * height * 4) as usize).to_vec();
        SelectObject(dc, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(dc);
        DestroyIcon(icon);
        if drawn == 0 {
            Err(last_error("DrawIconEx"))
        } else {
            Ok(pixels)
        }
    }
}

#[cfg(windows)]
fn largest_ico_image(icon: &[u8]) -> Result<&[u8], String> {
    if icon.len() < 6 || icon[2..4] != [1, 0] {
        return Err("Invalid VRCS icon".into());
    }
    let count = u16::from_le_bytes([icon[4], icon[5]]) as usize;
    let mut best: Option<(u32, usize, usize)> = None;
    for index in 0..count {
        let entry = 6 + index * 16;
        if entry + 16 > icon.len() {
            break;
        }
        let width = if icon[entry] == 0 {
            256
        } else {
            icon[entry] as u32
        };
        let height = if icon[entry + 1] == 0 {
            256
        } else {
            icon[entry + 1] as u32
        };
        let size = u32::from_le_bytes(icon[entry + 8..entry + 12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(icon[entry + 12..entry + 16].try_into().unwrap()) as usize;
        if offset
            .checked_add(size)
            .is_some_and(|end| end <= icon.len())
        {
            let area = width * height;
            if best.is_none_or(|(best_area, _, _)| area > best_area) {
                best = Some((area, offset, size));
            }
        }
    }
    best.map(|(_, offset, size)| &icon[offset..offset + size])
        .ok_or_else(|| "VRCS icon contains no usable image".into())
}

#[cfg(not(windows))]
fn render_app_icon(width: u32, height: u32) -> Result<Vec<u8>, String> {
    Ok(vec![0; (width * height * 4) as usize])
}

#[cfg(windows)]
fn render_text_mask(
    text: &str,
    width: u32,
    height: u32,
    rect: Rect,
    size: i32,
    align: Align,
) -> Result<Vec<u8>, String> {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
        SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER,
        DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_SEMIBOLD,
        OUT_DEFAULT_PRECIS, PROOF_QUALITY, TRANSPARENT,
    };

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
        let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
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
            SelectObject(dc, old_bitmap);
            DeleteObject(bitmap);
            DeleteDC(dc);
            return Err(last_error("CreateFontW"));
        }
        let old_font = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, 0x00ff_ffff);
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut target = RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        let alignment = match align {
            Align::Left => DT_LEFT,
            Align::Center => DT_CENTER,
        };
        let result = DrawTextW(
            dc,
            wide.as_mut_ptr(),
            wide.len() as i32,
            &mut target,
            alignment | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        let pixels =
            std::slice::from_raw_parts(bits.cast::<u8>(), (width * height * 4) as usize).to_vec();
        SelectObject(dc, old_font);
        SelectObject(dc, old_bitmap);
        DeleteObject(font);
        DeleteObject(bitmap);
        DeleteDC(dc);
        if result == 0 {
            Err(last_error("DrawTextW"))
        } else {
            Ok(pixels)
        }
    }
}

#[cfg(not(windows))]
fn render_text_mask(
    _: &str,
    width: u32,
    height: u32,
    _: Rect,
    _: i32,
    _: Align,
) -> Result<Vec<u8>, String> {
    Ok(vec![0; (width * height * 4) as usize])
}

#[cfg(windows)]
fn last_error(operation: &str) -> String {
    format!(
        "{operation} failed with Windows error {}",
        std::io::Error::last_os_error()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_overlay::dashboard::{
        DashboardHeadset, DashboardLabels, DashboardOcr, DashboardSaveState, DashboardState,
        DashboardViewModel, DashboardWrist, DASHBOARD_HEIGHT, DASHBOARD_WIDTH,
    };

    fn view(error: Option<String>) -> DashboardViewModel {
        DashboardViewModel {
            labels: DashboardLabels {
                title: "VRCS".into(),
                subtitle: "SteamVR quick settings".into(),
                master: "VR Overlay".into(),
                headset: "Headset".into(),
                wrist: "Wrist".into(),
                ocr: "OCR".into(),
                content: "Content".into(),
                hand: "Hand".into(),
                width: "Width".into(),
                opacity: "Opacity".into(),
                gesture: "Gesture".into(),
                preview: "Preview".into(),
                bindings: "Bindings".into(),
                saving: "Saving".into(),
                saved: "Saved".into(),
            },
            enabled: true,
            headset: DashboardHeadset {
                enabled: true,
                content: "Bilingual".into(),
                width: "1.20 m".into(),
                opacity: "92%".into(),
            },
            wrist: DashboardWrist {
                enabled: true,
                hand: "Left".into(),
                content: "Bilingual".into(),
                width: "0.32 m".into(),
                opacity: "94%".into(),
            },
            ocr: DashboardOcr {
                enabled: false,
                backend: "Cloud".into(),
                gesture: true,
            },
            status: "Ready".into(),
            save_state: DashboardSaveState::Idle,
            error,
        }
    }

    #[test]
    fn renders_an_opaque_dashboard_texture_at_the_declared_size() {
        let texture = render(&view(None), &DashboardState::default()).unwrap();

        assert_eq!((texture.width, texture.height), (1440, 900));
        assert_eq!(
            texture.pixels.len(),
            (DASHBOARD_WIDTH * DASHBOARD_HEIGHT * 4) as usize
        );
        assert!(texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn renders_error_copy_without_changing_the_texture_contract() {
        let texture = render(
            &view(Some("The settings service is unavailable".into())),
            &DashboardState::default(),
        )
        .unwrap();

        assert_eq!(
            (texture.width, texture.height),
            (DASHBOARD_WIDTH, DASHBOARD_HEIGHT)
        );
    }

    #[test]
    fn renders_a_square_dashboard_thumbnail() {
        let texture = render_thumbnail("VRCS").unwrap();

        assert_eq!((texture.width, texture.height), (256, 256));
        assert!(texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255));
    }

    #[cfg(windows)]
    #[test]
    fn dashboard_thumbnail_contains_the_existing_white_logo_detail() {
        let texture = render_thumbnail("VRCS").unwrap();

        assert!(texture
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3].iter().all(|channel| *channel >= 245)));
    }
}
