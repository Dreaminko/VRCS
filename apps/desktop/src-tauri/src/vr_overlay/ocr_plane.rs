use super::{
    ocr_capture::EyeCapture, ocr_geometry::Homography, ocr_renderer, renderer::Texture, transform,
};
use vrcs_core::ocr::TranslatedBlock;

pub struct PlaneOverlay {
    pub texture: Texture,
    pub pose: [[f32; 4]; 3],
    pub width_m: f32,
    pub texel_aspect: f32,
}

pub fn render(
    eyes: &[EyeCapture; 2],
    blocks: &[Vec<TranslatedBlock>; 2],
    opacity: f32,
    source_view: bool,
) -> Result<Option<(PlaneOverlay, Vec<TranslatedBlock>)>, String> {
    if blocks.iter().any(|eye| eye.len() > 256) {
        return Err("OCR render exceeds the block limit".into());
    }
    let eye = &eyes[0];
    let eye_pose = transform::compose(eye.head_pose, eye.eye_to_head);
    let to_reference = transform::inverse(eye_pose);
    let matches = match_blocks(eyes, blocks, to_reference);
    let points: Vec<_> = matches.iter().map(|(_, _, point)| *point).collect();
    let other_pose = transform::compose(
        to_reference,
        transform::compose(eyes[1].head_pose, eyes[1].eye_to_head),
    );
    let baseline = other_pose.map(|row| row[3]);
    // One pixel of disparity error grows quadratically with source depth.
    let error_per_m2 = eyes
        .iter()
        .map(|eye| (eye.projection[1] - eye.projection[0]) / eye.image.width as f32)
        .sum::<f32>()
        * 0.5
        / dot(baseline, baseline).sqrt();
    let Some((center, normal)) = fit_plane(&points, error_per_m2) else {
        tracing::debug!(
            matches = points.len(),
            "OCR source plane could not be estimated"
        );
        return Ok(None);
    };
    let mut selected = Vec::new();
    let mut fallback = Vec::new();
    let mut paired_right = vec![false; blocks[1].len()];
    for (left, other, point) in &matches {
        paired_right[*other] = true;
        let mut block = blocks[0][*left].clone();
        for translation in &mut block.translations {
            if translation
                .text
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
            {
                continue;
            }
            if let Some(ready) = blocks[1][*other].translations.iter().find(|t| {
                t.target_language == translation.target_language
                    && t.text.as_deref().is_some_and(|s| !s.trim().is_empty())
            }) {
                *translation = ready.clone();
            }
        }
        if dot(sub(*point, center), normal).abs() <= plane_tolerance(-center[2], error_per_m2) {
            selected.push(block);
        } else {
            fallback.push(block);
        }
    }
    fallback.extend(
        blocks[0]
            .iter()
            .enumerate()
            .filter(|(index, _)| !matches.iter().any(|(left, _, _)| left == index))
            .map(|(_, block)| block.clone()),
    );
    fallback.extend(
        blocks[1]
            .iter()
            .enumerate()
            .filter(|(index, _)| !paired_right[*index])
            .map(|(_, block)| block.clone()),
    );
    // Layout happens once. Only blocks that cannot be placed go to the wrist.
    let (texture, unplaced) = ocr_renderer::render_eye(eye, &selected, opacity, source_view)?;
    fallback.extend(
        selected
            .into_iter()
            .filter(|block| unplaced.contains(&block.source.id)),
    );
    Ok(place_texture(eye, eye_pose, texture, center, normal).map(|overlay| (overlay, fallback)))
}

fn match_blocks(
    eyes: &[EyeCapture; 2],
    blocks: &[Vec<TranslatedBlock>; 2],
    to_reference: [[f32; 4]; 3],
) -> Vec<(usize, usize, [f32; 3])> {
    let texts = blocks.each_ref().map(|blocks| {
        blocks
            .iter()
            .map(|block| {
                block
                    .source
                    .text
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    });
    let centers = blocks.each_ref().map(|blocks| {
        blocks
            .iter()
            .map(|block| {
                std::array::from_fn(|c| {
                    block.source.polygon.iter().map(|p| p[c]).sum::<f32>() * 0.25
                })
            })
            .collect::<Vec<[f32; 2]>>()
    });
    let mut left_best = vec![None::<(usize, [f32; 3], f32)>; blocks[0].len()];
    let mut right_best = vec![None::<(usize, f32)>; blocks[1].len()];
    for (left, text) in texts[0].iter().enumerate() {
        if text.is_empty() {
            continue;
        }
        for (right, other) in texts[1].iter().enumerate() {
            if text != other {
                continue;
            }
            let Some((point, score)) =
                triangulate(eyes, [centers[0][left], centers[1][right]], to_reference)
            else {
                continue;
            };
            if left_best[left].is_none_or(|(_, _, best)| score < best) {
                left_best[left] = Some((right, point, score));
            }
            if right_best[right].is_none_or(|(_, best)| score < best) {
                right_best[right] = Some((left, score));
            }
        }
    }
    // Mutual nearest rays disambiguate repeated labels without reusing a block.
    left_best
        .into_iter()
        .enumerate()
        .filter_map(|(left, best)| {
            let (right, point, _) = best?;
            (right_best[right]?.0 == left).then_some((left, right, point))
        })
        .collect()
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn scaled_add(a: [f32; 3], b: [f32; 3], scale: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + b[i] * scale)
}
fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(v, v).sqrt();
    (length.is_finite() && length > 1e-6).then(|| v.map(|x| x / length))
}

fn ray(eye: &EyeCapture, pixel: [f32; 2]) -> Option<[f32; 3]> {
    let [l, r, t, b] = eye.projection;
    if eye.image.width == 0
        || eye.image.height == 0
        || l >= r
        || t >= b
        || !eye
            .projection
            .iter()
            .chain(pixel.iter())
            .all(|v| v.is_finite())
        || pixel[0] < 0.
        || pixel[1] < 0.
        || pixel[0] > eye.image.width as f32
        || pixel[1] > eye.image.height as f32
    {
        return None;
    }
    Some([
        l + (r - l) * pixel[0] / eye.image.width as f32,
        -(t + (b - t) * pixel[1] / eye.image.height as f32),
        -1.,
    ])
}

fn triangulate(
    eyes: &[EyeCapture; 2],
    pixels: [[f32; 2]; 2],
    to_reference: [[f32; 4]; 3],
) -> Option<([f32; 3], f32)> {
    let mut origins = [[0.; 3]; 2];
    let mut directions = [[0.; 3]; 2];
    for index in 0..2 {
        let pose = transform::compose(
            to_reference,
            transform::compose(eyes[index].head_pose, eyes[index].eye_to_head),
        );
        let local = ray(&eyes[index], pixels[index])?;
        origins[index] = pose.map(|row| row[3]);
        directions[index] = normalize(std::array::from_fn(|r| {
            (0..3).map(|c| pose[r][c] * local[c]).sum()
        }))?;
    }
    let between = sub(origins[1], origins[0]);
    let cosine = dot(directions[0], directions[1]);
    let denominator = 1. - cosine * cosine;
    if denominator <= 1e-6 {
        return None;
    }
    let distance =
        (dot(between, directions[0]) - cosine * dot(between, directions[1])) / denominator;
    let other_distance = cosine * distance - dot(between, directions[1]);
    if distance <= 0. || other_distance <= 0. {
        return None;
    }
    let a = scaled_add(origins[0], directions[0], distance);
    let b = scaled_add(origins[1], directions[1], other_distance);
    let gap = sub(a, b);
    if dot(gap, gap).sqrt() > 0.02 + distance * 0.005 {
        return None;
    }
    let point = std::array::from_fn(|i| (a[i] + b[i]) * 0.5);
    (point.iter().all(|v| v.is_finite()) && (0.15..=20.).contains(&-point[2])).then_some((
        point,
        denominator + dot(gap, gap) / (distance * other_distance),
    ))
}

fn plane_tolerance(depth: f32, error_per_m2: f32) -> f32 {
    0.03 + depth * 0.01 + (depth * depth * error_per_m2).min(depth * 0.2)
}

fn fit_plane(points: &[[f32; 3]], error_per_m2: f32) -> Option<([f32; 3], [f32; 3])> {
    if let Some(plane) = least_squares_plane(points, error_per_m2) {
        return Some(plane);
    }
    if points.is_empty() {
        return None;
    }
    let mut depths: Vec<_> = points.iter().map(|p| -p[2]).collect();
    depths.sort_by(f32::total_cmp);
    let depth = depths[depths.len() / 2];
    let inliers: Vec<_> = points
        .iter()
        .copied()
        .filter(|p| (-p[2] - depth).abs() <= plane_tolerance(depth, error_per_m2))
        .collect();
    if inliers.len() < (points.len() * 2).div_ceil(3) {
        return None;
    }
    least_squares_plane(&inliers, error_per_m2)
}

fn least_squares_plane(points: &[[f32; 3]], error_per_m2: f32) -> Option<([f32; 3], [f32; 3])> {
    if points.is_empty() {
        return None;
    }
    let center: [f32; 3] =
        std::array::from_fn(|i| points.iter().map(|p| p[i]).sum::<f32>() / points.len() as f32);
    let depth = -center[2];
    let (mut xx, mut xy, mut yy, mut xz, mut yz) = (0., 0., 0., 0., 0.);
    for p in points {
        let [x, y, z] = sub(*p, center);
        xx += x * x;
        xy += x * y;
        yy += y * y;
        xz += x * z;
        yz += y * z;
    }
    let denominator = xx * yy - xy * xy;
    // A single word or collinear rows determine depth, but not a surface tilt.
    let mut normal = if points.len() >= 3 && denominator > 1e-8 {
        normalize([
            -(xz * yy - yz * xy) / denominator,
            -(yz * xx - xz * xy) / denominator,
            1.,
        ])?
    } else {
        [0., 0., 1.]
    };
    let fits = |normal: [f32; 3]| {
        points
            .iter()
            .all(|p| dot(sub(*p, center), normal).abs() <= plane_tolerance(depth, error_per_m2))
    };
    if normal[2] < 0.25 || !fits(normal) {
        if !fits([0., 0., 1.]) {
            return None;
        }
        normal = [0., 0., 1.];
    }
    Some((center, normal))
}

fn place_texture(
    eye: &EyeCapture,
    eye_pose: [[f32; 4]; 3],
    texture: Texture,
    center: [f32; 3],
    normal: [f32; 3],
) -> Option<PlaneOverlay> {
    let mut bounds = [texture.width, texture.height, 0, 0];
    for (index, pixel) in texture.pixels.as_chunks::<4>().0.iter().enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let (x, y) = (index as u32 % texture.width, index as u32 / texture.width);
        bounds = [
            bounds[0].min(x),
            bounds[1].min(y),
            bounds[2].max(x + 1),
            bounds[3].max(y + 1),
        ];
    }
    if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
        return None;
    }
    let [left, top, right, bottom] = [
        bounds[0].saturating_sub(2),
        bounds[1].saturating_sub(2),
        (bounds[2] + 2).min(texture.width),
        (bounds[3] + 2).min(texture.height),
    ];
    let axis_x = normalize([normal[2], 0., -normal[0]])?;
    let axis_y = [
        normal[1] * axis_x[2] - normal[2] * axis_x[1],
        normal[2] * axis_x[0] - normal[0] * axis_x[2],
        normal[0] * axis_x[1] - normal[1] * axis_x[0],
    ];
    let mut extent = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for pixel in [[left, top], [right, top], [right, bottom], [left, bottom]] {
        let direction = ray(eye, pixel.map(|v| v as f32))?;
        let distance = dot(normal, center) / dot(normal, direction);
        if !distance.is_finite() || distance <= 0. {
            return None;
        }
        let point = sub(direction.map(|v| v * distance), center);
        let (x, y) = (dot(point, axis_x), dot(point, axis_y));
        extent = [
            extent[0].min(x),
            extent[1].min(y),
            extent[2].max(x),
            extent[3].max(y),
        ];
    }
    let [x0, y0, x1, y1] = extent;
    let (width_m, height_m) = (x1 - x0, y1 - y0);
    if !width_m.is_finite() || !height_m.is_finite() || width_m <= 0.001 || height_m <= 0.001 {
        return None;
    }
    let [l, r, t, b] = eye.projection;
    let corners = [[x0, y1], [x1, y1], [x1, y0], [x0, y0]]
        .map(|[x, y]| scaled_add(scaled_add(center, axis_x, x), axis_y, y));
    if corners.iter().any(|p| p[2] >= -0.01) {
        return None;
    }
    let quad = corners.map(|p| {
        [
            (p[0] / -p[2] - l) / (r - l) * eye.image.width as f32,
            (-p[1] / -p[2] - t) / (b - t) * eye.image.height as f32,
        ]
    });
    let mapping = Homography::from_quad(quad)?;
    let (width, height) = (right - left, bottom - top);
    let mut pixels = vec![0; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let [sx, sy] = mapping.map([
                (x as f32 + 0.5) / width as f32,
                (y as f32 + 0.5) / height as f32,
            ])?;
            if sx < 0. || sy < 0. || sx >= texture.width as f32 || sy >= texture.height as f32 {
                continue;
            }
            pixels[((y * width + x) * 4) as usize..][..4].copy_from_slice(&ocr_renderer::sample(
                &texture.pixels,
                texture.width,
                texture.height,
                sx / texture.width as f32,
                sy / texture.height as f32,
            ));
        }
    }
    let position = scaled_add(
        scaled_add(center, axis_x, (x0 + x1) * 0.5),
        axis_y,
        (y0 + y1) * 0.5,
    );
    let local_pose = std::array::from_fn(|r| [axis_x[r], axis_y[r], normal[r], position[r]]);
    Some(PlaneOverlay {
        texture: Texture {
            width,
            height,
            pixels,
        },
        pose: transform::compose(eye_pose, local_pose),
        width_m,
        texel_aspect: width_m / height_m * height as f32 / width as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_overlay::transform;
    use vrcs_core::ocr::{BlockTranslation, TextBlock};

    fn fixture(depth: f32, head: [[f32; 4]; 3]) -> ([EyeCapture; 2], [Vec<TranslatedBlock>; 2]) {
        fixture_with_tilt(depth, head, [0.; 2])
    }

    fn fixture_with_tilt(
        depth: f32,
        head: [[f32; 4]; 3],
        tilt: [f32; 2],
    ) -> ([EyeCapture; 2], [Vec<TranslatedBlock>; 2]) {
        let eyes = [-0.032, 0.032].map(|offset| EyeCapture {
            image: Texture {
                width: 320,
                height: 240,
                pixels: vec![],
            },
            projection: if offset < 0. {
                [-1.2, 0.8, -0.9, 1.1]
            } else {
                [-0.8, 1.2, -0.9, 1.1]
            },
            eye_to_head: transform::matrix(0., 0., 0., [offset, 0., 0.]),
            head_pose: head,
        });
        let project = |eye: &EyeCapture, point: [f32; 3]| {
            let [l, r, t, b] = eye.projection;
            [
                (point[0] - eye.eye_to_head[0][3]) / -point[2] - l,
                -point[1] / -point[2] - t,
            ]
            .into_iter()
            .zip([r - l, b - t])
            .zip([320., 240.])
            .map(|((v, span), size)| v / span * size)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
        };
        let points = [
            [-0.3, 0.15, -depth],
            [0.3, 0.15, -depth],
            [0., -0.2, -depth],
        ]
        .map(|[x, y, z]| [x, y, z + tilt[0] * x + tilt[1] * y]);
        let blocks = std::array::from_fn(|index| {
            points
                .iter()
                .enumerate()
                .map(|(id, p)| {
                    let center: [f32; 2] = project(&eyes[index], *p);
                    TranslatedBlock {
                        source: TextBlock {
                            id,
                            text: format!("word {id}"),
                            confidence: 0.95,
                            polygon: [
                                [center[0] - 20., center[1] - 8.],
                                [center[0] + 20., center[1] - 8.],
                                [center[0] + 20., center[1] + 8.],
                                [center[0] - 20., center[1] + 8.],
                            ],
                        },
                        translations: vec![BlockTranslation {
                            target_language: "en".into(),
                            text: Some("VR".into()),
                            error_code: None,
                        }],
                    }
                })
                .collect()
        });
        (eyes, blocks)
    }

    #[test]
    fn result_plane_uses_source_depth_instead_of_hand_depth() {
        for depth in [1., 4.] {
            let (eyes, blocks) = fixture(depth, transform::matrix(0., 0., 0., [0.; 3]));
            let (overlay, limited) = render(&eyes, &blocks, 1., false)
                .unwrap()
                .expect("source plane");
            assert!(limited.is_empty());
            assert!((overlay.pose[2][3] + depth).abs() < 0.01);
            assert!(overlay.width_m > 0. && overlay.texel_aspect > 0.);
            let position = overlay.pose.map(|r| r[3]);
            let tangents = eyes.each_ref().map(|eye| {
                let view = eye.tracking_to_eye();
                let local: [f32; 3] = std::array::from_fn(|r| {
                    view[r][3] + (0..3).map(|c| view[r][c] * position[c]).sum::<f32>()
                });
                local[0] / -local[2]
            });
            assert!((tangents[0] - tangents[1] - 0.064 / depth).abs() < 0.0004);
        }
    }

    #[test]
    fn result_plane_keeps_capture_tracking_pose_after_head_rotation() {
        let head = transform::matrix(0., 45., 0., [1., 2., 3.]);
        let (eyes, blocks) = fixture(2., head);
        let (overlay, _) = render(&eyes, &blocks, 1., false)
            .unwrap()
            .expect("source plane");
        let local = transform::compose(transform::inverse(head), overlay.pose);
        assert!((local[2][3] + 2.).abs() < 0.01);
        assert!((local[0][0] - 1.).abs() < 0.01);
    }

    #[test]
    fn tilted_source_plane_sets_surface_orientation() {
        let (eyes, blocks) =
            fixture_with_tilt(2., transform::matrix(0., 0., 0., [0.; 3]), [0.5, 0.2]);
        let (overlay, _) = render(&eyes, &blocks, 1., false).unwrap().unwrap();
        let expected = normalize([-0.5, -0.2, 1.]).unwrap();
        let actual = overlay.pose.map(|r| r[2]);
        assert!(dot(expected, actual) > 0.999);
        let [x, y, z] = overlay.pose.map(|r| r[3]);
        assert!((z - 0.5 * x - 0.2 * y + 2.).abs() < 0.01);
    }

    #[test]
    fn planar_texture_keeps_source_coordinates_and_physical_aspect() {
        let (eyes, _) = fixture(2., transform::matrix(0., 0., 0., [0.; 3]));
        let eye = &eyes[0];
        let mut texture = Texture {
            width: 320,
            height: 240,
            pixels: vec![0; 320 * 240 * 4],
        };
        for y in 60..180 {
            for x in 80..240 {
                texture.pixels[(y * 320 + x) * 4..][..4]
                    .copy_from_slice(&[x as u8, y as u8, 80, 255]);
            }
        }
        let eye_pose = transform::compose(eye.head_pose, eye.eye_to_head);
        let center = [0., 0., -2.];
        let normal = normalize([-0.5, -0.2, 1.]).unwrap();
        let overlay = place_texture(eye, eye_pose, texture, center, normal).unwrap();
        let to_plane = transform::inverse(transform::compose(
            transform::inverse(eye_pose),
            overlay.pose,
        ));
        let height_m = overlay.width_m * overlay.texture.height as f32
            / overlay.texture.width as f32
            / overlay.texel_aspect;
        for pixel in [[100., 80.], [160., 120.], [220., 160.]] {
            let direction = ray(eye, pixel).unwrap();
            let point = direction.map(|v| v * dot(normal, center) / dot(normal, direction));
            let local: [f32; 3] = std::array::from_fn(|r| {
                to_plane[r][3] + (0..3).map(|c| to_plane[r][c] * point[c]).sum::<f32>()
            });
            let sampled = ocr_renderer::sample(
                &overlay.texture.pixels,
                overlay.texture.width,
                overlay.texture.height,
                local[0] / overlay.width_m + 0.5,
                0.5 - local[1] / height_m,
            );
            assert!((sampled[0] as f32 - (pixel[0] - 0.5)).abs() <= 2.);
            assert!((sampled[1] as f32 - (pixel[1] - 0.5)).abs() <= 2.);
            assert_eq!(sampled[3], 255);
        }
    }

    #[test]
    fn incompatible_source_depths_do_not_produce_an_invalid_plane() {
        assert!(fit_plane(&[[0., 0., -1.], [0.3, 0., -4.]], 0.).is_none());
        assert!(fit_plane(&[[0., 0., -1.], [0.3, 0., -4.]], 0.03125).is_none());
    }

    #[test]
    fn repeated_labels_and_punctuation_differences_keep_the_source_plane() {
        let (eyes, mut blocks) = fixture(2., transform::matrix(0., 0., 0., [0.; 3]));
        for block in &mut blocks[0] {
            block.source.text = "Open menu".into();
        }
        for block in &mut blocks[1] {
            block.source.text = "OPEN\nMENU!".into();
        }
        blocks[1].reverse();
        let (plane, _) = render(&eyes, &blocks, 1., false)
            .unwrap()
            .expect("source plane");
        assert!((plane.pose[2][3] + 2.).abs() < 0.01);
    }

    #[test]
    fn one_depth_outlier_does_not_discard_a_consistent_plane() {
        let points = [[-0.3, 0., -2.], [0., 0., -2.], [0.3, 0., -2.13]];
        let (center, normal) = fit_plane(&points, 0.).expect("consistent majority");
        assert!((center[2] + 2.).abs() < 0.001);
        assert_eq!(normal, [0., 0., 1.]);
    }

    #[test]
    fn uncertain_tilt_uses_consistent_depth_instead_of_discarding_the_plane() {
        let points = [[0., 0., -2.], [0.003, 0., -2.02], [0., 0.2, -2.]];
        let (center, normal) = fit_plane(&points, 0.).expect("depth without reliable tilt");
        assert!((center[2] + 2.).abs() < 0.01);
        assert_eq!(normal, [0., 0., 1.]);
    }

    #[test]
    fn one_pixel_disparity_error_keeps_collinear_labels_on_the_source_plane() {
        let (mut eyes, mut blocks) = fixture(2., transform::matrix(0., 0., 0., [0.; 3]));
        for eye in &mut eyes {
            eye.image.width = 1024;
            eye.image.height = 1024;
        }
        for eye_blocks in &mut blocks {
            for block in eye_blocks {
                block.source.polygon = std::array::from_fn(|index| {
                    [
                        block.source.polygon[index][0] * 1024. / 320.,
                        if index < 2 { 448.8 } else { 472.8 },
                    ]
                });
            }
        }
        for pixel in &mut blocks[1][2].source.polygon {
            pixel[0] += 1.;
        }
        let (plane, fallback) = render(&eyes, &blocks, 1., false)
            .unwrap()
            .expect("noisy plane");
        assert!((plane.pose[2][3] + 2.).abs() < 0.06);
        assert!(fallback.is_empty());
    }

    #[test]
    fn an_unmatched_block_falls_back_without_duplicating_placed_blocks() {
        let (eyes, mut blocks) = fixture(2., transform::matrix(0., 0., 0., [0.; 3]));
        let mut unmatched = blocks[0][0].clone();
        unmatched.source.id = 10;
        unmatched.source.text = "left only".into();
        unmatched.translations[0].text = Some("unplaced translation".into());
        blocks[0].push(unmatched);
        let (plane, fallback) = render(&eyes, &blocks, 1., false).unwrap().unwrap();
        assert!((plane.pose[2][3] + 2.).abs() < 0.01);
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].source.id, 10);
    }

    #[test]
    fn invalid_stereo_correspondence_does_not_place_a_result_at_arbitrary_depth() {
        let (eyes, mut blocks) = fixture(2., transform::matrix(0., 0., 0., [0.; 3]));
        for block in &mut blocks[1] {
            block.source.text = "unmatched".into();
        }
        assert!(render(&eyes, &blocks, 1., false).unwrap().is_none());
    }
}
