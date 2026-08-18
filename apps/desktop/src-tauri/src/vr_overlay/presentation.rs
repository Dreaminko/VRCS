use std::collections::VecDeque;
use std::time::{Duration, Instant};

use vrcs_core::{
    same_translation_language, PresentationEvent, Subtitle, SubtitleTranslation,
    VrOverlayHeadsetConfig, VrOverlayWristConfig,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PresentationFrame {
    pub content: PresentationContent,
    pub opacity: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PresentationContent {
    Headset(String),
    Wrist(Vec<WristMessage>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WristMessage {
    pub text: String,
    pub side: MessageSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageSide {
    Left,
    Right,
}

impl PresentationFrame {
    pub fn headset(text: impl Into<String>, opacity: f32) -> Self {
        Self {
            content: PresentationContent::Headset(text.into()),
            opacity,
        }
    }

    pub fn wrist(messages: Vec<WristMessage>) -> Self {
        Self {
            content: PresentationContent::Wrist(messages),
            opacity: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
struct PresentationItem {
    utterance_id: Option<String>,
    subtitle_id: Option<i64>,
    source: String,
    language: Option<String>,
    original: String,
    translation: Option<String>,
    expires_at: Instant,
}

const MAX_TERMINATED_UTTERANCES: usize = 32;

#[derive(Default)]
struct TerminatedUtterances(VecDeque<(String, String)>);

impl TerminatedUtterances {
    fn contains(&self, source: &str, utterance_id: &str) -> bool {
        self.0
            .iter()
            .any(|key| key.0 == source && key.1 == utterance_id)
    }

    fn remember(&mut self, source: &str, utterance_id: &str) {
        if self.contains(source, utterance_id) {
            return;
        }
        self.0.push_back((source.into(), utterance_id.into()));
        if self.0.len() > MAX_TERMINATED_UTTERANCES {
            self.0.pop_front();
        }
    }

    fn reset_source(&mut self, source: &str) {
        self.0.retain(|key| key.0 != source);
    }
}

#[derive(Default)]
pub struct HeadsetPresentation {
    current: Option<PresentationItem>,
    partial: Option<PresentationItem>,
    terminated: TerminatedUtterances,
}

impl HeadsetPresentation {
    pub fn apply(
        &mut self,
        event: PresentationEvent,
        now: Instant,
        config: &VrOverlayHeadsetConfig,
    ) {
        match event {
            PresentationEvent::RecognitionPartial {
                utterance_id,
                source,
                text,
                language,
            } if config.show_partials
                && source_enabled(&source, config)
                && !text.trim().is_empty()
                && !self.terminated.contains(&source, &utterance_id) =>
            {
                self.partial = Some(item_from_partial(
                    utterance_id,
                    source,
                    text,
                    language,
                    expiry(now, config.display_seconds),
                ));
            }
            PresentationEvent::RecognitionCancelled {
                utterance_id,
                source,
            } => {
                self.terminated.remember(&source, &utterance_id);
                if self.partial.as_ref().is_some_and(|item| {
                    item.source == source && item.utterance_id.as_deref() == Some(&utterance_id)
                }) {
                    self.partial = None;
                }
            }
            PresentationEvent::RecognitionReset { source } => {
                self.terminated.reset_source(&source);
                if self
                    .partial
                    .as_ref()
                    .is_some_and(|item| item.source == source)
                {
                    self.partial = None;
                }
            }
            PresentationEvent::Final {
                utterance_id,
                subtitle,
            } => {
                if let Some(utterance_id) = utterance_id.as_deref() {
                    self.terminated.remember(&subtitle.source, utterance_id);
                    if self.partial.as_ref().is_some_and(|item| {
                        item.source == subtitle.source
                            && item.utterance_id.as_deref() == Some(utterance_id)
                    }) {
                        self.partial = None;
                    }
                }
                if source_enabled(&subtitle.source, config) {
                    self.current = Some(item_from_subtitle(
                        subtitle,
                        utterance_id,
                        expiry(now, config.display_seconds),
                    ));
                }
            }
            PresentationEvent::TranslationPartial { subtitle_id, text }
                if config.show_translation_partials =>
            {
                if let Some(item) = self
                    .current
                    .as_mut()
                    .filter(|item| item.subtitle_id == Some(subtitle_id))
                {
                    item.translation = Some(text);
                }
            }
            PresentationEvent::TranslationCompleted {
                subtitle_id,
                translation,
            } => {
                if let Some(item) = self
                    .current
                    .as_mut()
                    .filter(|item| item.subtitle_id == Some(subtitle_id))
                {
                    item.translation = visible_translation(item.language.as_deref(), &translation);
                    if item.translation.is_some() {
                        item.expires_at = expiry(now, config.display_seconds);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn set_show_partials(&mut self, enabled: bool) {
        if !enabled {
            self.partial = None;
        }
    }

    pub fn frame(
        &self,
        now: Instant,
        config: &VrOverlayHeadsetConfig,
    ) -> Option<PresentationFrame> {
        let item = self
            .partial
            .as_ref()
            .filter(|_| config.show_partials)
            .or(self.current.as_ref())?;
        if !source_enabled(&item.source, config) {
            return None;
        }
        let opacity = fade_opacity(now, item.expires_at, config.fade_seconds)?;
        Some(PresentationFrame::headset(
            display_text(item, &config.content_mode, "\n"),
            opacity,
        ))
    }
}

#[derive(Default)]
pub struct WristPresentation {
    entries: VecDeque<PresentationItem>,
    partials: VecDeque<PresentationItem>,
    terminated: TerminatedUtterances,
    last_activity: Option<Instant>,
}

impl WristPresentation {
    pub fn apply(&mut self, event: PresentationEvent, now: Instant, config: &VrOverlayWristConfig) {
        match event {
            PresentationEvent::RecognitionPartial {
                utterance_id,
                source,
                text,
                language,
            } if config.show_partials
                && source_enabled(&source, config)
                && !text.trim().is_empty()
                && !self.terminated.contains(&source, &utterance_id) =>
            {
                let item = item_from_partial(utterance_id, source, text, language, now);
                if let Some(existing) = self
                    .partials
                    .iter_mut()
                    .find(|partial| partial.source == item.source)
                {
                    *existing = item;
                } else {
                    self.partials.push_back(item);
                }
                self.last_activity = Some(now);
            }
            PresentationEvent::RecognitionCancelled {
                utterance_id,
                source,
            } => {
                self.terminated.remember(&source, &utterance_id);
                self.partials.retain(|item| {
                    item.source != source || item.utterance_id.as_deref() != Some(&utterance_id)
                });
            }
            PresentationEvent::RecognitionReset { source } => {
                self.terminated.reset_source(&source);
                self.partials.retain(|item| item.source != source);
            }
            PresentationEvent::Final {
                utterance_id,
                subtitle,
            } => {
                if let Some(utterance_id) = utterance_id.as_deref() {
                    self.terminated.remember(&subtitle.source, utterance_id);
                    self.partials.retain(|item| {
                        item.source != subtitle.source
                            || item.utterance_id.as_deref() != Some(utterance_id)
                    });
                }
                if source_enabled(&subtitle.source, config) {
                    self.entries
                        .push_back(item_from_subtitle(subtitle, utterance_id, now));
                    self.trim(config.max_entries);
                    self.last_activity = Some(now);
                }
            }
            PresentationEvent::TranslationPartial { subtitle_id, text }
                if config.show_translation_partials =>
            {
                if let Some(item) = self
                    .entries
                    .iter_mut()
                    .find(|item| item.subtitle_id == Some(subtitle_id))
                {
                    item.translation = Some(text);
                }
            }
            PresentationEvent::TranslationCompleted {
                subtitle_id,
                translation,
            } => {
                if let Some(item) = self
                    .entries
                    .iter_mut()
                    .find(|item| item.subtitle_id == Some(subtitle_id))
                {
                    item.translation = visible_translation(item.language.as_deref(), &translation);
                    if item.translation.is_some() {
                        self.last_activity = Some(now);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn set_max_entries(&mut self, max_entries: u32) {
        self.trim(max_entries);
    }

    pub fn set_show_partials(&mut self, enabled: bool) {
        if !enabled {
            self.partials.clear();
        }
    }

    pub fn frame(&self, now: Instant, config: &VrOverlayWristConfig) -> Option<PresentationFrame> {
        if config.idle_hide_seconds > 0
            && self.last_activity.is_some_and(|last| {
                now.saturating_duration_since(last)
                    >= Duration::from_secs(config.idle_hide_seconds.into())
            })
        {
            return None;
        }

        let limit = config.max_entries.clamp(3, 10) as usize;
        let visible = self
            .entries
            .iter()
            .chain(self.partials.iter().filter(|_| config.show_partials))
            .filter(|item| source_enabled(&item.source, config));
        let mut messages: Vec<WristMessage> = visible
            .map(|item| wrist_message(item, &config.content_mode))
            .collect();
        if messages.len() > limit {
            messages.drain(..messages.len() - limit);
        }
        (!messages.is_empty()).then(|| PresentationFrame::wrist(messages))
    }

    fn trim(&mut self, max_entries: u32) {
        let limit = max_entries.clamp(3, 10) as usize;
        while self.entries.len() > limit {
            self.entries.pop_front();
        }
    }
}

fn item_from_subtitle(
    subtitle: Subtitle,
    utterance_id: Option<String>,
    expires_at: Instant,
) -> PresentationItem {
    let translation = subtitle
        .translations
        .last()
        .and_then(|translation| visible_translation(subtitle.language.as_deref(), translation));
    PresentationItem {
        utterance_id,
        subtitle_id: subtitle.id,
        source: subtitle.source,
        language: subtitle.language,
        original: subtitle.text,
        translation,
        expires_at,
    }
}

fn item_from_partial(
    utterance_id: String,
    source: String,
    text: String,
    language: Option<String>,
    expires_at: Instant,
) -> PresentationItem {
    PresentationItem {
        utterance_id: Some(utterance_id),
        subtitle_id: None,
        source,
        language,
        original: text,
        translation: None,
        expires_at,
    }
}

fn visible_translation(
    source_language: Option<&str>,
    translation: &SubtitleTranslation,
) -> Option<String> {
    let same_language = [source_language, translation.source_language.as_deref()]
        .into_iter()
        .flatten()
        .any(|source| same_translation_language(source, &translation.target_language));
    if translation.text.trim().is_empty() || same_language {
        return None;
    }
    Some(translation.text.clone())
}

fn wrist_message(item: &PresentationItem, content_mode: &str) -> WristMessage {
    WristMessage {
        text: display_text(item, content_mode, "\n"),
        side: match item.source.as_str() {
            "microphone" | "chatbox" => MessageSide::Right,
            _ => MessageSide::Left,
        },
    }
}

fn expiry(now: Instant, seconds: f32) -> Instant {
    now + Duration::from_secs_f32(seconds.max(0.0))
}

fn fade_opacity(now: Instant, expires_at: Instant, fade_seconds: f32) -> Option<f32> {
    if now <= expires_at {
        return Some(1.0);
    }
    if fade_seconds <= 0.0 {
        return None;
    }
    let elapsed = now.saturating_duration_since(expires_at).as_secs_f32();
    (elapsed < fade_seconds).then(|| 1.0 - elapsed / fade_seconds)
}

fn display_text(item: &PresentationItem, mode: &str, bilingual_separator: &str) -> String {
    let translation = item.translation.as_deref().filter(|text| !text.is_empty());
    match mode {
        "translation" => translation.unwrap_or(&item.original).to_owned(),
        "bilingual" => translation
            .map(|text| format!("{}{bilingual_separator}{text}", item.original))
            .unwrap_or_else(|| item.original.clone()),
        _ => item.original.clone(),
    }
}

trait SourceConfig {
    fn include_speaker(&self) -> bool;
    fn include_microphone(&self) -> bool;
    fn include_chatbox(&self) -> bool;
}

impl SourceConfig for VrOverlayHeadsetConfig {
    fn include_speaker(&self) -> bool {
        self.include_speaker
    }
    fn include_microphone(&self) -> bool {
        self.include_microphone
    }
    fn include_chatbox(&self) -> bool {
        self.include_chatbox
    }
}

impl SourceConfig for VrOverlayWristConfig {
    fn include_speaker(&self) -> bool {
        self.include_speaker
    }
    fn include_microphone(&self) -> bool {
        self.include_microphone
    }
    fn include_chatbox(&self) -> bool {
        self.include_chatbox
    }
}

fn source_enabled(source: &str, config: &impl SourceConfig) -> bool {
    match source {
        "speaker" => config.include_speaker(),
        "microphone" => config.include_microphone(),
        "chatbox" => config.include_chatbox(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subtitle(id: i64, text: &str) -> Subtitle {
        Subtitle {
            id: Some(id),
            conversation_id: None,
            text: text.into(),
            language: Some("en".into()),
            started_at: None,
            ended_at: None,
            source: "speaker".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            translations: Vec::new(),
        }
    }

    fn final_event(subtitle: Subtitle) -> PresentationEvent {
        PresentationEvent::Final {
            utterance_id: None,
            subtitle,
        }
    }

    fn partial_event(utterance_id: &str, source: &str, text: &str) -> PresentationEvent {
        PresentationEvent::RecognitionPartial {
            utterance_id: utterance_id.into(),
            source: source.into(),
            text: text.into(),
            language: Some("en".into()),
        }
    }

    fn translation(text: &str) -> vrcs_core::SubtitleTranslation {
        vrcs_core::SubtitleTranslation {
            text: text.into(),
            source_language: Some("en".into()),
            target_language: "zh".into(),
            provider: "test".into(),
            model: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn headset_final_fades() {
        let now = Instant::now();
        let config = VrOverlayHeadsetConfig {
            display_seconds: 2.0,
            fade_seconds: 1.0,
            ..Default::default()
        };
        let mut state = HeadsetPresentation::default();
        state.apply(final_event(subtitle(7, "hello")), now, &config);
        let fading = state
            .frame(now + Duration::from_millis(2500), &config)
            .unwrap();
        assert_eq!(fading.content, PresentationContent::Headset("hello".into()));
        assert!((fading.opacity - 0.5).abs() < 0.01);
        assert!(state.frame(now + Duration::from_secs(3), &config).is_none());
    }

    #[test]
    fn translation_completion_updates_matching_headset_item() {
        let now = Instant::now();
        let config = VrOverlayHeadsetConfig {
            content_mode: "bilingual".into(),
            ..Default::default()
        };
        let mut state = HeadsetPresentation::default();
        state.apply(final_event(subtitle(7, "hello")), now, &config);
        state.apply(
            PresentationEvent::TranslationCompleted {
                subtitle_id: 7,
                translation: translation("你好"),
            },
            now + Duration::from_secs(1),
            &config,
        );
        assert_eq!(
            state.frame(now, &config).unwrap().content,
            PresentationContent::Headset("hello\n你好".into())
        );
    }

    #[test]
    fn same_language_translation_is_hidden() {
        let now = Instant::now();
        let mut translation = translation("hello");
        translation.source_language = None;
        translation.target_language = "en".into();

        let headset_config = VrOverlayHeadsetConfig {
            content_mode: "bilingual".into(),
            ..Default::default()
        };
        let mut headset = HeadsetPresentation::default();
        headset.apply(final_event(subtitle(7, "hello")), now, &headset_config);
        headset.apply(
            PresentationEvent::TranslationCompleted {
                subtitle_id: 7,
                translation: translation.clone(),
            },
            now,
            &headset_config,
        );
        assert_eq!(
            headset.frame(now, &headset_config).unwrap().content,
            PresentationContent::Headset("hello".into())
        );

        let wrist_config = VrOverlayWristConfig {
            content_mode: "bilingual".into(),
            ..Default::default()
        };
        let mut item = subtitle(7, "hello");
        item.translations.push(translation);
        let mut wrist = WristPresentation::default();
        wrist.apply(final_event(item), now, &wrist_config);
        assert_eq!(
            wrist.frame(now, &wrist_config).unwrap().content,
            PresentationContent::Wrist(vec![WristMessage {
                text: "hello".into(),
                side: MessageSide::Left,
            }])
        );
    }

    #[test]
    fn wrist_keeps_only_configured_recent_finals() {
        let now = Instant::now();
        let config = VrOverlayWristConfig {
            max_entries: 3,
            ..Default::default()
        };
        let mut state = WristPresentation::default();
        for id in 1..=5 {
            state.apply(
                final_event(subtitle(id, &format!("line{id}"))),
                now,
                &config,
            );
        }
        assert_eq!(
            state.frame(now, &config).unwrap().content,
            PresentationContent::Wrist(vec![
                WristMessage {
                    text: "line3".into(),
                    side: MessageSide::Left,
                },
                WristMessage {
                    text: "line4".into(),
                    side: MessageSide::Left,
                },
                WristMessage {
                    text: "line5".into(),
                    side: MessageSide::Left,
                },
            ])
        );
    }

    #[test]
    fn wrist_trims_existing_history_when_limit_changes() {
        let now = Instant::now();
        let mut config = VrOverlayWristConfig {
            max_entries: 5,
            ..Default::default()
        };
        let mut state = WristPresentation::default();
        for id in 1..=5 {
            state.apply(
                final_event(subtitle(id, &format!("line{id}"))),
                now,
                &config,
            );
        }

        config.max_entries = 3;
        state.set_max_entries(config.max_entries);

        let PresentationContent::Wrist(messages) = state.frame(now, &config).unwrap().content
        else {
            panic!("expected wrist messages");
        };
        assert_eq!(
            messages
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            vec!["line3", "line4", "line5"]
        );
    }

    #[test]
    fn wrist_places_remote_left_and_local_sources_right() {
        let now = Instant::now();
        let config = VrOverlayWristConfig {
            include_microphone: true,
            include_chatbox: true,
            ..Default::default()
        };
        let mut state = WristPresentation::default();

        for (id, source) in [(1, "speaker"), (2, "microphone"), (3, "chatbox")] {
            let mut item = subtitle(id, source);
            item.source = source.into();
            state.apply(final_event(item), now, &config);
        }

        let PresentationContent::Wrist(messages) = state.frame(now, &config).unwrap().content
        else {
            panic!("expected wrist messages");
        };
        assert_eq!(messages[0].side, MessageSide::Left);
        assert_eq!(messages[1].side, MessageSide::Right);
        assert_eq!(messages[2].side, MessageSide::Right);
    }

    #[test]
    fn recognition_partials_are_visible_only_when_enabled() {
        let now = Instant::now();
        let mut headset_config = VrOverlayHeadsetConfig::default();
        let mut headset = HeadsetPresentation::default();
        headset.apply(partial_event("u1", "speaker", "hel"), now, &headset_config);
        assert!(headset.frame(now, &headset_config).is_none());

        headset_config.show_partials = true;
        headset.apply(
            partial_event("u1", "speaker", "hello"),
            now,
            &headset_config,
        );
        assert_eq!(
            headset.frame(now, &headset_config).unwrap().content,
            PresentationContent::Headset("hello".into())
        );

        let wrist_config = VrOverlayWristConfig {
            show_partials: true,
            ..Default::default()
        };
        let mut wrist = WristPresentation::default();
        wrist.apply(partial_event("u1", "speaker", "hello"), now, &wrist_config);
        assert_eq!(
            wrist.frame(now, &wrist_config).unwrap().content,
            PresentationContent::Wrist(vec![WristMessage {
                text: "hello".into(),
                side: MessageSide::Left,
            }])
        );
    }

    #[test]
    fn headset_final_replaces_partial_and_blocks_late_updates() {
        let now = Instant::now();
        let config = VrOverlayHeadsetConfig {
            show_partials: true,
            ..Default::default()
        };
        let mut state = HeadsetPresentation::default();
        state.apply(partial_event("u1", "speaker", "partial"), now, &config);
        state.apply(
            PresentationEvent::Final {
                utterance_id: Some("u1".into()),
                subtitle: subtitle(1, "final"),
            },
            now,
            &config,
        );
        state.apply(partial_event("u1", "speaker", "late partial"), now, &config);

        assert_eq!(
            state.frame(now, &config).unwrap().content,
            PresentationContent::Headset("final".into())
        );
    }

    #[test]
    fn disabling_partials_discards_hidden_state() {
        let now = Instant::now();
        let mut config = VrOverlayWristConfig {
            show_partials: true,
            ..Default::default()
        };
        let mut state = WristPresentation::default();
        state.apply(partial_event("u1", "speaker", "partial"), now, &config);

        state.set_show_partials(false);
        config.show_partials = true;

        assert!(state.frame(now, &config).is_none());
    }

    #[test]
    fn final_replaces_matching_wrist_partial_without_clearing_newer_utterance() {
        let now = Instant::now();
        let config = VrOverlayWristConfig {
            show_partials: true,
            include_microphone: true,
            ..Default::default()
        };
        let mut state = WristPresentation::default();
        state.apply(partial_event("u1", "speaker", "old partial"), now, &config);
        state.apply(
            partial_event("u2", "microphone", "new partial"),
            now,
            &config,
        );

        let mut final_subtitle = subtitle(1, "old final");
        final_subtitle.source = "speaker".into();
        state.apply(
            PresentationEvent::Final {
                utterance_id: Some("u1".into()),
                subtitle: final_subtitle,
            },
            now,
            &config,
        );
        state.apply(partial_event("u1", "speaker", "late partial"), now, &config);

        let PresentationContent::Wrist(messages) = state.frame(now, &config).unwrap().content
        else {
            panic!("expected wrist messages");
        };
        assert_eq!(
            messages
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            vec!["old final", "new partial"]
        );
    }

    #[test]
    fn terminated_wrist_partial_cannot_reappear() {
        let now = Instant::now();
        let config = VrOverlayWristConfig {
            show_partials: true,
            ..Default::default()
        };
        let mut state = WristPresentation::default();
        state.apply(partial_event("u1", "speaker", "partial"), now, &config);
        state.apply(
            PresentationEvent::RecognitionCancelled {
                utterance_id: "u1".into(),
                source: "speaker".into(),
            },
            now,
            &config,
        );
        state.apply(partial_event("u1", "speaker", "late partial"), now, &config);
        assert!(state.frame(now, &config).is_none());
    }

    #[test]
    fn recognition_reset_allows_reused_utterance_ids() {
        let now = Instant::now();
        let config = VrOverlayWristConfig {
            show_partials: true,
            ..Default::default()
        };
        let mut state = WristPresentation::default();
        state.apply(
            PresentationEvent::RecognitionCancelled {
                utterance_id: "u1".into(),
                source: "speaker".into(),
            },
            now,
            &config,
        );
        state.apply(
            PresentationEvent::RecognitionReset {
                source: "speaker".into(),
            },
            now,
            &config,
        );
        state.apply(partial_event("u1", "speaker", "new session"), now, &config);

        assert_eq!(
            state.frame(now, &config).unwrap().content,
            PresentationContent::Wrist(vec![WristMessage {
                text: "new session".into(),
                side: MessageSide::Left,
            }])
        );
    }

    #[test]
    fn recognition_reset_clears_only_matching_wrist_source() {
        let now = Instant::now();
        let config = VrOverlayWristConfig {
            show_partials: true,
            include_microphone: true,
            ..Default::default()
        };
        let mut state = WristPresentation::default();
        state.apply(partial_event("u1", "speaker", "remote"), now, &config);
        state.apply(partial_event("u2", "microphone", "local"), now, &config);
        state.apply(
            PresentationEvent::RecognitionReset {
                source: "speaker".into(),
            },
            now,
            &config,
        );

        assert_eq!(
            state.frame(now, &config).unwrap().content,
            PresentationContent::Wrist(vec![WristMessage {
                text: "local".into(),
                side: MessageSide::Right,
            }])
        );
    }

    #[test]
    fn disabled_or_unknown_sources_are_not_presented() {
        let now = Instant::now();
        let config = VrOverlayHeadsetConfig::default();
        let mut state = HeadsetPresentation::default();
        let mut item = subtitle(1, "private");
        item.source = "microphone".into();
        state.apply(final_event(item), now, &config);
        assert!(state.frame(now, &config).is_none());
    }
}
