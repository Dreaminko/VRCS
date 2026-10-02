#[derive(Debug, PartialEq, Eq)]
pub(super) struct VisibleRow {
    pub index: usize,
    pub top: i32,
    pub bottom: i32,
}

pub(super) fn visible_rows(
    heights: &[i32],
    viewport_top: i32,
    viewport_bottom: i32,
    gap: i32,
) -> Vec<VisibleRow> {
    let available_height = viewport_bottom - viewport_top;
    if available_height <= 0 {
        return Vec::new();
    }
    let required_height =
        heights.iter().sum::<i32>() + gap * heights.len().saturating_sub(1) as i32;
    let mut top = viewport_top - (required_height - available_height).max(0);
    let mut rows = Vec::new();
    for (index, &height) in heights.iter().enumerate() {
        let bottom = top + height;
        if bottom > viewport_top && top < viewport_bottom {
            rows.push(VisibleRow { index, top, bottom });
        }
        top = bottom + gap;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_messages_keep_their_top_position() {
        assert_eq!(
            visible_rows(&[80, 100], 32, 736, 10),
            vec![
                VisibleRow {
                    index: 0,
                    top: 32,
                    bottom: 112
                },
                VisibleRow {
                    index: 1,
                    top: 122,
                    bottom: 222
                },
            ]
        );
    }

    #[test]
    fn growing_translation_scrolls_to_its_latest_lines() {
        let before = visible_rows(&[250, 250], 32, 736, 10);
        assert_eq!(before[0].top, 32);
        let after = visible_rows(&[250, 600], 32, 736, 10);
        assert!(after[0].top < 32);
        assert_eq!(after.last().unwrap().bottom, 736);
        assert_eq!(
            after.last().unwrap().bottom - after.last().unwrap().top,
            600
        );
    }

    #[test]
    fn new_messages_remain_visible_after_old_rows_fill_the_page() {
        let rows = visible_rows(&[700, 700, 80], 32, 736, 10);
        assert_eq!(
            rows.iter().map(|row| row.index).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            rows.last().unwrap(),
            &VisibleRow {
                index: 2,
                top: 656,
                bottom: 736
            }
        );
    }

    #[test]
    fn a_single_translation_taller_than_the_page_keeps_its_bottom() {
        assert_eq!(
            visible_rows(&[2000], 32, 736, 10),
            vec![VisibleRow {
                index: 0,
                top: -1264,
                bottom: 736
            },]
        );
        assert!(visible_rows(&[], 32, 736, 10).is_empty());
    }
}
