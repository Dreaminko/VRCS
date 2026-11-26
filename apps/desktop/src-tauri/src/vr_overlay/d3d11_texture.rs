use std::cell::RefCell;
use std::ffi::c_void;
use std::rc::{Rc, Weak};

use windows::Win32::{
    Foundation::{E_POINTER, HMODULE},
    Graphics::{
        Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
        Direct3D11::{
            D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
            D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice,
            ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11ShaderResourceView,
            ID3D11Texture2D,
        },
        Dxgi::{
            Common::{
                DXGI_FORMAT_B8G8R8A8_TYPELESS, DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_B8G8R8X8_UNORM,
                DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM,
                DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
            },
            CreateDXGIFactory1, DXGI_ERROR_WAS_STILL_DRAWING, IDXGIAdapter1, IDXGIFactory1,
        },
    },
};
use windows::core::Interface;

use super::renderer::Texture;

pub(crate) struct SharedDevice {
    pub(crate) device: ID3D11Device,
    pub(crate) context: ID3D11DeviceContext,
    adapter_index: u32,
}

thread_local! {
    static SHARED_DEVICE: RefCell<Option<Weak<SharedDevice>>> = const { RefCell::new(None) };
}

pub struct Device {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    _shared: Rc<SharedDevice>,
    staging: [Option<Rc<StagingTexture>>; 2],
}

struct StagingTexture {
    texture: ID3D11Texture2D,
    width: u32,
    height: u32,
    format: u32,
}

pub struct Readback {
    staging: Rc<StagingTexture>,
    bgra: bool,
    pub bounds: [u32; 4],
    pub size: [u32; 2],
}

impl Device {
    pub fn raw_device(&self) -> *mut c_void {
        self.device.as_raw()
    }

    #[cfg(test)]
    pub unsafe fn read_shader_resource_region(
        &mut self,
        view: *mut c_void,
        region: impl FnOnce(u32, u32) -> Result<[u32; 4], String>,
    ) -> Result<(Texture, [u32; 4], [u32; 2]), String> {
        let readback = unsafe { self.begin_readback(0, view, region)? };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(image) = self.poll_readback(&readback)? {
                return Ok((image, readback.bounds, readback.size));
            }
            if std::time::Instant::now() >= deadline {
                return Err("Mirror readback timed out".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// The view must belong to this device; the copy stays on its owning context.
    pub unsafe fn begin_readback(
        &mut self,
        eye: usize,
        view: *mut c_void,
        region: impl FnOnce(u32, u32) -> Result<[u32; 4], String>,
    ) -> Result<Readback, String> {
        let Some(view) = (unsafe { ID3D11ShaderResourceView::from_raw_borrowed(&view) }) else {
            return Err("Mirror shader resource is null".into());
        };
        let resource = unsafe { view.GetResource() }
            .map_err(|_| "Mirror texture resource is null".to_string())?;
        let texture: ID3D11Texture2D = resource.cast().map_err(|e| {
            format!(
                "IUnknown::QueryInterface failed with HRESULT 0x{:08x}",
                e.code().0
            )
        })?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        let bgra = match desc.Format {
            DXGI_FORMAT_R8G8B8A8_TYPELESS
            | DXGI_FORMAT_R8G8B8A8_UNORM
            | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => false,
            DXGI_FORMAT_B8G8R8A8_TYPELESS
            | DXGI_FORMAT_B8G8R8A8_UNORM
            | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
            | DXGI_FORMAT_B8G8R8X8_UNORM => true,
            _ => return Err("Unsupported mirror pixel format".into()),
        };
        if desc.Width == 0
            || desc.Height == 0
            || desc.Width > 16384
            || desc.Height > 16384
            || desc.ArraySize != 1
            || desc.SampleDesc.Count != 1
        {
            return Err("Unsupported mirror texture dimensions or sampling".into());
        }
        let size = [desc.Width, desc.Height];
        let bounds @ [left, top, right, bottom] = region(desc.Width, desc.Height)?;
        if left >= right || top >= bottom || right > desc.Width || bottom > desc.Height {
            return Err("Invalid mirror capture region".into());
        }
        desc.Width = right - left;
        desc.Height = bottom - top;
        desc.MipLevels = 1;
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;
        let format = desc.Format.0 as u32;
        let slot = self.staging.get_mut(eye).ok_or("Invalid capture eye")?;
        if slot.as_ref().is_none_or(|cached| {
            cached.width != desc.Width || cached.height != desc.Height || cached.format != format
        }) {
            let mut staging = None;
            unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut staging)) }.map_err(
                |e| format!("Create mirror staging texture failed: 0x{:08x}", e.code().0),
            )?;
            let staging = staging.ok_or("Create mirror staging texture returned null")?;
            *slot = Some(Rc::new(StagingTexture {
                texture: staging,
                width: desc.Width,
                height: desc.Height,
                format,
            }));
        }
        let staging = slot.as_ref().unwrap().clone();
        // Copy only selected pixels from subresource zero before CPU readback.
        let source_box = D3D11_BOX {
            left,
            top,
            front: 0,
            right,
            bottom,
            back: 1,
        };
        unsafe {
            self.context.CopySubresourceRegion(
                &staging.texture,
                0,
                0,
                0,
                0,
                &texture,
                0,
                Some(&source_box),
            );
            self.context.Flush();
        }
        Ok(Readback {
            staging,
            bgra,
            bounds,
            size,
        })
    }

    pub fn poll_readback(&self, readback: &Readback) -> Result<Option<Texture>, String> {
        let staging = &readback.staging;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // D3D11_MAP_FLAG_DO_NOT_WAIT: never wait on the VR worker for GPU completion.
        match unsafe {
            self.context.Map(
                &staging.texture,
                0,
                D3D11_MAP_READ,
                D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                Some(&mut mapped),
            )
        } {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_WAS_STILL_DRAWING => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "Map mirror texture failed: 0x{:08x}",
                    error.code().0
                ));
            }
        }
        if mapped.pData.is_null() || mapped.RowPitch < staging.width * 4 {
            unsafe { self.context.Unmap(&staging.texture, 0) };
            return Err("Invalid mapped mirror row pitch".into());
        }
        let row_bytes = staging.width as usize * 4;
        let mut pixels = vec![0u8; row_bytes * staging.height as usize];
        let bgra = readback.bgra;
        for row in 0..staging.height as usize {
            let data = unsafe {
                std::slice::from_raw_parts(
                    mapped
                        .pData
                        .cast::<u8>()
                        .add(row * mapped.RowPitch as usize),
                    row_bytes,
                )
            };
            let dst = &mut pixels[row * row_bytes..(row + 1) * row_bytes];
            if bgra {
                for (pixel, out) in data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(dst.as_chunks_mut::<4>().0)
                {
                    *out = [pixel[2], pixel[1], pixel[0], 255];
                }
            } else {
                for (pixel, out) in data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(dst.as_chunks_mut::<4>().0)
                {
                    *out = [pixel[0], pixel[1], pixel[2], 255];
                }
            }
        }
        unsafe { self.context.Unmap(&staging.texture, 0) };
        Ok(Some(Texture {
            width: staging.width,
            height: staging.height,
            pixels,
        }))
    }

    pub fn create(adapter_index: u32) -> Result<Self, String> {
        let shared = acquire_shared_device(adapter_index)
            .map_err(|(stage, e)| format!("{stage} failed with HRESULT 0x{:08x}", e.code().0))?;
        Ok(Self {
            device: shared.device.clone(),
            context: shared.context.clone(),
            _shared: shared,
            staging: [None, None],
        })
    }
}

pub(crate) fn acquire_shared_device(
    adapter_index: u32,
) -> Result<Rc<SharedDevice>, (&'static str, windows::core::Error)> {
    if let Some(shared) = SHARED_DEVICE.with_borrow(|slot| slot.as_ref().and_then(Weak::upgrade))
        && shared.adapter_index == adapter_index
        && unsafe { shared.device.GetDeviceRemovedReason() }.is_ok()
    {
        return Ok(shared);
    }
    let adapter = dxgi_adapter(adapter_index)?;
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
    .map_err(|e| ("Create D3D11 device", e))?;
    let device = device.ok_or(("Create D3D11 device", E_POINTER.into()))?;
    let context = context.ok_or(("Create D3D11 device", E_POINTER.into()))?;
    let multithread: ID3D11Multithread = context
        .cast()
        .map_err(|e| ("Enable D3D11 multithread protection", e))?;
    unsafe {
        let _ = multithread.SetMultithreadProtected(true);
    }
    let shared = Rc::new(SharedDevice {
        device,
        context,
        adapter_index,
    });
    SHARED_DEVICE.with_borrow_mut(|slot| *slot = Some(Rc::downgrade(&shared)));
    Ok(shared)
}

fn dxgi_adapter(index: u32) -> Result<IDXGIAdapter1, (&'static str, windows::core::Error)> {
    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1() }.map_err(|e| ("Create DXGI factory", e))?;
    unsafe { factory.EnumAdapters1(index) }.map_err(|e| ("Select DXGI adapter", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_SHADER_RESOURCE, D3D11_SUBRESOURCE_DATA, D3D11_USAGE_DEFAULT,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;

    fn capture_test_texture(device: &Device, source: &Texture) -> ID3D11Texture2D {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: source.width,
            Height: source.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut pixels = source.pixels.clone();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        let initial = D3D11_SUBRESOURCE_DATA {
            pSysMem: pixels.as_ptr().cast(),
            SysMemPitch: source.width * 4,
            SysMemSlicePitch: source.width * source.height * 4,
        };
        let mut texture = None;
        unsafe {
            device
                .device
                .CreateTexture2D(&desc, Some(&initial), Some(&mut texture))
        }
        .unwrap();
        texture.unwrap()
    }

    #[test]
    #[ignore = "requires a Windows D3D11 adapter"]
    fn ocr_readback_preserves_rgba_and_row_pitch() {
        let mut device = Device::create(0).unwrap();
        let shared = acquire_shared_device(0).unwrap();
        assert_eq!(device.raw_device(), shared.device.as_raw());
        let source = Texture {
            width: 3,
            height: 2,
            pixels: vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140,
                150, 255, 160, 170, 180, 255,
            ],
        };
        let texture = capture_test_texture(&device, &source);
        let mut view = None;
        unsafe {
            device
                .device
                .CreateShaderResourceView(&texture, None, Some(&mut view))
        }
        .unwrap();
        let view = view.unwrap();
        let (captured, _, _) = unsafe {
            device.read_shader_resource_region(view.as_raw(), |width, height| {
                Ok([0, 0, width, height])
            })
        }
        .unwrap();
        assert_eq!((captured.width, captured.height), (3, 2));
        assert_eq!(captured.pixels, source.pixels);
        let (cropped, bounds, size) =
            unsafe { device.read_shader_resource_region(view.as_raw(), |_, _| Ok([1, 0, 2, 2])) }
                .unwrap();
        assert_eq!(bounds, [1, 0, 2, 2]);
        assert_eq!(size, [3, 2]);
        assert_eq!((cropped.width, cropped.height), (1, 2));
        assert_eq!(cropped.pixels, [40, 50, 60, 255, 130, 140, 150, 255]);
        assert!(
            unsafe { device.read_shader_resource_region(view.as_raw(), |_, _| Ok([2, 0, 4, 2])) }
                .is_err()
        );
        let readback =
            unsafe { device.begin_readback(0, view.as_raw(), |_, _| Ok([1, 0, 2, 2])) }.unwrap();
        let staging = readback.staging.texture.as_raw();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let pixels = loop {
            if let Some(image) = device.poll_readback(&readback).unwrap() {
                break image.pixels;
            }
            assert!(
                std::time::Instant::now() < until,
                "readback did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert_eq!(pixels, [40, 50, 60, 255, 130, 140, 150, 255]);
        drop(readback);
        let reused =
            unsafe { device.begin_readback(0, view.as_raw(), |_, _| Ok([1, 0, 2, 2])) }.unwrap();
        assert_eq!(reused.staging.texture.as_raw(), staging);
    }
}
