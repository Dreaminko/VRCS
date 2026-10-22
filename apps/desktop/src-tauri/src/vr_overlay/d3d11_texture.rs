use std::ffi::c_void;
use std::ptr::{null, null_mut};

use super::renderer::Texture;

const D3D_DRIVER_TYPE_UNKNOWN: u32 = 0;
const D3D11_SDK_VERSION: u32 = 7;
const D3D11_CREATE_DEVICE_BGRA_SUPPORT: u32 = 0x20;

const QUERY_INTERFACE_INDEX: usize = 0;
const RELEASE_INDEX: usize = 2;
const ENUM_ADAPTERS_1_INDEX: usize = 12;
const CREATE_TEXTURE_2D_INDEX: usize = 5;

const IID_IDXGI_FACTORY_1: Guid = Guid {
    data1: 0x770a_ae78,
    data2: 0xf26f,
    data3: 0x4dba,
    data4: [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87],
};

const IID_TEXTURE_2D: Guid = Guid {
    data1: 0x6f15_aaf2,
    data2: 0xd208,
    data3: 0x4e89,
    data4: [0x9a, 0xb4, 0x48, 0x95, 0x35, 0xd3, 0x4f, 0x9c],
};

#[repr(C)]
struct MappedSubresource {
    data: *mut c_void,
    row_pitch: u32,
    depth_pitch: u32,
}

type QueryInterface = unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32;
type EnumAdapters1 = unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32;
type CreateTexture2d = unsafe extern "system" fn(
    *mut c_void,
    *const Texture2dDesc,
    *const SubresourceData,
    *mut *mut c_void,
) -> i32;
type Release = unsafe extern "system" fn(*mut c_void) -> u32;

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[repr(C)]
struct SampleDesc {
    count: u32,
    quality: u32,
}

#[repr(C)]
struct Texture2dDesc {
    width: u32,
    height: u32,
    mip_levels: u32,
    array_size: u32,
    format: u32,
    sample_desc: SampleDesc,
    usage: u32,
    bind_flags: u32,
    cpu_access_flags: u32,
    misc_flags: u32,
}

#[repr(C)]
struct SubresourceData {
    system_memory: *const c_void,
    system_memory_pitch: u32,
    system_memory_slice_pitch: u32,
}

#[link(name = "dxgi")]
unsafe extern "system" {
    fn CreateDXGIFactory1(iid: *const Guid, factory: *mut *mut c_void) -> i32;
}

#[link(name = "d3d11")]
unsafe extern "system" {
    fn D3D11CreateDevice(
        adapter: *mut c_void,
        driver_type: u32,
        software: *mut c_void,
        flags: u32,
        feature_levels: *const u32,
        feature_level_count: u32,
        sdk_version: u32,
        device: *mut *mut c_void,
        selected_feature_level: *mut u32,
        immediate_context: *mut *mut c_void,
    ) -> i32;
}

pub struct Device {
    device: ComPtr,
    context: ComPtr,
}

impl Device {
    pub fn raw_device(&self) -> *mut c_void {
        self.device.0
    }

    /// The view must belong to this device and remain alive until readback ends.
    pub unsafe fn read_shader_resource_region(
        &self,
        view: *mut c_void,
        region: impl FnOnce(u32, u32) -> Result<[u32; 4], String>,
    ) -> Result<(Texture, [u32; 4], [u32; 2]), String> {
        if view.is_null() {
            return Err("Mirror shader resource is null".into());
        }
        type GetResource = unsafe extern "system" fn(*mut c_void, *mut *mut c_void);
        type GetDesc = unsafe extern "system" fn(*mut c_void, *mut Texture2dDesc);
        type Map = unsafe extern "system" fn(
            *mut c_void,
            *mut c_void,
            u32,
            u32,
            u32,
            *mut MappedSubresource,
        ) -> i32;
        type Unmap = unsafe extern "system" fn(*mut c_void, *mut c_void, u32);
        let get_resource: GetResource = unsafe { std::mem::transmute(method(view, 7)) };
        let mut resource = null_mut();
        unsafe {
            get_resource(view, &mut resource);
        }
        if resource.is_null() {
            return Err("Mirror texture resource is null".into());
        }
        let resource = ComPtr(resource);
        let texture = query_interface(resource.0, &IID_TEXTURE_2D)?;
        let get_desc: GetDesc = unsafe { std::mem::transmute(method(texture.0, 10)) };
        let mut desc: Texture2dDesc = unsafe { std::mem::zeroed() };
        unsafe {
            get_desc(texture.0, &mut desc);
        }
        let bgra = match desc.format {
            27..=29 => false,
            87 | 90 | 91 => true,
            _ => return Err("Unsupported mirror pixel format".into()),
        };
        if desc.width == 0
            || desc.height == 0
            || desc.width > 16384
            || desc.height > 16384
            || desc.array_size != 1
            || desc.sample_desc.count != 1
        {
            return Err("Unsupported mirror texture dimensions or sampling".into());
        }
        let size = [desc.width, desc.height];
        let bounds @ [left, top, right, bottom] = region(desc.width, desc.height)?;
        if left >= right || top >= bottom || right > desc.width || bottom > desc.height {
            return Err("Invalid mirror capture region".into());
        }
        desc.width = right - left;
        desc.height = bottom - top;
        desc.mip_levels = 1;
        desc.usage = 3; // D3D11_USAGE_STAGING
        desc.bind_flags = 0;
        desc.cpu_access_flags = 0x20000; // D3D11_CPU_ACCESS_READ
        desc.misc_flags = 0;
        let create: CreateTexture2d =
            unsafe { std::mem::transmute(method(self.device.0, CREATE_TEXTURE_2D_INDEX)) };
        let mut staging = null_mut();
        let result = unsafe { create(self.device.0, &desc, null(), &mut staging) };
        if result < 0 || staging.is_null() {
            unsafe {
                release(staging);
            }
            return Err(format!(
                "Create mirror staging texture failed: 0x{result:08x}"
            ));
        }
        let staging = ComPtr(staging);
        // Copy only selected pixels from subresource zero before CPU readback.
        type CopyRegion = unsafe extern "system" fn(
            *mut c_void,
            *mut c_void,
            u32,
            u32,
            u32,
            u32,
            *mut c_void,
            u32,
            *const c_void,
        );
        let copy: CopyRegion = unsafe { std::mem::transmute(method(self.context.0, 46)) };
        // D3D11_BOX: left, top, front, right, bottom, back.
        let source_box = [left, top, 0, right, bottom, 1];
        unsafe {
            copy(
                self.context.0,
                staging.0,
                0,
                0,
                0,
                0,
                texture.0,
                0,
                source_box.as_ptr().cast(),
            );
        }
        let map: Map = unsafe { std::mem::transmute(method(self.context.0, 14)) };
        let unmap: Unmap = unsafe { std::mem::transmute(method(self.context.0, 15)) };
        let mut mapped = MappedSubresource {
            data: null_mut(),
            row_pitch: 0,
            depth_pitch: 0,
        };
        let result = unsafe { map(self.context.0, staging.0, 0, 1, 0, &mut mapped) };
        if result < 0 {
            return Err(format!("Map mirror texture failed: 0x{result:08x}"));
        }
        if mapped.data.is_null() || mapped.row_pitch < desc.width * 4 {
            unsafe {
                unmap(self.context.0, staging.0, 0);
            }
            return Err("Invalid mapped mirror row pitch".into());
        }
        let row_bytes = desc.width as usize * 4;
        let mut pixels = Vec::with_capacity(row_bytes * desc.height as usize);
        for row in 0..desc.height as usize {
            let data = unsafe {
                std::slice::from_raw_parts(
                    mapped
                        .data
                        .cast::<u8>()
                        .add(row * mapped.row_pitch as usize),
                    row_bytes,
                )
            };
            pixels.extend_from_slice(data);
        }
        unsafe {
            unmap(self.context.0, staging.0, 0);
        }
        if bgra {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        // The compositor's alpha is not meaningful for recognition.
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        Ok((
            Texture {
                width: desc.width,
                height: desc.height,
                pixels,
            },
            bounds,
            size,
        ))
    }
    pub fn create(adapter_index: u32) -> Result<Self, String> {
        let adapter = dxgi_adapter(adapter_index)?;
        let mut device = null_mut();
        let mut context = null_mut();
        let result = unsafe {
            D3D11CreateDevice(
                adapter.0,
                D3D_DRIVER_TYPE_UNKNOWN,
                null_mut(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                null(),
                0,
                D3D11_SDK_VERSION,
                &mut device,
                null_mut(),
                &mut context,
            )
        };
        if result < 0 || device.is_null() || context.is_null() {
            unsafe {
                release(device);
                release(context);
            }
            return Err(format!(
                "D3D11CreateDevice failed with HRESULT 0x{result:08x}"
            ));
        }

        Ok(Self {
            device: ComPtr(device),
            context: ComPtr(context),
        })
    }
}

struct ComPtr(*mut c_void);

impl Drop for ComPtr {
    fn drop(&mut self) {
        unsafe { release(self.0) };
    }
}

fn dxgi_adapter(index: u32) -> Result<ComPtr, String> {
    let mut factory = null_mut();
    let result = unsafe { CreateDXGIFactory1(&IID_IDXGI_FACTORY_1, &mut factory) };
    if result < 0 || factory.is_null() {
        unsafe { release(factory) };
        return Err(format!(
            "CreateDXGIFactory1 failed with HRESULT 0x{result:08x}"
        ));
    }
    let factory = ComPtr(factory);

    let enumerate: EnumAdapters1 =
        unsafe { std::mem::transmute(method(factory.0, ENUM_ADAPTERS_1_INDEX)) };
    let mut adapter = null_mut();
    let result = unsafe { enumerate(factory.0, index, &mut adapter) };
    if result < 0 || adapter.is_null() {
        unsafe { release(adapter) };
        return Err(format!(
            "IDXGIFactory1::EnumAdapters1({index}) failed with HRESULT 0x{result:08x}"
        ));
    }
    Ok(ComPtr(adapter))
}

fn query_interface(object: *mut c_void, iid: &Guid) -> Result<ComPtr, String> {
    let query: QueryInterface =
        unsafe { std::mem::transmute(method(object, QUERY_INTERFACE_INDEX)) };
    let mut interface = null_mut();
    let result = unsafe { query(object, iid, &mut interface) };
    if result < 0 || interface.is_null() {
        unsafe { release(interface) };
        return Err(format!(
            "IUnknown::QueryInterface failed with HRESULT 0x{result:08x}"
        ));
    }
    Ok(ComPtr(interface))
}

unsafe fn method(object: *mut c_void, index: usize) -> *const c_void {
    let vtable = unsafe { *(object as *const *const *const c_void) };
    unsafe { *vtable.add(index) }
}

unsafe fn release(object: *mut c_void) {
    if object.is_null() {
        return;
    }
    let release: Release = unsafe { std::mem::transmute(method(object, RELEASE_INDEX)) };
    unsafe {
        release(object);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture_test_texture(device: &Device, source: &Texture) -> ComPtr {
        let desc = Texture2dDesc {
            width: source.width,
            height: source.height,
            mip_levels: 1,
            array_size: 1,
            format: 87, // DXGI_FORMAT_B8G8R8A8_UNORM
            sample_desc: SampleDesc {
                count: 1,
                quality: 0,
            },
            usage: 0,      // D3D11_USAGE_DEFAULT
            bind_flags: 8, // D3D11_BIND_SHADER_RESOURCE
            cpu_access_flags: 0,
            misc_flags: 0,
        };
        let mut pixels = source.pixels.clone();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        let initial = SubresourceData {
            system_memory: pixels.as_ptr().cast(),
            system_memory_pitch: source.width * 4,
            system_memory_slice_pitch: source.width * source.height * 4,
        };
        let create: CreateTexture2d =
            unsafe { std::mem::transmute(method(device.device.0, CREATE_TEXTURE_2D_INDEX)) };
        let mut texture = null_mut();
        assert!(unsafe { create(device.device.0, &desc, &initial, &mut texture) } >= 0);
        assert!(!texture.is_null());
        ComPtr(texture)
    }

    #[test]
    #[ignore = "requires a Windows D3D11 adapter"]
    fn ocr_readback_preserves_rgba_and_row_pitch() {
        let device = Device::create(0).unwrap();
        let source = Texture {
            width: 3,
            height: 2,
            pixels: vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 130, 140,
                150, 255, 160, 170, 180, 255,
            ],
        };
        let texture = capture_test_texture(&device, &source);
        type CreateView = unsafe extern "system" fn(
            *mut c_void,
            *mut c_void,
            *const c_void,
            *mut *mut c_void,
        ) -> i32;
        let create: CreateView = unsafe { std::mem::transmute(method(device.device.0, 7)) };
        let mut view = null_mut();
        assert!(unsafe { create(device.device.0, texture.0, null(), &mut view) } >= 0);
        let view = ComPtr(view);
        let (captured, _, _) = unsafe {
            device.read_shader_resource_region(view.0, |width, height| Ok([0, 0, width, height]))
        }
        .unwrap();
        assert_eq!((captured.width, captured.height), (3, 2));
        assert_eq!(captured.pixels, source.pixels);
        let (cropped, bounds, size) =
            unsafe { device.read_shader_resource_region(view.0, |_, _| Ok([1, 0, 2, 2])) }.unwrap();
        assert_eq!(bounds, [1, 0, 2, 2]);
        assert_eq!(size, [3, 2]);
        assert_eq!((cropped.width, cropped.height), (1, 2));
        assert_eq!(cropped.pixels, [40, 50, 60, 255, 130, 140, 150, 255]);
        assert!(
            unsafe { device.read_shader_resource_region(view.0, |_, _| Ok([2, 0, 4, 2])) }.is_err()
        );
    }
}
