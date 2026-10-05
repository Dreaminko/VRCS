use super::{
    ocr_capture::{EyeCapture, StereoCapture},
    ocr_geometry::Homography,
    renderer::Texture,
    transform,
};

pub use vrcs_core::ocr::TextRegions;

/// Checks unchanged image features under camera rotation. This does not estimate target depth.
/// Both images must be acquired while the translation overlays are hidden.
pub fn verify_regions(
    reference: &StereoCapture,
    current: &StereoCapture,
    regions: &TextRegions,
) -> bool {
    if reference.scene_pid != current.scene_pid
        || reference.origin != current.origin
        || reference
            .pose
            .iter()
            .flatten()
            .chain(current.pose.iter().flatten())
            .any(|v| !v.is_finite())
    {
        return false;
    }
    (0..2).all(|eye| {
        let old = &reference.eyes[eye];
        let new = &current.eyes[eye];
        let old_pose = transform::compose(old.head_pose, old.eye_to_head);
        let new_pose = transform::compose(new.head_pose, new.eye_to_head);
        if !valid_eye(old)
            || !valid_eye(new)
            || old_pose
                .iter()
                .flatten()
                .chain(new_pose.iter().flatten())
                .any(|v| !v.is_finite())
            || regions[eye].len() > 256
        {
            return false;
        }
        regions[eye]
            .iter()
            .all(|quad| region_matches(old, new, old_pose, new_pose, *quad))
    })
}

fn region_matches(
    old: &EyeCapture,
    new: &EyeCapture,
    old_pose: [[f32; 4]; 3],
    new_pose: [[f32; 4]; 3],
    quad: [[f32; 2]; 4],
) -> bool {
    let Some(mapping) = Homography::from_quad(quad) else {
        return false;
    };
    let mut patches = Vec::with_capacity(15);
    for v in [0.2, 0.5, 0.8] {
        for u in [0.15, 0.35, 0.5, 0.65, 0.85] {
            let Some(center) = mapping.map([u, v]) else {
                return false;
            };
            let mut predicted = Vec::with_capacity(25);
            let mut values = Vec::with_capacity(25);
            for dy in [-4., -2., 0., 2., 4.] {
                for dx in [-4., -2., 0., 2., 4.] {
                    let point = [center[0] + dx, center[1] + dy];
                    let Some(value) = gray(&old.image, point) else {
                        return false;
                    };
                    let Some(projected) = project_rotation(old, new, old_pose, new_pose, point)
                    else {
                        return false;
                    };
                    predicted.push(projected);
                    values.push(value);
                }
            }
            let variance = centered_energy(&values);
            if variance >= 25. * 144. {
                patches.push((variance, predicted, values));
            }
        }
    }
    patches.sort_by(|a, b| b.0.total_cmp(&a.0));
    patches.truncate(6);
    if patches.len() < 3 {
        return false;
    }

    let radius =
        ((old.image.width.min(old.image.height) as f32 * 0.015).round() as i32).clamp(4, 24);
    let mut best = (0, f32::NEG_INFINITY, 0, 0);
    for dy in (-radius..=radius).step_by(2) {
        for dx in (-radius..=radius).step_by(2) {
            let score = offset_score(&new.image, &patches, dx as f32, dy as f32);
            if score.0 > best.0 || (score.0 == best.0 && score.1 > best.1) {
                best = (score.0, score.1, dx, dy);
            }
        }
    }
    for dy in best.3 - 1..=best.3 + 1 {
        for dx in best.2 - 1..=best.2 + 1 {
            let score = offset_score(&new.image, &patches, dx as f32, dy as f32);
            if score.0 > best.0 || (score.0 == best.0 && score.1 > best.1) {
                best = (score.0, score.1, dx, dy);
            }
        }
    }
    best.0 >= (patches.len() * 2).div_ceil(3)
}

fn project_rotation(
    old: &EyeCapture,
    new: &EyeCapture,
    old_pose: [[f32; 4]; 3],
    new_pose: [[f32; 4]; 3],
    point: [f32; 2],
) -> Option<[f32; 2]> {
    let ray = [
        old.projection[0]
            + point[0] / old.image.width as f32 * (old.projection[1] - old.projection[0]),
        -(old.projection[2]
            + point[1] / old.image.height as f32 * (old.projection[3] - old.projection[2])),
        -1.,
    ];
    let world: [f32; 3] = std::array::from_fn(|r| (0..3).map(|c| old_pose[r][c] * ray[c]).sum());
    let rotated: [f32; 3] =
        std::array::from_fn(|c| (0..3).map(|r| new_pose[r][c] * world[r]).sum());
    if rotated[2] >= -0.0001 {
        return None;
    }
    Some([
        (rotated[0] / -rotated[2] - new.projection[0]) / (new.projection[1] - new.projection[0])
            * new.image.width as f32,
        (-rotated[1] / -rotated[2] - new.projection[2]) / (new.projection[3] - new.projection[2])
            * new.image.height as f32,
    ])
}

fn offset_score(
    image: &Texture,
    patches: &[(f32, Vec<[f32; 2]>, Vec<f32>)],
    dx: f32,
    dy: f32,
) -> (usize, f32) {
    let mut matched = 0;
    let mut count = 0;
    let mut sum = 0.;
    for (_, points, values) in patches {
        let mut sampled = [0.; 25];
        let valid = points.iter().zip(&mut sampled).all(|([x, y], sample)| {
            gray(image, [x + dx, y + dy]).is_some_and(|value| {
                *sample = value;
                true
            })
        });
        if valid {
            let score = correlation(values, &sampled);
            matched += usize::from(score >= 0.75);
            count += 1;
            sum += score;
        }
    }
    let average = if count == 0 {
        f32::NEG_INFINITY
    } else {
        sum / count as f32
    };
    (matched, average)
}

fn valid_eye(eye: &EyeCapture) -> bool {
    (1..=4096).contains(&eye.image.width)
        && (1..=4096).contains(&eye.image.height)
        && eye.image.pixels.len() == eye.image.width as usize * eye.image.height as usize * 4
        && eye.projection.iter().all(|v| v.is_finite())
        && eye.projection[0] < eye.projection[1]
        && eye.projection[2] < eye.projection[3]
}

fn gray(image: &Texture, [x, y]: [f32; 2]) -> Option<f32> {
    let (x, y) = (x - 0.5, y - 0.5);
    if !x.is_finite()
        || !y.is_finite()
        || x < 0.
        || y < 0.
        || x > (image.width - 1) as f32
        || y > (image.height - 1) as f32
    {
        return None;
    }
    let (ix, iy) = (x.floor() as u32, y.floor() as u32);
    let (fx, fy) = (x - ix as f32, y - iy as f32);
    let luminance = |px: u32, py: u32| {
        let i = ((py * image.width + px) * 4) as usize;
        0.299 * image.pixels[i] as f32
            + 0.587 * image.pixels[i + 1] as f32
            + 0.114 * image.pixels[i + 2] as f32
    };
    let nx = (ix + 1).min(image.width - 1);
    let ny = (iy + 1).min(image.height - 1);
    Some(
        (1. - fy) * ((1. - fx) * luminance(ix, iy) + fx * luminance(nx, iy))
            + fy * ((1. - fx) * luminance(ix, ny) + fx * luminance(nx, ny)),
    )
}

fn centered_energy(values: &[f32]) -> f32 {
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    values.iter().map(|v| (v - mean).powi(2)).sum()
}

fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let ma = a.iter().sum::<f32>() / a.len() as f32;
    let mb = b.iter().sum::<f32>() / b.len() as f32;
    let energy = (centered_energy(a) * centered_energy(b)).sqrt();
    if energy < 0.001 {
        return 0.;
    }
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - ma) * (b - mb))
        .sum::<f32>()
        / energy
}

#[cfg(test)]
mod tests {
    use super::super::{ocr_capture::EyeCapture, renderer::Texture, transform};
    use super::*;

    fn pattern(x: f32, y: f32) -> u8 {
        (128. + 35. * (x * 0.33).sin() + 35. * (y * 0.29).cos() + 40. * ((x - y) * 0.23).sin())
            .clamp(0., 255.)
            .round() as u8
    }
    fn frame(yaw: f32) -> StereoCapture {
        let (s, c) = yaw.to_radians().sin_cos();
        let mut pixels = Vec::new();
        for y in 0..120 {
            for x in 0..160 {
                let (rx, ry) = ((x as f32 + 0.5 - 80.) / 80., -(y as f32 + 0.5 - 60.) / 80.);
                let (wx, wz) = (c * rx - s, -s * rx - c);
                let value = pattern(80. + 80. * wx / -wz, 60. - 80. * ry / -wz);
                pixels.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let eye = EyeCapture {
            image: Texture {
                pixels,
                width: 160,
                height: 120,
            },
            projection: [-1., 1., -0.75, 0.75],
            eye_to_head: transform::matrix(0., 0., 0., [0.; 3]),
            head_pose: transform::matrix(0., yaw, 0., [0.; 3]),
        };
        StereoCapture {
            eyes: [eye.clone(), eye],
            pose: transform::matrix(0., yaw, 0., [0.; 3]),
            scene_pid: 123,
            captured_at: std::time::Instant::now(),
            origin: 1,
        }
    }
    fn regions() -> TextRegions {
        let quad = [[40., 35.], [120., 35.], [120., 85.], [40., 85.]];
        [vec![quad], vec![quad]]
    }

    #[test]
    fn static_image_regions_survive_head_rotation() {
        let reference = frame(0.);
        assert!(verify_regions(&reference, &frame(0.), &regions()));
        assert!(verify_regions(&reference, &frame(5.), &regions()));
        assert!(verify_regions(&reference, &frame(-10.), &regions()));
    }

    #[test]
    fn small_capture_alignment_error_does_not_invalidate_static_text() {
        let reference = frame(0.);
        let mut shifted = reference.clone();
        for eye in &mut shifted.eyes {
            let original = eye.image.pixels.clone();
            for y in 0..eye.image.height as usize {
                for x in 4..eye.image.width as usize {
                    let destination = (y * eye.image.width as usize + x) * 4;
                    let source = (y * eye.image.width as usize + x - 4) * 4;
                    eye.image.pixels[destination..destination + 4]
                        .copy_from_slice(&original[source..source + 4]);
                }
            }
        }
        assert!(verify_regions(&reference, &shifted, &regions()));
    }

    #[test]
    fn each_eye_uses_its_own_capture_pose() {
        let reference = frame(0.);
        let mut current = frame(3.);
        current.eyes[1] = frame(-7.).eyes[1].clone();
        current.pose = frame(15.).pose;
        assert!(verify_regions(&reference, &current, &regions()));
    }

    #[test]
    fn changed_images_with_constant_head_pose_and_missing_features_are_invalid() {
        let reference = frame(0.);
        let mut changed = frame(0.);
        for pixel in changed.eyes[1].image.pixels.as_chunks_mut::<4>().0 {
            for channel in &mut pixel[..3] {
                *channel = 255 - *channel;
            }
        }
        assert!(!verify_regions(&reference, &changed, &regions()));
        let mut blank = reference.clone();
        for eye in &mut blank.eyes {
            eye.image.pixels.fill(0);
        }
        assert!(!verify_regions(&blank, &blank, &regions()));
        assert!(!verify_regions(&reference, &frame(90.), &regions()));
    }

    #[test]
    fn visual_motion_without_head_motion_and_invalid_capture_metadata_are_rejected() {
        let reference = frame(0.);
        let mut moved = reference.clone();
        for y in 0..120 {
            for x in 15..160 {
                let destination = (y * 160 + x) * 4;
                let source = (y * 160 + x - 15) * 4;
                moved.eyes[0].image.pixels[destination..destination + 4]
                    .copy_from_slice(&reference.eyes[0].image.pixels[source..source + 4]);
            }
        }
        assert!(!verify_regions(&reference, &moved, &regions()));
        let mut changed = reference.clone();
        changed.scene_pid += 1;
        assert!(!verify_regions(&reference, &changed, &regions()));
        changed = reference.clone();
        changed.origin += 1;
        assert!(!verify_regions(&reference, &changed, &regions()));
        changed = reference.clone();
        changed.eyes[1].image.pixels.pop();
        assert!(!verify_regions(&reference, &changed, &regions()));
        changed = reference.clone();
        changed.pose[0][0] = f32::NAN;
        assert!(!verify_regions(&reference, &changed, &regions()));
        assert!(verify_regions(&reference, &reference, &[vec![], vec![]]));
    }
}
