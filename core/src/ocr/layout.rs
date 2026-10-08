use super::TextBlock;

pub(super) const SOURCE_CHAR_BUDGET: usize = 2000;

pub(super) struct TextGroup {
    pub source: TextBlock,
    pub fragments: Vec<TextBlock>,
    pub region: usize,
}

#[derive(Clone, Copy)]
struct Bounds {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Bounds {
    fn of(block: &TextBlock, axis: [f32; 2]) -> Self {
        let mut bounds = Self {
            left: f32::INFINITY,
            top: f32::INFINITY,
            right: f32::NEG_INFINITY,
            bottom: f32::NEG_INFINITY,
        };
        for point in block.polygon {
            let x = point[0] * axis[0] + point[1] * axis[1];
            let y = -point[0] * axis[1] + point[1] * axis[0];
            bounds.left = bounds.left.min(x);
            bounds.right = bounds.right.max(x);
            bounds.top = bounds.top.min(y);
            bounds.bottom = bounds.bottom.max(y);
        }
        bounds
    }

    fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }
    fn height(self) -> f32 {
        self.bottom - self.top
    }
    fn width(self) -> f32 {
        self.right - self.left
    }
    fn overlap(self, other: Self) -> f32 {
        self.right.min(other.right) - self.left.max(other.left)
    }
    fn similar_height(self, other: Self) -> bool {
        self.height() > 0.
            && other.height() > 0.
            && (0.75..=1.33).contains(&(self.height() / other.height()))
    }
    fn polygon(self, axis: [f32; 2]) -> [[f32; 2]; 4] {
        [
            [self.left, self.top],
            [self.right, self.top],
            [self.right, self.bottom],
            [self.left, self.bottom],
        ]
        .map(|[x, y]| [x * axis[0] - y * axis[1], x * axis[1] + y * axis[0]])
    }
}

struct Line {
    fragments: Vec<TextBlock>,
    text: String,
    axis: [f32; 2],
    bounds: Bounds,
}

fn direction(block: &TextBlock) -> [f32; 2] {
    let [x, y] = [
        block.polygon[1][0] - block.polygon[0][0],
        block.polygon[1][1] - block.polygon[0][1],
    ];
    let length = x.hypot(y);
    let height = (block.polygon[3][0] - block.polygon[0][0])
        .hypot(block.polygon[3][1] - block.polygon[0][1]);
    if height > length * 1.5 {
        return [0., 1.];
    }
    if length > 0. {
        [x / length, y / length]
    } else {
        [0., 1.]
    }
}

fn aligned(a: [f32; 2], b: [f32; 2]) -> bool {
    a[0] >= 0.5 && b[0] >= 0.5 && a[0] * b[0] + a[1] * b[1] >= 0.985
}

fn barrier_between(a: Bounds, b: Bounds, axis: [f32; 2], barriers: &[TextBlock]) -> bool {
    let vertical = b.top >= a.bottom - 0.01;
    barriers.iter().any(|block| {
        let bounds = Bounds::of(block, axis);
        if vertical {
            bounds.overlap(a) > 0.
                && bounds.overlap(b) > 0.
                && bounds.bottom > a.bottom
                && bounds.top < b.top
        } else {
            bounds.left < a.left.max(b.left)
                && bounds.right > a.right.min(b.right)
                && bounds.bottom > a.top.max(b.top)
                && bounds.top < a.bottom.min(b.bottom)
        }
    })
}

fn label(text: &str) -> bool {
    let text = text.trim();
    if text.ends_with([':', '：'])
        || text.starts_with(['•', '●', '-', '*'])
        || text.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return true;
    }
    let words: Vec<_> = text.split_whitespace().collect();
    !words.is_empty()
        && words.len() <= 3
        && words.iter().all(|word| {
            word.chars().next().is_some_and(char::is_uppercase)
                && !word.ends_with(['.', '!', '?', '。', '！', '？'])
        })
}

fn separate_labels(left: &str, right: &str) -> bool {
    let (a, b) = (left.chars().count(), right.chars().count());
    label(left) && label(right)
        || (!left.contains(char::is_whitespace)
            && !right.contains(char::is_whitespace)
            && a < 12
            && b < 12
            && right
                .chars()
                .next()
                .is_some_and(|c| c.is_uppercase() || cjk(c) && a < 4 && b < 4))
        || left.trim_end().ends_with([':', '：'])
}

fn continuation(first: &str, next: &str, a: Bounds, b: Bounds) -> bool {
    if label(first) || label(next) {
        return false;
    }
    let first = first.trim_end_matches(['"', '\'', '”', '’', ')', '）']);
    if first.ends_with([';', ':', '；', '：']) {
        return false;
    }
    let (first_cjk, next_cjk) = (
        first.chars().filter(|c| cjk(*c)).count(),
        next.chars().filter(|c| cjk(*c)).count(),
    );
    if first.ends_with(['.', '!', '?', '。', '！', '？']) {
        return (first.split_whitespace().count() >= 4 && next.split_whitespace().count() >= 4
            || first_cjk >= 8 && next_cjk >= 8)
            && a.overlap(b) >= a.width().max(b.width()) * 0.85;
    }
    first.split_whitespace().count() >= 2
        || first_cjk >= 2 && (first_cjk + next_cjk >= 10 || next.ends_with(['。', '！', '？']))
}

fn join_text(left: &str, right: &str) -> String {
    let (left, right) = (left.trim(), right.trim());
    let space = left
        .chars()
        .last()
        .zip(right.chars().next())
        .is_some_and(|(a, b)| {
            !cjk(a)
                && !cjk(b)
                && a != '-'
                && !matches!(b, '.' | ',' | '!' | '?' | ';' | ':' | ')' | ']')
        });
    format!("{left}{}{right}", if space { " " } else { "" })
}

fn cjk(c: char) -> bool {
    matches!(c as u32, 0x2e80..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x323af)
}

pub(super) fn group_blocks(mut blocks: Vec<TextBlock>, minimum: f32) -> Vec<TextGroup> {
    let started = std::time::Instant::now();
    blocks.sort_by(|a, b| {
        let (a_bounds, b_bounds) = (Bounds::of(a, [1., 0.]), Bounds::of(b, [1., 0.]));
        a_bounds
            .top
            .total_cmp(&b_bounds.top)
            .then(a_bounds.left.total_cmp(&b_bounds.left))
            .then(a.id.cmp(&b.id))
    });
    let (accepted, barriers): (Vec<_>, Vec<_>) = blocks
        .into_iter()
        .partition(|block| block.confidence >= minimum && !block.text.trim().is_empty());
    let mut lines: Vec<Line> = Vec::new();
    for block in accepted {
        let axis = direction(&block);
        let candidate = lines.iter_mut().rev().find(|line| {
            let b = Bounds::of(&block, line.axis);
            let a = line.bounds;
            let overlap = a.bottom.min(b.bottom) - a.top.max(b.top);
            let gap = (b.left - a.right).max(a.left - b.right);
            let (left, right) = if b.left < a.left {
                (&block.text, &line.text)
            } else {
                (&line.text, &block.text)
            };
            aligned(axis, line.axis)
                && a.similar_height(b)
                && overlap >= a.height().min(b.height()) * 0.7
                && gap >= -b.height() * 0.1
                && gap <= b.height()
                && !separate_labels(left, right)
                && !barrier_between(a, b, line.axis, &barriers)
        });
        if let Some(line) = candidate {
            line.bounds = line.bounds.union(Bounds::of(&block, line.axis));
            line.fragments.push(block);
            line.fragments.sort_by(|a, b| {
                Bounds::of(a, line.axis)
                    .left
                    .total_cmp(&Bounds::of(b, line.axis).left)
                    .then(a.id.cmp(&b.id))
            });
            line.text = line.fragments.iter().fold(String::new(), |text, block| {
                if text.is_empty() {
                    block.text.trim().to_owned()
                } else {
                    join_text(&text, &block.text)
                }
            });
        } else {
            lines.push(Line {
                text: block.text.trim().to_owned(),
                bounds: Bounds::of(&block, axis),
                axis,
                fragments: vec![block],
            });
        }
    }
    lines.sort_by(|a, b| {
        a.bounds
            .top
            .total_cmp(&b.bounds.top)
            .then(a.bounds.left.total_cmp(&b.bounds.left))
    });
    let mut regions: Vec<Vec<Line>> = Vec::new();
    for line in lines {
        if label(&line.text) {
            regions.push(vec![line]);
            continue;
        }
        let candidate = regions.iter_mut().rev().find(|region| {
            let first = &region[0];
            let last = region.last().unwrap();
            let a = last
                .fragments
                .iter()
                .map(|block| Bounds::of(block, first.axis))
                .reduce(Bounds::union)
                .unwrap();
            let b = line
                .fragments
                .iter()
                .map(|block| Bounds::of(block, first.axis))
                .reduce(Bounds::union)
                .unwrap();
            let gap = b.top - a.bottom;
            aligned(first.axis, line.axis)
                && a.similar_height(b)
                && first.bounds.similar_height(b)
                && gap >= -0.01
                && gap <= a.height().min(b.height()) * 0.6
                && (b.left - first.bounds.left).abs() <= b.height() * 0.6
                && b.right
                    <= first.bounds.right + (b.height() * 0.6).max(first.bounds.width() * 0.35)
                && continuation(&last.text, &line.text, a, b)
                && !barrier_between(a, b, first.axis, &barriers)
        });
        if let Some(region) = candidate {
            region.push(line);
        } else {
            regions.push(vec![line]);
        }
    }
    // Order complete regions by column before reading each column top to bottom.
    type Region = (Bounds, Vec<Line>);
    let mut columns: Vec<(Bounds, Vec<Region>)> = Vec::new();
    for region in regions {
        let bounds = region
            .iter()
            .flat_map(|line| &line.fragments)
            .map(|block| Bounds::of(block, [1., 0.]))
            .reduce(Bounds::union)
            .unwrap();
        if let Some((column, items)) = columns.iter_mut().find(|(_, items)| {
            items.iter().any(|(existing, _)| {
                let gap = (bounds.left - existing.right).max(existing.left - bounds.right);
                existing.overlap(bounds) >= existing.width().max(bounds.width()) * 0.5
                    || gap >= 0. && gap <= existing.height().min(bounds.height()) * 0.6
            })
        }) {
            *column = column.union(bounds);
            items.push((bounds, region));
        } else {
            columns.push((bounds, vec![(bounds, region)]));
        }
    }
    columns.sort_by(|a, b| {
        a.0.left
            .total_cmp(&b.0.left)
            .then(a.0.top.total_cmp(&b.0.top))
    });
    let mut groups = Vec::new();
    let mut region_id = 0;
    for (_, mut regions) in columns {
        regions.sort_by(|a, b| {
            a.0.top
                .total_cmp(&b.0.top)
                .then(a.0.left.total_cmp(&b.0.left))
        });
        for (_, region) in regions {
            let axis = region[0].axis;
            let mut text = String::new();
            let mut fragments = Vec::new();
            for line in region {
                if !text.is_empty()
                    && text.chars().count() + line.text.chars().count() + 1 > SOURCE_CHAR_BUDGET
                {
                    groups.push(make_group(
                        groups.len(),
                        region_id,
                        axis,
                        std::mem::take(&mut text),
                        std::mem::take(&mut fragments),
                    ));
                }
                text = if text.is_empty() {
                    line.text
                } else {
                    join_text(&text, &line.text)
                };
                fragments.extend(line.fragments);
            }
            groups.push(make_group(groups.len(), region_id, axis, text, fragments));
            region_id += 1;
        }
    }
    tracing::debug!(
        elapsed_us = started.elapsed().as_micros() as u64,
        groups = groups.len(),
        "OCR text grouped"
    );
    groups
}

fn make_group(
    id: usize,
    region: usize,
    axis: [f32; 2],
    text: String,
    fragments: Vec<TextBlock>,
) -> TextGroup {
    let polygon = if fragments.len() == 1 {
        fragments[0].polygon
    } else {
        fragments
            .iter()
            .map(|block| Bounds::of(block, axis))
            .reduce(Bounds::union)
            .unwrap()
            .polygon(axis)
    };
    let confidence = fragments
        .iter()
        .map(|block| block.confidence)
        .fold(1., f32::min);
    TextGroup {
        source: TextBlock {
            id,
            text,
            confidence,
            polygon,
        },
        fragments,
        region,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Sample {
        name: String,
        blocks: Vec<(String, f32, f32, f32)>,
        groups: Vec<Vec<usize>>,
        texts: Vec<String>,
    }

    #[test]
    fn annotated_layouts_preserve_members_text_and_geometry_at_different_scales() {
        let samples: Vec<Sample> =
            serde_json::from_str(include_str!("../../tests/fixtures/ocr-layout.json")).unwrap();
        for sample in samples {
            for scale in [0.5, 1., 2.] {
                let originals: Vec<_> = sample
                    .blocks
                    .iter()
                    .enumerate()
                    .map(|(id, (text, x, y, w))| TextBlock {
                        id,
                        text: text.clone(),
                        confidence: 0.95,
                        polygon: [[*x, *y], [x + w, *y], [x + w, y + 20.], [*x, y + 20.]]
                            .map(|[x, y]| [x * scale, y * scale]),
                    })
                    .collect();
                let mut input = originals.clone();
                input.reverse();
                let groups = group_blocks(input, 0.5);
                assert_eq!(
                    groups
                        .iter()
                        .map(|group| group
                            .fragments
                            .iter()
                            .map(|block| block.id)
                            .collect::<Vec<_>>())
                        .collect::<Vec<_>>(),
                    sample.groups,
                    "{} scale={scale}",
                    sample.name
                );
                assert_eq!(
                    groups
                        .iter()
                        .map(|group| group.source.text.clone())
                        .collect::<Vec<_>>(),
                    sample.texts,
                    "{} scale={scale}",
                    sample.name
                );
                for fragment in groups.iter().flat_map(|group| &group.fragments) {
                    assert_eq!(fragment, &originals[fragment.id]);
                }
            }
        }
    }

    #[test]
    fn rotated_lines_join_without_changing_original_quadrilaterals() {
        let angle = 30_f32.to_radians();
        let original: Vec<_> = [("Please tell me", 10.), ("where this is?", 34.)]
            .into_iter()
            .enumerate()
            .map(|(id, (text, y))| TextBlock {
                id,
                text: text.into(),
                confidence: 0.95,
                polygon: [[10., y], [190., y], [190., y + 20.], [10., y + 20.]].map(|[x, y]| {
                    [
                        200. + x * angle.cos() - y * angle.sin(),
                        100. + x * angle.sin() + y * angle.cos(),
                    ]
                }),
            })
            .collect();
        let groups = group_blocks(original.clone(), 0.5);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].source.text, "Please tell me where this is?");
        assert_eq!(groups[0].fragments, original);
        assert!(groups[0].source.polygon[1][1] > groups[0].source.polygon[0][1] + 50.);
    }

    #[test]
    fn vertical_text_columns_are_kept_as_independent_units() {
        let blocks = [10., 40.]
            .into_iter()
            .enumerate()
            .map(|(id, x)| TextBlock {
                id,
                text: "日本語の縦書き".into(),
                confidence: 0.9,
                polygon: [[x, 10.], [x + 20., 10.], [x + 20., 190.], [x, 190.]],
            })
            .collect();
        assert_eq!(group_blocks(blocks, 0.5).len(), 2);
    }

    #[test]
    fn maximum_fragment_scan_preserves_members_and_reports_grouping_time() {
        let blocks: Vec<_> = (0..256)
            .map(|id| {
                let (x, y) = ((id / 32) as f32 * 250., (id % 32) as f32 * 24.);
                TextBlock {
                    id,
                    text: "Menu Item".into(),
                    confidence: 0.9,
                    polygon: [[x, y], [x + 150., y], [x + 150., y + 20.], [x, y + 20.]],
                }
            })
            .collect();
        let mut elapsed = Vec::new();
        for _ in 0..50 {
            let started = std::time::Instant::now();
            let groups = group_blocks(blocks.clone(), 0.5);
            elapsed.push(started.elapsed());
            assert_eq!(groups.len(), 256);
            assert_eq!(
                groups
                    .iter()
                    .map(|group| group.fragments.len())
                    .sum::<usize>(),
                256
            );
        }
        elapsed.sort();
        eprintln!("256 fragments, 50 runs, grouping p95={:?}", elapsed[47]);
    }
}
