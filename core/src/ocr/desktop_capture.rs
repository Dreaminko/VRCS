use std::{
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use windows::{
    core::{factory, Error, Interface, BOOL, HRESULT},
    Graphics::{
        Capture::{
            Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind,
            GraphicsCaptureItem, GraphicsCaptureSession, IGraphicsCaptureSession3,
        },
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Security::Authorization::AppCapabilityAccess::AppCapabilityAccessStatus,
    Win32::{
        Foundation::{
            ERROR_TIMEOUT, E_ACCESSDENIED, E_FAIL, E_HANDLE, E_NOINTERFACE, E_NOTIMPL, E_POINTER,
            HMODULE, HWND, LPARAM, RECT, REGDB_E_CLASSNOTREG,
        },
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
                D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
                D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_SDK_VERSION,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
            },
            Dwm::DwmFlush,
            Dxgi::{IDXGIDevice, DXGI_ERROR_WAS_STILL_DRAWING},
            Gdi::{EnumDisplayMonitors, HDC, HMONITOR},
        },
        System::{
            Com::CoIncrementMTAUsage,
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
                GetWindowDisplayAffinity, IsWindow, IsWindowVisible, SetWindowDisplayAffinity,
                WDA_EXCLUDEFROMCAPTURE, WINDOW_DISPLAY_AFFINITY,
            },
        },
    },
};

pub struct DesktopCapture {
    pub origin: (i32, i32),
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Captures visible desktop content for local region selection, without changing focus.
/// Call from a blocking worker. Each monitor has a two-second capture/readback deadline.
pub fn capture_desktop(excluded_windows: &[isize]) -> Result<DesktopCapture, String> {
    capture_desktop_inner(excluded_windows).map_err(|error| {
        tracing::warn!(%error, "Desktop capture failed");
        let code = match error.code() {
            E_HANDLE | E_NOINTERFACE | E_NOTIMPL | REGDB_E_CLASSNOTREG => "desktop_ocr.unavailable",
            code if code == HRESULT::from_win32(ERROR_TIMEOUT.0) => "desktop_ocr.capture_timeout",
            _ => "desktop_ocr.capture_failed",
        };
        code.into()
    })
}

struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

struct DpiContext(DPI_AWARENESS_CONTEXT);

impl Drop for DpiContext {
    fn drop(&mut self) {
        unsafe { SetThreadDpiAwarenessContext(self.0) };
    }
}

// Restore screenshot visibility even if capture fails or its async caller is cancelled.
struct ExcludedWindows(Vec<(HWND, WINDOW_DISPLAY_AFFINITY)>);

impl ExcludedWindows {
    fn new(windows: &[isize]) -> windows::core::Result<Self> {
        let mut guard = Self(Vec::new());
        for &window in windows {
            let window = HWND(window as *mut _);
            if !unsafe { IsWindow(Some(window)) }.as_bool()
                || !unsafe { IsWindowVisible(window) }.as_bool()
            {
                continue;
            }
            let mut affinity = 0;
            unsafe {
                GetWindowDisplayAffinity(window, &mut affinity)?;
                SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE)?;
            }
            guard.0.push((window, WINDOW_DISPLAY_AFFINITY(affinity)));
        }
        if !guard.0.is_empty() {
            unsafe { DwmFlush()? };
        }
        Ok(guard)
    }
}

impl Drop for ExcludedWindows {
    fn drop(&mut self) {
        for &(window, affinity) in &self.0 {
            let _ = unsafe { SetWindowDisplayAffinity(window, affinity) };
        }
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

// The immediate context must not be used by overlapping capture workers.
static CAPTURE_DEVICE: Mutex<Option<CaptureDevice>> = Mutex::new(None);

struct CaptureDevice {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
}

impl CaptureDevice {
    fn new() -> windows::core::Result<Self> {
        if !GraphicsCaptureSession::IsSupported()? {
            return Err(Error::new(
                E_NOTIMPL,
                "Windows Graphics Capture is unavailable",
            ));
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
        let context = context.ok_or_else(|| Error::from(E_POINTER))?;
        Ok(Self { device, context })
    }
}

fn monitors() -> windows::core::Result<Vec<(HMONITOR, RECT)>> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _: HDC,
        rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(data.0 as *mut Vec<(HMONITOR, RECT)>);
        monitors.push((monitor, *rect));
        BOOL(1)
    }
    let mut monitors = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut monitors as *mut _ as isize),
        )
        .ok()?;
    }
    if monitors.is_empty() {
        return Err(Error::new(E_HANDLE, "No desktop monitors are available"));
    }
    Ok(monitors)
}

fn desktop_bounds(monitors: &[(HMONITOR, RECT)]) -> windows::core::Result<RECT> {
    let mut bounds = monitors
        .first()
        .ok_or_else(|| Error::new(E_HANDLE, "No desktop monitors are available"))?
        .1;
    for (_, rect) in monitors {
        bounds.left = bounds.left.min(rect.left);
        bounds.top = bounds.top.min(rect.top);
        bounds.right = bounds.right.max(rect.right);
        bounds.bottom = bounds.bottom.max(rect.bottom);
    }
    let width = i64::from(bounds.right) - i64::from(bounds.left);
    let height = i64::from(bounds.bottom) - i64::from(bounds.top);
    // The picker holds additional image buffers; also keep Win32 mouse coordinates valid.
    if width <= 0
        || height <= 0
        || width > i16::MAX as i64
        || height > i16::MAX as i64
        || width * height > 64 * 1024 * 1024
    {
        return Err(Error::new(E_FAIL, "The desktop is too large to capture"));
    }
    Ok(bounds)
}

fn capture_desktop_inner(excluded_windows: &[isize]) -> windows::core::Result<DesktopCapture> {
    // WinRT caches factories across calls. Keep the MTA alive for the process lifetime,
    // including when blocking workers retire, so those factories remain valid.
    static MTA: OnceLock<Result<(), HRESULT>> = OnceLock::new();
    MTA.get_or_init(|| unsafe {
        CoIncrementMTAUsage()
            .map(|_| ())
            .map_err(|error| error.code())
    })
    .map_err(Error::from)?;
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let _apartment = Apartment;
    let previous =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    if previous.0.is_null() {
        return Err(Error::from_thread());
    }
    let _dpi = DpiContext(previous);
    let displays = monitors()?;
    let bounds = desktop_bounds(&displays)?;
    let width = (bounds.right - bounds.left) as u32;
    let height = (bounds.bottom - bounds.top) as u32;
    let mut capture = DesktopCapture {
        origin: (bounds.left, bounds.top),
        width,
        height,
        pixels: vec![0; width as usize * height as usize * 4],
    };
    let mut cached = CAPTURE_DEVICE.lock().unwrap();
    if cached
        .as_ref()
        .is_some_and(|graphics| unsafe { graphics.device.GetDeviceRemovedReason().is_err() })
    {
        *cached = None;
    }
    if cached.is_none() {
        *cached = Some(CaptureDevice::new()?);
    }
    let graphics = cached.as_ref().unwrap();
    let dxgi: IDXGIDevice = graphics.device.cast()?;
    let capture_device: IDirect3DDevice =
        unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()? };
    let interop: IGraphicsCaptureItemInterop = factory::<GraphicsCaptureItem, _>()?;
    let _excluded = ExcludedWindows::new(excluded_windows)?;
    for &(monitor, rect) in &displays {
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };
        capture_monitor_into(&item, &capture_device, graphics, &rect, &mut capture)?;
    }
    if monitors()? != displays {
        return Err(Error::new(
            E_FAIL,
            "The display layout changed; capture again",
        ));
    }
    Ok(capture)
}

fn capture_monitor_into(
    item: &GraphicsCaptureItem,
    capture_device: &IDirect3DDevice,
    graphics: &CaptureDevice,
    bounds: &RECT,
    capture: &mut DesktopCapture,
) -> windows::core::Result<()> {
    let size = item.Size()?;
    if size.Width != bounds.right - bounds.left || size.Height != bounds.bottom - bounds.top {
        return Err(Error::new(
            E_FAIL,
            "The display layout changed; capture again",
        ));
    }
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        capture_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        size,
    )?;
    let mut capture_session = CaptureSession {
        pool,
        session: None,
    };
    let session = capture_session.pool.CreateCaptureSession(item)?;
    capture_session.session = Some(session.clone());
    session.SetIsCursorCaptureEnabled(false)?;
    if let Err(error) = disable_capture_border(&session) {
        tracing::warn!(%error, "Borderless desktop capture is unavailable; keeping the system border");
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    session.StartCapture()?;
    let frame = loop {
        match capture_session.pool.TryGetNextFrame() {
            Ok(frame) => break frame,
            Err(error) if is_pending_frame(&error) => wait_for_frame(deadline)?,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        let content = frame.ContentSize()?;
        if content.Width != size.Width || content.Height != size.Height {
            return Err(Error::new(
                E_FAIL,
                "The display layout changed; capture again",
            ));
        }
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
        read_pixels_into(
            graphics,
            &texture,
            bounds,
            (size.Width as u32, size.Height as u32),
            deadline,
            capture,
        )
    })();
    let _ = frame.Close();
    result
}

fn disable_capture_border(session: &GraphicsCaptureSession) -> windows::core::Result<()> {
    // Check support before requesting access on older Windows versions.
    session.cast::<IGraphicsCaptureSession3>()?;
    static BORDERLESS_ALLOWED: OnceLock<bool> = OnceLock::new();
    let allowed = *BORDERLESS_ALLOWED.get_or_init(|| {
        GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless)
            .and_then(|op| op.join())
            .is_ok_and(|access| access == AppCapabilityAccessStatus::Allowed)
    });
    if !allowed {
        return Err(Error::new(
            E_ACCESSDENIED,
            "Borderless capture access was denied",
        ));
    }
    session.SetIsBorderRequired(false)
}

#[cfg(test)]
fn paste_monitor(
    capture: &mut DesktopCapture,
    rect: &RECT,
    pixels: &[u8],
) -> windows::core::Result<()> {
    let x = i64::from(rect.left) - i64::from(capture.origin.0);
    let y = i64::from(rect.top) - i64::from(capture.origin.1);
    let width = i64::from(rect.right) - i64::from(rect.left);
    let height = i64::from(rect.bottom) - i64::from(rect.top);
    if x < 0
        || y < 0
        || width <= 0
        || height <= 0
        || x + width > capture.width as i64
        || y + height > capture.height as i64
        || width * height * 4 != pixels.len() as i64
    {
        return Err(Error::new(
            E_FAIL,
            "The monitor is outside the desktop image",
        ));
    }
    let stride = capture.width as usize * 4;
    let row_bytes = width as usize * 4;
    for row in 0..height as usize {
        let offset = (y as usize + row) * stride + x as usize * 4;
        capture.pixels[offset..offset + row_bytes]
            .copy_from_slice(&pixels[row * row_bytes..(row + 1) * row_bytes]);
    }
    Ok(())
}

fn is_pending_frame(error: &Error) -> bool {
    // The WinRT projection maps an empty frame (S_OK + null) to Error::empty().
    matches!(error.code(), HRESULT(0) | E_POINTER)
}

fn wait_for_frame(deadline: Instant) -> windows::core::Result<()> {
    if Instant::now() >= deadline {
        return Err(Error::new(
            HRESULT::from_win32(ERROR_TIMEOUT.0),
            "Timed out waiting for a desktop frame",
        ));
    }
    thread::sleep(Duration::from_millis(2));
    Ok(())
}

fn read_pixels_into(
    graphics: &CaptureDevice,
    texture: &ID3D11Texture2D,
    bounds: &RECT,
    (width, height): (u32, u32),
    deadline: Instant,
    capture: &mut DesktopCapture,
) -> windows::core::Result<()> {
    unsafe {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        if width > desc.Width || height > desc.Height {
            return Err(Error::new(
                E_FAIL,
                "The display layout changed; capture again",
            ));
        }
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;
        let mut staging = None;
        graphics
            .device
            .CreateTexture2D(&desc, None, Some(&mut staging))?;
        let staging = staging.ok_or_else(|| Error::from(E_POINTER))?;
        graphics.context.CopyResource(&staging, texture);
        graphics.context.Flush();
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        loop {
            match graphics.context.Map(
                &staging,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&mut mapped),
            ) {
                Ok(()) => break,
                Err(error) if error.code() == DXGI_ERROR_WAS_STILL_DRAWING => {
                    graphics.context.Flush();
                    wait_for_frame(deadline)?;
                }
                Err(error) => return Err(error),
            }
        }
        // The mapped staging texture stays alive and mapped until all rows are copied.
        let bgra = std::slice::from_raw_parts(
            mapped.pData as *const u8,
            mapped.RowPitch as usize * desc.Height as usize,
        );
        let result = bgra_to_rgba_into(
            capture,
            bounds,
            bgra,
            mapped.RowPitch as usize,
            width as usize,
            height as usize,
        );
        graphics.context.Unmap(&staging, 0);
        result.map_err(|message| Error::new(E_FAIL, message))
    }
}

fn bgra_to_rgba_into(
    capture: &mut DesktopCapture,
    rect: &RECT,
    pixels: &[u8],
    stride: usize,
    width: usize,
    height: usize,
) -> Result<(), String> {
    let row_bytes = width.checked_mul(4);
    let end = height.checked_mul(stride);
    let x = i64::from(rect.left) - i64::from(capture.origin.0);
    let y = i64::from(rect.top) - i64::from(capture.origin.1);
    let rect_width = i64::from(rect.right) - i64::from(rect.left);
    let rect_height = i64::from(rect.bottom) - i64::from(rect.top);
    if width == 0
        || height == 0
        || row_bytes.is_none_or(|n| n > stride)
        || end.is_none_or(|n| n > pixels.len())
        || x < 0
        || y < 0
        || rect_width != width as i64
        || rect_height != height as i64
        || x + width as i64 > capture.width as i64
        || y + height as i64 > capture.height as i64
    {
        return Err("The image is outside the captured frame".into());
    }
    let row_bytes = width * 4;
    let dst_stride = capture.width as usize * 4;
    let dst_x = x as usize * 4;
    let dst_y = y as usize;
    for row in 0..height {
        let source = &pixels[row * stride..row * stride + row_bytes];
        let offset = (dst_y + row) * dst_stride + dst_x;
        let destination = &mut capture.pixels[offset..offset + row_bytes];
        for (source, destination) in source
            .as_chunks::<4>()
            .0
            .iter()
            .zip(destination.as_chunks_mut::<4>().0)
        {
            *destination = [source[2], source[1], source[0], 255];
        }
    }
    Ok(())
}

#[cfg(test)]
fn bgra_to_rgba(
    pixels: &[u8],
    stride: usize,
    width: usize,
    height: usize,
) -> Result<Vec<u8>, String> {
    let row_bytes = width.checked_mul(4);
    let end = height.checked_mul(stride);
    if width == 0
        || height == 0
        || row_bytes.is_none_or(|n| n > stride)
        || end.is_none_or(|n| n > pixels.len())
    {
        return Err("The image is outside the captured frame".into());
    }
    let row_bytes = width * 4;
    let mut rgba = vec![0; row_bytes * height];
    for row in 0..height {
        let start = row * stride;
        let source = &pixels[start..start + row_bytes];
        let destination = &mut rgba[row * row_bytes..(row + 1) * row_bytes];
        for (source, destination) in source
            .as_chunks::<4>()
            .0
            .iter()
            .zip(destination.as_chunks_mut::<4>().0)
        {
            *destination = [source[2], source[1], source[0], 255];
        }
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires an interactive Windows desktop"]
    fn captures_repeatedly_without_a_game_window() {
        let mut devices = Vec::new();
        for _ in 0..2 {
            let capture = thread::spawn(|| capture_desktop(&[]))
                .join()
                .unwrap()
                .expect("Desktop capture failed");
            assert!(capture.width > 0 && capture.height > 0);
            assert_eq!(
                capture.pixels.len(),
                capture.width as usize * capture.height as usize * 4
            );
            devices.push(
                CAPTURE_DEVICE
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .device
                    .clone(),
            );
        }
        assert_eq!(
            devices[0], devices[1],
            "Capture workers must reuse the device"
        );
    }

    #[test]
    #[ignore = "Requires an interactive Windows desktop"]
    fn concurrent_capture_workers_share_the_device_safely() {
        let workers: Vec<_> = (0..3)
            .map(|_| thread::spawn(|| capture_desktop(&[])))
            .collect();
        for worker in workers {
            let capture = worker.join().unwrap().expect("Concurrent capture failed");
            assert!(capture.width > 0 && capture.height > 0);
            assert_eq!(
                capture.pixels.len(),
                capture.width as usize * capture.height as usize * 4
            );
        }
    }

    #[test]
    #[ignore = "Requires an interactive Windows desktop"]
    fn recreates_an_empty_capture_device_cache() {
        capture_desktop(&[]).unwrap();
        let old_device = CAPTURE_DEVICE.lock().unwrap().take().unwrap().device;
        capture_desktop(&[]).unwrap();
        let cached = CAPTURE_DEVICE.lock().unwrap();
        let new_device = &cached.as_ref().unwrap().device;
        assert_ne!(&old_device, new_device);
        assert!(unsafe { new_device.GetDeviceRemovedReason() }.is_ok());
    }

    #[test]
    #[ignore = "Requires an interactive Windows desktop"]
    fn restores_result_window_capture_visibility() {
        use windows::{
            core::w,
            Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
                WS_VISIBLE,
            },
        };
        unsafe {
            let window = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("STATIC"),
                w!(""),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                32,
                32,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let mut before = 0;
            let mut during = 0;
            let mut after = 0;
            let result = (|| -> windows::core::Result<()> {
                GetWindowDisplayAffinity(window, &mut before)?;
                let excluded = ExcludedWindows::new(&[window.0 as isize])?;
                GetWindowDisplayAffinity(window, &mut during)?;
                drop(excluded);
                GetWindowDisplayAffinity(window, &mut after)
            })();
            DestroyWindow(window).unwrap();
            result.unwrap();
            assert_eq!(during, WDA_EXCLUDEFROMCAPTURE.0);
            assert_eq!(after, before);
        }
    }

    #[test]
    fn null_capture_frame_waits_without_masking_capture_failures() {
        let frame: windows::core::Result<windows::Graphics::Capture::Direct3D11CaptureFrame> =
            unsafe { windows::core::Type::from_abi(std::ptr::null_mut()) };
        let error = frame.expect_err("The binding rejects a null frame");
        assert_eq!(error.code(), HRESULT(0));
        assert!(is_pending_frame(&error));
        assert!(is_pending_frame(&Error::from(E_POINTER)));
        assert!(!is_pending_frame(&Error::from(E_FAIL)));
    }

    #[test]
    fn converts_pixels_and_skips_bgra_row_padding() {
        let bgra = [
            1, 2, 3, 255, 4, 5, 6, 255, 0, 0, 0, 0, 7, 8, 9, 255, 10, 11, 12, 255, 0, 0, 0, 0,
        ];
        assert_eq!(
            bgra_to_rgba(&bgra, 12, 2, 2).unwrap(),
            [3, 2, 1, 255, 6, 5, 4, 255, 9, 8, 7, 255, 12, 11, 10, 255]
        );
    }

    #[test]
    fn rejects_invalid_pixel_buffers() {
        assert!(bgra_to_rgba(&[0; 16], 8, 3, 1).is_err());
        assert!(bgra_to_rgba(&[0; 16], 8, 1, 3).is_err());
        assert!(bgra_to_rgba(&[], 0, 0, 0).is_err());
    }

    #[test]
    fn combines_monitors_with_negative_coordinates_and_gaps() {
        let displays = [
            (
                HMONITOR::default(),
                RECT {
                    left: -2,
                    top: -1,
                    right: 0,
                    bottom: 1,
                },
            ),
            (
                HMONITOR::default(),
                RECT {
                    left: 1,
                    top: 0,
                    right: 3,
                    bottom: 2,
                },
            ),
        ];
        let bounds = desktop_bounds(&displays).unwrap();
        assert_eq!(
            (bounds.left, bounds.top, bounds.right, bounds.bottom),
            (-2, -1, 3, 2)
        );
        let mut capture = DesktopCapture {
            origin: (-2, -1),
            width: 5,
            height: 3,
            pixels: vec![0; 60],
        };
        paste_monitor(&mut capture, &displays[0].1, &[1; 16]).unwrap();
        paste_monitor(&mut capture, &displays[1].1, &[2; 16]).unwrap();
        let row = |y: usize| {
            capture.pixels[y * 20..(y + 1) * 20]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| p[0])
                .collect::<Vec<_>>()
        };
        assert_eq!(row(0), [1, 1, 0, 0, 0]);
        assert_eq!(row(1), [1, 1, 0, 2, 2]);
        assert_eq!(row(2), [0, 0, 0, 2, 2]);
        assert!(paste_monitor(&mut capture, &displays[0].1, &[1; 12]).is_err());
    }

    #[test]
    fn rejects_empty_or_oversized_desktops() {
        assert!(desktop_bounds(&[]).is_err());
        for rect in [
            RECT::default(),
            RECT {
                left: 0,
                top: 0,
                right: 32768,
                bottom: 1,
            },
            RECT {
                left: 0,
                top: 0,
                right: 16384,
                bottom: 16384,
            },
        ] {
            assert!(desktop_bounds(&[(HMONITOR::default(), rect)]).is_err());
        }
    }
}
