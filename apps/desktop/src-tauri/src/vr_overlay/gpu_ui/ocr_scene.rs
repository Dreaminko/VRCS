//! Rust port of `apps/desktop/src/vr-surface/ocr-scene.ts`.

use std::collections::HashSet;

use crate::vr_overlay::ocr_plane::{OcrScene, Quad};

const WIDTH: f32 = 2048.0;
const PADDING: f32 = 12.0;
const LINE_HEIGHT: f32 = 1.4;

type Bounds = [f32; 4];

#[derive(Debug, Clone)]
pub struct OcrCard {
    pub id: usize,
    pub quad: Quad,
    pub source: Quad,
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub lines: Vec<String>,
    pub connected: bool,
}

#[derive(Debug, Clone)]
pub struct OcrSceneLayout {
    pub cards: Vec<OcrCard>,
    pub unplaced: Vec<usize>,
    pub height: f32,
    pub capture_bounds: [u32; 4],
}

fn bounds(quad: &Quad) -> Bounds {
    [
        quad.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
        quad.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
        quad.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max),
        quad.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max),
    ]
}

fn bounds_overlap(a: Bounds, b: Bounds) -> bool {
    a[2] > b[0] && b[2] > a[0] && a[3] > b[1] && b[3] > a[1]
}

fn rectangle([l, t, r, b]: Bounds) -> Quad {
    [[l, t], [r, t], [r, b], [l, b]]
}

fn edge(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn separated_on_axes(source: &Quad, a: &Quad, b: &Quad) -> bool {
    for index in 0..4 {
        let from = source[index];
        let to = source[(index + 1) % 4];
        let axis_x = to[1] - from[1];
        let axis_y = from[0] - to[0];
        let (mut a_min, mut a_max) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut b_min, mut b_max) = (f32::INFINITY, f32::NEG_INFINITY);
        for i in 0..4 {
            let ap = a[i][0] * axis_x + a[i][1] * axis_y;
            a_min = a_min.min(ap);
            a_max = a_max.max(ap);
            let bp = b[i][0] * axis_x + b[i][1] * axis_y;
            b_min = b_min.min(bp);
            b_max = b_max.max(bp);
        }
        if a_max <= b_min + 0.01 || b_max <= a_min + 0.01 {
            return true;
        }
    }
    false
}

fn overlaps(a: &Quad, b: &Quad) -> bool {
    !separated_on_axes(a, a, b) && !separated_on_axes(b, a, b)
}

fn tokenize(paragraph: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0usize;
    let mut in_ws = None;
    for (idx, ch) in paragraph.char_indices() {
        let ws = ch.is_whitespace();
        match in_ws {
            None => in_ws = Some(ws),
            Some(prev) if prev != ws => {
                tokens.push(&paragraph[start..idx]);
                start = idx;
                in_ws = Some(ws);
            }
            Some(_) => {}
        }
    }
    if start < paragraph.len() {
        tokens.push(&paragraph[start..]);
    }
    tokens
}

fn wrap_text(
    text: &str,
    width: f32,
    font_size: f32,
    max_lines: usize,
    measure: &mut impl FnMut(&str, f32) -> f32,
) -> Option<Vec<String>> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for token in tokenize(paragraph) {
            let candidate = format!("{line}{token}");
            if measure(&candidate, font_size) <= width {
                line = candidate;
                continue;
            }
            if !line.trim().is_empty() {
                lines.push(line.trim_end().to_owned());
                line.clear();
            }
            if lines.len() >= max_lines {
                return None;
            }
            if token.trim().is_empty() {
                continue;
            }
            for ch in token.chars() {
                let mut cand = line.clone();
                cand.push(ch);
                if measure(&cand, font_size) > width {
                    if line.is_empty() || lines.len() + 1 >= max_lines {
                        return None;
                    }
                    lines.push(std::mem::take(&mut line));
                }
                line.push(ch);
            }
        }
        lines.push(line.trim_end().to_owned());
        if lines.len() > max_lines {
            return None;
        }
    }
    Some(lines)
}

struct FittedText {
    font_size: f32,
    lines: Vec<String>,
    height: f32,
}

fn fit_text(
    text: &str,
    width: f32,
    height: f32,
    measure: &mut impl FnMut(&str, f32) -> f32,
) -> Option<FittedText> {
    if width <= PADDING * 2.0 || height <= PADDING * 2.0 {
        return None;
    }
    for size in (24..=48).rev().step_by(4) {
        let font_size = size as f32;
        let max_lines = ((height - PADDING * 2.0) / (font_size * LINE_HEIGHT)).floor() as usize;
        if max_lines < 1 {
            continue;
        }
        if let Some(lines) = wrap_text(text, width - PADDING * 2.0, font_size, max_lines, measure) {
            return Some(FittedText {
                font_size,
                height: lines.len() as f32 * font_size * LINE_HEIGHT + PADDING * 2.0,
                lines,
            });
        }
    }
    None
}

/// Validates and computes a 3x3 projective homography mapping `[0..width, 0..height]` -> `quad`.
/// Returns `Some([h00, h01, h02, h10, h11, h12, h20, h21, 1.0])` (row-major).
pub fn quad_homography(quad: &Quad, width: f32, height: f32) -> Option<[f32; 9]> {
    if !quad.iter().flatten().all(|v| v.is_finite()) || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let [a, b, c, d] = *quad;
    let dx1 = b[0] - c[0];
    let dx2 = d[0] - c[0];
    let dx3 = a[0] - b[0] + c[0] - d[0];
    let dy1 = b[1] - c[1];
    let dy2 = d[1] - c[1];
    let dy3 = a[1] - b[1] + c[1] - d[1];
    let denom = dx1 * dy2 - dx2 * dy1;
    if denom.abs() < 1e-5 {
        return None;
    }
    let g = (dx3 * dy2 - dx2 * dy3) / denom;
    let h = (dx1 * dy3 - dx3 * dy1) / denom;
    if [1.0, 1.0 + g, 1.0 + h, 1.0 + g + h]
        .iter()
        .any(|v| *v <= 1e-5)
    {
        return None;
    }
    Some([
        (b[0] - a[0] + g * b[0]) / width,
        (d[0] - a[0] + h * d[0]) / height,
        a[0],
        (b[1] - a[1] + g * b[1]) / width,
        (d[1] - a[1] + h * d[1]) / height,
        a[1],
        g / width,
        h / height,
        1.0,
    ])
}

pub fn layout_ocr_scene(
    scene: &OcrScene,
    measure: &mut impl FnMut(&str, f32) -> f32,
) -> OcrSceneLayout {
    let height = WIDTH / scene.aspect;
    if !height.is_finite() || height <= 0.0 || scene.blocks.len() > 256 {
        let mut seen = HashSet::new();
        let unplaced = scene
            .blocks
            .iter()
            .filter_map(|b| seen.insert(b.id).then_some(b.id))
            .collect();
        return OcrSceneLayout {
            cards: Vec::new(),
            unplaced,
            height: WIDTH,
            capture_bounds: [0, 0, WIDTH as u32, WIDTH as u32],
        };
    }

    let scale_quad = |quad: &Quad| -> Quad { quad.map(|[x, y]| [x * WIDTH, y * height]) };

    struct ScaledBlock {
        id: usize,
        text: String,
        quad: Quad,
        masks: Vec<Quad>,
    }
    let blocks: Vec<ScaledBlock> = scene
        .blocks
        .iter()
        .map(|b| ScaledBlock {
            id: b.id,
            text: b.text.clone(),
            quad: scale_quad(&b.quad),
            masks: b.masks.iter().map(scale_quad).collect(),
        })
        .collect();

    struct Occupied {
        id: Option<usize>,
        quad: Quad,
        box_: Bounds,
    }
    let mut occupied: Vec<Occupied> = scene
        .excluded
        .iter()
        .map(|q| {
            let scaled = scale_quad(q);
            Occupied {
                id: None,
                quad: scaled,
                box_: bounds(&scaled),
            }
        })
        .collect();
    for b in &blocks {
        for mask in &b.masks {
            occupied.push(Occupied {
                id: Some(b.id),
                quad: *mask,
                box_: bounds(mask),
            });
        }
    }

    let mut cards = Vec::new();
    let mut unplaced = Vec::new();
    let mut pending = Vec::new();

    let is_free = |occupied: &[Occupied], quad: &Quad, id: usize| -> bool {
        let box_ = bounds(quad);
        let [l, t, r, b] = box_;
        l >= 4.0
            && t >= 4.0
            && r <= WIDTH - 4.0
            && b <= height - 4.0
            && !occupied.iter().any(|other| {
                other.id != Some(id)
                    && bounds_overlap(box_, other.box_)
                    && overlaps(quad, &other.quad)
            })
    };

    for b in &blocks {
        let width = (edge(b.quad[0], b.quad[1]) + edge(b.quad[3], b.quad[2])) / 2.0;
        let card_height = (edge(b.quad[0], b.quad[3]) + edge(b.quad[1], b.quad[2])) / 2.0;
        let fitted = quad_homography(&b.quad, width, card_height)
            .and_then(|_| fit_text(&b.text, width, card_height, measure));
        if let Some(text) = fitted
            && is_free(&occupied, &b.quad, b.id)
        {
            cards.push(OcrCard {
                id: b.id,
                quad: b.quad,
                source: b.quad,
                width,
                height: card_height,
                font_size: text.font_size,
                lines: text.lines,
                connected: false,
            });
            occupied.push(Occupied {
                id: Some(b.id),
                quad: b.quad,
                box_: bounds(&b.quad),
            });
        } else {
            pending.push(b);
        }
    }

    let mut connected_count = 0usize;
    for b in pending {
        let [l, t, r, bt] = bounds(&b.quad);
        let mut placed = false;
        for width in [(r - l).max(192.0), 320.0, 480.0, 720.0] {
            if width > WIDTH - 8.0 {
                continue;
            }
            let Some(text) = fit_text(&b.text, width, (height - 8.0).min(600.0), measure) else {
                continue;
            };
            let x = (l + r - width) / 2.0;
            let y = (t + bt - text.height) / 2.0;
            let quad = rectangle([x, y, x + width, y + text.height]);
            if is_free(&occupied, &quad, b.id) {
                cards.push(OcrCard {
                    id: b.id,
                    quad,
                    source: b.quad,
                    width,
                    height: text.height,
                    font_size: text.font_size,
                    lines: text.lines,
                    connected: false,
                });
                occupied.push(Occupied {
                    id: Some(b.id),
                    quad,
                    box_: bounds(&quad),
                });
                placed = true;
                break;
            }
            if connected_count >= 8 {
                continue;
            }
            for [cx, cy] in [
                [r + 20.0, (t + bt - text.height) / 2.0],
                [l - width - 20.0, (t + bt - text.height) / 2.0],
                [(l + r - width) / 2.0, bt + 20.0],
                [(l + r - width) / 2.0, t - text.height - 20.0],
            ] {
                let card = rectangle([cx, cy, cx + width, cy + text.height]);
                if !is_free(&occupied, &card, b.id) {
                    continue;
                }
                cards.push(OcrCard {
                    id: b.id,
                    quad: card,
                    source: b.quad,
                    width,
                    height: text.height,
                    font_size: text.font_size,
                    lines: text.lines,
                    connected: true,
                });
                occupied.push(Occupied {
                    id: Some(b.id),
                    quad: card,
                    box_: bounds(&card),
                });
                connected_count += 1;
                placed = true;
                break;
            }
            if placed {
                break;
            }
        }
        if !placed {
            unplaced.push(b.id);
        }
    }

    let placed_ids: HashSet<usize> = cards.iter().map(|c| c.id).collect();
    let mut points: Vec<[f32; 2]> = Vec::new();
    for card in &cards {
        points.extend_from_slice(&card.quad);
        if card.connected {
            points.extend_from_slice(&card.source);
            for quad in [card.source, card.quad] {
                let x = quad.iter().map(|p| p[0]).sum::<f32>() / 4.0;
                let y = quad.iter().map(|p| p[1]).sum::<f32>() / 4.0;
                points.push([x - 1.5, y - 1.5]);
                points.push([x + 1.5, y + 1.5]);
            }
        }
    }
    for b in &blocks {
        if placed_ids.contains(&b.id) {
            for mask in &b.masks {
                points.extend_from_slice(mask);
            }
        }
    }

    let capture_bounds = if points.is_empty() {
        [0, 0, 1, 1]
    } else {
        let mut cb = [WIDTH as u32, WIDTH as u32, 0u32, 0u32];
        for [x, y] in points {
            cb[0] = cb[0].min((x.floor() as i32 - 4).max(0) as u32);
            cb[1] = cb[1].min(((y * scene.aspect).floor() as i32 - 4).max(0) as u32);
            cb[2] = cb[2].max((x.ceil() as i32 + 4).min(WIDTH as i32) as u32);
            cb[3] = cb[3].max(((y * scene.aspect).ceil() as i32 + 4).min(WIDTH as i32) as u32);
        }
        cb
    };

    let mut seen = HashSet::new();
    unplaced.retain(|id| seen.insert(*id));

    OcrSceneLayout {
        cards,
        unplaced,
        height,
        capture_bounds,
    }
}
