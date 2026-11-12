//! CPU snapshot transport for the Linux WebKitGTK host.
//!
//! The host takes one snapshot at a time on the main thread. The worker never
//! blocks: `start_cropped` queues work and `poll` drains the completed frame.
use super::super::frame_receipt::FrameReceipt;
use super::linux_host::{HostSlot, Snapshot, MARKER_HEIGHT};
use super::metrics::CaptureCounters;
use super::recovery::Fault;
use crate::vr_overlay::renderer::Texture;
use std::sync::{mpsc::Receiver, Arc};

pub(crate) use super::error::CaptureError;

pub(crate) struct Captured {
    pub texture: Texture,
    pub receipt: FrameReceipt,
}

pub(crate) fn ensure_capture_supported() -> Result<(), CaptureError> {
    Ok(())
}

/// Construct, poll and drop on the VR worker thread. The WebView itself stays
/// on the main thread behind `HostSlot`.
pub struct Capture {
    slot: Arc<HostSlot>,
    width: u32,
    height: u32,
    pending: Option<(u32, [u32; 4], Receiver<Result<Snapshot, String>>)>,
    counters: Arc<CaptureCounters>,
    suspended: bool,
}

impl Capture {
    pub(super) fn with_counters(
        slot: &Arc<HostSlot>,
        _adapter_index: u32,
        width: u32,
        height: u32,
        counters: Arc<CaptureCounters>,
    ) -> Result<Self, CaptureError> {
        if width == 0 || height == 0 || height + MARKER_HEIGHT > 16384 {
            return Err("Invalid surface dimensions".into());
        }
        Ok(Self {
            slot: slot.clone(),
            width,
            height,
            pending: None,
            counters,
            suspended: false,
        })
    }

    pub fn active(&self) -> bool {
        !self.suspended
    }

    pub fn suspend(&mut self) {
        self.suspended = true;
        self.pending = None;
    }

    pub(super) fn resume(&mut self) -> Result<(), CaptureError> {
        self.suspended = false;
        Ok(())
    }

    pub(super) fn restart_source(&mut self, _slot: &Arc<HostSlot>) -> Result<(), CaptureError> {
        self.pending = None;
        self.suspended = false;
        Ok(())
    }

    pub(super) fn start_cropped(
        &mut self,
        requested: Option<u32>,
        bounds: [u32; 4],
    ) -> Result<bool, CaptureError> {
        if self.suspended || self.pending.is_some() {
            return Ok(false);
        }
        let Some(requested) = requested.filter(|serial| *serial != 0) else {
            return Ok(false);
        };
        let [left, top, right, bottom] = bounds;
        if left >= right || top >= bottom || right > self.width || bottom > self.height {
            return Err("Invalid capture crop".into());
        }
        let receiver = self
            .slot
            .request_snapshot(bounds)
            .map_err(|error| CaptureError::new(Fault::Host, error))?;
        CaptureCounters::increment(&self.counters.copies, 1);
        CaptureCounters::increment(
            &self.counters.copy_bytes,
            u64::from(right - left) * u64::from(bottom - top) * 4,
        );
        self.pending = Some((requested, bounds, receiver));
        Ok(true)
    }

    pub(super) fn poll(&mut self) -> Result<Option<Captured>, CaptureError> {
        let Some((requested, bounds, receiver)) = &self.pending else {
            return Ok(None);
        };
        match receiver.try_recv() {
            Ok(Ok(snapshot)) => {
                let (requested, bounds) = (*requested, *bounds);
                self.pending = None;
                Ok(Some(Captured {
                    texture: Texture {
                        pixels: snapshot.pixels,
                        width: snapshot.width,
                        height: snapshot.height,
                    },
                    receipt: FrameReceipt {
                        generation: snapshot.generation,
                        revision: snapshot.revision,
                        paint_serial: snapshot.paint,
                        requested_serial: requested,
                        bounds,
                    },
                }))
            }
            Ok(Err(error)) => {
                self.pending = None;
                Err(CaptureError::new(Fault::InvalidFrame, error))
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                Err(CaptureError::new(
                    Fault::Host,
                    "Surface snapshot channel disconnected",
                ))
            }
        }
    }

    pub(super) fn submitted(&mut self, _frame: &Captured) -> Result<(), CaptureError> {
        self.counters.submitted();
        Ok(())
    }
}
