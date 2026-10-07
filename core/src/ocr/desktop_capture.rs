use std::{
    mem::size_of,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use windows::{
    core::{factory, Error, Interface, HRESULT, PWSTR},
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::{
            CloseHandle, ERROR_TIMEOUT, E_ABORT, E_FAIL, E_HANDLE, E_NOINTERFACE, E_NOTIMPL,
            E_POINTER, HMODULE, HWND, POINT, RECT, REGDB_E_CLASSNOTREG,
        },
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
                D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
                D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_SDK_VERSION,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
            },
            Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS},
            Dxgi::{IDXGIDevice, DXGI_ERROR_WAS_STILL_DRAWING},
            Gdi::ClientToScreen,
        },
        System::{
            Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
            WinRT::{
                Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
                Graphics::Capture::IGraphicsCaptureItemInterop,
                RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED,
            },
        },
        UI::{
            HiDpi::{
                SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            },
            WindowsAndMessaging::{
                GetClientRect, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow,
            },
        },
    },
};

pub struct DesktopCapture {
    pub window: isize,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Captures a fresh VRChat client frame without changing focus.
/// Call from a blocking worker; capture and GPU readback share a two-second deadline.
pub fn capture_vrchat(
    previous_window: Option<isize>,
    result_window: Option<isize>,
) -> Result<DesktopCapture, String> {
    let foreground = unsafe { GetForegroundWindow() };
    let window = if is_vrchat(foreground) {
        foreground
    } else if result_window.is_some_and(|window| window == foreground.0 as isize) {
        let previous = previous_window
            .map(|window| HWND(window as *mut _))
            .filter(|window| is_vrchat(*window));
        previous.ok_or("desktop_ocr.unavailable")?
    } else {
        return Err("desktop_ocr.not_game".into());
    };
    if unsafe { IsIconic(window) }.as_bool() {
        return Err("desktop_ocr.minimized".into());
    }
    capture_window(window).map_err(|error| {
        tracing::warn!(%error, "Desktop OCR capture failed");
        let code = match error.code() {
            E_ABORT => "desktop_ocr.minimized",
            E_HANDLE | E_NOINTERFACE | E_NOTIMPL | REGDB_E_CLASSNOTREG => "desktop_ocr.unavailable",
            code if code == HRESULT::from_win32(ERROR_TIMEOUT.0) => "desktop_ocr.capture_timeout",
            _ => "desktop_ocr.capture_failed",
        };
        code.into()
    })
}

fn is_vrchat(window: HWND) -> bool {
    if !unsafe { IsWindow(Some(window)) }.as_bool() {
        return false;
    }
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut path = vec![0u16; 32768];
        let mut length = path.len() as u32;
        let queried = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        queried.is_ok()
            && Path::new(&String::from_utf16_lossy(&path[..length as usize]))
                .file_name()
                .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("VRChat.exe"))
    }
}

struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

struct CaptureSession {
    pool: Direct3D11CaptureFramePool,
    session: Option<GraphicsCaptureSession>,
}

impl Drop for CaptureSession {
    fn drop(&mut self) {
        if let Some(session) = &self.session {
            let _ = session.Close();
        }
        let _ = self.pool.Close();
    }
}

fn capture_window(window: HWND) -> windows::core::Result<DesktopCapture> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let _apartment = Apartment;
    if !GraphicsCaptureSession::IsSupported()? {
        return Err(Error::new(
            E_NOTIMPL,
            "Windows Graphics Capture is unavailable",
        ));
    }
    let interop: IGraphicsCaptureItemInterop = factory::<GraphicsCaptureItem, _>()?;
    let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(window)? };
    let size = item.Size()?;
    if size.Width <= 0 || size.Height <= 0 {
        return Err(Error::new(E_HANDLE, "The game window has no visible area"));
    }
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device: ID3D11Device = device.ok_or_else(|| Error::from(E_POINTER))?;
    let context: ID3D11DeviceContext = context.ok_or_else(|| Error::from(E_POINTER))?;
    let dxgi: IDXGIDevice = device.cast()?;
    let capture_device: IDirect3DDevice =
        unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()? };
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &capture_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size,
    )?;
    let mut capture = CaptureSession {
        pool,
        session: None,
    };
    let session = capture.pool.CreateCaptureSession(&item)?;
    capture.session = Some(session.clone());
    session.SetIsCursorCaptureEnabled(false)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    session.StartCapture()?;
    let frame = loop {
        match capture.pool.TryGetNextFrame() {
            Ok(frame) => break frame,
            Err(error) if is_pending_frame(&error) => wait_for_frame(window, deadline)?,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        let content = frame.ContentSize()?;
        let (x, y, width, height) = client_crop(window, content.Width, content.Height)?;
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
        let pixels = read_pixels(
            &device,
            &context,
            &texture,
            (x, y, width, height),
            window,
            deadline,
        )?;
        Ok(DesktopCapture {
            window: window.0 as isize,
            width,
            height,
            pixels,
        })
    })();
    let _ = frame.Close();
    result
}

fn is_pending_frame(error: &Error) -> bool {
    // The WinRT projection maps an empty frame (S_OK + null) to Error::empty().
    matches!(error.code(), HRESULT(0) | E_POINTER)
}

fn wait_for_frame(window: HWND, deadline: Instant) -> windows::core::Result<()> {
    if !unsafe { IsWindow(Some(window)) }.as_bool() {
        return Err(Error::new(E_HANDLE, "The game window was closed"));
    }
    if unsafe { IsIconic(window) }.as_bool() {
        return Err(Error::new(E_ABORT, "The game window was minimized"));
    }
    if Instant::now() >= deadline {
        return Err(Error::new(
            HRESULT::from_win32(ERROR_TIMEOUT.0),
            "Timed out waiting for a game frame",
        ));
    }
    thread::sleep(Duration::from_millis(10));
    Ok(())
}

struct DpiContext(DPI_AWARENESS_CONTEXT);

impl Drop for DpiContext {
    fn drop(&mut self) {
        unsafe { SetThreadDpiAwarenessContext(self.0) };
    }
}

fn client_crop(
    window: HWND,
    frame_width: i32,
    frame_height: i32,
) -> windows::core::Result<(u32, u32, u32, u32)> {
    unsafe {
        let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        if previous.0.is_null() {
            return Err(Error::from_thread());
        }
        let _dpi = DpiContext(previous);
        let mut client = RECT::default();
        GetClientRect(window, &mut client)?;
        let mut origin = POINT {
            x: client.left,
            y: client.top,
        };
        ClientToScreen(window, &mut origin).ok()?;
        let mut bounds = RECT::default();
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut bounds as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )?;
        let x = origin.x - bounds.left;
        let y = origin.y - bounds.top;
        let width = client.right - client.left;
        let height = client.bottom - client.top;
        if bounds.right - bounds.left != frame_width
            || bounds.bottom - bounds.top != frame_height
            || x < 0
            || y < 0
            || width <= 0
            || height <= 0
            || x + width > frame_width
            || y + height > frame_height
        {
            return Err(Error::new(
                E_FAIL,
                "The game window changed size; capture again",
            ));
        }
        Ok((x as u32, y as u32, width as u32, height as u32))
    }
}

fn read_pixels(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    texture: &ID3D11Texture2D,
    crop: (u32, u32, u32, u32),
    window: HWND,
    deadline: Instant,
) -> windows::core::Result<Vec<u8>> {
    unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        if crop.0 + crop.2 > desc.Width || crop.1 + crop.3 > desc.Height {
            return Err(Error::new(
                E_FAIL,
                "The game window changed size; capture again",
            ));
        }
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;
        let mut staging = None;
        device.CreateTexture2D(&desc, None, Some(&mut staging))?;
        let staging = staging.ok_or_else(|| Error::from(E_POINTER))?;
        context.CopyResource(&staging, texture);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        loop {
            match context.Map(
                &staging,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&mut mapped),
            ) {
                Ok(()) => break,
                Err(error) if error.code() == DXGI_ERROR_WAS_STILL_DRAWING => {
                    context.Flush();
                    wait_for_frame(window, deadline)?;
                }
                Err(error) => return Err(error),
            }
        }
        // The mapped staging texture stays alive and mapped until all rows are copied.
        let bgra = std::slice::from_raw_parts(
            mapped.pData as *const u8,
            mapped.RowPitch as usize * desc.Height as usize,
        );
        let (x, y, width, height) = crop;
        let rgba = bgra_to_rgba(
            bgra,
            mapped.RowPitch as usize,
            x as usize,
            y as usize,
            width as usize,
            height as usize,
        );
        context.Unmap(&staging, 0);
        rgba.map_err(|message| Error::new(E_FAIL, message))
    }
}

fn bgra_to_rgba(
    pixels: &[u8],
    stride: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, String> {
    let end_x = x.checked_add(width).and_then(|n| n.checked_mul(4));
    let end_y = y.checked_add(height).and_then(|n| n.checked_mul(stride));
    if width == 0
        || height == 0
        || end_x.is_none_or(|n| n > stride)
        || end_y.is_none_or(|n| n > pixels.len())
    {
        return Err("The game client area is outside the captured frame".into());
    }
    let mut rgba = Vec::with_capacity(width * height * 4);
    for row in y..y + height {
        let start = row * stride + x * 4;
        for pixel in pixels[start..start + width * 4].as_chunks::<4>().0 {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_capture_frame_waits_without_masking_capture_failures() {
        // WinRT returns S_OK with a null object while the frame pool is empty.
        let frame: windows::core::Result<windows::Graphics::Capture::Direct3D11CaptureFrame> =
            unsafe { windows::core::Type::from_abi(std::ptr::null_mut()) };
        let error = frame.expect_err("The binding rejects a null frame");
        assert_eq!(error.code(), HRESULT(0));
        assert!(is_pending_frame(&error));
        assert!(is_pending_frame(&Error::from(E_POINTER)));
        assert!(!is_pending_frame(&Error::from(E_FAIL)));
    }

    #[test]
    fn crops_client_pixels_and_skips_bgra_row_padding() {
        let bgra = [
            1, 2, 3, 255, 4, 5, 6, 255, 0, 0, 0, 0, 7, 8, 9, 255, 10, 11, 12, 255, 0, 0, 0, 0,
        ];
        assert_eq!(
            bgra_to_rgba(&bgra, 12, 1, 0, 1, 2).unwrap(),
            [6, 5, 4, 255, 12, 11, 10, 255]
        );
    }

    #[test]
    fn rejects_a_crop_outside_the_mapped_buffer() {
        assert!(bgra_to_rgba(&[0; 16], 8, 1, 0, 2, 1).is_err());
        assert!(bgra_to_rgba(&[0; 16], 8, 0, 1, 1, 2).is_err());
    }
}
