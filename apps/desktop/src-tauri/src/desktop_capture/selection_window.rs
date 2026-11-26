use super::selection::Region;
use std::{
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use vrcs_core::ocr::desktop_capture::DesktopCapture;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
            SetThreadDpiAwarenessContext,
        },
        Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE},
        WindowsAndMessaging::*,
    },
};

static WINDOW_CLASS: OnceLock<bool> = OnceLock::new();
static ACTIVE_PICKER: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
const CLASS_NAME: windows_sys::core::PCWSTR = windows_sys::core::w!("VRCSImageSelection");

pub fn cancel_active() {
    let hwnd = ACTIVE_PICKER.load(Ordering::Acquire);
    if hwnd != 0 {
        unsafe {
            PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
        }
    }
}

struct Picker {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    dimmed: Vec<u8>,
    hint: Vec<u16>,
    dpi: u32,
    start: Option<(i32, i32)>,
    end: (i32, i32),
    last_rect: Option<RECT>,
    cache: Option<PaintCache>,
    selected: Option<Region>,
    finished: bool,
}

struct CachedSurface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}

impl CachedSurface {
    unsafe fn new(screen_dc: HDC, width: i32, height: i32, pixels: Option<&[u8]>) -> Option<Self> {
        unsafe {
            let dc = CreateCompatibleDC(screen_dc);
            let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
            if dc.is_null() || bitmap.is_null() {
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                if !dc.is_null() {
                    DeleteDC(dc);
                }
                return None;
            }
            if let Some(pixels) = pixels {
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: width,
                        biHeight: -height,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB,
                        ..zeroed()
                    },
                    ..zeroed()
                };
                let copied = SetDIBits(
                    dc,
                    bitmap,
                    0,
                    height as u32,
                    pixels.as_ptr().cast(),
                    &info,
                    DIB_RGB_COLORS,
                );
                if copied == 0 {
                    DeleteObject(bitmap);
                    DeleteDC(dc);
                    return None;
                }
            }
            let previous = SelectObject(dc, bitmap);
            Some(Self {
                dc,
                bitmap,
                previous,
            })
        }
    }
}

impl Drop for CachedSurface {
    fn drop(&mut self) {
        unsafe {
            if !self.dc.is_null() {
                SelectObject(self.dc, self.previous);
                if !self.bitmap.is_null() {
                    DeleteObject(self.bitmap);
                }
                DeleteDC(self.dc);
            }
        }
    }
}

struct PaintCache {
    buffer: CachedSurface,
    dimmed: CachedSurface,
    bgra: CachedSurface,
    border_brush: HBRUSH,
    font: HFONT,
    font_dpi: u32,
}

impl PaintCache {
    unsafe fn font_for_dpi(&mut self, dpi: u32) -> HFONT {
        if self.font_dpi != dpi {
            unsafe {
                if !self.font.is_null() {
                    DeleteObject(self.font);
                }
                self.font = create_hint_font(dpi);
            }
            self.font_dpi = dpi;
        }
        self.font
    }
}

impl Drop for PaintCache {
    fn drop(&mut self) {
        unsafe {
            if !self.border_brush.is_null() {
                DeleteObject(self.border_brush);
            }
            if !self.font.is_null() {
                DeleteObject(self.font);
            }
        }
    }
}

#[cfg(test)]
fn select_region(
    capture: &DesktopCapture,
    cancelled: &AtomicBool,
    hint: &str,
) -> Result<Option<Region>, String> {
    if cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    let permit = super::reserve_picker().map_err(|_| "desktop_ocr.selection_busy")?;
    select_region_reserved(capture, cancelled, hint, &permit)
}

/// Runs on a blocking worker, using a reservation acquired before capture.
/// Cancellation also closes a pending picker.
pub fn select_region_reserved(
    capture: &DesktopCapture,
    cancelled: &AtomicBool,
    hint: &str,
    _permit: &super::PickerPermit,
) -> Result<Option<Region>, String> {
    let (width, height, pixels) = (capture.width, capture.height, &capture.pixels);
    if cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    if width > i32::MAX as u32 || height > i32::MAX as u32 {
        return Err("desktop_ocr.capture_failed".into());
    }
    // Validate the image before handing its buffer to GDI.
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4));
    if width == 0 || height == 0 || expected != Some(pixels.len()) {
        return Err("desktop_ocr.capture_failed".into());
    }
    unsafe {
        let previous_dpi = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        if previous_dpi.is_null() {
            return Err("desktop_ocr.unavailable".into());
        }
        let _dpi = std::mem::DropGuard::new(previous_dpi, |previous| {
            SetThreadDpiAwarenessContext(previous);
        });
        if capture.origin
            != (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
            )
            || width as i32 != GetSystemMetrics(SM_CXVIRTUALSCREEN)
            || height as i32 != GetSystemMetrics(SM_CYVIRTUALSCREEN)
        {
            return Err("desktop_ocr.capture_failed".into());
        }
        let previous_window = GetForegroundWindow();
        let instance = GetModuleHandleW(null());
        if !*WINDOW_CLASS.get_or_init(|| {
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                hCursor: LoadCursorW(null_mut(), IDC_CROSS),
                lpszClassName: CLASS_NAME,
                ..zeroed()
            };
            RegisterClassW(&class) != 0
        }) {
            return Err("desktop_ocr.unavailable".into());
        }
        let (bgra, dimmed) = picker_pixels(pixels);
        let mut picker = Picker {
            width,
            height,
            bgra,
            dimmed,
            hint: hint.encode_utf16().collect(),
            dpi: 96,
            start: None,
            end: (0, 0),
            last_rect: None,
            cache: None,
            selected: None,
            finished: false,
        };
        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS_NAME,
            CLASS_NAME,
            WS_POPUP,
            capture.origin.0,
            capture.origin.1,
            width as i32,
            height as i32,
            null_mut(),
            null_mut(),
            instance,
            (&mut picker as *mut Picker).cast(),
        );
        if window.is_null() {
            return Err("desktop_ocr.unavailable".into());
        }
        struct ActivePickerGuard;
        impl Drop for ActivePickerGuard {
            fn drop(&mut self) {
                ACTIVE_PICKER.store(0, Ordering::Release);
            }
        }
        ACTIVE_PICKER.store(window as isize, Ordering::Release);
        let _active_picker = ActivePickerGuard;
        if cancelled.load(Ordering::Acquire) {
            DestroyWindow(window);
            return Ok(None);
        }
        picker.dpi = GetDpiForWindow(window).max(96);
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
        if GetForegroundWindow() != window {
            DestroyWindow(window);
            return Err("desktop_ocr.unavailable".into());
        }
        UpdateWindow(window);
        let mut message: MSG = zeroed();
        while !picker.finished && !cancelled.load(Ordering::Acquire) {
            let status = GetMessageW(&mut message, null_mut(), 0, 0);
            if status <= 0 {
                picker.finished = true;
                break;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let restore_focus = GetForegroundWindow() == window;
        DestroyWindow(window);
        if restore_focus && IsWindow(previous_window) != 0 && IsIconic(previous_window) == 0 {
            SetForegroundWindow(previous_window);
        }
        Ok(if cancelled.load(Ordering::Acquire) {
            None
        } else {
            picker.selected
        })
    }
}

fn picker_pixels(rgba: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut bgra = vec![0; rgba.len()];
    let mut dimmed = vec![0; rgba.len()];
    for ((source, normal), dimmed) in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(bgra.as_chunks_mut::<4>().0)
        .zip(dimmed.as_chunks_mut::<4>().0)
    {
        let [r, g, b, a] = *source;
        *normal = [b, g, r, a];
        *dimmed = [b / 2, g / 2, r / 2, a];
    }
    (bgra, dimmed)
}

fn mouse_point(position: LPARAM) -> (i32, i32) {
    (position as i16 as i32, (position >> 16) as i16 as i32)
}

fn selection_rect(picker: &Picker) -> Option<RECT> {
    picker
        .start
        .and_then(|start| Region::from_drag(start, picker.end, picker.width, picker.height))
        .map(|region| RECT {
            left: region.left as i32,
            top: region.top as i32,
            right: (region.left + region.width) as i32,
            bottom: (region.top + region.height) as i32,
        })
}

fn rect_intersect(a: RECT, b: RECT) -> Option<RECT> {
    let rect = RECT {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    };
    (rect.left < rect.right && rect.top < rect.bottom).then_some(rect)
}

fn dirty_union_inflated(
    a: Option<RECT>,
    b: Option<RECT>,
    pad: i32,
    width: i32,
    height: i32,
) -> Option<RECT> {
    let rect = match (a, b) {
        (Some(a), Some(b)) => RECT {
            left: a.left.min(b.left),
            top: a.top.min(b.top),
            right: a.right.max(b.right),
            bottom: a.bottom.max(b.bottom),
        },
        (Some(r), None) | (None, Some(r)) => r,
        (None, None) => return None,
    };
    rect_intersect(
        RECT {
            left: rect.left.saturating_sub(pad),
            top: rect.top.saturating_sub(pad),
            right: rect.right.saturating_add(pad),
            bottom: rect.bottom.saturating_add(pad),
        },
        RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        },
    )
}

fn hint_rect_for(picker: &Picker) -> RECT {
    let scale = |value: i32| value * picker.dpi as i32 / 96;
    RECT {
        left: scale(16),
        top: scale(16),
        right: picker.width as i32 - scale(16),
        bottom: scale(80),
    }
}

unsafe fn create_hint_font(dpi: u32) -> HFONT {
    let scale = |value: i32| value * dpi as i32 / 96;
    unsafe {
        CreateFontW(
            -scale(18),
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
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            windows_sys::core::w!("Segoe UI"),
        )
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
            return 1;
        }
        let picker = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut Picker;
        if picker.is_null() {
            return DefWindowProcW(window, message, wparam, lparam);
        }
        // Keep accesses short: focus and mouse-capture calls can re-enter this procedure.
        match message {
            WM_LBUTTONDOWN => {
                (*picker).start = Some(mouse_point(lparam));
                (*picker).end = mouse_point(lparam);
                (*picker).last_rect = None;
                SetCapture(window);
                InvalidateRect(window, null(), 0);
                0
            }
            WM_MOUSEMOVE => {
                if (*picker).start.is_some() {
                    let prev = (*picker).last_rect;
                    (*picker).end = mouse_point(lparam);
                    let curr = selection_rect(&*picker);
                    (*picker).last_rect = curr;
                    if let Some(dirty) = dirty_union_inflated(
                        prev,
                        curr,
                        2,
                        (*picker).width as i32,
                        (*picker).height as i32,
                    ) {
                        InvalidateRect(window, &dirty, 0);
                    }
                }
                0
            }
            WM_LBUTTONUP => {
                if let Some(start) = (*picker).start.take() {
                    (*picker).selected = Region::from_drag(
                        start,
                        mouse_point(lparam),
                        (*picker).width,
                        (*picker).height,
                    );
                    (*picker).finished = (*picker).selected.is_some();
                    (*picker).last_rect = None;
                    ReleaseCapture();
                    InvalidateRect(window, null(), 0);
                }
                0
            }
            WM_CAPTURECHANGED => {
                (*picker).start = None;
                (*picker).last_rect = None;
                InvalidateRect(window, null(), 0);
                0
            }
            WM_KEYDOWN if wparam == VK_ESCAPE as usize => {
                (*picker).finished = true;
                0
            }
            WM_DISPLAYCHANGE => {
                (*picker).finished = true;
                (*picker).selected = None;
                0
            }
            WM_DPICHANGED => {
                (*picker).dpi = GetDpiForWindow(window).max(96);
                InvalidateRect(window, null(), 0);
                0
            }
            WM_RBUTTONDOWN | WM_CLOSE | WM_KILLFOCUS => {
                // Window teardown can lose focus after a drag has already committed.
                if !(*picker).finished {
                    (*picker).finished = true;
                    (*picker).selected = None;
                }
                0
            }
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                paint(window, &mut *picker);
                0
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                DefWindowProcW(window, message, wparam, lparam)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

unsafe fn ensure_paint_cache(dc: HDC, picker: &mut Picker) -> Option<&mut PaintCache> {
    if picker.cache.is_none() {
        let (width, height) = (picker.width as i32, picker.height as i32);
        unsafe {
            let buffer = CachedSurface::new(dc, width, height, None)?;
            let dimmed = CachedSurface::new(dc, width, height, Some(&picker.dimmed))?;
            let bgra = CachedSurface::new(dc, width, height, Some(&picker.bgra))?;
            let border_brush = CreateSolidBrush(0x00f0bf80);
            if border_brush.is_null() {
                return None;
            }
            let font = create_hint_font(picker.dpi);
            picker.cache = Some(PaintCache {
                buffer,
                dimmed,
                bgra,
                border_brush,
                font,
                font_dpi: picker.dpi,
            });
        }
        picker.bgra = Vec::new();
        picker.dimmed = Vec::new();
    }
    picker.cache.as_mut()
}

unsafe fn paint_cached_dirty(dc: HDC, picker: &mut Picker, dirty: RECT) -> bool {
    let selection = selection_rect(picker);
    let hint_empty = picker.hint.is_empty();
    let hint_box = hint_rect_for(picker);
    let dpi = picker.dpi;
    unsafe {
        let Some(cache) = ensure_paint_cache(dc, picker) else {
            return false;
        };
        let buf_dc = cache.buffer.dc;
        let dirty_w = dirty.right - dirty.left;
        let dirty_h = dirty.bottom - dirty.top;
        BitBlt(
            buf_dc,
            dirty.left,
            dirty.top,
            dirty_w,
            dirty_h,
            cache.dimmed.dc,
            dirty.left,
            dirty.top,
            SRCCOPY,
        );
        if let Some(rect) = selection {
            let saved = SaveDC(buf_dc);
            IntersectClipRect(buf_dc, dirty.left, dirty.top, dirty.right, dirty.bottom);
            if let Some(hit) = rect_intersect(rect, dirty) {
                BitBlt(
                    buf_dc,
                    hit.left,
                    hit.top,
                    hit.right - hit.left,
                    hit.bottom - hit.top,
                    cache.bgra.dc,
                    hit.left,
                    hit.top,
                    SRCCOPY,
                );
            }
            FrameRect(buf_dc, &rect, cache.border_brush);
            RestoreDC(buf_dc, saved);
        }
        if !hint_empty && rect_intersect(hint_box, dirty).is_some() {
            let saved = SaveDC(buf_dc);
            IntersectClipRect(buf_dc, dirty.left, dirty.top, dirty.right, dirty.bottom);
            let font = cache.font_for_dpi(dpi);
            let previous = SelectObject(
                buf_dc,
                if font.is_null() {
                    GetStockObject(DEFAULT_GUI_FONT)
                } else {
                    font
                },
            );
            SetBkMode(buf_dc, TRANSPARENT as i32);
            SetTextColor(buf_dc, 0x00ffffff);
            let mut text_rect = hint_box;
            DrawTextW(
                buf_dc,
                picker.hint.as_ptr(),
                picker.hint.len() as i32,
                &mut text_rect,
                DT_CENTER | DT_WORDBREAK | DT_NOPREFIX,
            );
            SelectObject(buf_dc, previous);
            RestoreDC(buf_dc, saved);
        }
        BitBlt(
            dc, dirty.left, dirty.top, dirty_w, dirty_h, buf_dc, dirty.left, dirty.top, SRCCOPY,
        );
    }
    true
}

unsafe fn paint(window: HWND, picker: &mut Picker) {
    unsafe {
        let mut paint: PAINTSTRUCT = zeroed();
        let dc = BeginPaint(window, &mut paint);
        if let Some(dirty) = rect_intersect(
            paint.rcPaint,
            RECT {
                left: 0,
                top: 0,
                right: picker.width as i32,
                bottom: picker.height as i32,
            },
        ) && !paint_cached_dirty(dc, picker, dirty)
        {
            paint_frame(dc, picker);
        }
        EndPaint(window, &paint);
    }
}

unsafe fn paint_frame(dc: HDC, picker: &Picker) {
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: picker.width as i32,
                biHeight: -(picker.height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..zeroed()
            },
            ..zeroed()
        };
        let draw = |pixels: &[u8]| {
            StretchDIBits(
                dc,
                0,
                0,
                picker.width as i32,
                picker.height as i32,
                0,
                0,
                picker.width as i32,
                picker.height as i32,
                pixels.as_ptr().cast(),
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
        };
        draw(&picker.dimmed);
        if let Some(region) = picker
            .start
            .and_then(|start| Region::from_drag(start, picker.end, picker.width, picker.height))
        {
            let rect = RECT {
                left: region.left as i32,
                top: region.top as i32,
                right: (region.left + region.width) as i32,
                bottom: (region.top + region.height) as i32,
            };
            let saved = SaveDC(dc);
            IntersectClipRect(dc, rect.left, rect.top, rect.right, rect.bottom);
            draw(&picker.bgra);
            RestoreDC(dc, saved);
            let brush = CreateSolidBrush(0x00f0bf80);
            FrameRect(dc, &rect, brush);
            DeleteObject(brush);
        }
        if !picker.hint.is_empty() {
            let scale = |value: i32| value * picker.dpi as i32 / 96;
            let font = CreateFontW(
                -scale(18),
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
                CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32,
                windows_sys::core::w!("Segoe UI"),
            );
            let previous = SelectObject(
                dc,
                if font.is_null() {
                    GetStockObject(DEFAULT_GUI_FONT)
                } else {
                    font
                },
            );
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, 0x00ffffff);
            let mut text_rect = RECT {
                left: scale(16),
                top: scale(16),
                right: picker.width as i32 - scale(16),
                bottom: scale(80),
            };
            DrawTextW(
                dc,
                picker.hint.as_ptr(),
                picker.hint.len() as i32,
                &mut text_rect,
                DT_CENTER | DT_WORDBREAK | DT_NOPREFIX,
            );
            SelectObject(dc, previous);
            if !font.is_null() {
                DeleteObject(font);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_images_preserve_colors_alpha_and_source_pixels() {
        let rgba = [11, 22, 33, 44, 255, 128, 1, 0];
        let (bgra, dimmed) = picker_pixels(&rgba);
        assert_eq!(bgra, [33, 22, 11, 44, 1, 128, 255, 0]);
        assert_eq!(dimmed, [16, 11, 5, 44, 0, 64, 127, 0]);
        assert_eq!(rgba, [11, 22, 33, 44, 255, 128, 1, 0]);
    }

    #[test]
    fn pending_drag_can_still_be_cancelled() {
        unsafe {
            let instance = GetModuleHandleW(null());
            let name = windows_sys::core::w!("VRCSOcrSelectionCancellationTest");
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: name,
                ..zeroed()
            };
            assert_ne!(RegisterClassW(&class), 0);
            for (message, key) in [
                (WM_KILLFOCUS, 0),
                (WM_RBUTTONDOWN, 0),
                (WM_DISPLAYCHANGE, 0),
                (WM_KEYDOWN, VK_ESCAPE as usize),
            ] {
                let mut picker = Picker {
                    width: 100,
                    height: 100,
                    bgra: vec![0; 40000],
                    dimmed: vec![0; 40000],
                    hint: Vec::new(),
                    dpi: 96,
                    start: Some((20, 70)),
                    end: (80, 90),
                    last_rect: None,
                    cache: None,
                    selected: None,
                    finished: false,
                };
                let window = CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    name,
                    name,
                    WS_POPUP,
                    0,
                    0,
                    100,
                    100,
                    null_mut(),
                    null_mut(),
                    instance,
                    (&mut picker as *mut Picker).cast(),
                );
                assert!(!window.is_null());
                SendMessageW(window, message, key, 0);
                assert!(picker.finished);
                assert_eq!(picker.selected, None);
                assert_ne!(DestroyWindow(window), 0);
            }
            UnregisterClassW(name, instance);
        }
    }

    #[test]
    fn completed_drag_survives_focused_window_destruction() {
        unsafe {
            let mut picker = Picker {
                width: 100,
                height: 100,
                bgra: vec![0; 40000],
                dimmed: vec![0; 40000],
                hint: Vec::new(),
                dpi: 96,
                start: None,
                end: (0, 0),
                last_rect: None,
                cache: None,
                selected: None,
                finished: false,
            };
            let instance = GetModuleHandleW(null());
            let name = windows_sys::core::w!("VRCSOcrSelectionLifecycleTest");
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: name,
                ..zeroed()
            };
            assert_ne!(RegisterClassW(&class), 0);
            let window = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                name,
                name,
                WS_POPUP,
                0,
                0,
                100,
                100,
                null_mut(),
                null_mut(),
                instance,
                (&mut picker as *mut Picker).cast(),
            );
            assert!(!window.is_null());
            ShowWindow(window, SW_SHOW);
            windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(window);
            assert_eq!(
                windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus(),
                window
            );
            let point = |x: u16, y: u16| (x as isize) | ((y as isize) << 16);
            SendMessageW(window, WM_LBUTTONDOWN, 0, point(20, 70));
            SendMessageW(window, WM_LBUTTONUP, 0, point(80, 90));
            let expected = Some(Region {
                left: 20,
                top: 70,
                width: 60,
                height: 20,
            });
            assert_eq!(picker.selected, expected);
            assert!(picker.finished);
            windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(window);
            assert_eq!(
                windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus(),
                window
            );
            assert_ne!(DestroyWindow(window), 0);
            UnregisterClassW(name, instance);
            assert_eq!(
                picker.selected, expected,
                "Closing the completed picker must retain its region"
            );
        }
    }

    #[test]
    fn cancellation_before_opening_does_not_show_a_window() {
        let cancelled = AtomicBool::new(true);
        let capture = DesktopCapture {
            origin: (0, 0),
            width: 0,
            height: 0,
            pixels: Vec::new(),
        };
        assert_eq!(select_region(&capture, &cancelled, "").unwrap(), None);
    }

    #[test]
    fn drawing_preserves_selected_pixels_and_dims_the_rest() {
        unsafe {
            let mut picker = Picker {
                width: 100,
                height: 100,
                bgra: vec![0; 100 * 100 * 4],
                dimmed: vec![0; 100 * 100 * 4],
                hint: Vec::new(),
                dpi: 96,
                start: Some((20, 70)),
                end: (80, 90),
                last_rect: None,
                cache: None,
                selected: None,
                finished: false,
            };
            for (index, pixel) in picker.bgra.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                pixel.copy_from_slice(&[(index / 100) as u8, 80, 160, 255]);
            }
            for (index, pixel) in picker.dimmed.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                pixel.copy_from_slice(&[(index / 100 / 2) as u8, 40, 80, 255]);
            }
            let dc = CreateCompatibleDC(null_mut());
            assert!(!dc.is_null());
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: 100,
                    biHeight: -100,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..zeroed()
                },
                ..zeroed()
            };
            let mut pixels = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut pixels, null_mut(), 0);
            assert!(!bitmap.is_null());
            let previous = SelectObject(dc, bitmap);
            let full = RECT {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            };
            assert!(paint_cached_dirty(dc, &mut picker, full));
            GdiFlush();
            let rendered = std::slice::from_raw_parts(pixels as *const u8, 100 * 100 * 4);
            let inside = (80 * 100 + 50) * 4;
            let outside = (80 * 100 + 10) * 4;
            assert_eq!(&rendered[inside..inside + 3], &[80, 80, 160]);
            assert_eq!(&rendered[outside..outside + 3], &[40, 40, 80]);
            picker.hint = "Drag to select".encode_utf16().collect();
            assert!(paint_cached_dirty(dc, &mut picker, full));
            GdiFlush();
            assert_eq!(&rendered[inside..inside + 3], &[80, 80, 160]);
            picker.end = (40, 90);
            assert!(paint_cached_dirty(dc, &mut picker, full));
            GdiFlush();
            assert_eq!(&rendered[inside..inside + 3], &[40, 40, 80]);
            let remaining = (80 * 100 + 30) * 4;
            assert_eq!(&rendered[remaining..remaining + 3], &[80, 80, 160]);
            picker.cache = None;
            SelectObject(dc, previous);
            DeleteObject(bitmap);
            DeleteDC(dc);
        }
    }
}
