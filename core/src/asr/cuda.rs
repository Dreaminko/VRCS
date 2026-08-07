use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CudaCapability {
    pub available: bool,
    pub device_count: u32,
    pub error: Option<String>,
}

pub fn cuda_capability() -> CudaCapability {
    match cuda_device_count() {
        Ok(device_count) if device_count > 0 => CudaCapability {
            available: true,
            device_count,
            error: None,
        },
        Ok(_) => CudaCapability {
            available: false,
            device_count: 0,
            error: Some("未发现 CUDA 设备".into()),
        },
        Err(error) => CudaCapability {
            available: false,
            device_count: 0,
            error: Some(error),
        },
    }
}

#[cfg(all(feature = "cuda", windows))]
fn cuda_device_count() -> Result<u32, String> {
    use windows::core::s;
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

    type CuInit = unsafe extern "system" fn(u32) -> i32;
    type CuDeviceGetCount = unsafe extern "system" fn(*mut i32) -> i32;

    unsafe {
        let library = LoadLibraryA(s!("nvcuda.dll"))
            .map_err(|error| format!("无法加载 CUDA 驱动：{error}"))?;
        let result = (|| {
            let init = GetProcAddress(library, s!("cuInit"))
                .ok_or_else(|| "CUDA 驱动缺少 cuInit".to_string())?;
            let get_count = GetProcAddress(library, s!("cuDeviceGetCount"))
                .ok_or_else(|| "CUDA 驱动缺少 cuDeviceGetCount".to_string())?;
            let init: CuInit = std::mem::transmute(init);
            let get_count: CuDeviceGetCount = std::mem::transmute(get_count);

            let status = init(0);
            if status != 0 {
                return Err(format!("CUDA 驱动初始化失败（错误码 {status}）"));
            }
            let mut count = 0;
            let status = get_count(&mut count);
            if status != 0 {
                return Err(format!("无法枚举 CUDA 设备（错误码 {status}）"));
            }
            Ok(count.max(0) as u32)
        })();
        let _ = FreeLibrary(library);
        result
    }
}

#[cfg(not(all(feature = "cuda", windows)))]
fn cuda_device_count() -> Result<u32, String> {
    Err("当前构建未包含 CUDA 后端".into())
}
