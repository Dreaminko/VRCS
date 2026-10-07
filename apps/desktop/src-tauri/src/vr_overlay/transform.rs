use vrcs_core::{VrOverlayHeadsetConfig, VrOverlayWristConfig};

pub fn headset(config: &VrOverlayHeadsetConfig) -> [[f32; 4]; 3] {
    matrix(
        config.pitch_deg,
        config.yaw_deg,
        config.roll_deg,
        [config.offset_x_m, config.offset_y_m, -config.distance_m],
    )
}

pub fn wrist(config: &VrOverlayWristConfig) -> [[f32; 4]; 3] {
    matrix(
        config.pitch_deg,
        config.yaw_deg,
        config.roll_deg,
        [config.offset_x_m, config.offset_y_m, config.offset_z_m],
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
}
