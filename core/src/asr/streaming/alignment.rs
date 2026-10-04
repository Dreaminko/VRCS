//! Bounded, deterministic alignment of native text. No credentials or model calls.

pub(super) fn terminal(c: char) -> bool {
    matches!(
        c,
        '.' | '。'
            | '．'
            | '｡'
            | '!'
            | '！'
            | '?'
            | '？'
            | '؟'
            | '‼'
            | '⁇'
            | '⁈'
            | '⁉'
            | '\n'
            | '\r'
    )
}

fn closing(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '”' | '’' | '」' | '』' | ')' | '）' | ']' | '】' | '》' | '»'
    )
}

/// UTF-8 byte boundaries, including quotes and whitespace. Decimals and common
/// abbreviations are not sentence ends. Never split a code point.
pub(super) fn sentences(text: &str) -> Vec<usize> {
    let chars: Vec<_> = text.char_indices().collect();
    let mut ends = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (index, c) = chars[i];
        if !terminal(c) {
            i += 1;
            continue;
        }
        if c == '.' {
            let before = i.checked_sub(1).map(|j| chars[j].1);
            let after = chars.get(i + 1).map(|(_, c)| *c);
            if before.is_some_and(|c| c.is_ascii_digit())
                && after.is_some_and(|c| c.is_ascii_digit())
            {
                i += 1;
                continue;
            }
            let word = text[..index]
                .split_whitespace()
                .last()
                .unwrap_or("")
                .to_ascii_lowercase();
            if ["mr", "mrs", "ms", "dr", "prof", "e.g", "i.e", "vs"].contains(&word.as_str())
                || after.is_some_and(|c| c.is_ascii_alphabetic())
            {
                i += 1;
                continue;
            }
        }
        let mut end = index + c.len_utf8();
        let mut j = i + 1;
        for &(offset, next) in &chars[j..] {
            if closing(next) || next.is_whitespace() || terminal(next) {
                end = offset + next.len_utf8();
                j += 1;
            } else {
                break;
            }
        }
        if ends.last().is_none_or(|last| *last < end) {
            ends.push(end);
        }
        if ends.len() == 24 {
            break;
        }
        i = j;
    }
    ends
}

fn weight(text: &str) -> f64 {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| {
            if matches!(c as u32, 0x3040..=0x30ff | 0x3400..=0x9fff | 0xac00..=0xd7af) {
                1.0
            } else {
                0.35
            }
        })
        .sum::<f64>()
        .max(1.0)
}

fn numbers(text: &str) -> Vec<String> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            current.push(c);
        } else if !current.is_empty() {
            numbers.push(std::mem::take(&mut current));
        }
    }
    numbers.sort();
    numbers
}

fn cost(source: &str, target: &str, ratio: f64) -> f64 {
    let length = (weight(target) / weight(source) / ratio).ln().abs();
    // Numbers are anchors, never a reason to drop or rewrite either stream.
    length
        + if numbers(source) == numbers(target) {
            0.0
        } else {
            2.0
        }
}

pub(super) type Cut = (usize, usize);

/// Monotonic dynamic programming allowing up to three sentences on either side.
/// Unmatched suffixes stay pending instead of being shifted into the next pair.
pub(super) fn align(
    source: &str,
    target: &str,
    context: &[(String, String)],
    include_tail: bool,
) -> Vec<Cut> {
    let mut a = sentences(source);
    let mut b = sentences(target);
    if include_tail {
        if !source.trim().is_empty() && a.last().copied() != Some(source.len()) {
            a.push(source.len());
        }
        if !target.trim().is_empty() && b.last().copied() != Some(target.len()) {
            b.push(target.len());
        }
    }
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    a.insert(0, 0);
    b.insert(0, 0);
    let (n, m) = (a.len() - 1, b.len() - 1);
    let ratio = context
        .last()
        .filter(|(s, t)| !s.trim().is_empty() && !t.trim().is_empty())
        .map_or(1.0, |(s, t)| weight(t) / weight(s))
        .clamp(0.4, 2.5);
    let mut scores = vec![vec![f64::INFINITY; m + 1]; n + 1];
    let mut previous = vec![vec![None; m + 1]; n + 1];
    scores[0][0] = 0.0;
    for i in 0..=n {
        for j in 0..=m {
            if !scores[i][j].is_finite() {
                continue;
            }
            for p in 1..=3.min(n - i) {
                for q in 1..=3.min(m - j) {
                    let pair = cost(&source[a[i]..a[i + p]], &target[b[j]..b[j + q]], ratio);
                    if pair > 1.4 {
                        continue;
                    }
                    let score = scores[i][j] + pair + 0.3 * (p + q - 2) as f64;
                    if score < scores[i + p][j + q] {
                        scores[i + p][j + q] = score;
                        previous[i + p][j + q] = Some((i, j));
                    }
                }
            }
        }
    }
    let mut best = None;
    let mut best_score = f64::INFINITY;
    for i in 1..=n {
        for j in 1..=m {
            if i != n && j != m {
                continue;
            }
            // Source lookahead may be untranslated. Leaving it pending must
            // cost less than merging it solely to improve coverage. Preserve
            // the stronger target coverage cost for one-to-many translations.
            let score = scores[i][j] + 0.1 * (n - i) as f64 + 0.75 * (m - j) as f64;
            if score < best_score {
                best_score = score;
                best = Some((i, j));
            }
        }
    }
    let Some((mut i, mut j)) = best else {
        return Vec::new();
    };
    let mut cuts = Vec::new();
    while let Some((p, q)) = previous[i][j] {
        cuts.push((a[i], b[j]));
        i = p;
        j = q;
    }
    cuts.reverse();
    cuts
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_decimals_quotes_and_abbreviations() {
        let text = "Dr. Lee paid 3.14. Next🙂。”";
        let ends = sentences(text);
        assert_eq!(&text[..ends[0]], "Dr. Lee paid 3.14. ");
        assert_eq!(&text[ends[0]..ends[1]], "Next🙂。”");
    }
    #[test]
    fn numbers_keep_one_source_with_three_translated_sentences() {
        let source = "Platform 2.0 started in 2020, with 288 communities and 3487 tasks.";
        let target = "平台2.0于2020年启动。共有288家社区。发布3487项任务。";
        assert!(align(source, "平台2.0于2020年启动。", &[], false).is_empty());
        assert_eq!(
            align(source, target, &[], false),
            [(source.len(), target.len())]
        );
    }
    #[test]
    fn repeated_sentences_have_unambiguous_byte_offsets() {
        let source = "Thank you. Thank you.";
        let target = "谢谢。谢谢。";
        assert_eq!(
            align(source, target, &[], false),
            [(11, 9), (source.len(), target.len())]
        );
    }
    #[test]
    fn delayed_suffix_is_not_consumed() {
        assert_eq!(
            align("Hello. The train leaves at 7.", "你好。", &[], false),
            [(7, 9)]
        );
    }
    #[test]
    fn untranslated_source_lookahead_is_not_swallowed_by_a_long_pair() {
        let prefix =
            "I bought a red umbrella and a blue notebook yesterday but forgot to bring them home. ";
        let source = format!("{prefix}Thank you.");
        let target = "我昨天买了一把红伞和一本蓝色笔记本,但忘了带回家。";
        assert_eq!(
            align(&source, target, &[], false),
            [(prefix.len(), target.len())]
        );
    }
}
