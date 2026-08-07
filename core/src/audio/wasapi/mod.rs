mod capture;
mod devices;
mod pcm;

pub(crate) use capture::capture_main;
pub(crate) use devices::{find_process_id, list_devices, resolve_device_id};

pub(crate) enum CaptureTarget {
    Process(u32),
    Device {
        wasapi_id: Option<String>,
        direction: DeviceDirection,
    },
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum DeviceDirection {
    Render,
    Capture,
}
