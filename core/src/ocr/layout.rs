use super::TextBlock;

#[derive(Clone, Copy)]
struct Bounds {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Bounds {
    fn of(block: &TextBlock) -> Self {
        Self {
            left: block
                .polygon
                .iter()
                .map(|p| p[0])
                .fold(f32::INFINITY, f32::min),
            top: block
                .polygon
                .iter()
                .map(|p| p[1])
                .fold(f32::INFINITY, f32::min),
            right: block
                .polygon
                .iter()
                .map(|p| p[0])
                .fold(f32::NEG_INFINITY, f32::max),
            bottom: block
                .polygon
                .iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max),
        }
    }

    fn height(self) -> f32 {
        self.bottom - self.top
    }
    fn width(self) -> f32 {
        self.right - self.left
    }
    fn similar_height(self, other: Self) -> bool {
        self.height() > 0.
            && other.height() > 0.
            && (0.75..=1.33).contains(&(self.height() / other.height()))
    }
}

// Desktop text has no overlay anchors. Keep VR blocks in their original geometry.
pub(super) fn merge_blocks(mut blocks: Vec<TextBlock>) -> Vec<TextBlock> {
    sort(&mut blocks);
    let mut lines: Vec<TextBlock> = Vec::new();
    for block in blocks {
        let bounds = Bounds::of(&block);
        let line = lines.iter_mut().rev().find(|line| {
            let previous = Bounds::of(line);
            let overlap = previous.bottom.min(bounds.bottom) - previous.top.max(bounds.top);
            let gap = (bounds.left - previous.right).max(previous.left - bounds.right);
            let (left, right) = if bounds.left < previous.left {
                (&block.text, &line.text)
            } else {
                (&line.text, &block.text)
            };
            previous.similar_height(bounds)
                && horizontal(line)
                && horizontal(&block)
                && overlap >= previous.height().min(bounds.height()) * 0.7
                && gap >= -bounds.height() * 0.1
                && gap <= bounds.height()
                && !left.trim_end().ends_with([':', '：'])
                && !separate_labels(left.trim(), right.trim())
        });
        if let Some(line) = line {
            let prepend = bounds.left < Bounds::of(line).left;
            join(line, block, prepend);
        } else {
            lines.push(block);
        }
    }
    sort(&mut lines);
    let mut paragraphs: Vec<(TextBlock, Bounds)> = Vec::new();
    for line in lines {
        let bounds = Bounds::of(&line);
        let paragraph = paragraphs.iter_mut().rev().find(|(block, last)| {
            let text = block
                .text
                .trim_end_matches(['\"', '\'', '”', '’', ')', '）']);
            let continuation = line
                .text
                .chars()
                .next()
                .is_some_and(|c| c.is_lowercase() || cjk(c));
            let minimum = if text.chars().any(cjk) { 4 } else { 12 };
            let gap = bounds.top - last.bottom;
            horizontal(&line)
                && last.similar_height(bounds)
                && last.width() >= last.height() * 6.
                && text.chars().count() >= minimum
                && continuation
                && !text.ends_with(['.', '!', '?', ';', ':', '。', '！', '？', '；', '：'])
                && gap >= 0.
                && gap <= last.height().min(bounds.height()) * 0.6
                && (bounds.left - last.left).abs() <= bounds.height() * 0.6
                && bounds.right <= last.right + bounds.height() * 0.6
        });
        if let Some((paragraph, last)) = paragraph {
            join(paragraph, line, false);
            *last = bounds;
        } else {
            paragraphs.push((line, bounds));
        }
    }
    paragraphs
        .into_iter()
        .enumerate()
        .map(|(id, (mut block, _))| {
            block.id = id;
            block
        })
        .collect()
}

fn separate_labels(left: &str, right: &str) -> bool {
    let (left_len, right_len) = (left.chars().count(), right.chars().count());
    !left.contains(char::is_whitespace)
        && !right.contains(char::is_whitespace)
        && left_len < 12
        && right_len < 12
        && right
            .chars()
            .next()
            .is_some_and(|c| c.is_uppercase() || (cjk(c) && left_len < 4 && right_len < 4))
}

fn horizontal(block: &TextBlock) -> bool {
    (block.polygon[1][1] - block.polygon[0][1]).abs() <= Bounds::of(block).height() * 0.25
}

fn sort(blocks: &mut [TextBlock]) {
    blocks.sort_by(|a, b| {
        let (a, b) = (Bounds::of(a), Bounds::of(b));
        a.top.total_cmp(&b.top).then(a.left.total_cmp(&b.left))
    });
}

fn join(block: &mut TextBlock, other: TextBlock, prepend: bool) {
    let (a, b) = (Bounds::of(block), Bounds::of(&other));
    let (left, right) = if prepend {
        (other.text.trim(), block.text.trim())
    } else {
        (block.text.trim(), other.text.trim())
    };
    let last = left.chars().last();
    let first = right.chars().next();
    let space = last.zip(first).is_some_and(|(last, first)| {
        !cjk(last)
            && !cjk(first)
            && last != '-'
            && !matches!(first, '.' | ',' | '!' | '?' | ';' | ':' | ')' | ']')
    });
    block.text = format!("{left}{}{right}", if space { " " } else { "" });
    block.confidence = block.confidence.min(other.confidence);
    block.polygon = [
        [a.left.min(b.left), a.top.min(b.top)],
        [a.right.max(b.right), a.top.min(b.top)],
        [a.right.max(b.right), a.bottom.max(b.bottom)],
        [a.left.min(b.left), a.bottom.max(b.bottom)],
    ];
}

fn cjk(c: char) -> bool {
    matches!(c as u32, 0x2e80..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x323af)
}
