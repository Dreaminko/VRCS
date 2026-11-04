//! Incoming Chatbox text and bounded, per-participant refresh suppression.
use super::names::{Reader, Value};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub(super) fn parse(message: &[u8]) -> Option<(u16, String)> {
    if message.len() > 1024 * 1024 {
        return None;
    }
    // Unreliable Photon commands may have an eight-byte sequence prefix.
    let at = message
        .windows(2)
        .take(9)
        .position(|header| header == [0xf3, 4])?;
    if *message.get(at + 2)? != 43 {
        return None;
    }
    let mut reader = Reader::new(&message[at + 3..]);
    let count = reader.byte()?;
    let mut actor = None;
    let mut text = None;
    for _ in 0..count {
        let key = reader.byte()?;
        let value = reader.dynamic(0)?;
        match key {
            254 => {
                if actor.is_some() {
                    return None;
                }
                actor = Some(value.actor()?);
            }
            245 => {
                if text.is_some() {
                    return None;
                }
                let Value::Text(value) = value else {
                    return None;
                };
                text = Some(value);
            }
            _ => {}
        }
    }
    if !reader.finished() {
        return None;
    }
    Some((actor?, text?.to_owned()))
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Candidate,
    Duplicate,
    Cleared,
    TooLong,
}

#[derive(Default)]
pub(super) struct Deduplicator {
    recent: VecDeque<Recent>,
}

struct Recent {
    scope: String,
    text: String,
    last_seen: Instant,
}

impl Deduplicator {
    pub(super) fn classify(&mut self, scope: &str, text: &str, now: Instant) -> Outcome {
        if text.chars().take(5001).count() > 5000 {
            return Outcome::TooLong;
        }
        if text.trim().is_empty() {
            self.remove(scope);
            return Outcome::Cleared;
        }
        let Some(index) = self.recent.iter().position(|entry| entry.scope == scope) else {
            return Outcome::Candidate;
        };
        let mut entry = self.recent.remove(index).unwrap();
        let outcome = if entry.text == text
            && now.saturating_duration_since(entry.last_seen) < Duration::from_secs(8)
        {
            entry.last_seen = now;
            Outcome::Duplicate
        } else {
            Outcome::Candidate
        };
        self.recent.push_back(entry);
        outcome
    }

    /// Call only for a candidate whose event was successfully enqueued.
    pub(super) fn commit(&mut self, scope: &str, text: &str, now: Instant) {
        self.remove(scope);
        if self.recent.len() == 512 {
            self.recent.pop_front();
        }
        self.recent.push_back(Recent {
            scope: scope.to_owned(),
            text: text.to_owned(),
            last_seen: now,
        });
    }

    pub(super) fn remove(&mut self, scope: &str) {
        if let Some(index) = self.recent.iter().position(|entry| entry.scope == scope) {
            self.recent.remove(index);
        }
    }

    pub(super) fn clear(&mut self) {
        self.recent.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Vec<u8> {
        let mut result = vec![7];
        let mut length = value.len();
        while length >= 128 {
            result.push((length as u8 & 127) | 128);
            length >>= 7;
        }
        result.push(length as u8);
        result.extend(value.as_bytes());
        result
    }

    fn fixture(value: &str) -> Vec<u8> {
        let mut result = vec![0xf3, 4, 43, 2, 254, 13, 1, 2, 245];
        result.extend(text(value));
        result
    }

    #[test]
    fn parses_unicode_varint_text_and_reordered_parameters_after_sequence_prefix() {
        let value = format!("  {}\nemoji: 🌏\n  ", "中文".repeat(30));
        assert_eq!(parse(&fixture(&value)), Some((513, value.clone())));
        for prefix in [0, 4, 8] {
            let mut message = vec![0; prefix];
            message.extend([0xf3, 4, 43, 3, 245]);
            message.extend(text(&value));
            message.extend([99, 2, 1, 254, 9, 0x82, 8]);
            assert_eq!(parse(&message), Some((513, value.clone())));
        }
    }

    #[test]
    fn rejects_truncation_wrong_events_types_and_invalid_actor() {
        let message = fixture("Hello 🌏\nnext line");
        for end in 0..message.len() {
            assert_eq!(parse(&message[..end]), None, "truncation at {end}");
        }
        for invalid in [
            vec![0xf3, 4, 42, 2, 254, 11, 7, 245, 7, 1, b'A'],
            vec![0xf3, 2, 43, 2, 254, 11, 7, 245, 7, 1, b'A'],
            vec![0xf3, 4, 43, 2, 254, 11, 7, 245, 67, 1, b'A'],
            vec![0xf3, 4, 43, 2, 254, 11, 0, 245, 7, 1, b'A'],
            vec![0xf3, 4, 43, 2, 254, 12, 1, 245, 7, 1, b'A'],
            vec![0xf3, 4, 43, 2, 254, 11, 7, 245, 7, 1, 255],
            vec![0xf3, 4, 43, 1, 245, 7, 1, b'A'],
            vec![0xf3, 4, 43, 1, 254, 11, 7],
        ] {
            assert_eq!(parse(&invalid), None, "{invalid:?}");
        }
        let mut prefix_too_long = vec![0; 9];
        prefix_too_long.extend(&message);
        assert_eq!(parse(&prefix_too_long), None);
        assert_eq!(parse(&vec![0; 1024 * 1024 + 1]), None);
    }

    #[test]
    fn validates_every_parameter_and_rejects_trailing_or_ambiguous_data() {
        let message = fixture("A");
        let mut wrong_event = vec![0xf3, 4, 42, 0];
        wrong_event.extend(&message);
        assert_eq!(parse(&wrong_event), None);
        let mut trailing = message.clone();
        trailing.push(0);
        assert_eq!(parse(&trailing), None);
        for extra in [
            vec![99, 7, 5, b'x'],
            vec![254, 11, 8],
            vec![245, 7, 1, b'B'],
        ] {
            let mut invalid = message.clone();
            invalid[3] = 3;
            invalid.extend(extra);
            assert_eq!(parse(&invalid), None);
        }
        let mut invalid_varint = vec![0xf3, 4, 43, 2, 254, 11, 7, 245, 7];
        invalid_varint.extend([255; 10]);
        assert_eq!(parse(&invalid_varint), None);
    }

    #[test]
    fn refresh_silence_uses_last_seen_and_accepts_at_eight_seconds() {
        let now = Instant::now();
        let mut dedup = Deduplicator::default();
        assert_eq!(dedup.classify("one", "A", now), Outcome::Candidate);
        dedup.commit("one", "A", now);
        assert_eq!(
            dedup.classify("one", "A", now + Duration::from_secs(7)),
            Outcome::Duplicate
        );
        assert_eq!(
            dedup.classify("one", "A", now + Duration::from_millis(14_999)),
            Outcome::Duplicate
        );
        assert_eq!(
            dedup.classify("one", "A", now + Duration::from_millis(22_999)),
            Outcome::Candidate
        );
    }

    #[test]
    fn text_changes_and_scopes_are_independent_and_failed_enqueue_is_not_committed() {
        let now = Instant::now();
        let mut dedup = Deduplicator::default();
        for value in ["A", "B", "A"] {
            assert_eq!(dedup.classify("one", value, now), Outcome::Candidate);
            dedup.commit("one", value, now);
        }
        assert_eq!(dedup.classify("two", "A", now), Outcome::Candidate);
        assert_eq!(dedup.classify("one", "B", now), Outcome::Candidate);
        assert_eq!(dedup.classify("one", "B", now), Outcome::Candidate);
        assert_eq!(dedup.classify("one", "A", now), Outcome::Duplicate);
    }

    #[test]
    fn whitespace_clears_scope_and_unicode_length_is_not_utf8_byte_length() {
        let now = Instant::now();
        let mut dedup = Deduplicator::default();
        dedup.commit("one", "A", now);
        assert_eq!(
            dedup.classify("one", " \t\n\u{3000}", now),
            Outcome::Cleared
        );
        assert_eq!(dedup.classify("one", "A", now), Outcome::Candidate);
        assert_eq!(dedup.classify("one", "", now), Outcome::Cleared);
        assert_eq!(
            dedup.classify("one", &"🌏".repeat(5000), now),
            Outcome::Candidate
        );
        assert_eq!(
            dedup.classify("one", &"🌏".repeat(5001), now),
            Outcome::TooLong
        );
        assert_eq!(
            dedup.classify("one", &" ".repeat(5001), now),
            Outcome::TooLong
        );
        dedup.commit("one", " A\n", now);
        assert_eq!(dedup.classify("one", "A", now), Outcome::Candidate);
        assert_eq!(dedup.classify("one", " A\n", now), Outcome::Duplicate);
    }

    #[test]
    fn evicts_least_recently_active_scope_and_resets_explicit_lifecycles() {
        let now = Instant::now();
        let mut dedup = Deduplicator::default();
        for index in 0..512 {
            dedup.commit(&index.to_string(), "A", now);
        }
        assert_eq!(dedup.classify("0", "A", now), Outcome::Duplicate);
        dedup.commit("new", "A", now);
        assert_eq!(dedup.classify("1", "A", now), Outcome::Candidate);
        assert_eq!(dedup.classify("0", "A", now), Outcome::Duplicate);
        dedup.remove("0");
        assert_eq!(dedup.classify("0", "A", now), Outcome::Candidate);
        dedup.clear();
        assert_eq!(dedup.classify("new", "A", now), Outcome::Candidate);
        assert_eq!(dedup.classify("511", "A", now), Outcome::Candidate);
    }
}
