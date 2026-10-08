use super::selection::Region;
use std::{
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::{
            GetDpiForWindow, SetThreadDpiAwarenessContext,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        },
        Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE},
        WindowsAndMessaging::*,
    },
};

static PICKER_LOCK: Mutex<()> = Mutex::new(());
static WINDOW_CLASS: OnceLock<bool> = OnceLock::new();
const CLASS_NAME: windows_sys::core::PCWSTR = windows_sys::core::w!("VRCSOcrSelection");

struct Picker {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
    dimmed: Vec<u8>,
    hint: Vec<u16>,
    dpi: u32,
    start: Option<(i32, i32)>,
    end: (i32, i32),
    selected: Option<Region>,
    finished: bool,
}

/// Displays the captured client frame at its physical screen position.
/// Runs on a blocking worker. Cancellation also closes a pending picker.
pub fn select_region(
    game_window: isize,
    width: u32,
    height: u32,
    pixels: &[u8],
    cancelled: &AtomicBool,
    hint: &str,
) -> Result<Option<Region>, String> {
    let _lock = PICKER_LOCK.lock().map_err(|_| "desktop_ocr.unavailable")?;
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
        struct DpiGuard(isize);
        impl Drop for DpiGuard {
            fn drop(&mut self) {
                unsafe {
                    SetThreadDpiAwarenessContext(self.0 as _);
                }
            }
        }
        let _dpi = DpiGuard(previous_dpi as isize);
        let game = game_window as HWND;
        let mut client: RECT = zeroed();
        let mut origin = POINT { x: 0, y: 0 };
        if IsWindow(game) == 0
            || IsIconic(game) != 0
            || GetClientRect(game, &mut client) == 0
            || ClientToScreen(game, &mut origin) == 0
            || client.right - client.left != width as i32
            || client.bottom - client.top != height as i32
        {
            return Err("desktop_ocr.capture_failed".into());
        }
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
        let mut bgra = pixels.to_vec();
        for pixel in bgra.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        let mut dimmed = bgra.clone();
        for pixel in dimmed.as_chunks_mut::<4>().0 {
            pixel[0] /= 2;
            pixel[1] /= 2;
            pixel[2] /= 2;
        }
        let mut picker = Picker {
            width,
            height,
            bgra,
            dimmed,
            hint: hint.encode_utf16().collect(),
            dpi: 96,
            start: None,
            end: (0, 0),
            selected: None,
            finished: false,
        };
        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS_NAME,
            CLASS_NAME,
            WS_POPUP,
            origin.x,
            origin.y,
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
            while !picker.finished
                && !cancelled.load(Ordering::Acquire)
                && PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0
            {
                if message.message == WM_QUIT {
                    picker.finished = true;
                    break;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if IsWindow(game) == 0 || IsIconic(game) != 0 {
                picker.finished = true;
            }
            if !picker.finished {
                MsgWaitForMultipleObjectsEx(0, null(), 50, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            }
        }
        let restore_focus = GetForegroundWindow() == window;
        DestroyWindow(window);
        if restore_focus && IsWindow(game) != 0 {
            SetForegroundWindow(game);
        }
        Ok(if cancelled.load(Ordering::Acquire) {
            None
        } else {
            picker.selected
        })
    }
}

fn mouse_point(position: LPARAM) -> (i32, i32) {
    (position as i16 as i32, (position >> 16) as i16 as i32)
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
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
            SetCapture(window);
            InvalidateRect(window, null(), 0);
            0
        }
        WM_MOUSEMOVE => {
            if (*picker).start.is_some() {
                (*picker).end = mouse_point(lparam);
                InvalidateRect(window, null(), 0);
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
                ReleaseCapture();
                InvalidateRect(window, null(), 0);
            }
            0
        }
        WM_CAPTURECHANGED => {
            (*picker).start = None;
            InvalidateRect(window, null(), 0);
            0
        }
        WM_KEYDOWN if wparam == VK_ESCAPE as usize => {
            (*picker).finished = true;
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
            paint(window, &*picker);
            0
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            DefWindowProcW(window, message, wparam, lparam)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn paint(window: HWND, picker: &Picker) {
    let mut paint: PAINTSTRUCT = zeroed();
    let dc = BeginPaint(window, &mut paint);
    paint_buffered_frame(dc, picker);
    EndPaint(window, &paint);
}

unsafe fn paint_buffered_frame(dc: HDC, picker: &Picker) {
    let buffer = CreateCompatibleDC(dc);
    let bitmap = CreateCompatibleBitmap(dc, picker.width as i32, picker.height as i32);
    if buffer.is_null() || bitmap.is_null() {
        paint_frame(dc, picker);
    } else {
        let previous = SelectObject(buffer, bitmap);
        // Present only the completed frame, including the selection and hint.
        paint_frame(buffer, picker);
        BitBlt(
            dc,
            0,
            0,
            picker.width as i32,
            picker.height as i32,
            buffer,
            0,
            0,
            SRCCOPY,
        );
        SelectObject(buffer, previous);
    }
    if !bitmap.is_null() {
        DeleteObject(bitmap);
    }
    if !buffer.is_null() {
        DeleteDC(buffer);
    }
}

unsafe fn paint_frame(dc: HDC, picker: &Picker) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(select_region(0, 0, 0, &[], &cancelled, "").unwrap(), None);
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
            paint_buffered_frame(dc, &picker);
            GdiFlush();
            let rendered = std::slice::from_raw_parts(pixels as *const u8, 100 * 100 * 4);
            let inside = (80 * 100 + 50) * 4;
            let outside = (80 * 100 + 10) * 4;
            assert_eq!(&rendered[inside..inside + 3], &[80, 80, 160]);
            assert_eq!(&rendered[outside..outside + 3], &[40, 40, 80]);
            picker.hint = "Drag to select".encode_utf16().collect();
            paint_buffered_frame(dc, &picker);
            GdiFlush();
            assert_eq!(&rendered[inside..inside + 3], &[80, 80, 160]);
            picker.end = (40, 90);
            paint_buffered_frame(dc, &picker);
            GdiFlush();
            assert_eq!(&rendered[inside..inside + 3], &[40, 40, 80]);
            let remaining = (80 * 100 + 30) * 4;
            assert_eq!(&rendered[remaining..remaining + 3], &[80, 80, 160]);
            SelectObject(dc, previous);
            DeleteObject(bitmap);
            DeleteDC(dc);
        }
    }
}
