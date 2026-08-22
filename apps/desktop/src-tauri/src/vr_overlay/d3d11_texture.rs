use std::ffi::c_void;
use std::ptr::{null, null_mut};

use super::renderer::Texture;

const D3D_DRIVER_TYPE_UNKNOWN: u32 = 0;
const D3D11_SDK_VERSION: u32 = 7;
const D3D11_CREATE_DEVICE_BGRA_SUPPORT: u32 = 0x20;
const D3D11_USAGE_DEFAULT: u32 = 0;
const D3D11_BIND_SHADER_RESOURCE: u32 = 0x8;
const D3D11_RESOURCE_MISC_SHARED: u32 = 0x2;
const DXGI_FORMAT_B8G8R8A8_UNORM: u32 = 87;

const QUERY_INTERFACE_INDEX: usize = 0;
const RELEASE_INDEX: usize = 2;
const ENUM_ADAPTERS_1_INDEX: usize = 12;
const CREATE_TEXTURE_2D_INDEX: usize = 5;
const GET_SHARED_HANDLE_INDEX: usize = 8;
const COPY_RESOURCE_INDEX: usize = 47;
const FLUSH_INDEX: usize = 111;

const IID_IDXGI_FACTORY_1: Guid = Guid {
    data1: 0x770a_ae78,
    data2: 0xf26f,
    data3: 0x4dba,
    data4: [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87],
};

const IID_IDXGI_RESOURCE: Guid = Guid {
    data1: 0x035f_3ab4,
    data2: 0x482e,
    data3: 0x4e50,
    data4: [0xb4, 0x1f, 0x8a, 0x7f, 0x8b, 0xd8, 0x96, 0x0b],
};

type QueryInterface = unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32;
type EnumAdapters1 = unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32;
type CreateTexture2d = unsafe extern "system" fn(
    *mut c_void,
    *const Texture2dDesc,
    *const SubresourceData,
    *mut *mut c_void,
) -> i32;
type GetSharedHandle = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32;
type CopyResource = unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void);
type Flush = unsafe extern "system" fn(*mut c_void);
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

pub struct OverlayTexture {
    texture: ComPtr,
    shared_handle: *mut c_void,
    width: u32,
    height: u32,
}

impl Device {
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

    pub fn create_shared_texture(&self, source: &Texture) -> Result<OverlayTexture, String> {
        validate(source)?;
        let texture = self.create_texture_resource(source, D3D11_RESOURCE_MISC_SHARED)?;
        let resource = query_interface(texture.0, &IID_IDXGI_RESOURCE)?;
        let get_shared_handle: GetSharedHandle =
            unsafe { std::mem::transmute(method(resource.0, GET_SHARED_HANDLE_INDEX)) };
        let mut shared_handle = null_mut();
        let result = unsafe { get_shared_handle(resource.0, &mut shared_handle) };
        if result < 0 || shared_handle.is_null() {
            return Err(format!(
                "IDXGIResource::GetSharedHandle failed with HRESULT 0x{result:08x}"
            ));
        }

        self.flush();
        Ok(OverlayTexture {
            texture,
            shared_handle,
            width: source.width,
            height: source.height,
        })
    }

    pub fn copy_texture(
        &self,
        destination: &OverlayTexture,
        source: &Texture,
    ) -> Result<(), String> {
        validate(source)?;
        if destination.width != source.width || destination.height != source.height {
            return Err("Overlay texture dimensions changed unexpectedly".into());
        }

        let upload = self.create_texture_resource(source, 0)?;
        let copy: CopyResource =
            unsafe { std::mem::transmute(method(self.context.0, COPY_RESOURCE_INDEX)) };
        unsafe { copy(self.context.0, destination.texture.0, upload.0) };
        self.flush();
        Ok(())
    }

    fn create_texture_resource(&self, source: &Texture, misc_flags: u32) -> Result<ComPtr, String> {
        let pitch = source.width * 4;
        let pixels = rgba_to_bgra(&source.pixels);
        let desc = Texture2dDesc {
            width: source.width,
            height: source.height,
            mip_levels: 1,
            array_size: 1,
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            sample_desc: SampleDesc {
                count: 1,
                quality: 0,
            },
            usage: D3D11_USAGE_DEFAULT,
            bind_flags: D3D11_BIND_SHADER_RESOURCE,
            cpu_access_flags: 0,
            misc_flags,
        };
        let initial = SubresourceData {
            system_memory: pixels.as_ptr().cast(),
            system_memory_pitch: pitch,
            system_memory_slice_pitch: pitch * source.height,
        };
        let create: CreateTexture2d =
            unsafe { std::mem::transmute(method(self.device.0, CREATE_TEXTURE_2D_INDEX)) };
        let mut texture = null_mut();
        let result = unsafe { create(self.device.0, &desc, &initial, &mut texture) };
        if result < 0 || texture.is_null() {
            unsafe { release(texture) };
            return Err(format!(
                "ID3D11Device::CreateTexture2D failed with HRESULT 0x{result:08x}"
            ));
        }

        Ok(ComPtr(texture))
    }

    fn flush(&self) {
        let flush: Flush = unsafe { std::mem::transmute(method(self.context.0, FLUSH_INDEX)) };
        unsafe { flush(self.context.0) };
    }
}

impl OverlayTexture {
    pub fn shared_handle(&self) -> *mut c_void {
        self.shared_handle
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

fn rgba_to_bgra(source: &[u8]) -> Vec<u8> {
    let mut pixels = source.to_vec();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    pixels
}

fn validate(texture: &Texture) -> Result<(), String> {
    let expected = texture
        .width
        .checked_mul(texture.height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Overlay texture dimensions overflow".to_string())?
        as usize;
    if texture.width == 0 || texture.height == 0 || texture.pixels.len() != expected {
        return Err("Overlay texture has invalid dimensions or pixel data".into());
    }
    Ok(())
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
    use super::rgba_to_bgra;

    #[test]
    fn converts_rgba_pixels_to_bgra_without_changing_alpha() {
        assert_eq!(
            rgba_to_bgra(&[10, 20, 30, 40, 50, 60, 70, 80]),
            vec![30, 20, 10, 40, 70, 60, 50, 80]
        );
    }
}
