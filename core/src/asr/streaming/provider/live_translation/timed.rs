use super::*;
use crate::asr::streaming::alignment::{Boundary, Frame, Mapping, Unit, Window};

#[derive(Default)]
pub(super) struct Timing {
    session_id: String,
    revision: u64,
    sequence: u64,
    started: Option<Instant>,
    input_offset: usize,
    output_offset: usize,
    input_frames: VecDeque<(usize, usize, Frame)>,
    output_frames: VecDeque<(usize, usize, Frame)>,
    context: VecDeque<(String, String)>,
    candidates: Vec<(usize, usize)>,
    candidate_sequence: u64,
}

#[cfg(test)]
pub(in crate::asr::streaming) fn append_timed(
    config: &AsrConfig,
    state: &mut State,
    text: &str,
    translation: &str,
) -> Result<Option<CloudEvent>, String> {
    append_delta(config, state, text, translation, None, None)
}

pub(in crate::asr::streaming) fn append_delta(
    config: &AsrConfig,
    state: &mut State,
    text: &str,
    translation: &str,
    elapsed_ms: Option<u64>,
    event_id: Option<&str>,
) -> Result<Option<CloudEvent>, String> {
    if text.is_empty() && translation.is_empty() {
        return Ok(None);
    }
    let timing = state.timing.get_or_insert_with(Timing::default);
    if timing.session_id.is_empty() {
        timing.session_id = uuid::Uuid::new_v4().to_string();
    }
    let started = *timing.started.get_or_insert_with(Instant::now);
    timing.sequence += 1;
    let frame = Frame {
        sequence: timing.sequence,
        event_id: event_id.map(str::to_owned),
        elapsed_ms,
        received_ms: started.elapsed().as_millis() as u64,
    };
    for (frames, start, delta) in [
        (
            &mut timing.input_frames,
            timing.input_offset + state.input.len(),
            text,
        ),
        (
            &mut timing.output_frames,
            timing.output_offset + state.output.len(),
            translation,
        ),
    ] {
        if !delta.is_empty() {
            frames.push_back((start, start + delta.len(), frame.clone()));
            if frames.len() > 1024 {
                frames.pop_front();
            }
        }
    }
    append(config, state, text, translation)
}

fn frames_for(frames: &VecDeque<(usize, usize, Frame)>, start: usize, end: usize) -> Vec<Frame> {
    let frames: Vec<_> = frames
        .iter()
        .filter(|(a, b, _)| *a < end && *b > start)
        .map(|(_, _, f)| f.clone())
        .collect();
    if frames.len() <= 8 {
        return frames;
    }
    frames[..4]
        .iter()
        .chain(&frames[frames.len() - 4..])
        .cloned()
        .collect()
}

// Storage chunks limit model input, without deciding subtitle boundaries.
fn units(
    text: &str,
    prefix: &str,
    offset: usize,
    frames: &VecDeque<(usize, usize, Frame)>,
) -> (Vec<Unit>, bool) {
    let mut units = Vec::new();
    let mut start = 0;
    while start < text.len() && units.len() < 4 {
        let end = text[start..]
            .char_indices()
            .nth(1000)
            .map_or(text.len(), |(index, _)| start + index);
        units.push(Unit {
            id: format!("{prefix}-{}", offset + start),
            text: text[start..end].to_owned(),
            frames: frames_for(frames, offset + start, offset + end),
        });
        start = end;
    }
    (units, start < text.len())
}

fn boundary_end(units: &[Unit], text: &str, boundary: &Boundary) -> Option<usize> {
    if boundary.quote.is_empty() || boundary.quote.chars().count() > 200 {
        return None;
    }
    let mut offset = 0;
    for unit in units {
        if unit.id == boundary.unit_id {
            // Overlapping repetitions are also ambiguous. Do not guess which one.
            let mut matches = text.char_indices().filter(|(index, _)| {
                let end = index + boundary.quote.len();
                end > offset
                    && end <= offset + unit.text.len()
                    && text[*index..].starts_with(&boundary.quote)
            });
            let (index, _) = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            return Some(index + boundary.quote.len());
        }
        offset += unit.text.len();
    }
    None
}

impl Timing {
    pub(super) fn collect(
        &mut self,
        state: &mut State,
        config: &AsrConfig,
        flush: bool,
    ) -> Option<CloudEvent> {
        let mut snapshot = state.snapshot.clone()?;
        let mut completed = Vec::new();
        let mut translations = Vec::new();
        let same_language = config.backend == crate::providers::SERVICE_GEMINI_LIVE_TRANSLATE
            && snapshot.language.as_deref().is_some_and(|language| {
                crate::translation::same_translation_language(language, &snapshot.target_language)
            });
        // No silence/punctuation timer decides a live translation subtitle boundary.
        // At shutdown or the memory limit only, preserve the unresolved text as a group.
        if (flush || state.input.len() + state.output.len() >= MAX_TRANSCRIPT_BYTES / 2)
            && !state.input.trim().is_empty()
        {
            let mut source = snapshot.clone();
            source.utterance_id = format!("live-source-{}", uuid::Uuid::new_v4());
            source.text = std::mem::take(&mut state.input);
            source.translation.clear();
            completed.push(result(config, source.clone(), !same_language));
            source.translation = std::mem::take(&mut state.output);
            self.input_offset += source.text.len();
            self.output_offset += source.translation.len();
            self.input_frames.clear();
            self.output_frames.clear();
            self.context
                .push_back((source.text.clone(), source.translation.clone()));
            while self.context.len() > 2 {
                self.context.pop_front();
            }
            if !same_language {
                translations.push(result(config, source, false));
            }
            self.candidates.clear();
            self.revision += 1;
            tracing::warn!(
                flush,
                "Retained unresolved native text without model-selected boundaries"
            );
        }
        self.update_preview(&mut snapshot, state, flush);
        let changed = state.snapshot.as_ref() != Some(&snapshot);
        state.snapshot = Some(snapshot.clone());
        (!completed.is_empty() || !translations.is_empty() || changed).then_some(
            CloudEvent::LiveTranslation {
                service: config.backend.clone(),
                snapshot,
                completed,
                translations,
            },
        )
    }

    fn update_preview(&self, snapshot: &mut LiveTranslation, state: &State, flush: bool) {
        snapshot.text.clear();
        snapshot.translation.clear();
        if !flush {
            append_display_text(&mut snapshot.text, &state.input);
            append_display_text(&mut snapshot.translation, &state.output);
        }
    }

    fn window(&self, state: &State) -> Option<Window> {
        if state.input.trim().is_empty() || state.output.trim().is_empty() {
            return None;
        }
        let (sources, source_truncated) = units(
            &state.input,
            "source",
            self.input_offset,
            &self.input_frames,
        );
        let (targets, target_truncated) = units(
            &state.output,
            "target",
            self.output_offset,
            &self.output_frames,
        );
        Some(Window {
            session_id: self.session_id.clone(),
            revision: self.revision,
            sequence: self.sequence,
            sources,
            targets,
            source_truncated,
            target_truncated,
            context: self
                .context
                .iter()
                .map(|(s, t)| {
                    (
                        s.chars().take(1000).collect(),
                        t.chars().take(1000).collect(),
                    )
                })
                .collect(),
        })
    }
}

pub(in crate::asr::streaming) fn window(state: &State) -> Option<Window> {
    state.timing.as_ref()?.window(state)
}

fn contains_number(text: &str) -> bool {
    let mut ideographic_digits = 0;
    text.chars().any(|c| {
        ideographic_digits = if "零〇一二两兩三四五六七八九十百千万萬亿億兆".contains(c)
        {
            ideographic_digits + 1
        } else {
            0
        };
        c.is_numeric() || ideographic_digits >= 2
    })
}

// Risky boundaries need agreement on a later snapshot with additional native text.
// Preview is unaffected. At shutdown the final model pass can commit complete groups.
pub(in crate::asr::streaming) fn confirm(
    config: &AsrConfig,
    state: &mut State,
    window: &Window,
    mapping: &Mapping,
) -> Option<CloudEvent> {
    let timing = state.timing.as_ref()?;
    if timing.session_id != window.session_id || timing.revision != window.revision {
        return None;
    }
    let source: String = window.sources.iter().map(|u| u.text.as_str()).collect();
    let target: String = window.targets.iter().map(|u| u.text.as_str()).collect();
    if !state.input.starts_with(&source) || !state.output.starts_with(&target) {
        return None;
    }
    let sequence = window.sequence;
    let mut candidate = Vec::new();
    let mut accepted = Vec::new();
    let (mut a, mut b) = (0, 0);
    let mut deferred = false;
    for group in &mapping.groups {
        let end_a = boundary_end(&window.sources, &source, &group.source_end)?;
        let end_b = boundary_end(&window.targets, &target, &group.target_end)?;
        if end_a <= a || end_b <= b {
            return None;
        }
        if !group.fully_translated {
            break;
        }
        let pair = (timing.input_offset + end_a, timing.output_offset + end_b);
        candidate.push(pair);
        let text_a = &source[a..end_a];
        let text_b = &target[b..end_b];
        // Missing terminal punctuation triggers extra verification, never a local cut.
        let closed = |text: &str| {
            text.chars()
                .rev()
                .find(|c| !c.is_whitespace() && !closing(*c))
                .is_some_and(terminal)
        };
        let risky = (end_a == source.len() && !closed(text_a))
            || (end_b == target.len() && !closed(text_b))
            || text_a.chars().count() > 240
            || text_b.chars().count() > 240
            || (contains_number(text_a) || contains_number(text_b))
            || (text_a.chars().count() >= 8
                && (source[..a].contains(text_a.trim())
                    || source[end_a..].contains(text_a.trim())))
            || (text_b.chars().count() >= 8
                && (target[..b].contains(text_b.trim())
                    || target[end_b..].contains(text_b.trim())));
        let confirmed = sequence > timing.candidate_sequence && timing.candidates.contains(&pair);
        deferred |= risky && !confirmed;
        if !deferred {
            accepted.push(group.clone());
        }
        a = end_a;
        b = end_b;
    }
    validated_cuts(window, mapping, &source, &target)?;
    let timing = state.timing.as_mut()?;
    timing.candidates = candidate;
    timing.candidate_sequence = sequence;
    if accepted.is_empty() {
        return None;
    }
    apply(config, state, window, &Mapping { groups: accepted })
}

type Cut = (usize, usize, usize, usize);

fn validated_cuts(
    window: &Window,
    mapping: &Mapping,
    source_text: &str,
    target_text: &str,
) -> Option<Vec<Cut>> {
    let mut cuts = Vec::new();
    let (mut source_start, mut target_start) = (0, 0);
    // Validate all boundaries before mutating any pending text.
    for (index, group) in mapping.groups.iter().enumerate() {
        let source_end = boundary_end(&window.sources, &source_text, &group.source_end)?;
        let target_end = boundary_end(&window.targets, &target_text, &group.target_end)?;
        if source_end <= source_start
            || target_end <= target_start
            || !has_content(&source_text[source_start..source_end])
            || !has_content(&target_text[target_start..target_end])
            || (!group.fully_translated && index + 1 != mapping.groups.len())
        {
            return None;
        }
        if group.fully_translated {
            cuts.push((source_start, source_end, target_start, target_end));
        }
        source_start = source_end;
        target_start = target_end;
    }
    Some(cuts)
}

pub(in crate::asr::streaming) fn apply(
    config: &AsrConfig,
    state: &mut State,
    window: &Window,
    mapping: &Mapping,
) -> Option<CloudEvent> {
    let timing = state.timing.as_ref()?;
    if timing.session_id != window.session_id || timing.revision != window.revision {
        return None;
    }
    let source_text: String = window.sources.iter().map(|u| u.text.as_str()).collect();
    let target_text: String = window.targets.iter().map(|u| u.text.as_str()).collect();
    if !state.input.starts_with(&source_text) || !state.output.starts_with(&target_text) {
        return None;
    }
    let cuts = validated_cuts(window, mapping, &source_text, &target_text)?;
    let &(_, source_end, _, target_end) = cuts.last()?;
    let mut snapshot = state.snapshot.clone()?;
    let mut timing = state.timing.take()?;
    let mut completed = Vec::new();
    let mut translations = Vec::new();
    for (source_start, source_end, target_start, target_end) in cuts {
        let mut source = snapshot.clone();
        source.utterance_id = format!("live-source-{}", uuid::Uuid::new_v4());
        source.text = source_text[source_start..source_end].to_owned();
        source.translation.clear();
        completed.push(result(config, source.clone(), true));
        source.translation = target_text[target_start..target_end].to_owned();
        timing
            .context
            .push_back((source.text.clone(), source.translation.clone()));
        while timing.context.len() > 2 {
            timing.context.pop_front();
        }
        translations.push(result(config, source, false));
    }
    state.input.drain(..source_end);
    state.output.drain(..target_end);
    timing.input_offset += source_end;
    timing.output_offset += target_end;
    timing
        .input_frames
        .retain(|(_, end, _)| *end > timing.input_offset);
    timing
        .output_frames
        .retain(|(_, end, _)| *end > timing.output_offset);
    timing.revision += 1;
    tracing::debug!(
        revision = timing.revision,
        groups = translations.len(),
        "Applied model-selected live translation boundaries"
    );
    timing.update_preview(&mut snapshot, state, false);
    state.timing = Some(timing);
    state.snapshot = Some(snapshot.clone());
    Some(CloudEvent::LiveTranslation {
        service: config.backend.clone(),
        snapshot,
        completed,
        translations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::streaming::alignment::Group;
    use std::time::Duration;

    fn config() -> AsrConfig {
        AsrConfig {
            backend: crate::providers::SERVICE_OPENAI_REALTIME_TRANSLATE.into(),
            live_translation_target: Some("ja".into()),
            ..Default::default()
        }
    }
    fn state_with(text: &str, translation: &str) -> State {
        let mut state = State::default();
        append_timed(&config(), &mut state, text, translation).unwrap();
        state
    }
    fn group(w: &Window, source: &str, target: &str, fully_translated: bool) -> Group {
        let boundary = |units: &[Unit], quote: &str| Boundary {
            unit_id: units
                .iter()
                .find(|u| u.text.contains(quote))
                .unwrap()
                .id
                .clone(),
            quote: quote.into(),
        };
        Group {
            source_end: boundary(&w.sources, source),
            target_end: boundary(&w.targets, target),
            fully_translated,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn silence_and_punctuation_never_cut_openai_subtitles() {
        let mut state = state_with(
            "平台2.0自2020年启动，吸引288家社区。后续",
            "2020年の開始以来、",
        );
        for seconds in [2, 20, 60] {
            tokio::time::advance(Duration::from_secs(seconds)).await;
            assert!(poll(&config(), &mut state).is_none());
            assert_eq!(state.input, "平台2.0自2020年启动，吸引288家社区。后续");
            assert_eq!(state.output, "2020年の開始以来、");
        }
        let w = window(&state).unwrap();
        assert_eq!(w.sources[0].text, state.input);
        assert_eq!(w.targets[0].text, state.output);
    }

    #[test]
    fn every_delta_previews_before_any_model_result() {
        let mut state = state_with("原文一。原文二。", "");
        for delta in ["翻", "訳", "。", "続", "き"] {
            let Some(CloudEvent::LiveTranslation {
                snapshot,
                completed,
                translations,
                ..
            }) = append_timed(&config(), &mut state, "", delta).unwrap()
            else {
                panic!()
            };
            assert!(completed.is_empty());
            assert!(translations.is_empty());
            assert!(!snapshot.translation.is_empty());
        }
        assert_eq!(state.snapshot.as_ref().unwrap().translation, "翻訳。続き");
    }

    #[test]
    fn screenshot_statistics_stay_with_the_platform_group() {
        let source = "点亮计划2.0自2020年启动，吸引288家社区，发布3487个任务，解决2794个项目问题。";
        let next = "通过产业提需求、社区搭平台、学生做研发，形成闭环格局。";
        let intro = "プラットフォームで、2020年の開始以来、";
        let mut state = state_with(&format!("{source}{next}"), intro);
        let w = window(&state).unwrap();
        assert!(apply(
            &config(),
            &mut state,
            &w,
            &Mapping {
                groups: vec![group(&w, source, intro, false)]
            }
        )
        .is_none());
        assert_eq!(state.input, format!("{source}{next}"));
        let statistics = "288のコミュニティ、3487の案件、2794の課題を解決しました。";
        let conclusion = "産業界とコミュニティ、学生が好循環を形成しました。";
        append_timed(
            &config(),
            &mut state,
            "",
            &format!("{statistics}{conclusion}"),
        )
        .unwrap();
        let w = window(&state).unwrap();
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            snapshot,
            ..
        }) = apply(
            &config(),
            &mut state,
            &w,
            &Mapping {
                groups: vec![
                    group(&w, source, statistics, true),
                    group(&w, next, conclusion, true),
                ],
            },
        )
        else {
            panic!()
        };
        assert_eq!(completed.len(), 2);
        assert_eq!(translations[0].transcript.text, source);
        assert_eq!(
            translations[0].transcript.translation,
            format!("{intro}{statistics}")
        );
        assert_eq!(translations[1].transcript.text, next);
        assert_eq!(translations[1].transcript.translation, conclusion);
        assert!(snapshot.translation.is_empty());
    }

    #[test]
    fn model_can_cut_inside_units_and_merge_multiple_sentences() {
        let mut state = state_with("First. Second. Third.", "第一部分。第一补充。第二和第三。");
        let w = window(&state).unwrap();
        let Some(CloudEvent::LiveTranslation { translations, .. }) = apply(
            &config(),
            &mut state,
            &w,
            &Mapping {
                groups: vec![
                    group(&w, "First.", "第一补充。", true),
                    group(&w, "Third.", "第二和第三。", true),
                ],
            },
        ) else {
            panic!()
        };
        assert_eq!(translations[0].transcript.text, "First.");
        assert_eq!(
            translations[0].transcript.translation,
            "第一部分。第一补充。"
        );
        assert_eq!(translations[1].transcript.text, " Second. Third.");
        assert_eq!(translations[1].transcript.translation, "第二和第三。");
    }

    #[test]
    fn numeric_statistics_include_ideographic_numbers_without_treating_every_first_clause_as_numeric(
    ) {
        assert!(contains_number("二百八十八家社区"));
        assert!(contains_number("二千七百九十四个项目"));
        assert!(contains_number("12,257 students"));
        assert!(!contains_number("这是第一句。"));
    }

    #[test]
    fn risky_numeric_boundary_needs_agreement_after_new_text() {
        let mut state = state_with("In 2020. Next.", "2020年。次。");
        let w = window(&state).unwrap();
        let mapping = Mapping {
            groups: vec![group(&w, "In 2020.", "2020年。", true)],
        };
        assert!(confirm(&config(), &mut state, &w, &mapping).is_none());
        assert!(confirm(&config(), &mut state, &w, &mapping).is_none());
        assert_eq!(state.output, "2020年。次。");
        append_timed(&config(), &mut state, " More.", "続き。").unwrap();
        let next = window(&state).unwrap();
        let Some(CloudEvent::LiveTranslation { translations, .. }) =
            confirm(&config(), &mut state, &next, &mapping)
        else {
            panic!()
        };
        assert_eq!(translations[0].transcript.text, "In 2020.");
        assert_eq!(translations[0].transcript.translation, "2020年。");
        assert_eq!(state.output, "次。続き。");
    }

    #[test]
    fn changed_candidate_is_deferred_and_final_pass_keeps_provider_text_exact() {
        let mut state = state_with("There are 288 teams and 3487 tasks.", "288組、3487件。");
        let w = window(&state).unwrap();
        let partial = Mapping {
            groups: vec![group(&w, "288 teams", "288組", true)],
        };
        assert!(confirm(&config(), &mut state, &w, &partial).is_none());
        append_timed(&config(), &mut state, " Next.", " 次。").unwrap();
        let next = window(&state).unwrap();
        let complete = Mapping {
            groups: vec![group(&next, "3487 tasks.", "3487件。", true)],
        };
        assert!(confirm(&config(), &mut state, &next, &complete).is_none());
        assert_eq!(state.input, "There are 288 teams and 3487 tasks. Next.");
        let Some(CloudEvent::LiveTranslation { translations, .. }) =
            apply(&config(), &mut state, &next, &complete)
        else {
            panic!()
        };
        assert_eq!(translations[0].transcript.translation, "288組、3487件。");
    }

    #[test]
    fn complete_short_model_boundary_commits_without_waiting_for_another_utterance() {
        let mut state = state_with("Hello.", "こんにちは。");
        let w = window(&state).unwrap();
        assert!(confirm(
            &config(),
            &mut state,
            &w,
            &Mapping {
                groups: vec![group(&w, "Hello.", "こんにちは。", true)]
            }
        )
        .is_some());
    }

    #[test]
    fn model_can_cut_text_without_punctuation_or_silence() {
        let mut state = state_with("先打开窗户再关门", "窓を開けてからドアを閉める");
        let w = window(&state).unwrap();
        let Some(CloudEvent::LiveTranslation { translations, .. }) = apply(
            &config(),
            &mut state,
            &w,
            &Mapping {
                groups: vec![
                    group(&w, "先打开窗户", "窓を開けて", true),
                    group(&w, "再关门", "からドアを閉める", true),
                ],
            },
        ) else {
            panic!()
        };
        assert_eq!(translations.len(), 2);
    }

    #[test]
    fn uncertain_suffix_remains_visible_and_new_deltas_do_not_cancel_a_valid_prefix() {
        let mut state = state_with("Hello. Next", "你好。还");
        let w = window(&state).unwrap();
        append_timed(&config(), &mut state, " sentence.", "没结束。").unwrap();
        let Some(CloudEvent::LiveTranslation {
            snapshot,
            translations,
            ..
        }) = apply(
            &config(),
            &mut state,
            &w,
            &Mapping {
                groups: vec![
                    group(&w, "Hello.", "你好。", true),
                    group(&w, "Next", "还", false),
                ],
            },
        )
        else {
            panic!()
        };
        assert_eq!(translations.len(), 1);
        assert_eq!(state.input, " Next sentence.");
        assert_eq!(state.output, "还没结束。");
        assert_eq!(snapshot.translation, "还没结束。");
        let next = window(&state).unwrap();
        assert_eq!(next.context, vec![("Hello.".into(), "你好。".into())]);
        assert!(apply(
            &config(),
            &mut state,
            &w,
            &Mapping {
                groups: vec![group(&w, "Hello.", "你好。", true)]
            }
        )
        .is_none());
    }

    #[test]
    fn invalid_ambiguous_or_backward_boundaries_are_rejected_atomically() {
        let mut state = state_with("Thank you. Thank you. Next.", "谢谢。谢谢。再见。");
        let w = window(&state).unwrap();
        let good = group(&w, "Thank you. Next.", "谢谢。再见。", true);
        let mut invented = good.clone();
        invented.source_end.unit_id = "invented".into();
        let mut rewritten = good.clone();
        rewritten.target_end.quote = "改写。".into();
        let mut incomplete = good.clone();
        incomplete.fully_translated = false;
        for groups in [
            vec![group(&w, "Thank you.", "谢谢。", true)],
            vec![invented],
            vec![rewritten],
            vec![good.clone(), good.clone()],
            vec![incomplete, good],
        ] {
            assert!(apply(&config(), &mut state, &w, &Mapping { groups }).is_none());
            assert_eq!(window(&state), Some(w.clone()));
        }
    }

    #[test]
    fn overlapping_repeated_anchors_are_ambiguous() {
        let mut state = state_with("aaa", "你好");
        let w = window(&state).unwrap();
        assert!(apply(
            &config(),
            &mut state,
            &w,
            &Mapping {
                groups: vec![group(&w, "aa", "你好", true)]
            }
        )
        .is_none());
    }

    #[test]
    fn windows_bound_context_without_cutting_or_discarding_pending_text() {
        let state = state_with(&"甲".repeat(5000), &"乙".repeat(5000));
        let w = window(&state).unwrap();
        assert_eq!((w.sources.len(), w.targets.len()), (4, 4));
        assert!(w.source_truncated && w.target_truncated);
        assert_eq!(state.input.chars().count(), 5000);
        assert_eq!(state.output.chars().count(), 5000);
        assert!(w.sources.iter().all(|u| u.text.chars().count() == 1000));
    }

    #[test]
    fn unicode_whitespace_repeated_frame_times_and_metadata_are_preserved() {
        let mut state = State::default();
        append_delta(&config(), &mut state, "Hello🙂. ", "", Some(400), Some("a")).unwrap();
        append_delta(&config(), &mut state, "Next.", "", Some(400), Some("b")).unwrap();
        for i in 0..20 {
            append_delta(&config(), &mut state, "", "你", Some(400), Some("c")).unwrap();
            assert_eq!(
                state.snapshot.as_ref().unwrap().translation.chars().count(),
                i + 1
            );
        }
        append_delta(
            &config(),
            &mut state,
            "",
            "好🙂。\n  再见👋。",
            Some(400),
            Some("d"),
        )
        .unwrap();
        let w = window(&state).unwrap();
        assert_eq!(w.sources[0].frames.len(), 2);
        assert_eq!(w.targets[0].frames.len(), 8);
        assert_eq!(
            w.targets[0].frames.last().unwrap().event_id.as_deref(),
            Some("d")
        );
        let Some(CloudEvent::LiveTranslation { translations, .. }) = apply(
            &config(),
            &mut state,
            &w,
            &Mapping {
                groups: vec![group(&w, "Next.", "再见👋。", true)],
            },
        ) else {
            panic!()
        };
        assert_eq!(translations[0].transcript.text, "Hello🙂. Next.");
        assert_eq!(
            translations[0].transcript.translation,
            format!("{}好🙂。\n  再见👋。", "你".repeat(20))
        );
    }

    #[test]
    fn model_boundaries_can_cross_storage_chunks() {
        let source = format!("{}结束。下一条。", "甲".repeat(999));
        let target = format!("{}おわり。次。", "乙".repeat(999));
        let mut state = state_with(&source, &target);
        let w = window(&state).unwrap();
        let Some(CloudEvent::LiveTranslation { translations, .. }) = apply(
            &config(),
            &mut state,
            &w,
            &Mapping {
                groups: vec![group(&w, "束。", "わり。", true)],
            },
        ) else {
            panic!()
        };
        assert_eq!(
            translations[0].transcript.text,
            format!("{}结束。", "甲".repeat(999))
        );
        assert_eq!(state.input, "下一条。");
        assert_eq!(state.output, "次。");
    }

    #[test]
    fn boundary_quotes_can_cross_storage_chunks_to_disambiguate_the_end() {
        let source = format!("{}开始继续结束", "甲".repeat(998));
        let target = format!("{}始まり続きおわり", "乙".repeat(998));
        let mut state = state_with(&source, &target);
        let w = window(&state).unwrap();
        let Some(CloudEvent::LiveTranslation { translations, .. }) = apply(
            &config(),
            &mut state,
            &w,
            &Mapping {
                groups: vec![Group {
                    source_end: Boundary {
                        unit_id: w.sources[1].id.clone(),
                        quote: "开始继续".into(),
                    },
                    target_end: Boundary {
                        unit_id: w.targets[1].id.clone(),
                        quote: "始まり続き".into(),
                    },
                    fully_translated: true,
                }],
            },
        ) else {
            panic!()
        };
        assert_eq!(
            translations[0].transcript.text,
            format!("{}开始继续", "甲".repeat(998))
        );
        assert_eq!(state.input, "结束");
        assert_eq!(state.output, "おわり");
    }

    #[test]
    fn a_new_session_rejects_old_work_and_finish_keeps_unresolved_text_once() {
        let state = state_with("First. Tail", "第一句。尾句");
        let w = window(&state).unwrap();
        let mut next = state_with("First. Tail", "第一句。尾句");
        assert!(apply(
            &config(),
            &mut next,
            &w,
            &Mapping {
                groups: vec![group(&w, "First.", "第一句。", true)]
            }
        )
        .is_none());
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            snapshot,
            ..
        }) = finish(&config(), &mut next)
        else {
            panic!()
        };
        assert_eq!(completed[0].transcript.text, "First. Tail");
        assert_eq!(translations[0].transcript.translation, "第一句。尾句");
        assert!(snapshot.translation.is_empty());
        assert!(finish(&config(), &mut next).is_none());
    }

    #[test]
    fn missing_output_finishes_as_a_failure_and_memory_fallback_invalidates_old_work() {
        let mut missing = state_with("First. Second.", "");
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = finish(&config(), &mut missing)
        else {
            panic!()
        };
        assert_eq!(completed.len(), 1);
        assert_eq!(translations.len(), 1);
        assert_eq!(
            translations[0].transcript.utterance_id,
            completed[0].transcript.utterance_id
        );
        assert!(!translations[0].pending);
        assert!(translations[0].transcript.translation.is_empty());

        let mut state = state_with("First.", "第一句。");
        let w = window(&state).unwrap();
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = append_timed(
            &config(),
            &mut state,
            "",
            &"x".repeat(MAX_TRANSCRIPT_BYTES / 2),
        )
        .unwrap()
        else {
            panic!()
        };
        assert_eq!(completed.len(), 1);
        assert_eq!(
            translations[0].transcript.translation.len(),
            "第一句。".len() + MAX_TRANSCRIPT_BYTES / 2
        );
        assert!(state.input.is_empty() && state.output.is_empty());
        assert!(apply(
            &config(),
            &mut state,
            &w,
            &Mapping {
                groups: vec![group(&w, "First.", "第一句。", true)]
            }
        )
        .is_none());
    }
}
