use super::{
    ocr_capture::{region_crop, CaptureCrop, EyeCapture},
    renderer::Texture,
};

#[derive(Clone)]
pub struct Selection {
    basis: [[f32; 4]; 3],
    world: [[f32; 3]; 4],
    pub scene_pid: u32,
    pub origin: i32,
}

impl Selection {
    pub fn from_corners(
        corners: [[f32; 3]; 2],
        basis: [[f32; 4]; 3],
        scene_pid: u32,
        origin: i32,
    ) -> Option<Self> {
        if corners
            .iter()
            .flatten()
            .chain(basis.iter().flatten())
            .any(|v| !v.is_finite())
        {
            return None;
        }
        // Reuse the initial frame axes. Head motion changes projection, not frame geometry.
        let view = super::transform::inverse(basis);
        let corners: [[f32; 3]; 2] = corners.map(|point| {
            std::array::from_fn(|r| view[r][3] + (0..3).map(|c| view[r][c] * point[c]).sum::<f32>())
        });
        if corners.iter().flatten().any(|v| !v.is_finite())
            || corners.iter().any(|p| !(-2.0..=-0.05).contains(&p[2]))
        {
            return None;
        }
        let [left, right] = if corners[0][0] <= corners[1][0] {
            corners
        } else {
            [corners[1], corners[0]]
        };
        let bottom = left[1].min(right[1]);
        let top = left[1].max(right[1]);
        if right[0] - left[0] < 0.015 || top - bottom < 0.015 {
            return None;
        }
        // Vertical edges share each anchor's depth, so the tilted plane retains both corners.
        let world = [
            [left[0], top, left[2]],
            [right[0], top, right[2]],
            [right[0], bottom, right[2]],
            [left[0], bottom, left[2]],
        ]
        .map(|p| {
            std::array::from_fn(|r| basis[r][3] + (0..3).map(|c| basis[r][c] * p[c]).sum::<f32>())
        });
        Some(Self {
            basis,
            world,
            scene_pid,
            origin,
        })
    }

    pub fn with_corners(&self, corners: [[f32; 3]; 2]) -> Option<Self> {
        Self::from_corners(corners, self.basis, self.scene_pid, self.origin)
    }

    pub fn project(&self, eye: &EyeCapture) -> Option<[[f32; 2]; 4]> {
        let view = eye.tracking_to_eye();
        let [left, right, top, bottom] = eye.projection;
        if !eye
            .projection
            .iter()
            .chain(view.iter().flatten())
            .all(|v| v.is_finite())
            || left >= right
            || top >= bottom
            || eye.image.width == 0
            || eye.image.height == 0
        {
            return None;
        }
        let mut quad = [[0.; 2]; 4];
        for (world, pixel) in self.world.iter().zip(&mut quad) {
            let local: [f32; 3] = std::array::from_fn(|r| {
                view[r][3] + (0..3).map(|c| view[r][c] * world[c]).sum::<f32>()
            });
            if local[2] >= -0.01 {
                return None;
            }
            *pixel = [
                (local[0] / -local[2] - left) / (right - left) * eye.image.width as f32,
                (-local[1] / -local[2] - top) / (bottom - top) * eye.image.height as f32,
            ];
        }
        Some(quad)
    }

    pub fn crop_bounds(&self, eye: &EyeCapture) -> Option<[u32; 4]> {
        let quad = self.project(eye)?;
        let bounds = [
            quad.iter()
                .map(|p| p[0])
                .fold(f32::INFINITY, f32::min)
                .floor()
                .clamp(0., eye.image.width as f32) as u32,
            quad.iter()
                .map(|p| p[1])
                .fold(f32::INFINITY, f32::min)
                .floor()
                .clamp(0., eye.image.height as f32) as u32,
            quad.iter()
                .map(|p| p[0])
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .clamp(0., eye.image.width as f32) as u32,
            quad.iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil()
                .clamp(0., eye.image.height as f32) as u32,
        ];
        (bounds[2] >= bounds[0] + 2 && bounds[3] >= bounds[1] + 2).then_some(bounds)
    }

    pub fn preview(&self) -> super::ocr_plane::PlaneOverlay {
        let right: [f32; 3] = std::array::from_fn(|i| self.world[1][i] - self.world[0][i]);
        let up: [f32; 3] = std::array::from_fn(|i| self.world[0][i] - self.world[3][i]);
        let width_m = right.iter().map(|v| v * v).sum::<f32>().sqrt();
        let height_m = up.iter().map(|v| v * v).sum::<f32>().sqrt();
        let x = right.map(|v| v / width_m);
        let y = up.map(|v| v / height_m);
        let z = [
            x[1] * y[2] - x[2] * y[1],
            x[2] * y[0] - x[0] * y[2],
            x[0] * y[1] - x[1] * y[0],
        ];
        let pose = std::array::from_fn(|i| {
            [
                x[i],
                y[i],
                z[i],
                (self.world[0][i] + self.world[2][i]) * 0.5,
            ]
        });
        let (width, height) = (256, 256);
        let mut pixels = vec![0; width as usize * height as usize * 4];
        for y in 0..height {
            for x in 0..width {
                if x < 2 || x >= width - 2 || y < 2 || y >= height - 2 {
                    let offset = ((y * width + x) * 4) as usize;
                    pixels[offset..offset + 4].copy_from_slice(&[40, 220, 255, 255]);
                }
            }
        }
        super::ocr_plane::PlaneOverlay {
            texture: Texture {
                width,
                height,
                pixels,
            },
            pose,
            width_m,
            texel_aspect: width_m / height_m,
        }
    }

    pub fn crop(&self, eye: &EyeCapture) -> Result<CaptureCrop, String> {
        let bounds = self
            .crop_bounds(eye)
            .ok_or("OCR frame left the view before capture")?;
        let quad = self.project(eye).ok_or("Invalid OCR frame position")?;
        let inverse = super::ocr_geometry::Homography::from_quad(quad)
            .and_then(super::ocr_geometry::Homography::inverse)
            .ok_or("Invalid OCR frame position")?;
        let mut crop = region_crop(&eye.image, bounds)?;
        // Head rotation turns the frozen rectangle into a quadrilateral. Hide the surrounding
        // bounding-box pixels before recognition or upload, while retaining the crop transform.
        for y in 0..crop.image.height {
            for x in 0..crop.image.width {
                let point = [
                    crop.offset[0] + (x as f32 + 0.5) * crop.scale[0],
                    crop.offset[1] + (y as f32 + 0.5) * crop.scale[1],
                ];
                let inside = inverse
                    .map(point)
                    .is_some_and(|[u, v]| (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v));
                if !inside {
                    let offset = ((y * crop.image.width + x) * 4) as usize;
                    crop.image.pixels[offset..offset + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
        Ok(crop)
    }
}

#[cfg(test)]
mod tests {
    use super::super::transform;
    use super::*;

    fn eye(x: f32, head: [[f32; 4]; 3]) -> EyeCapture {
        EyeCapture {
            image: Texture {
                width: 200,
                height: 100,
                pixels: vec![],
            },
            projection: [-1., 1., -0.5, 0.5],
            eye_to_head: transform::matrix(0., 0., 0., [x, 0., 0.]),
            head_pose: head,
        }
    }

    fn assert_quad(actual: [[f32; 2]; 4], expected: [[f32; 2]; 4]) {
        for (a, b) in actual
            .into_iter()
            .flatten()
            .zip(expected.into_iter().flatten())
        {
            assert!((a - b).abs() < 0.0001, "{a} != {b}");
        }
    }

    fn world_corners(corners: [[f32; 3]; 2], head: [[f32; 4]; 3]) -> [[f32; 3]; 2] {
        corners.map(|point| {
            std::array::from_fn(|r| head[r][3] + (0..3).map(|c| head[r][c] * point[c]).sum::<f32>())
        })
    }

    #[test]
    fn world_anchors_are_not_transformed_twice_when_the_head_is_rotated_and_translated() {
        let head = transform::matrix(15., -25., 30., [1., 1.6, 0.]);
        let anchors = world_corners([[-0.2, 0.1, -0.4], [0.2, -0.1, -0.7]], head);
        let selection = Selection::from_corners(anchors, head, 7, 1).unwrap();
        for (actual, expected) in [selection.world[0], selection.world[2]]
            .into_iter()
            .zip(anchors)
        {
            for (a, b) in actual.into_iter().zip(expected) {
                assert!((a - b).abs() < 0.0001, "{a} != {b}");
            }
        }
    }

    #[test]
    fn dragging_world_anchors_resizes_the_frame_and_preserves_unequal_depths() {
        let basis = transform::matrix(15., -25., 30., [1., 1.6, -2.]);
        let initial = world_corners([[-0.2, 0.1, -0.4], [0.2, -0.1, -0.7]], basis);
        let selection = Selection::from_corners(initial, basis, 7, 1).unwrap();
        assert!(selection.with_corners([initial[0]; 2]).is_none());
        let anchors = world_corners([[-0.3, 0.2, -0.3], [0.4, -0.2, -0.8]], basis);
        let resized = selection.with_corners([anchors[1], anchors[0]]).unwrap();
        for (actual, expected) in [resized.world[0], resized.world[2]]
            .into_iter()
            .zip(anchors)
        {
            for (a, b) in actual.into_iter().zip(expected) {
                assert!((a - b).abs() < 0.0001, "{a} != {b}");
            }
        }
        let plane = resized.preview();
        assert!((plane.width_m - 0.7_f32.hypot(0.5)).abs() < 0.0001);
        assert!((plane.width_m / plane.texel_aspect - 0.4).abs() < 0.0001);
        assert_eq!((resized.scene_pid, resized.origin), (7, 1));
    }

    #[test]
    fn head_rotation_keeps_pixels_outside_the_confirmed_frame_out_of_ocr() {
        let head = transform::matrix(0., 0., 0., [0.; 3]);
        let selection =
            Selection::from_corners([[-0.2, 0.1, -0.5], [0.2, -0.1, -0.5]], head, 7, 1).unwrap();
        let mut view = eye(0., transform::matrix(0., 0., 30., [0.; 3]));
        view.image.pixels = [200, 200, 200, 255].repeat(200 * 100);
        let crop = selection.crop(&view).unwrap();
        assert_eq!(&crop.image.pixels[..4], &[0, 0, 0, 255]);
        let center =
            ((crop.image.height / 2 * crop.image.width + crop.image.width / 2) * 4) as usize;
        assert_eq!(
            &crop.image.pixels[center..center + 4],
            &[200, 200, 200, 255]
        );
        let bounds = selection.crop_bounds(&view).unwrap();
        let readback = region_crop(&view.image, bounds).unwrap().image;
        view.projection = super::super::ocr_capture::crop_projection(
            view.projection,
            [view.image.width, view.image.height],
            bounds,
        );
        view.image = readback;
        let selected = selection.crop(&view).unwrap();
        assert_eq!(selected.image.width, crop.image.width);
        assert_eq!(selected.image.height, crop.image.height);
        assert_eq!(selected.image.pixels, crop.image.pixels);
    }

    #[test]
    fn dragged_frame_projects_to_each_eye_and_stays_fixed_after_head_motion() {
        let head = transform::matrix(0., 0., 0., [0.; 3]);
        let selection =
            Selection::from_corners([[-0.2, 0.1, -0.5], [0.4, -0.2, -0.5]], head, 7, 1).unwrap();
        assert_quad(
            selection.project(&eye(-0.03, head)).unwrap(),
            [[66., 30.], [186., 30.], [186., 90.], [66., 90.]],
        );
        assert_quad(
            selection.project(&eye(0.03, head)).unwrap(),
            [[54., 30.], [174., 30.], [174., 90.], [54., 90.]],
        );
        let moved = transform::matrix(0., 0., 0., [0.1, 0., 0.]);
        assert_quad(
            selection.project(&eye(0.03, moved)).unwrap(),
            [[34., 30.], [154., 30.], [154., 90.], [34., 90.]],
        );
    }

    #[test]
    fn asymmetric_openvr_projection_keeps_the_crop_on_the_hand_frame() {
        let head = transform::matrix(0., 0., 0., [0.; 3]);
        let selection =
            Selection::from_corners([[-0.2, 0.1, -0.5], [0.4, -0.2, -0.5]], head, 7, 1).unwrap();
        for (x, left, right) in [(-0.03, 66., 186.), (0.03, 54., 174.)] {
            let mut view = eye(x, head);
            view.projection =
                super::super::ocr_capture::projection_from_openvr([-1., 1., -0.7, 0.3]);
            assert_quad(
                selection.project(&view).unwrap(),
                [[left, 10.], [right, 10.], [right, 70.], [left, 70.]],
            );
        }
    }

    #[test]
    fn frame_corners_stay_on_the_controllers_at_different_depths() {
        let head = transform::matrix(0., 0., 0., [0.; 3]);
        let selection =
            Selection::from_corners([[-0.2, 0.1, -0.25], [0.2, -0.1, -0.75]], head, 7, 1).unwrap();
        for x in [-0.03, 0.03] {
            let quad = selection.project(&eye(x, head)).unwrap();
            let expected = if x < 0. {
                [[32., 10.], [130.66667, 63.33333]]
            } else {
                [[8., 10.], [122.66667, 63.33333]]
            };
            for (actual, expected) in [quad[0], quad[2]].into_iter().zip(expected) {
                for (a, b) in actual.into_iter().zip(expected) {
                    assert!((a - b).abs() < 0.0001, "{a} != {b}");
                }
            }
        }
    }

    #[test]
    fn preview_plane_retains_both_hand_anchors_and_has_a_transparent_center() {
        for head in [
            transform::matrix(0., 0., 0., [0.; 3]),
            transform::matrix(25., -30., 15., [1., 1.6, -2.]),
        ] {
            for corners in [
                [[-0.2, 0.1, -0.25], [0.2, -0.1, -0.75]],
                [[0.2, 0.1, -0.75], [-0.2, -0.1, -0.25]],
            ] {
                let selection =
                    Selection::from_corners(world_corners(corners, head), head, 7, 1).unwrap();
                let plane = selection.preview();
                let height_m = plane.width_m * plane.texture.height as f32
                    / (plane.texture.width as f32 * plane.texel_aspect);
                for ([u, v], expected) in [[-0.5, 0.5], [0.5, 0.5], [0.5, -0.5], [-0.5, -0.5]]
                    .into_iter()
                    .zip(selection.world)
                {
                    for (row, expected) in plane.pose.iter().zip(expected) {
                        let actual = row[3] + row[0] * u * plane.width_m + row[1] * v * height_m;
                        assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
                    }
                }
                let texture = plane.texture;
                assert_eq!(&texture.pixels[..4], &[40, 220, 255, 255]);
                let center =
                    ((texture.height / 2 * texture.width + texture.width / 2) * 4) as usize;
                assert_eq!(&texture.pixels[center..center + 4], &[0; 4]);
            }
        }
    }

    #[test]
    fn reversed_corners_work_and_invalid_frames_are_rejected() {
        let head = transform::matrix(0., 0., 0., [0.; 3]);
        let selection =
            Selection::from_corners([[0.4, -0.2, -0.5], [-0.2, 0.1, -0.5]], head, 7, 1).unwrap();
        assert_quad(
            selection.project(&eye(0., head)).unwrap(),
            [[60., 30.], [180., 30.], [180., 90.], [60., 90.]],
        );
        for corners in [
            [[0., 0., -0.5]; 2],
            [[f32::NAN, 0., -0.5], [0.4, 0.2, -0.5]],
            [[0., 0., 0.5], [0.4, 0.2, -0.5]],
        ] {
            assert!(Selection::from_corners(corners, head, 7, 1).is_none());
        }
    }
}
