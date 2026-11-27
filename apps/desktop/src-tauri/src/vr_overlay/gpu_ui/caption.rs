//! Caption visibility filtering, single-line tail truncation, and reader pagination.

use std::collections::{BTreeMap, HashSet};
use unicode_segmentation::UnicodeSegmentation;

use crate::vr_overlay::dashboard::CaptionVisibility;
use crate::vr_overlay::presentation::WristMessage;

fn is_other_player_source(source: &str) -> bool {
    matches!(source, "" | "speaker" | "vrchat_chatbox")
}

fn source_language_key(value: Option<&str>) -> Option<String> {
    let tag = value?.trim().replace('_', "-");
    if tag.is_empty() {
        return None;
    }
    let lower = tag.to_ascii_lowercase();
    let mapped = match lower.as_str() {
        "english" => "en",
        "japanese" => "ja",
        "korean" => "ko",
        "chinese" | "simplified chinese" | "traditional chinese" => "zh",
        "spanish" => "es",
        "french" => "fr",
        "german" => "de",
        "russian" => "ru",
        "portuguese" => "pt",
        "italian" => "it",
        "indonesian" => "id",
        "vietnamese" => "vi",
        "thai" => "th",
        "arabic" => "ar",
        _ => lower.as_str(),
    };
    let primary = mapped.split('-').next().unwrap_or(mapped);
    if matches!(
        primary,
        "" | "auto" | "und" | "unknown" | "mul" | "zxx"
    ) {
        None
    } else {
        Some(primary.to_owned())
    }
}

fn original_source_language(
    explicit: Option<&str>,
    translation_sources: &[String],
    detected: Option<&str>,
) -> Option<String> {
    if let Some(lang) = source_language_key(explicit) {
        return Some(lang);
    }
    let detected_set: HashSet<String> = translation_sources
        .iter()
        .filter_map(|s| source_language_key(Some(s.as_str())))
        .collect();
    if detected_set.len() == 1 {
        detected_set.into_iter().next()
    } else if detected_set.len() > 1 {
        None
    } else {
        source_language_key(detected)
    }
}

pub fn visible_caption_messages<'a>(
    headset: bool,
    messages: &'a [WristMessage],
    visibility: &CaptionVisibility,
    max_visible_players: Option<usize>,
    max_entries: usize,
) -> Vec<&'a WristMessage> {
    let hidden_langs: Vec<String> = visibility
        .hidden_source_languages
        .iter()
        .filter_map(|s| source_language_key(Some(s.as_str())))
        .collect();

    let mut filtered = Vec::with_capacity(messages.len());
    for msg in messages {
        if msg.text.trim().is_empty() {
            continue;
        }
        if let Some(orig) = &msg.original
            && is_other_player_source(&orig.source)
        {
            let track_key = orig.track_id.as_deref().unwrap_or("other:unidentified");
            if visibility.hidden_tracks.iter().any(|t| t == track_key) {
                continue;
            }
            if !hidden_langs.is_empty() {
                let src_lang = original_source_language(
                    orig.language.as_deref(),
                    &orig.translation_sources,
                    orig.detected_language.as_deref(),
                );
                if let Some(lang) = src_lang
                    && hidden_langs.iter().any(|h| h == &lang)
                {
                    continue;
                }
            }
        }
        filtered.push(msg);
    }

    let lane_key = |msg: &WristMessage| -> String {
        if let Some(orig) = &msg.original {
            format!(
                "{}:{}",
                orig.source,
                orig.track_id.as_deref().unwrap_or("unidentified")
            )
        } else {
            let side = match msg.side {
                crate::vr_overlay::presentation::MessageSide::Left => "left",
                crate::vr_overlay::presentation::MessageSide::Right => "right",
            };
            format!("{side}:{}", msg.speaker.as_deref().unwrap_or("sample"))
        }
    };
    let is_live = |msg: &WristMessage| msg.original.as_ref().is_some_and(|o| o.live);
    let is_other = |msg: &WristMessage| {
        msg.original.as_ref().map_or(
            msg.side == crate::vr_overlay::presentation::MessageSide::Left,
            |o| is_other_player_source(&o.source),
        )
    };
    fn player_key(msg: &WristMessage) -> &str {
        msg.original
            .as_ref()
            .and_then(|o| o.track_id.as_deref())
            .or(msg.speaker.as_deref())
            .unwrap_or("unidentified")
    }

    if headset {
        if max_visible_players.is_none() {
            let mut latest_live = None;
            let mut latest_any = None;
            let mut text_lanes: BTreeMap<String, &WristMessage> = BTreeMap::new();
            let mut text_order = Vec::new();
            for msg in &filtered {
                if msg.original.as_ref().is_some_and(|o| o.source == "vrchat_chatbox") {
                    let key = lane_key(msg);
                    if !text_lanes.contains_key(&key) {
                        text_order.push(key.clone());
                    }
                    text_lanes.insert(key, *msg);
                } else {
                    latest_any = Some(*msg);
                    if is_live(msg) {
                        latest_live = Some(*msg);
                    }
                }
            }
            let mut out = Vec::new();
            if let Some(latest) = latest_live.or(latest_any) {
                out.push(latest);
            }
            for key in text_order {
                if let Some(msg) = text_lanes.remove(&key) {
                    out.push(msg);
                }
            }
            return out;
        }
        let mut lanes: BTreeMap<String, (usize, &WristMessage)> = BTreeMap::new();
        for msg in &filtered {
            if !is_live(msg) {
                let key = lane_key(msg);
                let order = lanes.get(&key).map_or(lanes.len(), |(o, _)| *o);
                lanes.insert(key, (order, *msg));
            }
        }
        for msg in &filtered {
            if is_live(msg) {
                let key = lane_key(msg);
                let order = lanes.get(&key).map_or(lanes.len(), |(o, _)| *o);
                lanes.insert(key, (order, *msg));
            }
        }
        let mut entries: Vec<_> = lanes.into_values().collect();
        entries.sort_by(|(oa, a), (ob, b)| {
            is_live(b).cmp(&is_live(a)).then_with(|| oa.cmp(ob))
        });
        filtered = entries.into_iter().map(|(_, m)| m).collect();
    }

    if let Some(limit) = max_visible_players
        && limit > 0
    {
        let mut selected: HashSet<&str> = HashSet::new();
        for msg in &filtered {
            if selected.len() >= limit {
                break;
            }
            if is_live(msg) && is_other(msg) {
                selected.insert(player_key(msg));
            }
        }
        for msg in filtered.iter().rev() {
            if selected.len() >= limit {
                break;
            }
            if !is_live(msg) && is_other(msg) {
                selected.insert(player_key(msg));
            }
        }
        filtered.retain(|msg| !is_other(msg) || selected.contains(player_key(msg)));
    }

    if !headset {
        let count = max_entries.clamp(3, 10);
        if filtered.len() > count {
            let start = filtered.len() - count;
            filtered.drain(..start);
        }
    }

    filtered
}

/// Binary-search the longest suffix of `text` that fits, prefixed with `…`.
pub fn visible_tail(text: &str, mut fits: impl FnMut(&str) -> bool) -> String {
    if text.is_empty() || fits(text) {
        return text.to_owned();
    }
    if !fits("…") {
        return String::new();
    }
    let mut starts: Vec<usize> = text.grapheme_indices(true).map(|(i, _)| i).collect();
    starts.push(text.len());
    let mut low = 0usize;
    let mut high = starts.len() - 1;
    while low < high {
        let mid = (low + high) / 2;
        let cand = format!("…{}", &text[starts[mid]..]);
        if fits(&cand) {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    let mut start = starts[low];
    let fitted = format!("…{}", &text[start..]);
    if start > 0 {
        let prev_ascii = text[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric());
        let next_ascii = text[start..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric());
        if prev_ascii
            && next_ascii
            && let Some(ws) = text[start..].find(char::is_whitespace)
        {
            let after_ws = &text[start + ws..];
            if let Some(non_ws) = after_ws.find(|c: char| !c.is_whitespace()) {
                start += ws + non_ws;
            }
        }
    }
    let readable = format!("…{}", text[start..].trim_start());
    if fits(&readable) {
        readable
    } else {
        fitted
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeometry {
    pub count: usize,
    pub page: usize,
    pub offset: f32,
}

pub fn page_geometry(
    height: f32,
    viewport: f32,
    requested: usize,
    line_height: f32,
) -> PageGeometry {
    if !height.is_finite() || !viewport.is_finite() || !line_height.is_finite() || viewport <= 0.0
    {
        return PageGeometry {
            count: 1,
            page: 0,
            offset: 0.0,
        };
    }
    let overflow = (height - viewport).max(0.0);
    let overlap = (viewport / 3.0).min(line_height.max(1.0) * 2.0);
    let stride = (viewport - overlap).max(1.0);
    let count = ((overflow / stride).ceil() as usize + 1).max(1);
    let page = requested.min(count - 1);
    PageGeometry {
        count,
        page,
        offset: overflow.min(page as f32 * stride),
    }
}
