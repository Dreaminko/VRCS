//! Backend-neutral tracking identifiers and poses.
//!
//! OpenVR indices, XrPaths, action sources and tracking-universe origins stay in
//! the backend adapters. Shared rendering and interaction code sees only these
//! opaque values, which stay stable for one backend session.

/// Backend-local device identity (one controller or hand per session).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceId(pub u32);

/// Backend-local origin for poses. OpenVR maps its tracking universe here;
/// OpenXR maps its reference-space generation, so values are never mixed
/// across backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpaceId(pub u32);

/// Opaque pointing source. OpenVR keeps its action source here; OpenXR can use
/// the device identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PointerId {
    pub(crate) device: DeviceId,
    pub(crate) source: u64,
}

impl PointerId {
    pub(crate) fn new(device: DeviceId, source: u64) -> Self {
        Self { device, source }
    }

    pub fn device(self) -> DeviceId {
        self.device
    }
}

pub type Pose = [[f32; 4]; 3];

/// Build a row-major 3x4 pose from a position and quaternion `[x, y, z, w]`.
pub fn pose_from_quaternion(position: [f32; 3], orientation: [f32; 4]) -> Pose {
    let [x, y, z, w] = orientation;
    let norm = (x * x + y * y + z * z + w * w).sqrt();
    let (x, y, z, w) = if norm > 0.0 {
        (x / norm, y / norm, z / norm, w / norm)
    } else {
        (0.0, 0.0, 0.0, 1.0)
    };
    let (xx, yy, zz) = (x * x, y * y, z * z);
    let (xy, xz, yz) = (x * y, x * z, y * z);
    let (wx, wy, wz) = (w * x, w * y, w * z);
    [
        [
            1. - 2. * (yy + zz),
            2. * (xy - wz),
            2. * (xz + wy),
            position[0],
        ],
        [
            2. * (xy + wz),
            1. - 2. * (xx + zz),
            2. * (yz - wx),
            position[1],
        ],
        [
            2. * (xz - wy),
            2. * (yz + wx),
            1. - 2. * (xx + yy),
            position[2],
        ],
    ]
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackedPose {
    pub pose: Pose,
    pub space: SpaceId,
}

#[derive(Debug, Clone, Copy)]
pub struct PointerInput {
    pub pointer: PointerId,
    pub device: DeviceId,
    pub trigger: Option<bool>,
    pub grip: Option<bool>,
    pub scroll_y: f32,
}
