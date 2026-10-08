use std::ffi::CStr;
use std::sync::OnceLock;

use serde::Serialize;
use whisper_rs::whisper_rs_sys as sys;

pub(super) struct GpuDevice {
    pub backend: String,
    pub index: i32,
}

pub(super) fn gpu_devices() -> &'static [GpuDevice] {
    static DEVICES: OnceLock<Vec<GpuDevice>> = OnceLock::new();
    DEVICES.get_or_init(|| {
        let mut devices = Vec::new();
        // GGML owns these device and registry pointers for the process lifetime.
        // Whisper numbers GPU and integrated GPU devices together, excluding CPUs.
        unsafe {
            for index in 0..sys::ggml_backend_dev_count() {
                let device = sys::ggml_backend_dev_get(index);
                let kind = sys::ggml_backend_dev_type(device);
                if kind != sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_GPU
                    && kind != sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_IGPU
                {
                    continue;
                }
                let registry = sys::ggml_backend_dev_backend_reg(device);
                let backend = CStr::from_ptr(sys::ggml_backend_reg_name(registry))
                    .to_string_lossy()
                    .into_owned();
                devices.push(GpuDevice {
                    backend,
                    index: devices.len() as i32,
                });
            }
        }
        devices
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct VulkanCapability {
    pub available: bool,
    pub device_count: usize,
    pub error: Option<String>,
}

pub fn vulkan_capability() -> VulkanCapability {
    let device_count = if cfg!(feature = "vulkan") {
        gpu_devices()
            .iter()
            .filter(|device| device.backend == "Vulkan")
            .count()
    } else {
        0
    };
    VulkanCapability {
        available: device_count > 0,
        device_count,
        error: if !cfg!(feature = "vulkan") {
            Some("This build does not include the Vulkan backend".into())
        } else if device_count == 0 {
            Some("No Vulkan devices found".into())
        } else {
            None
        },
    }
}
