use vrcs_core::VrOverlayHeadsetConfig;
#[cfg(any(windows, linux_vr, test))]
use vrcs_core::{VrOverlayWristConfig, VrOverlayWristMenuConfig};

pub fn headset(config: &VrOverlayHeadsetConfig) -> [[f32; 4]; 3] {
    matrix(
        config.pitch_deg,
        config.yaw_deg,
        config.roll_deg,
        [config.offset_x_m, config.offset_y_m, -config.distance_m],
    )
}

#[cfg(any(windows, linux_vr, test))]
pub fn wrist(config: &VrOverlayWristConfig) -> [[f32; 4]; 3] {
    matrix(
        config.pitch_deg,
        config.yaw_deg,
        config.roll_deg,
        [config.offset_x_m, config.offset_y_m, config.offset_z_m],
    )
}

#[cfg(any(windows, linux_vr, test))]
pub const WRIST_MENU_CAPSULE_OFFSET: f32 = 0.40;

#[cfg(any(windows, linux_vr, test))]
pub fn wrist_menu(config: &VrOverlayWristMenuConfig) -> [[f32; 4]; 3] {
    compose(
        matrix(
            config.pitch_deg,
            config.yaw_deg,
            config.roll_deg,
            [config.offset_x_m, config.offset_y_m, config.offset_z_m],
        ),
        matrix(
            0.0,
            0.0,
            0.0,
            [0.0, WRIST_MENU_CAPSULE_OFFSET * config.width_m, 0.0],
        ),
    )
}

pub fn compose(parent: [[f32; 4]; 3], child: [[f32; 4]; 3]) -> [[f32; 4]; 3] {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            let product: f32 = (0..3).map(|k| parent[row][k] * child[k][column]).sum();
            product + if column == 3 { parent[row][3] } else { 0. }
        })
    })
}

/// Inverts a rigid OpenVR pose: transpose the rotation, then transform its translation.
pub fn inverse(pose: [[f32; 4]; 3]) -> [[f32; 4]; 3] {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            if column < 3 {
                pose[column][row]
            } else {
                -(0..3).map(|k| pose[k][row] * pose[k][3]).sum::<f32>()
            }
        })
    })
}

/// Builds a row-major OpenVR transform using intrinsic X (pitch), Y (yaw),
/// then Z (roll) rotations. The composed matrix is Rz * Ry * Rx.
pub fn matrix(pitch_deg: f32, yaw_deg: f32, roll_deg: f32, translation: [f32; 3]) -> [[f32; 4]; 3] {
    let (sx, cx) = pitch_deg.to_radians().sin_cos();
    let (sy, cy) = yaw_deg.to_radians().sin_cos();
    let (sz, cz) = roll_deg.to_radians().sin_cos();

    [
        [
            cz * cy,
            cz * sy * sx - sz * cx,
            cz * sy * cx + sz * sx,
            translation[0],
        ],
        [
            sz * cy,
            sz * sy * sx + cz * cx,
            sz * sy * cx - cz * sx,
            translation[1],
        ],
        [-sy, cy * sx, cy * cx, translation[2]],
    ]
}

/// Decomposes a rigid row-major 3x4 pose back into `(pitch_deg, yaw_deg, roll_deg, translation)`.
#[cfg(any(windows, linux_vr, test))]
pub fn decompose(pose: [[f32; 4]; 3]) -> (f32, f32, f32, [f32; 3]) {
    let translation = [pose[0][3], pose[1][3], pose[2][3]];
    let yaw = (-pose[2][0]).clamp(-1.0, 1.0).asin();
    let (pitch, roll) = if yaw.cos().abs() > 1e-5 {
        (pose[2][1].atan2(pose[2][2]), pose[1][0].atan2(pose[0][0]))
    } else {
        ((-pose[1][2]).atan2(pose[1][1]), 0.0)
    };
    (
        pitch.to_degrees(),
        yaw.to_degrees(),
        roll.to_degrees(),
        translation,
    )
}

#[cfg(any(windows, linux_vr, test))]
fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (len.is_finite() && len > 1e-5).then(|| [v[0] / len, v[1] / len, v[2] / len])
}

#[cfg(any(windows, linux_vr, test))]
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(any(windows, linux_vr, test))]
fn basis_pose(right: [f32; 3], up: [f32; 3], forward: [f32; 3], t: [f32; 3]) -> [[f32; 4]; 3] {
    [
        [right[0], up[0], forward[0], t[0]],
        [right[1], up[1], forward[1], t[1]],
        [right[2], up[2], forward[2], t[2]],
    ]
}

/// Removes roll relative to world up while preserving the panel's +Z normal.
#[cfg(any(windows, linux_vr, test))]
pub fn level_roll(pose: [[f32; 4]; 3]) -> [[f32; 4]; 3] {
    let Some(z) = normalize([pose[0][2], pose[1][2], pose[2][2]]) else {
        return pose;
    };
    let Some(right) = normalize(cross([0.0, 1.0, 0.0], z)) else {
        return pose;
    };
    let up = cross(z, right);
    basis_pose(right, up, z, [pose[0][3], pose[1][3], pose[2][3]])
}

/// Builds a level pose at `position` whose +Z front normal faces `target`.
#[cfg(any(windows, linux_vr, test))]
pub fn face_point(position: [f32; 3], target: [f32; 3]) -> Option<[[f32; 4]; 3]> {
    let z = normalize([
        target[0] - position[0],
        target[1] - position[1],
        target[2] - position[2],
    ])?;
    let right = normalize(cross([0.0, 1.0, 0.0], z))?;
    let up = cross(z, right);
    Some(basis_pose(right, up, z, position))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
    }

    #[test]
    fn identity_keeps_translation() {
        let value = matrix(0.0, 0.0, 0.0, [1.0, 2.0, 3.0]);
        assert_eq!(
            value,
            [
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 2.0],
                [0.0, 0.0, 1.0, 3.0],
            ]
        );
    }

    #[test]
    fn ninety_degree_yaw_rotates_forward_to_left() {
        let value = matrix(0.0, 90.0, 0.0, [0.0; 3]);
        assert_close(value[0][0], 0.0);
        assert_close(value[0][2], 1.0);
        assert_close(value[2][0], -1.0);
        assert_close(value[2][2], 0.0);
    }

    #[test]
    fn eye_offset_is_rotated_and_translated_into_tracking_space() {
        let headset = matrix(0., 90., 0., [1., 2., 3.]);
        let eye = matrix(0., 0., 0., [-0.032, 0., 0.]);
        let value = compose(headset, eye);
        assert_close(value[0][3], 1.);
        assert_close(value[1][3], 2.);
        assert_close(value[2][3], 3.032);
        assert_close(value[0][2], 1.);
        assert_close(value[2][0], -1.);
    }

    #[test]
    fn headset_uses_negative_forward_distance() {
        let config = VrOverlayHeadsetConfig {
            offset_x_m: 0.1,
            offset_y_m: -0.2,
            distance_m: 1.4,
            ..Default::default()
        };
        let value = headset(&config);
        assert_close(value[0][3], 0.1);
        assert_close(value[1][3], -0.2);
        assert_close(value[2][3], -1.4);
    }

    #[test]
    fn decompose_round_trips_normal_and_gimbal_lock_poses() {
        for (p, y, r, t) in [
            (-8.0, 0.0, 0.0, [0.1, -0.28, -1.2]),
            (-55.0, 35.0, -22.0, [0.03, 0.08, -0.06]),
            (25.0, -140.0, 170.0, [-0.4, 0.5, -2.1]),
            (32.0, 90.0, 0.0, [0.0, 0.2, -1.0]),
            (-18.0, -90.0, 0.0, [0.3, -0.1, -0.8]),
        ] {
            let m = matrix(p, y, r, t);
            let (dp, dy, dr, dt) = decompose(m);
            let rebuilt = matrix(dp, dy, dr, dt);
            for (a, b) in m.into_iter().flatten().zip(rebuilt.into_iter().flatten()) {
                assert_close(a, b);
            }
        }
    }

    #[test]
    fn wrist_menu_defaults_match_legacy_anchor_and_free_rotation() {
        let cfg = VrOverlayWristMenuConfig::default();
        let m = wrist_menu(&cfg);
        assert_close(m[0][3], 0.0);
        assert_close(m[1][3], 0.06);
        assert_close(m[2][3], 0.09);
        assert_close(m[1][2], 1.0);

        let rotated = VrOverlayWristMenuConfig {
            pitch_deg: -45.0,
            yaw_deg: 30.0,
            roll_deg: 90.0,
            ..cfg.clone()
        };
        let rm = wrist_menu(&rotated);
        // Capsule center stays at [offset_x_m, offset_y_m, offset_z_m].
        let center_x = rm[0][3] + rm[0][1] * (-WRIST_MENU_CAPSULE_OFFSET * rotated.width_m);
        let center_y = rm[1][3] + rm[1][1] * (-WRIST_MENU_CAPSULE_OFFSET * rotated.width_m);
        let center_z = rm[2][3] + rm[2][1] * (-WRIST_MENU_CAPSULE_OFFSET * rotated.width_m);
        assert_close(center_x, rotated.offset_x_m);
        assert_close(center_y, rotated.offset_y_m);
        assert_close(center_z, rotated.offset_z_m);
    }
}
