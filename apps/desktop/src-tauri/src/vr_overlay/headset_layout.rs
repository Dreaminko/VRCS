use unicode_segmentation::UnicodeSegmentation;

// Reserve two readable lines even when the current caption is short. Text length
// never participates in font selection, so later deltas cannot shrink the font.
pub(super) fn font_size(
    maximum: u32,
    available_line_height: i32,
    mut measure_height: impl FnMut(u32) -> Option<i32>,
) -> u32 {
    let mut low = 16;
    let mut high = maximum.max(low);
    let mut best = low;
    while low <= high {
        let size = (low + high) / 2;
        if measure_height(size).is_some_and(|height| height <= available_line_height) {
            best = size;
            low = size + 1;
        } else {
            high = size - 1;
        }
    }
    best
}

// Keep the newest complete graphemes instead of squeezing the whole paragraph.
pub(super) fn visible_tail(text: &str, mut fits: impl FnMut(&str) -> bool) -> String {
    if text.is_empty() || fits(text) {
        return text.to_owned();
    }
    if !fits("…") {
        return String::new();
    }
    let starts = text
        .grapheme_indices(true)
        .map(|(start, _)| start)
        .chain(std::iter::once(text.len()))
        .collect::<Vec<_>>();
    let mut low = 0;
    let mut high = starts.len() - 1;
    while low < high {
        let middle = (low + high) / 2;
        if fits(&format!("…{}", &text[starts[middle]..])) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    let mut start = starts[low];
    let fitted = format!("…{}", &text[start..]);
    // Prefer a complete English word when the cut falls inside one. A single
    // oversized token (for example a URL) still keeps its visible ending.
    if start > 0
        && text.as_bytes()[start - 1].is_ascii_alphanumeric()
        && text
            .as_bytes()
            .get(start)
            .is_some_and(u8::is_ascii_alphanumeric)
    {
        if let Some((offset, ch)) = text[start..]
            .char_indices()
            .find(|(_, ch)| ch.is_whitespace())
        {
            let word_start = start + offset + ch.len_utf8();
            if word_start < text.len() {
                start = word_start;
            }
        }
    }
    let readable = format!("…{}", text[start..].trim_start());
    // Joining the marker to a nearly full-width word can change wrapping.
    // Retain the measured suffix if word-boundary cleanup would overflow.
    if fits(&readable) {
        readable
    } else {
        fitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_font_size_is_preserved_and_large_fonts_only_fit_by_height() {
        let measure = |size| Some((size as i32 * 6 + 4) / 5);
        assert_eq!(font_size(54, 82, measure), 54);
        assert_eq!(font_size(96, 82, measure), 68);
        assert_eq!(font_size(24, 82, measure), 24);
    }

    #[test]
    fn growing_paragraphs_keep_the_same_latest_text_with_an_overflow_marker() {
        let suffix = "最新字幕仍然清晰可读";
        let fits = |text: &str| text.graphemes(true).count() <= 10;
        let first = visible_tail(&format!("{}{suffix}", "旧内容".repeat(10)), fits);
        let longer = visible_tail(&format!("{}{suffix}", "更长的旧内容".repeat(100)), fits);
        assert_eq!(first, longer);
        assert!(first.starts_with('…'));
        assert!(first.ends_with("清晰可读"));
        assert!(fits(&first));
    }

    #[test]
    fn short_captions_are_not_truncated() {
        assert_eq!(visible_tail("短句", |_| true), "短句");
        assert_eq!(visible_tail("", |_| false), "");
    }

    #[test]
    fn emoji_and_combining_marks_are_not_split() {
        let text = "之前的内容 👨‍👩‍👧‍👦e\u{301} 🇯🇵";
        let result = visible_tail(text, |text| text.graphemes(true).count() <= 6);
        assert_eq!(result, "…👨‍👩‍👧‍👦e\u{301} 🇯🇵");
    }

    #[test]
    fn latin_words_and_long_unbroken_tokens_keep_a_readable_tail() {
        let fits = |text: &str| text.graphemes(true).count() <= 17;
        assert_eq!(
            visible_tail("old prefix vocabulary latest words", fits),
            "…latest words"
        );
        let token = "https://example.com/very-long-token";
        let tail = visible_tail(token, fits);
        assert!(tail.ends_with("very-long-token"));
        assert!(fits(&tail));
    }

    #[test]
    fn word_boundary_cleanup_cannot_overflow_a_two_line_window() {
        let fits = |text: &str| {
            let mut lines = 1;
            let mut width = 0;
            for word in text.split_whitespace() {
                let size = word.graphemes(true).count();
                if size > 8 {
                    return false;
                }
                if width > 0 && width + 1 + size > 8 {
                    lines += 1;
                    width = size;
                } else {
                    width += usize::from(width > 0) + size;
                }
            }
            lines <= 2
        };
        let result = visible_tail("old 1234567890 abcdefgh", fits);
        assert_eq!(result, "…4567890 abcdefgh");
        assert!(fits(&result));
    }
}
