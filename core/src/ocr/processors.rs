//! Pixel processing for the PP-OCRv6 ONNX models.
//!
//! Follows PaddleX text detection/recognition processors (PaddlePaddle Authors,
//! Apache-2.0): https://github.com/PaddlePaddle/PaddleX/tree/develop/paddlex/inference/models
//! Geometry is implemented here so the local backend needs no OpenCV runtime.

use ndarray::Array4;
use std::collections::VecDeque;

pub type Quad = [[f32; 2]; 4];

pub fn det_input(rgba: &[u8], width: u32, height: u32) -> Result<Array4<f32>, String> {
    validate_image(rgba, width, height)?;
    let ratio = (736.0 / width.min(height) as f32)
        .max(1.0)
        .min(1536.0 / width.max(height) as f32);
    let resize = |side: u32| ((side as f32 * ratio / 32.0).round() as usize * 32).max(32);
    let (w, h) = (resize(width), resize(height));
    let mean = [0.485, 0.456, 0.406];
    let std = [0.229, 0.224, 0.225];
    Ok(Array4::from_shape_fn((1, 3, h, w), |(_, c, y, x)| {
        let sx = (x as f32 + 0.5) * width as f32 / w as f32 - 0.5;
        let sy = (y as f32 + 0.5) * height as f32 / h as f32 - 0.5;
        (bilinear(rgba, width as usize, height as usize, 4, sx, sy, 2 - c).round() / 255.0
            - mean[c])
            / std[c]
    }))
}

pub fn det_boxes(
    prob: &[f32],
    map_width: usize,
    map_height: usize,
    width: u32,
    height: u32,
) -> Result<Vec<Quad>, String> {
    if map_width == 0
        || map_height == 0
        || width == 0
        || height == 0
        || map_width.checked_mul(map_height) != Some(prob.len())
        || prob.iter().any(|p| !p.is_finite())
    {
        return Err("Invalid OCR detector output".into());
    }
    let bitmap: Vec<bool> = prob.iter().map(|&p| p > 0.2).collect();
    let mut visited = vec![false; prob.len()];
    let mut boxes = Vec::new();
    let mut candidates = 0;
    for seed in 0..prob.len() {
        if visited[seed] || !bitmap[seed] {
            continue;
        }
        candidates += 1;
        if candidates > 3000 || boxes.len() == 256 {
            break;
        }
        let mut queue = VecDeque::from([seed]);
        visited[seed] = true;
        let mut count = 0;
        while let Some(index) = queue.pop_front() {
            count += 1;
            for [dx, dy] in NEIGHBORS {
                let (x, y) = (index % map_width, index / map_width);
                let (nx, ny) = (x as isize + dx, y as isize + dy);
                if nx < 0 || ny < 0 || nx >= map_width as isize || ny >= map_height as isize {
                    continue;
                }
                let next = ny as usize * map_width + nx as usize;
                if bitmap[next] && !visited[next] {
                    visited[next] = true;
                    queue.push_back(next);
                }
            }
        }
        if count < 9 {
            continue;
        }
        let contour = trace_contour(&bitmap, map_width, map_height, seed, count);
        let Some((quad, short)) = minimum_rect(&contour) else {
            continue;
        };
        if short < 3.0 || box_score(prob, map_width, map_height, quad) < 0.45 {
            continue;
        }
        let expanded = round_offset(quad, 1.4);
        let Some((mut quad, short)) = minimum_rect(&expanded) else {
            continue;
        };
        if short < 5.0 {
            continue;
        }
        for point in &mut quad {
            point[0] = (point[0] * width as f32 / map_width as f32)
                .round()
                .clamp(0.0, (width - 1) as f32);
            point[1] = (point[1] * height as f32 / map_height as f32)
                .round()
                .clamp(0.0, (height - 1) as f32);
        }
        if distance(quad[0], quad[1]) > 3.0 && distance(quad[0], quad[3]) > 3.0 {
            boxes.push(quad);
        }
    }
    boxes.sort_by(|a, b| {
        a[0][1]
            .total_cmp(&b[0][1])
            .then(a[0][0].total_cmp(&b[0][0]))
    });
    // Paddle's reading order swaps nearby lines into left-to-right order.
    for i in 1..boxes.len() {
        let mut j = i;
        while j > 0
            && (boxes[j][0][1] - boxes[j - 1][0][1]).abs() < 10.0
            && boxes[j][0][0] < boxes[j - 1][0][0]
        {
            boxes.swap(j, j - 1);
            j -= 1;
        }
    }
    Ok(boxes)
}

pub fn crop_and_rec_input(
    rgba: &[u8],
    width: u32,
    height: u32,
    quad: Quad,
) -> Result<Array4<f32>, String> {
    validate_image(rgba, width, height)?;
    if quad.iter().flatten().any(|p| !p.is_finite()) {
        return Err("Invalid OCR text polygon".into());
    }
    let w = distance(quad[0], quad[1]).max(distance(quad[2], quad[3])) as usize;
    let h = distance(quad[0], quad[3]).max(distance(quad[1], quad[2])) as usize;
    if w < 2 || h < 2 || w > 2 * width as usize || h > 2 * height as usize {
        return Err("Invalid OCR text crop dimensions".into());
    }
    let transform = homography(
        [
            [0.0, 0.0],
            [w as f32, 0.0],
            [w as f32, h as f32],
            [0.0, h as f32],
        ],
        quad,
    )?;
    let rotated = h as f32 / w as f32 >= 1.5;
    let (crop_w, crop_h) = if rotated { (h, w) } else { (w, h) };
    let mut crop = vec![0u8; crop_w * crop_h * 3];
    for y in 0..crop_h {
        for x in 0..crop_w {
            // np.rot90 performs a counterclockwise quarter turn for tall crops.
            let (px, py) = if rotated {
                ((w - 1 - y) as f32, x as f32)
            } else {
                (x as f32, y as f32)
            };
            let [sx, sy] = project(transform, px, py);
            for c in 0..3 {
                crop[(y * crop_w + x) * 3 + c] =
                    bicubic(rgba, width as usize, height as usize, sx, sy, 2 - c)
                        .round()
                        .clamp(0.0, 255.0) as u8;
            }
        }
    }
    let resized_w = (48.0 * crop_w as f32 / crop_h as f32)
        .ceil()
        .clamp(1.0, 3200.0) as usize;
    let input_w = resized_w.max(320);
    Ok(Array4::from_shape_fn(
        (1, 3, 48, input_w),
        |(_, c, y, x)| {
            if x >= resized_w {
                return 0.0;
            }
            let sx = (x as f32 + 0.5) * crop_w as f32 / resized_w as f32 - 0.5;
            let sy = (y as f32 + 0.5) * crop_h as f32 / 48.0 - 0.5;
            bilinear(&crop, crop_w, crop_h, 3, sx, sy, c).round() / 127.5 - 1.0
        },
    ))
}

pub fn ctc_decode(
    prob: &[f32],
    steps: usize,
    classes: usize,
    characters: &[String],
) -> Result<(String, f32), String> {
    if classes == 0
        || classes != characters.len()
        || steps.checked_mul(classes) != Some(prob.len())
        || prob.iter().any(|p| !p.is_finite())
    {
        return Err("Invalid OCR recognizer output or dictionary".into());
    }
    let mut text = String::new();
    let (mut previous, mut score, mut count) = (0, 0.0, 0);
    for row in prob.chunks_exact(classes) {
        let mut best = 0;
        for id in 1..classes {
            if row[id] > row[best] {
                best = id;
            }
        }
        if best != 0 && best != previous {
            text.push_str(&characters[best]);
            score += row[best];
            count += 1;
        }
        previous = best;
    }
    Ok((
        text,
        if count == 0 {
            0.0
        } else {
            score / count as f32
        },
    ))
}

pub(super) fn validate_image(rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    if width == 0
        || height == 0
        || (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            != Some(rgba.len())
    {
        return Err("Invalid OCR RGBA image".into());
    }
    Ok(())
}

fn pixel(
    image: &[u8],
    width: usize,
    height: usize,
    channels: usize,
    x: isize,
    y: isize,
    c: usize,
) -> f32 {
    image[(y.clamp(0, height as isize - 1) as usize * width
        + x.clamp(0, width as isize - 1) as usize)
        * channels
        + c] as f32
}

fn bilinear(
    image: &[u8],
    width: usize,
    height: usize,
    channels: usize,
    x: f32,
    y: f32,
    c: usize,
) -> f32 {
    let (ix, iy) = (x.floor() as isize, y.floor() as isize);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let upper = pixel(image, width, height, channels, ix, iy, c) * (1.0 - fx)
        + pixel(image, width, height, channels, ix + 1, iy, c) * fx;
    let lower = pixel(image, width, height, channels, ix, iy + 1, c) * (1.0 - fx)
        + pixel(image, width, height, channels, ix + 1, iy + 1, c) * fx;
    upper * (1.0 - fy) + lower * fy
}

fn bicubic(image: &[u8], width: usize, height: usize, x: f32, y: f32, c: usize) -> f32 {
    // OpenCV INTER_CUBIC uses a=-0.75 with replicated edge pixels.
    let weight = |t: f32| {
        let t = t.abs();
        if t <= 1.0 {
            1.25 * t.powi(3) - 2.25 * t.powi(2) + 1.0
        } else if t < 2.0 {
            -0.75 * t.powi(3) + 3.75 * t.powi(2) - 6.0 * t + 3.0
        } else {
            0.0
        }
    };
    let (ix, iy) = (x.floor() as isize, y.floor() as isize);
    let mut result = 0.0;
    for dy in -1..=2 {
        for dx in -1..=2 {
            result += pixel(image, width, height, 4, ix + dx, iy + dy, c)
                * weight(x - (ix + dx) as f32)
                * weight(y - (iy + dy) as f32);
        }
    }
    result
}

const NEIGHBORS: [[isize; 2]; 8] = [
    [-1, 0],
    [-1, -1],
    [0, -1],
    [1, -1],
    [1, 0],
    [1, 1],
    [0, 1],
    [-1, 1],
];

fn trace_contour(
    bitmap: &[bool],
    width: usize,
    height: usize,
    seed: usize,
    count: usize,
) -> Vec<[f32; 2]> {
    let start = [(seed % width) as isize, (seed / width) as isize];
    let mut current = start;
    let mut back = [start[0] - 1, start[1]];
    let mut first = None;
    let mut contour = Vec::new();
    for _ in 0..count.saturating_mul(16) {
        let relative = [back[0] - current[0], back[1] - current[1]];
        let origin = NEIGHBORS.iter().position(|&n| n == relative).unwrap_or(0);
        let next = (0..8).find_map(|offset| {
            let index = (origin + offset) % 8;
            let [dx, dy] = NEIGHBORS[index];
            let p = [current[0] + dx, current[1] + dy];
            (p[0] >= 0
                && p[1] >= 0
                && p[0] < width as isize
                && p[1] < height as isize
                && bitmap[p[1] as usize * width + p[0] as usize])
                .then_some((index, p))
        });
        let Some((direction, next)) = next else {
            break;
        };
        if current == start && first == Some(next) {
            break;
        }
        first.get_or_insert(next);
        contour.push([current[0] as f32, current[1] as f32]);
        let [dx, dy] = NEIGHBORS[(direction + 7) % 8];
        back = [current[0] + dx, current[1] + dy];
        current = next;
    }
    contour
}

fn cross(o: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}
fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn convex_hull(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut points = points.to_vec();
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let mut hull = Vec::new();
    for &p in &points {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    let lower = hull.len();
    for &p in points[..points.len() - 1].iter().rev() {
        while hull.len() > lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(p);
    }
    hull.pop();
    hull
}

fn minimum_rect(points: &[[f32; 2]]) -> Option<(Quad, f32)> {
    let hull = convex_hull(points);
    if hull.len() < 3 {
        return None;
    }
    let mut best_area = f32::INFINITY;
    let mut best = None;
    for i in 0..hull.len() {
        let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
        let length = distance(a, b);
        let u = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
        let v = [-u[1], u[0]];
        let (mut lo_u, mut hi_u, mut lo_v, mut hi_v) = (
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
        );
        for p in &hull {
            let (pu, pv) = (p[0] * u[0] + p[1] * u[1], p[0] * v[0] + p[1] * v[1]);
            lo_u = lo_u.min(pu);
            hi_u = hi_u.max(pu);
            lo_v = lo_v.min(pv);
            hi_v = hi_v.max(pv);
        }
        let area = (hi_u - lo_u) * (hi_v - lo_v);
        if area < best_area {
            best_area = area;
            let mut q = [[lo_u, lo_v], [hi_u, lo_v], [hi_u, hi_v], [lo_u, hi_v]]
                .map(|p| [p[0] * u[0] + p[1] * v[0], p[0] * u[1] + p[1] * v[1]]);
            // Match Paddle's left-pair/right-pair point ordering.
            q.sort_by(|a, b| a[0].total_cmp(&b[0]));
            let (tl, bl) = if q[0][1] <= q[1][1] {
                (q[0], q[1])
            } else {
                (q[1], q[0])
            };
            let (tr, br) = if q[2][1] <= q[3][1] {
                (q[2], q[3])
            } else {
                (q[3], q[2])
            };
            best = Some(([tl, tr, br, bl], (hi_u - lo_u).min(hi_v - lo_v)));
        }
    }
    best
}

fn box_score(prob: &[f32], width: usize, height: usize, q: Quad) -> f32 {
    let xmin = q
        .iter()
        .map(|p| p[0].floor() as isize)
        .min()
        .unwrap()
        .clamp(0, width as isize - 1) as usize;
    let xmax = q
        .iter()
        .map(|p| p[0].ceil() as isize)
        .max()
        .unwrap()
        .clamp(0, width as isize - 1) as usize;
    let ymin = q
        .iter()
        .map(|p| p[1].floor() as isize)
        .min()
        .unwrap()
        .clamp(0, height as isize - 1) as usize;
    let ymax = q
        .iter()
        .map(|p| p[1].ceil() as isize)
        .max()
        .unwrap()
        .clamp(0, height as isize - 1) as usize;
    let (mut score, mut count) = (0.0, 0);
    for y in ymin..=ymax {
        for x in xmin..=xmax {
            let point = [x as f32, y as f32];
            if (0..4).all(|i| cross(q[i], q[(i + 1) % 4], point) >= -1e-3) {
                score += prob[y * width + x];
                count += 1;
            }
        }
    }
    if count == 0 {
        0.0
    } else {
        score / count as f32
    }
}

fn round_offset(q: Quad, ratio: f32) -> Vec<[f32; 2]> {
    let area = (0..4)
        .map(|i| q[i][0] * q[(i + 1) % 4][1] - q[i][1] * q[(i + 1) % 4][0])
        .sum::<f32>()
        .abs()
        / 2.0;
    let perimeter = (0..4).map(|i| distance(q[i], q[(i + 1) % 4])).sum::<f32>();
    let radius = area * ratio / perimeter;
    let mut points = Vec::with_capacity(68);
    for i in 0..4 {
        let previous = q[(i + 3) % 4];
        let current = q[i];
        let next = q[(i + 1) % 4];
        let from = (previous[0] - current[0]).atan2(current[1] - previous[1]);
        let to = (current[0] - next[0]).atan2(next[1] - current[1]);
        let angle = (to - from).rem_euclid(std::f32::consts::TAU);
        for step in 0..=16 {
            let theta = from + angle * step as f32 / 16.0;
            points.push([
                current[0] + radius * theta.cos(),
                current[1] + radius * theta.sin(),
            ]);
        }
    }
    points
}

fn homography(from: Quad, to: Quad) -> Result<[f32; 8], String> {
    let mut matrix = [[0.0_f64; 9]; 8];
    for i in 0..4 {
        let (x, y, u, v) = (
            from[i][0] as f64,
            from[i][1] as f64,
            to[i][0] as f64,
            to[i][1] as f64,
        );
        matrix[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        matrix[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    for column in 0..8 {
        let pivot = (column..8)
            .max_by(|&a, &b| matrix[a][column].abs().total_cmp(&matrix[b][column].abs()))
            .unwrap();
        if matrix[pivot][column].abs() < 1e-10 {
            return Err("Degenerate OCR text polygon".into());
        }
        matrix.swap(column, pivot);
        let divisor = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= divisor;
        }
        let pivot_row = matrix[column];
        for (row_index, row) in matrix.iter_mut().enumerate() {
            if row_index == column {
                continue;
            }
            let factor = row[column];
            for (value, pivot_value) in row[column..].iter_mut().zip(&pivot_row[column..]) {
                *value -= factor * pivot_value;
            }
        }
    }
    Ok(std::array::from_fn(|i| matrix[i][8] as f32))
}

fn project(h: [f32; 8], x: f32, y: f32) -> [f32; 2] {
    let denominator = h[6] * x + h[7] * y + 1.0;
    [
        (h[0] * x + h[1] * y + h[2]) / denominator,
        (h[3] * x + h[4] * y + h[5]) / denominator,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_image(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        (0..width * height).flat_map(|_| color).collect()
    }

    #[test]
    fn detector_normalizes_bgr_and_ignores_alpha() {
        let input = det_input(&solid_image(32, 32, [255, 128, 0, 0]), 32, 32).unwrap();
        assert!(input.shape()[2] <= 1536 && input.shape()[3] <= 1536);
        assert!((input[[0, 0, 0, 0]] - (-0.485 / 0.229)).abs() < 1e-5);
        assert!((input[[0, 1, 0, 0]] - ((128.0 / 255.0 - 0.456) / 0.224)).abs() < 1e-5);
        assert!((input[[0, 2, 0, 0]] - ((1.0 - 0.406) / 0.225)).abs() < 1e-5);
    }

    #[test]
    fn detector_preserves_rotated_text_geometry() {
        let mut prob = vec![0.0; 64 * 64];
        for y in 0..64 {
            for x in 0..64 {
                let along = (x as f32 + y as f32 - 64.0) / 2.0_f32.sqrt();
                let across = (y as f32 - x as f32) / 2.0_f32.sqrt();
                if along.abs() <= 18.0 && across.abs() <= 4.0 {
                    prob[y * 64 + x] = 0.95;
                }
            }
        }
        let boxes = det_boxes(&prob, 64, 64, 128, 128).unwrap();
        assert_eq!(boxes.len(), 1);
        let q = boxes[0];
        assert!((q[1][1] - q[0][1]).abs() > 12.0, "{q:?}");
        let area = ((q[1][0] - q[0][0]) * (q[3][1] - q[0][1])
            - (q[1][1] - q[0][1]) * (q[3][0] - q[0][0]))
            .abs();
        assert!(area > 1100.0 && area < 4000.0, "{area}");
    }

    #[test]
    fn detector_rejects_low_score_and_empty_regions() {
        assert!(det_boxes(&vec![0.3; 32 * 32], 32, 32, 32, 32)
            .unwrap()
            .is_empty());
        assert!(det_boxes(&vec![0.0; 32 * 32], 32, 32, 32, 32)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn round_offset_expands_a_rotated_rectangle_by_area_over_perimeter() {
        let quad = [[0.0, 0.0], [20.0, 20.0], [16.0, 24.0], [-4.0, 4.0]];
        let (expanded, short) = minimum_rect(&round_offset(quad, 1.4)).unwrap();
        let original_short = 32.0_f32.sqrt();
        let original_long = 800.0_f32.sqrt();
        let radius =
            original_short * original_long * 1.4 / (2.0 * (original_short + original_long));
        assert!((short - original_short - 2.0 * radius).abs() < 0.01);
        assert!((distance(expanded[0], expanded[1]) - original_long - 2.0 * radius).abs() < 0.01);
    }

    #[test]
    fn detector_uses_multiple_of_32_dimensions_with_a_bounded_long_side() {
        let input = det_input(&solid_image(2000, 32, [0, 0, 0, 255]), 2000, 32).unwrap();
        assert_eq!(input.shape(), &[1, 3, 32, 1536]);
    }

    #[test]
    fn recognizer_normalizes_bgr_with_zero_padding() {
        let input = crop_and_rec_input(
            &solid_image(32, 16, [255, 0, 0, 255]),
            32,
            16,
            [[0.0, 0.0], [31.0, 0.0], [31.0, 15.0], [0.0, 15.0]],
        )
        .unwrap();
        assert_eq!(input.shape(), &[1, 3, 48, 320]);
        assert_eq!(input[[0, 0, 0, 0]], -1.0);
        assert_eq!(input[[0, 2, 0, 0]], 1.0);
        assert_eq!(input[[0, 2, 0, 319]], 0.0);
    }

    #[test]
    fn perspective_crop_maps_a_trapezoid_to_a_text_line() {
        let mut rgba = solid_image(64, 32, [0, 0, 0, 255]);
        for y in 0..32 {
            for x in 0..64 {
                rgba[(y * 64 + x) * 4] = (x * 4) as u8;
            }
        }
        let input = crop_and_rec_input(
            &rgba,
            64,
            32,
            [[8.0, 4.0], [56.0, 4.0], [44.0, 28.0], [20.0, 28.0]],
        )
        .unwrap();
        assert!(input[[0, 2, 0, 0]] < input[[0, 2, 47, 0]] - 0.2);
        assert!(input[[0, 2, 0, 80]] > input[[0, 2, 47, 80]] + 0.1);
    }

    #[test]
    fn tall_text_is_rotated_counterclockwise_before_recognition() {
        let mut rgba = solid_image(16, 64, [0, 0, 0, 255]);
        for y in 0..64 {
            for x in 0..16 {
                rgba[(y * 16 + x) * 4] = (y * 4) as u8;
            }
        }
        let input = crop_and_rec_input(
            &rgba,
            16,
            64,
            [[0.0, 0.0], [15.0, 0.0], [15.0, 63.0], [0.0, 63.0]],
        )
        .unwrap();
        assert!(input[[0, 2, 24, 0]] < -0.9);
        assert!(input[[0, 2, 24, 150]] > 0.3);
    }

    #[test]
    fn projective_transform_maps_all_four_correspondences() {
        let from = [[0.0, 0.0], [48.0, 0.0], [48.0, 24.0], [0.0, 24.0]];
        let to = [[8.0, 4.0], [56.0, 4.0], [44.0, 28.0], [20.0, 28.0]];
        let h = homography(from, to).unwrap();
        for (a, b) in from.into_iter().zip(to) {
            assert!(distance(project(h, a[0], a[1]), b) < 1e-4);
        }
    }

    #[test]
    fn ctc_removes_repeats_but_retains_repeats_separated_by_blank() {
        let chars = ["", "日", "本", " "].map(String::from);
        let ids = [1, 1, 0, 1, 2, 3, 3];
        let mut prob = vec![0.0; ids.len() * 4];
        for (t, id) in ids.into_iter().enumerate() {
            prob[t * 4 + id] = 0.9;
        }
        let (text, score) = ctc_decode(&prob, 7, 4, &chars).unwrap();
        assert_eq!(text, "日日本 ");
        assert!((score - 0.9).abs() < 1e-5);
        assert_eq!(
            ctc_decode(&[1.0, 0.0, 0.0, 0.0], 1, 4, &chars).unwrap(),
            (String::new(), 0.0)
        );
    }

    #[test]
    fn malformed_image_and_model_outputs_are_rejected() {
        assert!(det_input(&[0; 3], 1, 1).is_err());
        assert!(det_boxes(&[f32::NAN], 1, 1, 1, 1).is_err());
        assert!(ctc_decode(&[0.0], 1, 2, &[String::new(), "a".into()]).is_err());
        assert!(crop_and_rec_input(&[0; 4], 1, 1, [[0.0, 0.0]; 4]).is_err());
    }
}
