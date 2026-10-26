//! Captures an undecorated WebView host directly into SteamVR-compatible textures.

use std::{
    marker::PhantomData,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use windows::{
    core::{factory, IInspectable, Interface, BOOL},
    Foundation::{TimeSpan, TypedEventHandler},
    Graphics::{
        Capture::{
            Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
            GraphicsCaptureSession,
        },
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        SizeInt32,
    },
    Win32::{
        Foundation::{HMODULE, HWND},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Query, ID3D11Texture2D,
                D3D11_ASYNC_GETDATA_DONOTFLUSH, D3D11_BIND_RENDER_TARGET,
                D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_CPU_ACCESS_READ,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
                D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_QUERY_DESC, D3D11_QUERY_EVENT,
                D3D11_RESOURCE_MISC_SHARED, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
            },
            Dxgi::{
                Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
                CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1, IDXGIResource,
                DXGI_ERROR_WAS_STILL_DRAWING,
            },
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED,
        },
    },
};

const MARKER_WIDTH: u32 = 256;
pub const MARKER_HEIGHT: u32 = 16;

#[derive(Clone, Copy, Debug)]
pub struct Captured {
    pub shared_handle: usize,
    pub generation: u32,
    pub revision: u32,
    pub paint_serial: u32,
    pub requested_serial: u32,
}

/// Construct, poll, and drop on the same VR worker thread.
pub struct Capture {
    pending: Option<Pending>,
    source: Option<Source>,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    marker: ID3D11Texture2D,
    query: ID3D11Query,
    slots: [Slot; 2],
    next_slot: usize,
    width: u32,
    height: u32,
    hwnd: isize,
    counters: Arc<super::metrics::CaptureCounters>,
    // COM objects must be released before the apartment is uninitialized.
    _apartment: Apartment,
}

struct Apartment(PhantomData<Rc<()>>);

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

struct Slot {
    texture: ID3D11Texture2D,
    // A legacy DXGI shared handle belongs to its texture, not CloseHandle.
    handle: usize,
}

struct Pending {
    _frame: Frame,
    slot: usize,
    requested_serial: u32,
}

struct Frame(Direct3D11CaptureFrame);

impl Drop for Frame {
    fn drop(&mut self) {
        let _ = self.0.Close();
    }
}

struct Source {
    session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
    item: GraphicsCaptureItem,
    closed_token: Option<i64>,
    closed: Arc<AtomicBool>,
    frame_token: Option<i64>,
}

impl Source {
    fn close(&mut self) {
        if let Some(token) = self.frame_token.take() {
            let _ = self.pool.RemoveFrameArrived(token);
        }
        if let Some(token) = self.closed_token.take() {
            let _ = self.item.RemoveClosed(token);
        }
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.close();
    }
}

impl Capture {
    pub(super) fn with_counters(
        hwnd: isize,
        adapter_index: u32,
        width: u32,
        height: u32,
        counters: Arc<super::metrics::CaptureCounters>,
    ) -> Result<Self, String> {
        let capture_height = height
            .checked_add(MARKER_HEIGHT)
            .ok_or("Invalid surface size")?;
        if hwnd == 0
            || width < MARKER_WIDTH
            || height == 0
            || width > 16384
            || capture_height > 16384
        {
            return Err("Invalid WebView capture handle or dimensions".into());
        }
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .map_err(|e| failure("Initialize WinRT", e))?;
        let apartment = Apartment(PhantomData);
        if !GraphicsCaptureSession::IsSupported().map_err(|e| failure("Check WGC support", e))? {
            return Err("Windows Graphics Capture is unavailable".into());
        }

        let (device, context) = create_device(adapter_index)?;
        let slots = [
            create_slot(&device, width, height)?,
            create_slot(&device, width, height)?,
        ];
        let mut marker_desc = texture_desc(MARKER_WIDTH, 2);
        marker_desc.Usage = D3D11_USAGE_STAGING;
        marker_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let marker = create_texture(&device, &marker_desc)?;
        let mut query = None;
        unsafe {
            device.CreateQuery(
                &D3D11_QUERY_DESC {
                    Query: D3D11_QUERY_EVENT,
                    MiscFlags: 0,
                },
                Some(&mut query),
            )
        }
        .map_err(|e| failure("Create capture completion query", e))?;
        let query = query.ok_or("D3D11 returned no capture query")?;
        let source = Source::new(&device, hwnd, width, capture_height, &counters)?;
        Ok(Self {
            pending: None,
            source: Some(source),
            context,
            device,
            marker,
            query,
            slots,
            next_slot: 0,
            width,
            height,
            hwnd,
            counters,
            _apartment: apartment,
        })
    }

    pub fn active(&self) -> bool {
        self.source.is_some()
    }

    pub fn suspend(&mut self) {
        // Stop delivery before releasing in-flight frames. D3D keeps queued resources alive.
        self.source.take();
        self.pending.take();
    }

    pub fn resume(&mut self) -> Result<(), String> {
        if self.source.is_none() {
            self.source = Some(Source::new(
                &self.device,
                self.hwnd,
                self.width,
                self.height + MARKER_HEIGHT,
                &self.counters,
            )?);
        }
        Ok(())
    }

    /// Completes a pending copy without waiting or starting another one.
    pub fn poll(&mut self) -> Result<Option<Captured>, String> {
        let Some(source) = &self.source else {
            return Ok(None);
        };
        if source.closed.load(Ordering::Acquire) {
            return Err("The WebView capture window was closed".into());
        }
        unsafe { self.device.GetDeviceRemovedReason() }
            .map_err(|e| failure("Capture device lost", e))?;
        if self.pending.is_some() {
            return self.finish();
        }
        Ok(None)
    }

    /// Start only after handling the completed frame, so its submitted slot is protected.
    pub fn start(&mut self, requested: Option<u32>) -> Result<bool, String> {
        if self.source.is_none() || self.pending.is_some() {
            return Ok(false);
        }
        let Some(requested_serial) = requested.filter(|serial| *serial != 0) else {
            return Ok(false);
        };
        let Some(frame) = self.next_frame()? else {
            return Ok(false);
        };
        let size = frame
            .0
            .ContentSize()
            .map_err(|e| failure("Read captured frame size", e))?;
        check_size(size, self.width, self.height + MARKER_HEIGHT)?;
        let surface = frame
            .0
            .Surface()
            .map_err(|e| failure("Get WGC frame surface", e))?;
        let access: IDirect3DDxgiInterfaceAccess = surface
            .cast()
            .map_err(|e| failure("Get captured DXGI surface", e))?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface() }
            .map_err(|e| failure("Get captured D3D11 texture", e))?;
        let content = D3D11_BOX {
            left: 0,
            top: 0,
            front: 0,
            right: self.width,
            bottom: self.height,
            back: 1,
        };
        unsafe {
            self.context.CopySubresourceRegion(
                &self.slots[self.next_slot].texture,
                0,
                0,
                0,
                0,
                &texture,
                0,
                Some(&content),
            );
            for row in 0..2 {
                let marker = D3D11_BOX {
                    left: 0,
                    top: self.height + 4 + row * 8,
                    front: 0,
                    right: MARKER_WIDTH,
                    bottom: self.height + 5 + row * 8,
                    back: 1,
                };
                self.context.CopySubresourceRegion(
                    &self.marker,
                    0,
                    0,
                    row,
                    0,
                    &texture,
                    0,
                    Some(&marker),
                );
            }
            self.context.End(&self.query);
            self.context.Flush();
        }
        super::metrics::CaptureCounters::increment(&self.counters.copies, 1);
        super::metrics::CaptureCounters::increment(
            &self.counters.copy_bytes,
            u64::from(self.width) * u64::from(self.height) * 4,
        );
        self.pending = Some(Pending {
            _frame: frame,
            slot: self.next_slot,
            requested_serial,
        });
        Ok(true)
    }

    fn next_frame(&self) -> Result<Option<Frame>, String> {
        let Some(source) = &self.source else {
            return Ok(None);
        };
        let mut latest = None;
        // Keep latency bounded without spinning while capture is producing frames.
        for _ in 0..2 {
            match source.pool.TryGetNextFrame() {
                Ok(frame) => latest = Some(Frame(frame)),
                Err(error) if error.code().0 == 0 => break,
                Err(error) => return Err(failure("Get next WGC frame", error)),
            }
        }
        Ok(latest)
    }

    fn completed(&self, flags: u32) -> Result<bool, String> {
        let mut ready = BOOL(0);
        unsafe {
            self.context.GetData(
                &self.query,
                Some((&mut ready as *mut BOOL).cast()),
                std::mem::size_of::<BOOL>() as u32,
                flags,
            )
        }
        .map_err(|e| failure("Check capture GPU completion", e))?;
        Ok(ready.as_bool())
    }

    fn finish(&mut self) -> Result<Option<Captured>, String> {
        if !self.completed(D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32)? {
            return Ok(None);
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        match unsafe {
            self.context.Map(
                &self.marker,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&mut mapped),
            )
        } {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_WAS_STILL_DRAWING => return Ok(None),
            Err(error) => return Err(failure("Read capture marker", error)),
        }
        let key = if mapped.pData.is_null() || mapped.RowPitch < MARKER_WIDTH * 4 {
            None
        } else {
            // Only two 1 KiB rows cross from GPU to CPU, including row-pitch padding.
            let rows = unsafe {
                std::slice::from_raw_parts(
                    mapped.pData.cast(),
                    mapped.RowPitch as usize + (MARKER_WIDTH * 4) as usize,
                )
            };
            decode_marker(rows, mapped.RowPitch as usize)
        };
        unsafe {
            self.context.Unmap(&self.marker, 0);
        }
        let pending = self
            .pending
            .take()
            .expect("capture completion requires an in-flight frame");
        let Some((generation, revision, paint_serial)) = key else {
            return Ok(None);
        };
        Ok(Some(Captured {
            shared_handle: self.slots[pending.slot].handle,
            generation,
            revision,
            paint_serial,
            requested_serial: pending.requested_serial,
        }))
    }

    /// Protect the submitted slot. Idle surfaces must never start a speculative copy.
    pub fn submitted(&mut self, frame: Captured) -> Result<(), String> {
        let slot = self
            .slots
            .iter()
            .position(|slot| slot.handle == frame.shared_handle)
            .expect("submitted texture belongs to this capture");
        self.next_slot = 1 - slot;
        super::metrics::CaptureCounters::increment(&self.counters.submissions, 1);
        Ok(())
    }

    /// Full readback is restricted to the development capture smoke test.
    #[cfg(all(debug_assertions, feature = "vr-preview"))]
    pub fn snapshot(&self, frame: Captured) -> Result<Vec<u8>, String> {
        let slot = self
            .slots
            .iter()
            .find(|slot| slot.handle == frame.shared_handle)
            .ok_or("Unknown capture texture")?;
        let mut desc = texture_desc(self.width, self.height);
        desc.Usage = D3D11_USAGE_STAGING;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let staging = create_texture(&self.device, &desc)?;
        unsafe {
            self.context.CopyResource(&staging, &slot.texture);
        }
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(|e| failure("Read smoke test texture", e))?;
        let mut pixels = Vec::with_capacity((self.width * self.height * 4) as usize);
        for y in 0..self.height as usize {
            let row = unsafe {
                std::slice::from_raw_parts(
                    mapped.pData.cast::<u8>().add(y * mapped.RowPitch as usize),
                    self.width as usize * 4,
                )
            };
            pixels.extend_from_slice(row);
        }
        unsafe {
            self.context.Unmap(&staging, 0);
        }
        Ok(pixels)
    }
}

impl Source {
    fn new(
        device: &ID3D11Device,
        hwnd: isize,
        width: u32,
        capture_height: u32,
        counters: &Arc<super::metrics::CaptureCounters>,
    ) -> Result<Self, String> {
        let dxgi_device: IDXGIDevice = device.cast().map_err(|e| failure("Get DXGI device", e))?;
        let capture_device: IDirect3DDevice =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device) }
                .and_then(|value| value.cast())
                .map_err(|e| failure("Create WinRT D3D11 device", e))?;
        let interop: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .map_err(|e| failure("Create WGC factory", e))?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(HWND(hwnd as *mut _)) }
            .map_err(|e| failure("Capture WebView window", e))?;
        let size = item.Size().map_err(|e| failure("Read WGC item size", e))?;
        check_size(size, width, capture_height)?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &capture_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )
        .map_err(|e| failure("Create WGC frame pool", e))?;
        let session = match pool.CreateCaptureSession(&item) {
            Ok(session) => session,
            Err(error) => {
                let _ = pool.Close();
                return Err(failure("Create WGC session", error));
            }
        };
        let mut source = Source {
            session,
            pool,
            item,
            closed_token: None,
            closed: Arc::new(AtomicBool::new(false)),
            frame_token: None,
        };
        let closed = source.closed.clone();
        source.closed_token = Some(
            source
                .item
                .Closed(
                    &TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(move |_, _| {
                        closed.store(true, Ordering::Release);
                        Ok(())
                    }),
                )
                .map_err(|e| failure("Observe capture closure", e))?,
        );
        source
            .session
            .SetIsCursorCaptureEnabled(false)
            .map_err(|e| failure("Disable capture cursor", e))?;
        // Older Windows versions may not expose this interface.
        let interval_supported = source
            .session
            .SetMinUpdateInterval(TimeSpan { Duration: 166_667 })
            .is_ok();
        if super::metrics::enabled() {
            tracing::info!(interval_supported, "WGC minimum update interval capability");
        }
        if super::metrics::enabled() {
            let counters = counters.clone();
            source.frame_token = Some(
                source
                    .pool
                    .FrameArrived(
                        &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(
                            move |_, _| {
                                super::metrics::CaptureCounters::increment(
                                    &counters.source_frames,
                                    1,
                                );
                                Ok(())
                            },
                        ),
                    )
                    .map_err(|e| failure("Observe source frames", e))?,
            );
        }
        source
            .session
            .StartCapture()
            .map_err(|e| failure("Start WebView capture", e))?;

        Ok(source)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // Close the pool before returning a frame, so WGC cannot reuse its surface.
        // D3D11 defers resource destruction for queued copies; do not wait on the GPU.
        self.source.take();
        self.pending.take();
        unsafe {
            self.context.ClearState();
            self.context.Flush();
        }
    }
}

fn create_device(adapter_index: u32) -> Result<(ID3D11Device, ID3D11DeviceContext), String> {
    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1() }.map_err(|e| failure("Create capture DXGI factory", e))?;
    let adapter = unsafe { factory.EnumAdapters1(adapter_index) }
        .map_err(|e| failure("Select SteamVR capture adapter", e))?;
    let (mut device, mut context) = (None, None);
    unsafe {
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }
    .map_err(|e| failure("Create capture D3D11 device", e))?;
    Ok((
        device.ok_or("D3D11 returned no capture device")?,
        context.ok_or("D3D11 returned no capture context")?,
    ))
}

fn texture_desc(width: u32, height: u32) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        ..Default::default()
    }
}

fn create_texture(
    device: &ID3D11Device,
    desc: &D3D11_TEXTURE2D_DESC,
) -> Result<ID3D11Texture2D, String> {
    let mut texture = None;
    unsafe { device.CreateTexture2D(desc, None, Some(&mut texture)) }
        .map_err(|e| failure("Create capture texture", e))?;
    texture.ok_or_else(|| "D3D11 returned no capture texture".into())
}

fn create_slot(device: &ID3D11Device, width: u32, height: u32) -> Result<Slot, String> {
    let mut desc = texture_desc(width, height);
    desc.BindFlags = (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32;
    desc.MiscFlags = D3D11_RESOURCE_MISC_SHARED.0 as u32;
    let texture = create_texture(device, &desc)?;
    let resource: IDXGIResource = texture
        .cast()
        .map_err(|e| failure("Share capture texture", e))?;
    let handle = unsafe { resource.GetSharedHandle() }
        .map_err(|e| failure("Get capture texture shared handle", e))?;
    if handle.is_invalid() {
        return Err("D3D11 returned an invalid shared texture handle".into());
    }
    Ok(Slot {
        texture,
        handle: handle.0 as usize,
    })
}

fn check_size(size: SizeInt32, width: u32, height: u32) -> Result<(), String> {
    if size.Width != width as i32 || size.Height != height as i32 {
        return Err(format!(
            "WebView capture size changed: expected {width}x{height}, got {}x{}",
            size.Width, size.Height
        ));
    }
    Ok(())
}

fn decode_marker(rows: &[u8], row_pitch: usize) -> Option<(u32, u32, u32)> {
    let mut values = [0u32; 4];
    for bit in 0..128 {
        let offset = (bit / 64) * row_pitch + ((bit % 64) * 4 + 2) * 4;
        let pixel = rows.get(offset..offset + 4)?;
        let one = pixel[..3].iter().all(|&channel| channel >= 224);
        if pixel[3] < 224 || (!one && !pixel[..3].iter().all(|&channel| channel <= 31)) {
            return None;
        }
        values[bit / 32] = (values[bit / 32] << 1) | u32::from(one);
    }
    (values[0] != 0 && values[1] != 0 && values[2] != 0 && values[3] == !values[2])
        .then_some((values[0], values[1], values[2]))
}

fn failure(operation: &str, error: windows::core::Error) -> String {
    format!("{operation}: {error}")
}

#[cfg(test)]
mod tests {
    use super::decode_marker;

    #[test]
    fn validates_two_marker_rows_with_padding_and_paint_complement() {
        let values = [0x8020_0103u32, 0x0408_10ffu32, 0xffff_fffeu32, 1u32];
        let pitch = 1280;
        let mut rows = vec![0; pitch * 2];
        for (index, value) in values.into_iter().enumerate() {
            for bit in 0..32 {
                let color = if value & (1 << (31 - bit)) != 0 {
                    255
                } else {
                    0
                };
                for x in 0..4 {
                    let offset = (index / 2) * pitch + ((index % 2) * 32 * 4 + bit * 4 + x) * 4;
                    rows[offset..offset + 4].copy_from_slice(&[color, color, color, 255]);
                }
            }
        }
        assert_eq!(
            decode_marker(&rows, pitch),
            Some((values[0], values[1], values[2]))
        );
        rows[pitch + 8] = 128;
        assert_eq!(decode_marker(&rows, pitch), None);
        assert_eq!(decode_marker(&[0; 2048], 1024), None);
    }
}
