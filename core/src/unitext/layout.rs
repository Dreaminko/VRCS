use crate::config::UniTextConfig;
use crate::models::SpeakerIdentity;

use super::advance;
use super::protocol::{ViewportConfig, VirtualScreen, normalize_bmp_codepoint};

const MAX_ENTRIES: usize = 12;
const SPEAKER_COLORS: [u8; 4] = [2, 4, 5, 6];

#[derive(Clone)]
struct DisplayEntry {
    seq: u64,
    key: String,
    utterance_id: Option<String>,
    source_utterance_id: Option<String>,
    subtitle_id: Option<i64>,
    source: String,
    speaker_id: Option<String>,
    speaker_name: Option<String>,
    speaker_color: u8,
    original: String,
    translation: Option<String>,
    preserve_original_override: Option<bool>,
}

#[derive(Clone, PartialEq, Eq)]
struct RenderedRow {
    text: String,
    color: u8,
    flags: u8,
}

#[derive(Clone, Copy)]
struct OverflowState {
    entry_seq: u64,
    revealed_len: usize,
}

#[derive(Default)]
pub(crate) struct UniTextLayout {
    entries: Vec<DisplayEntry>,
    rendered_rows: Vec<RenderedRow>,
    status: u8,
    next_seq: u64,
    overflow: Option<OverflowState>,
    pending_overflow_lines: usize,
    self_player_name: Option<String>,
}

impl UniTextLayout {
    pub(crate) fn clear(&mut self, screen: &mut VirtualScreen) {
        self.entries.clear();
        self.rendered_rows.clear();
        self.status = 0;
        self.overflow = None;
        self.pending_overflow_lines = 0;
        screen.clear_all();
    }

    pub(crate) fn reset_viewport(&mut self) {
        self.rendered_rows.clear();
        self.overflow = None;
        self.pending_overflow_lines = 0;
    }

    pub(crate) fn has_pending_overflow(&self) -> bool {
        self.pending_overflow_lines > 0
    }

    pub(crate) fn set_status(&mut self, status: u8, screen: &mut VirtualScreen) {
        self.status = status;
        screen.set_status(status);
    }

    pub(crate) fn set_self_player_name(&mut self, name: &str) -> bool {
        let next = Some(name.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if self.self_player_name == next {
            return false;
        }
        self.self_player_name = next.clone();
        for entry in &mut self.entries {
            if is_self_source(&entry.source) {
                entry.speaker_name = next.clone();
            }
        }
        true
    }

    pub(crate) fn upsert_final(
        &mut self,
        utterance_id: Option<&str>,
        subtitle_id: Option<i64>,
        source: &str,
        speaker: Option<&SpeakerIdentity>,
        text: String,
    ) {
        let speaker_name = speaker_label(source, speaker, self.self_player_name.as_deref());
        let speaker_color = speaker_palette_color(source, speaker);
        let key = subtitle_id
            .map(|id| format!("sub:{id}"))
            .or_else(|| utterance_id.map(|uid| format!("utt:{uid}")))
            .unwrap_or_else(|| format!("src:{source}:{}", self.entries.len()));

        if let Some(existing) = self.entries.iter_mut().find(|e| {
            e.key == key
                || (utterance_id.is_some()
                    && (e.utterance_id.as_deref() == utterance_id
                        || e.source_utterance_id.as_deref() == utterance_id))
                || (subtitle_id.is_some() && e.subtitle_id == subtitle_id)
        }) {
            existing.key = key;
            existing.subtitle_id = subtitle_id.or(existing.subtitle_id);
            apply_speaker(existing, speaker);
            existing.original = text;
        } else {
            let seq = self.next_seq;
            self.next_seq += 1;
            self.entries.push(DisplayEntry {
                seq,
                key,
                utterance_id: utterance_id.map(str::to_owned),
                source_utterance_id: None,
                subtitle_id,
                source: source.to_owned(),
                speaker_id: speaker.map(|s| s.id.clone()),
                speaker_name,
                speaker_color,
                original: text,
                translation: None,
                preserve_original_override: None,
            });
            if self.entries.len() > MAX_ENTRIES {
                let drop_count = self.entries.len() - MAX_ENTRIES;
                self.entries.drain(0..drop_count);
            }
        }
        self.status = 0;
    }

    pub(crate) fn update_translation(
        &mut self,
        subtitle_id: i64,
        text: String,
        preferred: bool,
    ) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.subtitle_id == Some(subtitle_id))
        else {
            return false;
        };
        if !preferred && entry.translation.is_some() {
            return false;
        }
        entry.translation = Some(text);
        self.status = 0;
        true
    }

    pub(crate) fn upsert_live(
        &mut self,
        utterance_id: &str,
        source_utterance_id: Option<&str>,
        source: &str,
        speaker: Option<&SpeakerIdentity>,
        original: String,
        translation: String,
    ) {
        let match_id = source_utterance_id.unwrap_or(utterance_id);
        let speaker_name = speaker_label(source, speaker, self.self_player_name.as_deref());
        let speaker_color = speaker_palette_color(source, speaker);
        let translated = (!translation.trim().is_empty()).then_some(translation);

        let has_translation = if let Some(existing) = self.entries.iter_mut().find(|e| {
            e.utterance_id.as_deref() == Some(utterance_id)
                || e.utterance_id.as_deref() == Some(match_id)
                || e.source_utterance_id.as_deref() == Some(match_id)
                || (source_utterance_id.is_some()
                    && e.source_utterance_id.as_deref() == source_utterance_id)
        }) {
            existing.utterance_id = Some(utterance_id.to_owned());
            if let Some(src_id) = source_utterance_id {
                existing.source_utterance_id = Some(src_id.to_owned());
            }
            apply_speaker(existing, speaker);
            if !original.trim().is_empty() {
                existing.original = original;
            }
            if translated.is_some() {
                existing.translation = translated;
            }
            existing.translation.is_some()
        } else {
            let has_trans = translated.is_some();
            let seq = self.next_seq;
            self.next_seq += 1;
            self.entries.push(DisplayEntry {
                seq,
                key: format!("utt:{match_id}"),
                utterance_id: Some(utterance_id.to_owned()),
                source_utterance_id: source_utterance_id.map(str::to_owned),
                subtitle_id: None,
                source: source.to_owned(),
                speaker_id: speaker.map(|s| s.id.clone()),
                speaker_name,
                speaker_color,
                original,
                translation: translated,
                preserve_original_override: None,
            });
            if self.entries.len() > MAX_ENTRIES {
                let drop_count = self.entries.len() - MAX_ENTRIES;
                self.entries.drain(0..drop_count);
            }
            has_trans
        };
        self.status = if has_translation { 0 } else { 2 };
    }

    pub(crate) fn upsert_manual_chatbox(
        &mut self,
        key: String,
        original: String,
        translation: Option<String>,
        preserve_original_override: Option<bool>,
    ) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.entries.push(DisplayEntry {
            seq,
            key,
            utterance_id: None,
            source_utterance_id: None,
            subtitle_id: None,
            source: "chatbox".into(),
            speaker_id: None,
            speaker_name: self.self_player_name.clone(),
            speaker_color: speaker_palette_color("chatbox", None),
            original,
            translation,
            preserve_original_override,
        });
        if self.entries.len() > MAX_ENTRIES {
            let drop_count = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(0..drop_count);
        }
        self.status = 0;
    }

    pub(crate) fn update_speaker(&mut self, speaker: &SpeakerIdentity) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            if entry.speaker_id.as_deref() == Some(speaker.id.as_str()) {
                changed |= apply_speaker(entry, Some(speaker));
            }
        }
        changed
    }

    pub(crate) fn cancel_utterance(&mut self, utterance_id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| {
            !(e.subtitle_id.is_none()
                && (e.utterance_id.as_deref() == Some(utterance_id)
                    || e.source_utterance_id.as_deref() == Some(utterance_id)))
        });
        self.status = 0;
        self.entries.len() != before
    }

    pub(crate) fn advance_overflow(
        &mut self,
        screen: &mut VirtualScreen,
        config: &UniTextConfig,
        viewport: ViewportConfig,
    ) -> bool {
        if self.pending_overflow_lines == 0 {
            return false;
        }
        if let Some(ref mut ov) = self.overflow {
            ov.revealed_len += 1;
        }
        self.sync_to_screen(screen, config, viewport);
        true
    }

    pub(crate) fn sync_to_screen(
        &mut self,
        screen: &mut VirtualScreen,
        config: &UniTextConfig,
        viewport: ViewportConfig,
    ) {
        debug_assert_eq!(screen.cols, viewport.cols);
        debug_assert_eq!(screen.rows, viewport.rows);

        let mut all_rows = Vec::new();
        let mut last_entry_start = 0usize;
        let mut last_entry_seq: Option<u64> = None;

        for entry in &self.entries {
            let entry_start = all_rows.len();
            let orig = compact_whitespace(&entry.original);
            let trans = entry
                .translation
                .as_deref()
                .map(compact_whitespace)
                .filter(|t| !t.is_empty() && t != &orig);

            if (trans.is_some() || !orig.is_empty())
                && let Some(name) = entry.speaker_name.as_deref()
            {
                all_rows.push(RenderedRow {
                    text: format_speaker_header(name, viewport.row_px, viewport.cols),
                    color: entry.speaker_color,
                    flags: 0x02,
                });
            }

            match trans {
                Some(t) => {
                    let preserve_original = entry
                        .preserve_original_override
                        .unwrap_or(config.preserve_original_text);
                    if preserve_original && !orig.is_empty() {
                        for line in advance::wrap_text(&orig, viewport.row_px, viewport.cols) {
                            all_rows.push(RenderedRow {
                                text: line,
                                color: 1,
                                flags: 0x01,
                            });
                        }
                        for line in advance::wrap_text(&t, viewport.row_px, viewport.cols) {
                            all_rows.push(RenderedRow {
                                text: line,
                                color: entry.speaker_color,
                                flags: 0x02,
                            });
                        }
                    } else {
                        for line in advance::wrap_text(&t, viewport.row_px, viewport.cols) {
                            all_rows.push(RenderedRow {
                                text: line,
                                color: entry.speaker_color,
                                flags: 0x02,
                            });
                        }
                    }
                }
                None if !orig.is_empty() => {
                    let color = if is_self_source(&entry.source) {
                        0
                    } else {
                        entry.speaker_color
                    };
                    for line in advance::wrap_text(&orig, viewport.row_px, viewport.cols) {
                        all_rows.push(RenderedRow {
                            text: line,
                            color,
                            flags: 0x00,
                        });
                    }
                }
                None => {}
            }

            if all_rows.len() > entry_start {
                last_entry_start = entry_start;
                last_entry_seq = Some(entry.seq);
            }
        }

        let rows = viewport.rows;
        let total = all_rows.len();
        let last_entry_len = total.saturating_sub(last_entry_start);
        let new_rows: Vec<RenderedRow> = if last_entry_len > rows
            && let Some(seq) = last_entry_seq
        {
            let revealed_len = match self.overflow {
                Some(ov) if ov.entry_seq == seq => ov.revealed_len.clamp(rows, last_entry_len),
                _ => rows,
            };
            self.overflow = Some(OverflowState {
                entry_seq: seq,
                revealed_len,
            });
            self.pending_overflow_lines = last_entry_len.saturating_sub(revealed_len);
            let window_end = last_entry_start + revealed_len;
            let window_start = window_end - rows;
            all_rows[window_start..window_end].to_vec()
        } else {
            self.overflow = None;
            self.pending_overflow_lines = 0;
            let start = total.saturating_sub(rows);
            all_rows.into_iter().skip(start).collect()
        };

        let mut best_shift = 0usize;
        if !self.rendered_rows.is_empty() {
            let mut best_score = (0..rows)
                .filter(|&i| {
                    new_rows
                        .get(i)
                        .is_some_and(|r| !r.text.is_empty() && self.rendered_rows.get(i) == Some(r))
                })
                .count();
            for shift in 1..rows {
                let score = (0..rows - shift)
                    .filter(|&i| {
                        new_rows.get(i).is_some_and(|r| {
                            !r.text.is_empty() && self.rendered_rows.get(i + shift) == Some(r)
                        })
                    })
                    .count();
                if score > best_score {
                    best_score = score;
                    best_shift = shift;
                }
            }
        }
        if best_shift > 0 {
            screen.scroll_up(best_shift);
        }

        for r in 0..rows {
            if let Some(row) = new_rows.get(r) {
                screen.set_row_text(r, &row.text);
                screen.set_row_style(r, row.color, row.flags);
            } else {
                screen.set_row_text(r, "");
                screen.set_row_style(r, 0, 0);
            }
        }
        screen.set_status(self.status);
        self.rendered_rows = new_rows;
    }
}

pub(crate) fn build_test_screen(viewport: ViewportConfig) -> VirtualScreen {
    let mut screen = VirtualScreen::new(viewport.cols, viewport.rows);
    let summary = format!(
        "UniText v1 {}c x {}r @ {}px",
        viewport.cols, viewport.rows, viewport.row_px
    );
    let samples = [
        ("VRCS UniText Connected!".to_owned(), 7, 0x02),
        ("这是 UniText 共享翻译测试".to_owned(), 3, 0x02),
        ("Shared live subtitles ready".to_owned(), 2, 0x00),
        ("リアルタイム共有字幕テスト".to_owned(), 4, 0x00),
        ("다국어 자막 표시 테스트".to_owned(), 5, 0x00),
        (summary, 1, 0x01),
        ("RowPage 1 (Row 6): 扩展行测试".to_owned(), 6, 0x02),
        ("RowPage 1 (Row 7): Wide & tall OK".to_owned(), 7, 0x00),
        ("RowPage 1 (Row 8): Ring-Buffer OK".to_owned(), 2, 0x00),
        ("RowPage 1 (Row 9): 中文混排测试".to_owned(), 3, 0x00),
        ("RowPage 1 (Row 10): 日本語OK!".to_owned(), 4, 0x00),
        ("RowPage 1 (Row 11): 안녕하세요!".to_owned(), 5, 0x02),
    ];
    for (r, (text, color, flags)) in samples.into_iter().take(viewport.rows).enumerate() {
        let fitted = advance::wrap_text(&text, viewport.row_px, viewport.cols)
            .into_iter()
            .next()
            .unwrap_or_default();
        screen.set_row_text(r, &fitted);
        screen.set_row_style(r, color, flags);
    }
    screen.set_status(2);
    screen
}

fn trimmed_display_name(speaker: &SpeakerIdentity) -> Option<String> {
    speaker
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn is_self_source(source: &str) -> bool {
    matches!(source, "microphone" | "chatbox")
}

fn speaker_label(
    source: &str,
    speaker: Option<&SpeakerIdentity>,
    self_player_name: Option<&str>,
) -> Option<String> {
    if is_self_source(source) {
        return self_player_name.map(str::to_owned);
    }
    let speaker = speaker?;
    Some(
        trimmed_display_name(speaker)
            .unwrap_or_else(|| format!("#{}", speaker.index.saturating_add(1))),
    )
}

fn apply_speaker(entry: &mut DisplayEntry, speaker: Option<&SpeakerIdentity>) -> bool {
    if is_self_source(&entry.source) {
        return false;
    }
    let Some(speaker) = speaker else {
        return false;
    };
    entry.speaker_id = Some(speaker.id.clone());
    let next_name = trimmed_display_name(speaker)
        .or_else(|| entry.speaker_name.clone())
        .or_else(|| Some(format!("#{}", speaker.index.saturating_add(1))));
    let next_color = speaker_palette_color(&entry.source, Some(speaker));
    let changed = entry.speaker_name != next_name || entry.speaker_color != next_color;
    entry.speaker_name = next_name;
    entry.speaker_color = next_color;
    changed
}

fn format_speaker_header(name: &str, max_px: usize, max_cells: usize) -> String {
    let clean = compact_whitespace(name);
    let colon_cp = normalize_bmp_codepoint('：');
    let colon_px = advance::glyph_advance(colon_cp) as usize;
    let char_budget = max_cells.saturating_sub(1);
    let px_budget = max_px.saturating_sub(colon_px);

    let mut out = String::new();
    let mut used_px = 0usize;
    let mut used_chars = 0usize;
    for ch in clean.chars() {
        let cp = normalize_bmp_codepoint(ch);
        let adv = advance::glyph_advance(cp) as usize;
        if used_chars >= char_budget || used_px + adv > px_budget {
            break;
        }
        if let Some(norm_ch) = char::from_u32(cp as u32) {
            out.push(norm_ch);
            used_px += adv;
            used_chars += 1;
        }
    }
    if max_cells > 0 && max_px >= colon_px {
        out.push('：');
    }
    out
}

fn speaker_palette_color(source: &str, speaker: Option<&SpeakerIdentity>) -> u8 {
    if is_self_source(source) {
        return 3;
    }
    match speaker {
        Some(s) => SPEAKER_COLORS[(s.index as usize) % SPEAKER_COLORS.len()],
        None => 2,
    }
}

fn compact_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::unitext::protocol::{CTRL_CLEAR_ROW, DiffScheduler, ShadowCrt, UniTextTier};

    fn screen_row_string(screen: &VirtualScreen, visual_row: usize) -> String {
        screen
            .visual_row_cells(visual_row)
            .into_iter()
            .take_while(|&cp| cp != 0)
            .map(|cp| char::from_u32(cp as u32).unwrap_or('\u{FFFD}'))
            .collect()
    }

    #[test]
    fn best_shift_layout_scrolls_when_bottom_row_grows_and_dedups_live_final() {
        let config = UniTextConfig::default();
        let viewport = ViewportConfig::default();
        let mut layout = UniTextLayout::default();
        let mut screen = VirtualScreen::new(viewport.cols, viewport.rows);

        for i in 1..=3 {
            layout.upsert_final(
                Some(&format!("u{i}")),
                Some(i),
                "speaker",
                None,
                format!("Orig {i}"),
            );
            layout.update_translation(i, format!("Trans {i}"), true);
        }
        layout.sync_to_screen(&mut screen, &config, viewport);
        assert_eq!(screen.top_row, 0);

        // Live preview with a separate utterance_id sharing source_utterance_id
        layout.upsert_live(
            "live-4",
            Some("u4"),
            "speaker",
            None,
            "Streaming four".into(),
            "正在说第四句".into(),
        );
        layout.sync_to_screen(&mut screen, &config, viewport);
        let top_after_live = screen.top_row;
        assert_ne!(top_after_live, 0);

        // Growing bottom live line keeps upper lines matched and scrolls rather than rewriting all 6 rows
        layout.upsert_live(
            "live-4",
            Some("u4"),
            "speaker",
            None,
            "Streaming four".into(),
            "正在说第四句话还在变长中".into(),
        );
        layout.sync_to_screen(&mut screen, &config, viewport);
        assert_eq!(screen.top_row, top_after_live);

        // Finalizing with source_utterance_id updates in-place instead of duplicating
        layout.upsert_final(
            Some("u4"),
            Some(4),
            "speaker",
            None,
            "Streaming four".into(),
        );
        layout.sync_to_screen(&mut screen, &config, viewport);
        assert_eq!(layout.entries.len(), 4);
    }

    #[test]
    fn long_bilingual_sentence_wraps_and_scrolls_without_truncation_across_viewports() {
        let chinese_orig = "今天我们一起在 VRChat 里全面测试自适应行宽与可调显示行数的统一版 UniText v1 实时双语字幕协议：当系统通过 OSCQuery 自动读取到当前模型配置的行像素宽度与四到十二行的视窗容量后，哪怕遇到超过一百五十个汉字的超长复合句、演讲稿或者详细技术说明，编码器也会严格按照真实字符像素宽度精准折行，并借助环形缓冲区逐行滚动展示全部内容，绝不会在中途丢掉句尾！";
        let english_trans = "Today in VRChat we are thoroughly testing the unified UniText v1 bilingual subtitle protocol with resizable viewport width and configurable 4-to-12 row display capacity: once OSCQuery detects the active avatar's exact pixel budget and visible row count, even very long bilingual sentences, technical explanations, and multi-clause utterances exceeding a hundred words wrap cleanly inside the proportional pixel budget and scroll smoothly via the ring buffer without ever truncating the start or dropping the final words of the sentence!";
        assert!(chinese_orig.chars().count() > 100);
        assert!(english_trans.chars().count() > 100);

        let viewports = [
            ViewportConfig {
                cols: 64,
                rows: 6,
                row_px: 384,
                send_bank: true,
                send_row_page: false,
            },
            ViewportConfig {
                cols: 64,
                rows: 8,
                row_px: 256,
                send_bank: true,
                send_row_page: true,
            },
            ViewportConfig {
                cols: 64,
                rows: 12,
                row_px: 384,
                send_bank: true,
                send_row_page: true,
            },
        ];

        let config = UniTextConfig::default();
        let tier = UniTextTier::Standard;

        for viewport in viewports {
            let expected_orig = advance::wrap_text(chinese_orig, viewport.row_px, viewport.cols);
            let expected_trans = advance::wrap_text(english_trans, viewport.row_px, viewport.cols);
            let mut expected_all = Vec::new();
            expected_all.extend(expected_orig);
            expected_all.extend(expected_trans);
            assert!(
                expected_all.len() > viewport.rows,
                "viewport {}x{} must require overflow scrolling (got {} lines)",
                viewport.rows,
                viewport.row_px,
                expected_all.len()
            );

            let mut layout = UniTextLayout::default();
            let mut screen = VirtualScreen::new(viewport.cols, viewport.rows);
            let mut shadow = ShadowCrt::new(viewport.rows);
            let mut sched = DiffScheduler::default();
            let now = Instant::now();

            layout.upsert_final(
                Some("long-1"),
                Some(1),
                "speaker",
                None,
                chinese_orig.into(),
            );
            layout.update_translation(1, english_trans.into(), true);
            layout.sync_to_screen(&mut screen, &config, viewport);

            while let Some(pkt) = sched.next_packet(&screen, &shadow, tier, now) {
                shadow.apply(&pkt, tier.payload_bytes());
            }

            let mut observed_lines: Vec<String> = (0..viewport.rows)
                .map(|r| screen_row_string(&screen, r))
                .collect();
            for line in &observed_lines {
                assert!(
                    advance::text_advance(line) <= viewport.row_px,
                    "row exceeded {}px: {line}",
                    viewport.row_px
                );
                assert!(line.chars().count() <= viewport.cols);
            }
            assert_eq!(observed_lines, expected_all[..viewport.rows]);
            assert_eq!(
                shadow.read_visual_cells(viewport.cols, viewport.rows),
                screen.read_visual_cells()
            );
            assert_eq!(shadow.read_top_row(), screen.top_row);

            let mut prev_top = screen.top_row as usize;
            while layout.has_pending_overflow() {
                assert!(layout.advance_overflow(&mut screen, &config, viewport));
                let expected_top = (prev_top + 1) % viewport.rows;
                assert_eq!(
                    screen.top_row as usize, expected_top,
                    "top_row must wrap modulo {}",
                    viewport.rows
                );
                prev_top = expected_top;

                let new_bottom_phys = screen.visual_to_physical(viewport.rows - 1);
                let mut saw_clear_for_recycled = false;
                while let Some(pkt) = sched.next_packet(&screen, &shadow, tier, now) {
                    if pkt.ctrl == CTRL_CLEAR_ROW {
                        saw_clear_for_recycled = true;
                    }
                    let is_text_write = matches!(pkt.ctrl, 1..=239 | 243..=249);
                    if is_text_write {
                        let written_phys_row = match pkt.ctrl {
                            244..=249 => (pkt.ctrl - 244) as usize + (pkt.row_page as usize) * 6,
                            1..=96 => {
                                ((pkt.ctrl as usize - 1) * 2) / 32 + (pkt.row_page as usize) * 6
                            }
                            97..=144 => {
                                ((pkt.ctrl as usize - 97) * 4) / 32 + (pkt.row_page as usize) * 6
                            }
                            145..=239 => {
                                ((pkt.ctrl as usize - 145) * 2) / 32 + (pkt.row_page as usize) * 6
                            }
                            243 => (95 * 2) / 32 + (pkt.row_page as usize) * 6,
                            _ => unreachable!(),
                        };
                        assert_eq!(
                            written_phys_row, new_bottom_phys,
                            "scroll_up(1) must not retransmit upper rows"
                        );
                    }
                    shadow.apply(&pkt, tier.payload_bytes());
                }
                assert!(
                    saw_clear_for_recycled,
                    "recycled row must be cleared before writing"
                );
                assert_eq!(
                    shadow.read_visual_cells(viewport.cols, viewport.rows),
                    screen.read_visual_cells()
                );
                assert_eq!(shadow.read_top_row(), screen.top_row);

                let bottom_line = screen_row_string(&screen, viewport.rows - 1);
                assert!(advance::text_advance(&bottom_line) <= viewport.row_px);
                assert!(bottom_line.chars().count() <= viewport.cols);
                observed_lines.push(bottom_line);
            }

            assert_eq!(
                observed_lines, expected_all,
                "overflow scrolling on {}x{}px must reveal the entire bilingual utterance without truncation",
                viewport.rows, viewport.row_px
            );
        }
    }

    #[test]
    fn build_test_screen_respects_all_row_and_width_limits() {
        let viewports = [
            ViewportConfig {
                cols: 32,
                rows: 4,
                row_px: 192,
                send_bank: false,
                send_row_page: false,
            },
            ViewportConfig::default(),
            ViewportConfig {
                cols: 64,
                rows: 8,
                row_px: 256,
                send_bank: true,
                send_row_page: true,
            },
            ViewportConfig {
                cols: 64,
                rows: 12,
                row_px: 384,
                send_bank: true,
                send_row_page: true,
            },
        ];

        for viewport in viewports {
            let screen = build_test_screen(viewport);
            assert_eq!(screen.cols, viewport.cols);
            assert_eq!(screen.rows, viewport.rows);
            for r in 0..viewport.rows {
                let text = screen_row_string(&screen, r);
                assert!(!text.is_empty(), "test screen row {r} should be populated");
                assert!(
                    advance::text_advance(&text) <= viewport.row_px,
                    "row {r} exceeded {}px: {text}",
                    viewport.row_px
                );
                assert!(text.chars().count() <= viewport.cols);
            }
        }
    }

    #[test]
    fn per_player_speaker_renders_on_own_header_line_above_original_and_translation() {
        let viewport = ViewportConfig::default();
        let mut layout = UniTextLayout::default();
        let mut screen = VirtualScreen::new(viewport.cols, viewport.rows);
        let speaker = SpeakerIdentity {
            id: "track:alice".into(),
            index: 0,
            display_name: Some("Alice".into()),
            user_id: Some("usr_alice".into()),
        };

        layout.upsert_final(
            Some("u1"),
            Some(1),
            "speaker",
            Some(&speaker),
            "Hello world".into(),
        );
        layout.update_translation(1, "你好世界".into(), true);

        let mut config = UniTextConfig {
            preserve_original_text: true,
            ..UniTextConfig::default()
        };
        layout.sync_to_screen(&mut screen, &config, viewport);
        assert_eq!(screen_row_string(&screen, 0), "Alice：");
        assert_eq!(screen_row_string(&screen, 1), "Hello world");
        assert_eq!(screen_row_string(&screen, 2), "你好世界");

        config.preserve_original_text = false;
        layout.sync_to_screen(&mut screen, &config, viewport);
        assert_eq!(screen_row_string(&screen, 0), "Alice：");
        assert_eq!(screen_row_string(&screen, 1), "你好世界");
        assert_eq!(screen_row_string(&screen, 2), "");
    }

    #[test]
    fn unresolved_speaker_falls_back_to_ordinal_and_updates_in_place() {
        let config = UniTextConfig::default();
        let viewport = ViewportConfig::default();
        let mut layout = UniTextLayout::default();
        let mut screen = VirtualScreen::new(viewport.cols, viewport.rows);
        let unresolved = SpeakerIdentity {
            id: "track:1".into(),
            index: 0,
            display_name: None,
            user_id: None,
        };

        layout.upsert_live(
            "live-1",
            Some("u1"),
            "speaker",
            Some(&unresolved),
            "Hi".into(),
            "嗨".into(),
        );
        layout.sync_to_screen(&mut screen, &config, viewport);
        assert_eq!(screen_row_string(&screen, 0), "#1：");
        assert_eq!(screen_row_string(&screen, 1), "Hi");
        assert_eq!(screen_row_string(&screen, 2), "嗨");

        let resolved = SpeakerIdentity {
            id: "track:1".into(),
            index: 0,
            display_name: Some("Alice".into()),
            user_id: Some("usr_alice".into()),
        };
        assert!(layout.update_speaker(&resolved));
        layout.sync_to_screen(&mut screen, &config, viewport);
        assert_eq!(screen_row_string(&screen, 0), "Alice：");

        // Subsequent updates lacking display_name must not overwrite resolved name.
        layout.upsert_final(
            Some("u1"),
            Some(1),
            "speaker",
            Some(&unresolved),
            "Hi".into(),
        );
        layout.sync_to_screen(&mut screen, &config, viewport);
        assert_eq!(screen_row_string(&screen, 0), "Alice：");
    }

    #[test]
    fn microphone_and_none_speaker_emit_no_header_row() {
        let config = UniTextConfig::default();
        let viewport = ViewportConfig::default();
        let mut layout = UniTextLayout::default();
        let mut screen = VirtualScreen::new(viewport.cols, viewport.rows);
        let mic_speaker = SpeakerIdentity {
            id: "mic".into(),
            index: 0,
            display_name: Some("Me".into()),
            user_id: None,
        };

        layout.upsert_final(
            Some("m1"),
            Some(1),
            "microphone",
            Some(&mic_speaker),
            "My line".into(),
        );
        layout.upsert_final(Some("s1"), Some(2), "speaker", None, "System line".into());
        layout.sync_to_screen(&mut screen, &config, viewport);

        assert_eq!(screen_row_string(&screen, 0), "My line");
        assert_eq!(screen_row_string(&screen, 1), "System line");
        assert_eq!(screen_row_string(&screen, 2), "");
    }

    #[test]
    fn long_speaker_name_fits_single_header_row_with_colon() {
        let config = UniTextConfig::default();
        let viewport = ViewportConfig {
            cols: 32,
            rows: 4,
            row_px: 192,
            send_bank: false,
            send_row_page: false,
        };
        let mut layout = UniTextLayout::default();
        let mut screen = VirtualScreen::new(viewport.cols, viewport.rows);
        let speaker = SpeakerIdentity {
            id: "track:long".into(),
            index: 1,
            display_name: Some("这是一个非常非常长的VRChat玩家昵称ABCDEFGHIJKLMN😀".into()),
            user_id: None,
        };

        layout.upsert_final(
            Some("u1"),
            Some(1),
            "speaker",
            Some(&speaker),
            "Test".into(),
        );
        layout.sync_to_screen(&mut screen, &config, viewport);

        let header = screen_row_string(&screen, 0);
        assert!(header.ends_with('：'));
        assert!(advance::text_advance(&header) <= viewport.row_px);
        assert!(header.chars().count() <= viewport.cols);
        assert_eq!(screen_row_string(&screen, 1), "Test");
    }

    #[test]
    fn microphone_displays_self_player_name_header_when_configured() {
        let config = UniTextConfig::default();
        let viewport = ViewportConfig::default();
        let mut layout = UniTextLayout::default();
        let mut screen = VirtualScreen::new(viewport.cols, viewport.rows);

        assert!(layout.set_self_player_name(" Dreaminko "));
        layout.upsert_final(Some("m1"), Some(1), "microphone", None, "My line".into());
        layout.sync_to_screen(&mut screen, &config, viewport);

        assert_eq!(screen_row_string(&screen, 0), "Dreaminko：");
        assert_eq!(screen_row_string(&screen, 1), "My line");

        assert!(layout.set_self_player_name(""));
        layout.sync_to_screen(&mut screen, &config, viewport);

        assert_eq!(screen_row_string(&screen, 0), "My line");
        assert_eq!(screen_row_string(&screen, 1), "");
    }

    #[test]
    fn manual_chatbox_respects_send_mode_override_and_self_name() {
        let mut config = UniTextConfig {
            preserve_original_text: true,
            ..UniTextConfig::default()
        };
        let viewport = ViewportConfig::default();
        let mut layout = UniTextLayout::default();
        let mut screen = VirtualScreen::new(viewport.cols, viewport.rows);
        layout.set_self_player_name("Alice");

        layout.upsert_manual_chatbox(
            "cb-1".into(),
            "Original hidden".into(),
            Some("Only translation".into()),
            Some(false),
        );
        layout.sync_to_screen(&mut screen, &config, viewport);
        assert_eq!(screen_row_string(&screen, 0), "Alice：");
        assert_eq!(screen_row_string(&screen, 1), "Only translation");
        assert_eq!(screen_row_string(&screen, 2), "");

        config.preserve_original_text = false;
        layout.upsert_manual_chatbox(
            "cb-2".into(),
            "Both shown".into(),
            Some("Bilingual".into()),
            Some(true),
        );
        layout.sync_to_screen(&mut screen, &config, viewport);
        assert_eq!(screen_row_string(&screen, 2), "Alice：");
        assert_eq!(screen_row_string(&screen, 3), "Both shown");
        assert_eq!(screen_row_string(&screen, 4), "Bilingual");
    }
}
