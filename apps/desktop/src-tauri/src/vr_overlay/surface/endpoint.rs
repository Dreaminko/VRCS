//! Where the VR worker obtains pixels for one surface host.
use super::super::surface_protocol::HostId;
#[cfg(linux_vr)]
use super::linux_host::HostSlot;
#[cfg(linux_vr)]
use std::sync::Arc;
#[cfg(windows)]
use windows::Graphics::Capture::GraphicsCaptureItem;

#[cfg(windows)]
pub(crate) type CaptureSource = GraphicsCaptureItem;
#[cfg(linux_vr)]
pub(crate) type CaptureSource = Arc<HostSlot>;

/// Kept alive by the surface entry and by the backend while OpenVR may still
/// reference the submitted texture.
#[derive(Clone)]
pub(crate) struct CaptureEndpoint {
    pub host: HostId,
    pub source: CaptureSource,
    #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
    pub parent: isize,
}
