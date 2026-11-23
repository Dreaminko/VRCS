use std::sync::OnceLock;

use serde::Deserialize;

use super::protocol::normalize_bmp_codepoint;

pub(crate) const MAX_ADVANCE: u8 = 12;

#[derive(Deserialize)]
struct AdvanceDoc {
    ranges: Vec<[u32; 3]>,
}

static ADVANCES: OnceLock<[u8; 65536]> = OnceLock::new();

fn advances_table() -> &'static [u8; 65536] {
    ADVANCES.get_or_init(|| {
        let doc: AdvanceDoc = serde_json::from_str(include_str!("glyph-advance.json"))
            .expect("embedded glyph-advance.json must be valid");
        let mut table = [u8::MAX; 65536];
        for [start, end, adv] in doc.ranges {
            assert!(start <= end && end <= 0xFFFF, "invalid glyph advance range");
            assert!(
                adv <= MAX_ADVANCE as u32,
                "glyph advance exceeds MAX_ADVANCE"
            );
            for cp in start..=end {
                table[cp as usize] = adv as u8;
            }
        }
        assert!(
            table.iter().all(|&a| a <= MAX_ADVANCE),
            "glyph-advance.json must cover every BMP codepoint"
        );
        assert_eq!(table[0], 0, "U+0000 advance must be 0");
        table
    })
}

pub(crate) fn glyph_advance(cp: u16) -> u8 {
    advances_table()[cp as usize]
}

#[cfg(test)]
pub(crate) fn text_advance(text: &str) -> usize {
    text.chars()
        .map(|ch| glyph_advance(normalize_bmp_codepoint(ch)) as usize)
        .sum()
}

#[cfg(test)]
pub(crate) fn compute_row_layout(row_cells: &[u16]) -> Vec<(u16, u8)> {
    let mut out = Vec::with_capacity(row_cells.len());
    let mut x: u16 = 0;
    for &cp in row_cells {
        let adv = if cp == 0 { 0 } else { glyph_advance(cp) };
        out.push((x, adv));
        x = x.saturating_add(adv as u16);
    }
    out
}

fn is_break_space(cp: u16) -> bool {
    matches!(cp, 0x0020 | 0x3000)
}

fn is_wide_break_opportunity(cp: u16) -> bool {
    (0x2E80..=0x9FFF).contains(&cp)
        || (0xAC00..=0xD7AF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0xFF00..=0xFFEF).contains(&cp)
}

/// Greedily wrap text to satisfy both `sum(advance) <= max_px` and `len(chars) <= max_cells`.
pub(crate) fn wrap_text(text: &str, max_px: usize, max_cells: usize) -> Vec<String> {
    let cps: Vec<u16> = text
        .chars()
        .filter(|&ch| ch != '\r')
        .map(|ch| {
            if ch == '\n' {
                0x0020
            } else {
                normalize_bmp_codepoint(ch)
            }
        })
        .collect();

    let mut lines = Vec::new();
    let mut i = 0;
    let n = cps.len();
    while i < n {
        while i < n && is_break_space(cps[i]) && !lines.is_empty() {
            i += 1;
        }
        if i >= n {
            break;
        }
        let mut width = 0usize;
        let mut j = i;
        let mut last_break: Option<usize> = None;
        while j < n && (j - i) < max_cells {
            let adv = glyph_advance(cps[j]) as usize;
            if width + adv > max_px {
                break;
            }
            width += adv;
            if is_break_space(cps[j])
                || is_wide_break_opportunity(cps[j])
                || (j + 1 < n && is_wide_break_opportunity(cps[j + 1]))
            {
                last_break = Some(j);
            }
            j += 1;
        }
        let end = if j >= n {
            n
        } else if is_break_space(cps[j]) {
            j
        } else if let Some(lb) = last_break {
            lb + 1
        } else {
            j.max(i + 1)
        };
        let line: String = cps[i..end]
            .iter()
            .map(|&cp| char::from_u32(cp as u32).unwrap_or('\u{FFFD}'))
            .collect();
        lines.push(line.trim_end_matches(' ').to_owned());
        i = end;
    }
    lines
}
