/// Fixed-size text pages. Widths are supplied by the active font rasterizer.
pub(super) struct Pages {
    pub lines: Vec<Vec<String>>,
}

impl Pages {
    pub fn new(
        text: &str,
        width: i32,
        height: i32,
        line_height: i32,
        measure_width: impl Fn(&str) -> i32,
    ) -> Self {
        let mut lines = Vec::new();
        for paragraph in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
            if paragraph.is_empty() {
                lines.push(String::new());
                continue;
            }
            let boundaries: Vec<usize> = paragraph
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(paragraph.len()))
                .collect();
            let mut start = 0;
            while start + 1 < boundaries.len() {
                let mut low = start + 1;
                let last = boundaries.len() - 1;
                let mut high = low;
                while high < last
                    && measure_width(&paragraph[boundaries[start]..boundaries[high]]) <= width
                {
                    high = (start + (high - start) * 2).min(last);
                }
                let mut end = low;
                while low <= high {
                    let middle = (low + high) / 2;
                    if measure_width(&paragraph[boundaries[start]..boundaries[middle]]) <= width {
                        end = middle;
                        low = middle + 1;
                    } else {
                        high = middle - 1;
                    }
                }
                // Keep whole words when possible, and hard-wrap tokens without spaces.
                if end < boundaries.len() - 1 {
                    if let Some(boundary) = (start + 1..=end).rev().find(|&i| {
                        paragraph[boundaries[i - 1]..boundaries[i]]
                            .chars()
                            .all(char::is_whitespace)
                    }) {
                        end = boundary;
                    }
                }
                lines.push(paragraph[boundaries[start]..boundaries[end]].to_owned());
                start = end;
            }
        }
        let rows = (height / line_height.max(1)).max(1) as usize;
        Self {
            lines: lines.chunks(rows).map(|page| page.to_vec()).collect(),
        }
    }

    pub fn at(&self, page: usize) -> (&[String], bool) {
        (
            &self.lines[page.min(self.lines.len() - 1)],
            page >= self.lines.len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn width(text: &str) -> i32 {
        text.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
    }

    #[test]
    fn mixed_language_and_long_tokens_remain_reachable() {
        let text = "日本語 English words 中文 superlongtokenwithoutspaces🙂🙂🙂";
        let pages = Pages::new(text, 12, 3, 1, width);
        assert!(pages.lines.len() > 1);
        assert_eq!(
            pages.lines.iter().flatten().cloned().collect::<String>(),
            text
        );
        assert!(pages.lines.iter().flatten().all(|line| width(line) <= 12));
        assert!(pages.lines.iter().all(|lines| lines.len() <= 3));
    }
    #[test]
    fn explicit_blank_lines_are_preserved_across_pages() {
        let pages = Pages::new("first\r\n\r\nthird\n", 10, 2, 1, width);
        assert_eq!(pages.lines, vec![vec!["first", ""], vec!["third", ""]]);
    }
    #[test]
    fn shorter_content_keeps_last_page_and_marks_end() {
        let pages = Pages::new("one\ntwo\nthree", 10, 2, 1, width);
        assert_eq!(pages.at(1), (&["three".to_owned()][..], false));
        assert_eq!(pages.at(2), (&["three".to_owned()][..], true));
    }
    #[test]
    fn empty_text_and_oversized_glyph_make_progress() {
        assert_eq!(Pages::new("", 10, 2, 1, width).lines, vec![vec![""]]);
        assert_eq!(
            Pages::new("界界", 1, 1, 1, width).lines,
            vec![vec!["界"], vec!["界"]]
        );
    }
}
