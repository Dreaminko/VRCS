use super::{ocr_capture::EyeCapture, ocr_geometry::Homography, transform};
use serde::Serialize;
use vrcs_core::ocr::{TextBlock, TranslatedBlock};

#[derive(Clone)]
pub struct PlaneOverlay {
    pub pose: [[f32; 4]; 3],
    pub width_m: f32,
    pub texel_aspect: f32,
}

pub type Quad = [[f32; 2]; 4];

#[derive(Clone, PartialEq, Serialize)]
pub struct OcrSceneBlock {
    pub id: usize,
    pub text: String,
    pub quad: Quad,
    pub masks: Vec<Quad>,
}

#[derive(Clone, PartialEq, Serialize)]
pub struct OcrScene {
    pub blocks: Vec<OcrSceneBlock>,
    pub excluded: Vec<Quad>,
    pub opacity: f32,
    pub aspect: f32,
}

pub struct OcrPresentation {
    pub plane: PlaneOverlay,
    pub scene: OcrScene,
    pub blocks: Vec<TranslatedBlock>,
}

pub fn render(
    eyes: &[EyeCapture; 2],
    blocks: &[Vec<TranslatedBlock>; 2],
    opacity: f32,
    source_view: bool,
) -> Result<(Option<OcrPresentation>, Vec<TranslatedBlock>), String> {
    if blocks.iter().any(|eye| eye.len() > 256) {
        return Err("OCR render exceeds the block limit".into());
    }
    // Use the more complete recognition without copying the captured images.
    let completeness = blocks.each_ref().map(|groups| {
        (
            groups
                .iter()
                .map(|group| group.fragments().len())
                .sum::<usize>(),
            groups
                .iter()
                .map(|group| group.fragments().len())
                .max()
                .unwrap_or(0),
        )
    });
    let primary = usize::from(completeness[1] > completeness[0]);
    let eyes = &[&eyes[primary], &eyes[1 - primary]];
    let blocks = &[&blocks[primary], &blocks[1 - primary]];
    let eye = eyes[0];
    let eye_pose = transform::compose(eye.head_pose, eye.eye_to_head);
    let to_reference = transform::inverse(eye_pose);
    let fragments: [Vec<_>; 2] = blocks.each_ref().map(|groups| {
        groups
            .iter()
            .enumerate()
            .flat_map(|(index, group)| {
                group
                    .fragments()
                    .iter()
                    .map(move |fragment| (index, fragment))
            })
            .collect()
    });
    let matches = match_blocks(eyes, &fragments, to_reference);
    let right_id_offset = blocks[0]
        .iter()
        .map(|block| block.source.id)
        .max()
        .map_or(0, |id| id + 1);
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
            fallback_groups = blocks.iter().map(|groups| groups.len()).sum::<usize>(),
            "OCR source plane could not be estimated"
        );
        let fallback = blocks
            .iter()
            .enumerate()
            .flat_map(|(eye, groups)| {
                groups.iter().cloned().map(move |mut group| {
                    if eye == 1 {
                        group.source.id += right_id_offset;
                    }
                    group
                })
            })
            .collect();
        return Ok((None, fallback));
    };
    let mut selected = Vec::new();
    let mut fallback = Vec::new();
    let mut paired_right = vec![false; blocks[1].len()];
    for (left, group) in blocks[0].iter().enumerate() {
        let member_matches: Vec<_> = matches
            .iter()
            .filter(|(member, _, _)| fragments[0][*member].0 == left)
            .collect();
        let related: Vec<_> = blocks[1]
            .iter()
            .enumerate()
            .filter(|(right, other)| {
                member_matches
                    .iter()
                    .any(|(_, member, _)| fragments[1][*member].0 == *right)
                    && (normalized_text(&group.source.text) == normalized_text(&other.source.text)
                        || member_matches
                            .iter()
                            .filter(|(_, member, _)| fragments[1][*member].0 == *right)
                            .count()
                            == other.fragments().len())
            })
            .map(|(right, _)| right)
            .collect();
        if related.is_empty() {
            fallback.push(group.clone());
            continue;
        }
        for &right in &related {
            paired_right[right] = true;
        }
        let mut block = group.clone();
        let other = related.iter().find_map(|&right| {
            (normalized_text(&group.source.text) == normalized_text(&blocks[1][right].source.text))
                .then_some(&blocks[1][right])
        });
        for (target, translation) in block.translations.iter_mut().enumerate() {
            if translation
                .text
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
            {
                continue;
            }
            if let Some(ready) = other
                .and_then(|other| other.translations.get(target))
                .filter(|t| {
                    t.target_language == translation.target_language
                        && t.text.as_deref().is_some_and(|s| !s.trim().is_empty())
                })
            {
                *translation = ready.clone();
            }
        }
        if member_matches.iter().all(|(_, _, point)| {
            dot(sub(*point, center), normal).abs() <= plane_tolerance(-center[2], error_per_m2)
        }) {
            selected.push(block);
        } else {
            fallback.push(block);
        }
    }
    fallback.extend(
        blocks[1]
            .iter()
            .enumerate()
            .filter(|(index, _)| !paired_right[*index])
            .map(|(_, block)| {
                let mut block = block.clone();
                block.source.id += right_id_offset;
                block
            }),
    );
    // Keep scene geometry in Rust. The surface reports text that does not fit.
    let excluded: Vec<_> = blocks[0]
        .iter()
        .filter(|group| {
            !selected
                .iter()
                .any(|block| block.source.id == group.source.id)
        })
        .cloned()
        .collect();
    let (presentation, unplaced) = build_scene(
        eye,
        eye_pose,
        &selected,
        &excluded,
        center,
        normal,
        opacity,
        source_view,
    );
    fallback.extend(
        selected
            .into_iter()
            .filter(|block| unplaced.contains(&block.source.id)),
    );
    Ok((presentation, fallback))
}

fn match_blocks(
    eyes: &[&EyeCapture; 2],
    blocks: &[Vec<(usize, &TextBlock)>; 2],
    to_reference: [[f32; 4]; 3],
) -> Vec<(usize, usize, [f32; 3])> {
    let texts = blocks.each_ref().map(|blocks| {
        blocks
            .iter()
            .map(|(_, block)| normalized_text(&block.text))
            .collect::<Vec<_>>()
    });
    let centers = blocks.each_ref().map(|blocks| {
        blocks
            .iter()
            .map(|(_, block)| {
                std::array::from_fn(|c| block.polygon.iter().map(|p| p[c]).sum::<f32>() * 0.25)
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

fn normalized_text(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
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
    eyes: &[&EyeCapture; 2],
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
        let local = ray(eyes[index], pixels[index])?;
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

fn display_text(block: &TranslatedBlock, source_view: bool) -> String {
    let translated = if source_view {
        String::new()
    } else {
        block
            .translations
            .iter()
            .filter_map(|translation| translation.text.as_deref())
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    };
    if translated.is_empty() {
        block.source.text.clone()
    } else {
        translated
    }
}

#[allow(clippy::too_many_arguments)]
fn build_scene(
    eye: &EyeCapture,
    eye_pose: [[f32; 4]; 3],
    selected: &[TranslatedBlock],
    excluded: &[TranslatedBlock],
    center: [f32; 3],
    normal: [f32; 3],
    opacity: f32,
    source_view: bool,
) -> (Option<OcrPresentation>, Vec<usize>) {
    let Some(axis_x) = normalize([normal[2], 0., -normal[0]]) else {
        return (None, selected.iter().map(|block| block.source.id).collect());
    };
    let axis_y = [
        normal[1] * axis_x[2] - normal[2] * axis_x[1],
        normal[2] * axis_x[0] - normal[0] * axis_x[2],
        normal[0] * axis_x[1] - normal[1] * axis_x[0],
    ];
    let project = |quad: Quad| -> Option<Quad> {
        Homography::from_quad(quad).and_then(Homography::inverse)?;
        let mut projected = [[0.; 2]; 4];
        for (pixel, point) in quad.into_iter().zip(&mut projected) {
            let direction = ray(eye, pixel)?;
            let distance = dot(normal, center) / dot(normal, direction);
            if !distance.is_finite() || distance <= 0. {
                return None;
            }
            let relative = sub(direction.map(|v| v * distance), center);
            *point = [dot(relative, axis_x), dot(relative, axis_y)];
        }
        Some(projected)
    };
    let mut blocks = Vec::new();
    let mut unplaced = Vec::new();
    for block in selected {
        let projected = project(block.source.polygon).zip(
            block
                .fragments()
                .iter()
                .map(|fragment| project(fragment.polygon))
                .collect::<Option<Vec<_>>>(),
        );
        let Some((quad, masks)) = projected else {
            unplaced.push(block.source.id);
            continue;
        };
        blocks.push(OcrSceneBlock {
            id: block.source.id,
            text: display_text(block, source_view),
            quad,
            masks,
        });
    }
    if blocks.is_empty() {
        return (None, unplaced);
    }
    let mut extent = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for point in blocks
        .iter()
        .flat_map(|block| std::iter::once(&block.quad).chain(&block.masks))
        .flatten()
    {
        extent = [
            extent[0].min(point[0]),
            extent[1].min(point[1]),
            extent[2].max(point[0]),
            extent[3].max(point[1]),
        ];
    }
    let width = extent[2] - extent[0];
    let height = extent[3] - extent[1];
    if !width.is_finite() || !height.is_finite() || width <= 0.001 || height <= 0.001 {
        unplaced.extend(blocks.iter().map(|block| block.id));
        return (None, unplaced);
    }
    // Leave nearby space for readable cards without changing source positions.
    let margin_x = (width * 0.15).max(height / 8.).max(0.04);
    let margin_y = (height * 0.2).max(width / 8.).max(0.035);
    let [x0, y0, x1, y1] = [
        extent[0] - margin_x,
        extent[1] - margin_y,
        extent[2] + margin_x,
        extent[3] + margin_y,
    ];
    let (width_m, height_m) = (x1 - x0, y1 - y0);
    let normalize_quad = |quad: Quad| quad.map(|[x, y]| [(x - x0) / width_m, (y1 - y) / height_m]);
    for block in &mut blocks {
        block.quad = normalize_quad(block.quad);
        for mask in &mut block.masks {
            *mask = normalize_quad(*mask);
        }
    }
    let excluded = excluded
        .iter()
        .flat_map(TranslatedBlock::fragments)
        .filter_map(|fragment| project(fragment.polygon))
        .map(normalize_quad)
        .collect();
    let position = scaled_add(
        scaled_add(center, axis_x, (x0 + x1) * 0.5),
        axis_y,
        (y0 + y1) * 0.5,
    );
    let local_pose = std::array::from_fn(|r| [axis_x[r], axis_y[r], normal[r], position[r]]);
    let aspect = width_m / height_m;
    (
        Some(OcrPresentation {
            plane: PlaneOverlay {
                pose: transform::compose(eye_pose, local_pose),
                width_m,
                texel_aspect: aspect,
            },
            blocks: selected
                .iter()
                .filter(|source| blocks.iter().any(|block| block.id == source.source.id))
                .cloned()
                .collect(),
            scene: OcrScene {
                blocks,
                excluded,
                opacity: opacity.clamp(0., 1.),
                aspect,
            },
        }),
        unplaced,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_overlay::{renderer::Texture, transform};
    use vrcs_core::ocr::{BlockTranslation, TextBlock};

    fn fixture(depth: f32, head: [[f32; 4]; 3]) -> ([EyeCapture; 2], [Vec<TranslatedBlock>; 2]) {
        fixture_with_tilt(depth, head, [0.; 2])
    }

    fn fixture_with_tilt(
        depth: f32,
        head: [[f32; 4]; 3],
        tilt: [f32; 2],
    ) -> ([EyeCapture; 2], [Vec<TranslatedBlock>; 2]) {
        let raw_projections = [[-1.2, 0.8, -0.9, 1.1], [-0.8, 1.2, -0.9, 1.1]];
        let eyes = std::array::from_fn(|index| EyeCapture {
            image: Texture {
                width: 320,
                height: 240,
                pixels: vec![],
            },
            projection: super::super::ocr_capture::projection_from_openvr(raw_projections[index]),
            eye_to_head: transform::matrix(
                0.,
                0.,
                0.,
                [if index == 0 { -0.032 } else { 0.032 }, 0., 0.],
            ),
            head_pose: head,
        });
        let project = |index: usize, point: [f32; 3]| {
            let eye = &eyes[index];
            let [l, r, t, b] = raw_projections[index];
            [
                (point[0] - eye.eye_to_head[0][3]) / -point[2] - l,
                b - point[1] / -point[2],
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
                    let center: [f32; 2] = project(index, *p);
                    TranslatedBlock {
                        fragments: vec![],
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
    fn asymmetric_openvr_projection_preserves_image_edge_rays() {
        let (mut eyes, _) = fixture(2., transform::matrix(0., 0., 0., [0.; 3]));
        for raw in [
            [-1.2, 0.8, -1.3, 0.7],
            [-0.8, 1.2, -0.7, 1.3],
            [-1., 1., -1., 1.],
        ] {
            eyes[0].projection = super::super::ocr_capture::projection_from_openvr(raw);
            for (pixel, expected) in [
                ([0., 0.], [raw[0], raw[3], -1.]),
                ([320., 240.], [raw[1], raw[2], -1.]),
            ] {
                for (actual, expected) in ray(&eyes[0], pixel).unwrap().into_iter().zip(expected) {
                    assert!(
                        (actual - expected).abs() < 0.00001,
                        "{actual} != {expected}"
                    );
                }
            }
        }
    }

    #[test]
    fn grouped_text_uses_original_fragment_centers_for_depth() {
        let (eyes, blocks) = fixture(2., transform::matrix(0., 0., 0., [0.; 3]));
        let groups = blocks.map(|blocks| {
            let mut group = blocks[0].clone();
            group.fragments = blocks.iter().map(|block| block.source.clone()).collect();
            group.source.text = "word 0 word 1 word 2".into();
            // Equal display envelopes have no stereo disparity. Raw fragments still do.
            group.source.polygon = [[20., 20.], [200., 20.], [200., 100.], [20., 100.]];
            vec![group]
        });
        let (overlay, fallback) = render(&eyes, &groups, 0.8, false).unwrap();
        let overlay = overlay.expect("source plane");
        assert!((overlay.plane.pose[2][3] + 2.).abs() < 0.01);
        assert!(fallback.is_empty());
    }

    #[test]
    fn partially_matched_groups_stay_on_the_source_plane() {
        let (eyes, blocks) = fixture(2., transform::matrix(0., 0., 0., [0.; 3]));
        let mut groups = blocks.each_ref().map(|blocks| {
            let mut group = blocks[0].clone();
            group.source.text = "first complete group".into();
            group.fragments = blocks[..2]
                .iter()
                .map(|block| block.source.clone())
                .collect();
            vec![group, blocks[2].clone()]
        });
        groups[1][0].fragments[1].text = "unmatched fragment".into();
        let (plane, fallback) = render(&eyes, &groups, 0.8, false).unwrap();
        assert!(plane.is_some());
        assert!(
            fallback.is_empty(),
            "one OCR mismatch must not discard a paragraph"
        );
    }

    #[test]
    fn different_eye_grouping_does_not_discard_a_complete_paragraph() {
        let (eyes, mut blocks) = fixture(2., transform::matrix(0., 0., 0., [0.; 3]));
        let mut group = blocks[0][0].clone();
        group.fragments = blocks[0].iter().map(|block| block.source.clone()).collect();
        group.source.text = "word 0 word 1 word 2".into();
        group.source.polygon = [[100., 100.], [230., 100.], [230., 160.], [100., 160.]];
        blocks[0] = vec![group];
        let (plane, fallback) = render(&eyes, &blocks, 0.8, false).unwrap();
        assert!(plane.is_some());
        assert!(
            fallback.is_empty(),
            "right-eye lines are already represented by the paragraph"
        );
    }

    #[test]
    fn a_missing_line_in_one_eye_keeps_the_complete_eye_paragraph() {
        let (eyes, blocks) = fixture(2., transform::matrix(0., 0., 0., [0.; 3]));
        let mut groups = blocks.map(|blocks| {
            let mut group = blocks[0].clone();
            group.fragments = blocks.iter().map(|block| block.source.clone()).collect();
            group.source.text = "word 0 word 1 word 2".into();
            group.source.polygon = [[100., 100.], [230., 100.], [230., 160.], [100., 160.]];
            vec![group]
        });
        groups[1][0].fragments.pop();
        groups[1][0].source.text = "word 0 word 1".into();
        for (eyes, groups) in [
            (&eyes, &groups),
            (
                &[eyes[1].clone(), eyes[0].clone()],
                &[groups[1].clone(), groups[0].clone()],
            ),
        ] {
            let (plane, fallback) = render(eyes, groups, 0.8, false).unwrap();
            assert!((plane.expect("source plane").plane.pose[2][3] + 2.).abs() < 0.01);
            assert!(fallback.is_empty());
        }
    }

    #[test]
    fn result_plane_uses_source_depth_instead_of_hand_depth() {
        for depth in [1., 4.] {
            let (eyes, blocks) = fixture(depth, transform::matrix(0., 0., 0., [0.; 3]));
            let (overlay, limited) = render(&eyes, &blocks, 1., false).unwrap();
            let overlay = overlay.expect("source plane");
            assert!(limited.is_empty());
            assert!((overlay.plane.pose[2][3] + depth).abs() < 0.01);
            assert!(overlay.plane.width_m > 0. && overlay.plane.texel_aspect > 0.);
            let position = overlay.plane.pose.map(|r| r[3]);
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
        let (overlay, _) = render(&eyes, &blocks, 1., false).unwrap();
        let overlay = overlay.expect("source plane");
        let local = transform::compose(transform::inverse(head), overlay.plane.pose);
        assert!((local[2][3] + 2.).abs() < 0.01);
        assert!((local[0][0] - 1.).abs() < 0.01);
    }

    #[test]
    fn tilted_source_plane_sets_surface_orientation() {
        let (eyes, blocks) =
            fixture_with_tilt(2., transform::matrix(0., 0., 0., [0.; 3]), [0.5, 0.2]);
        let (overlay, _) = render(&eyes, &blocks, 1., false).unwrap();
        let overlay = overlay.expect("source plane");
        let expected = normalize([-0.5, -0.2, 1.]).unwrap();
        let actual = overlay.plane.pose.map(|r| r[2]);
        assert!(dot(expected, actual) > 0.999);
        let [x, y, z] = overlay.plane.pose.map(|r| r[3]);
        assert!((z - 0.5 * x - 0.2 * y + 2.).abs() < 0.01);
    }

    #[test]
    fn scene_quads_keep_source_coordinates_and_physical_aspect() {
        let (eyes, blocks) =
            fixture_with_tilt(2., transform::matrix(0., 0., 0., [0.; 3]), [0.5, 0.2]);
        let (presentation, fallback) = render(&eyes, &blocks, 0.8, false).unwrap();
        let presentation = presentation.unwrap();
        assert!(fallback.is_empty());
        let plane = &presentation.plane;
        let scene = &presentation.scene;
        assert!((scene.aspect - plane.texel_aspect).abs() < 0.00001);
        let height_m = plane.width_m / plane.texel_aspect;
        let to_eye = eyes[0].tracking_to_eye();
        for block in &scene.blocks {
            for ([u, v], expected) in block
                .quad
                .into_iter()
                .zip(blocks[0][block.id].source.polygon)
            {
                let local = [(u - 0.5) * plane.width_m, (0.5 - v) * height_m, 0.];
                let world: [f32; 3] = std::array::from_fn(|r| {
                    plane.pose[r][3] + (0..3).map(|c| plane.pose[r][c] * local[c]).sum::<f32>()
                });
                let eye: [f32; 3] = std::array::from_fn(|r| {
                    to_eye[r][3] + (0..3).map(|c| to_eye[r][c] * world[c]).sum::<f32>()
                });
                let [l, r, t, b] = eyes[0].projection;
                let actual = [
                    (eye[0] / -eye[2] - l) / (r - l) * 320.,
                    (-eye[1] / -eye[2] - t) / (b - t) * 240.,
                ];
                for (actual, expected) in actual.into_iter().zip(expected) {
                    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
                }
            }
        }
    }

    #[test]
    fn invalid_scene_quad_falls_back_without_losing_the_translation() {
        let (eyes, mut blocks) = fixture(2., transform::matrix(0., 0., 0., [0.; 3]));
        blocks[0][0].source.polygon[0][0] = -20.;
        let (presentation, fallback) = render(&eyes, &blocks, 1., false).unwrap();
        assert!(presentation.is_some());
        assert!(fallback.iter().any(|block| block.source.id == 0));
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
        let (plane, _) = render(&eyes, &blocks, 1., false).unwrap();
        let plane = plane.expect("source plane");
        assert!((plane.plane.pose[2][3] + 2.).abs() < 0.01);
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
        let (plane, fallback) = render(&eyes, &blocks, 1., false).unwrap();
        let plane = plane.expect("noisy plane");
        assert!((plane.plane.pose[2][3] + 2.).abs() < 0.06);
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
        let (plane, fallback) = render(&eyes, &blocks, 1., false).unwrap();
        let plane = plane.expect("source plane");
        assert!((plane.plane.pose[2][3] + 2.).abs() < 0.01);
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].source.id, 10);
    }

    #[test]
    fn invalid_stereo_correspondence_does_not_place_a_result_at_arbitrary_depth() {
        let (eyes, mut blocks) = fixture(2., transform::matrix(0., 0., 0., [0.; 3]));
        for block in &mut blocks[1] {
            block.source.text = "unmatched".into();
        }
        assert!(render(&eyes, &blocks, 1., false).unwrap().0.is_none());
    }
}
