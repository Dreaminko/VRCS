use std::collections::VecDeque;
use std::time::{Duration, Instant};

use vrcs_core::{PresentationEvent, Subtitle, VrOverlayHeadsetConfig, VrOverlayWristConfig};

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
    subtitle_id: Option<i64>,
    source: String,
    original: String,
    translation: Option<String>,
    expires_at: Instant,
}

#[derive(Default)]
pub struct HeadsetPresentation {
    current: Option<PresentationItem>,
}

impl HeadsetPresentation {
    pub fn apply(
        &mut self,
        event: PresentationEvent,
        now: Instant,
        config: &VrOverlayHeadsetConfig,
    ) {
        match event {
            PresentationEvent::Final { subtitle } if source_enabled(&subtitle.source, config) => {
                self.current = Some(item_from_subtitle(
                    subtitle,
                    expiry(now, config.display_seconds),
                ));
            }
            PresentationEvent::TranslationPartial {
                subtitle_id, text, ..
            } if config.show_translation_partials => {
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
                ..
            } => {
                if let Some(item) = self
                    .current
                    .as_mut()
                    .filter(|item| item.subtitle_id == Some(subtitle_id))
                {
                    item.translation = Some(translation.text);
                    item.expires_at = expiry(now, config.display_seconds);
                }
            }
            _ => {}
        }
    }

    pub fn frame(
        &self,
        now: Instant,
        config: &VrOverlayHeadsetConfig,
    ) -> Option<PresentationFrame> {
        let item = self.current.as_ref()?;
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
    last_activity: Option<Instant>,
}

impl WristPresentation {
    pub fn apply(&mut self, event: PresentationEvent, now: Instant, config: &VrOverlayWristConfig) {
        match event {
            PresentationEvent::Final { subtitle } if source_enabled(&subtitle.source, config) => {
                self.entries.push_back(item_from_subtitle(subtitle, now));
                self.trim(config.max_entries);
                self.last_activity = Some(now);
            }
            PresentationEvent::TranslationPartial {
                subtitle_id, text, ..
            } if config.show_translation_partials => {
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
                ..
            } => {
                if let Some(item) = self
                    .entries
                    .iter_mut()
                    .find(|item| item.subtitle_id == Some(subtitle_id))
                {
                    item.translation = Some(translation.text);
                    self.last_activity = Some(now);
                }
            }
            _ => {}
        }
    }

    pub fn set_max_entries(&mut self, max_entries: u32) {
        self.trim(max_entries);
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

        let messages: Vec<WristMessage> = self
            .entries
            .iter()
            .filter(|item| source_enabled(&item.source, config))
            .map(|item| wrist_message(item, &config.content_mode))
            .collect();
        (!messages.is_empty()).then(|| PresentationFrame::wrist(messages))
    }

    fn trim(&mut self, max_entries: u32) {
        let limit = max_entries.clamp(3, 10) as usize;
        while self.entries.len() > limit {
            self.entries.pop_front();
        }
    }
}

fn item_from_subtitle(subtitle: Subtitle, expires_at: Instant) -> PresentationItem {
    let translation = subtitle
        .translations
        .last()
        .map(|translation| translation.text.clone());
    PresentationItem {
        subtitle_id: subtitle.id,
        source: subtitle.source,
        original: subtitle.text,
        translation,
        expires_at,
    }
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
        let mut config = VrOverlayHeadsetConfig::default();
        config.display_seconds = 2.0;
        config.fade_seconds = 1.0;
        let mut state = HeadsetPresentation::default();
        state.apply(
            PresentationEvent::Final {
                subtitle: subtitle(7, "hello"),
            },
            now,
            &config,
        );
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
        let mut config = VrOverlayHeadsetConfig::default();
        config.content_mode = "bilingual".into();
        let mut state = HeadsetPresentation::default();
        state.apply(
            PresentationEvent::Final {
                subtitle: subtitle(7, "hello"),
            },
            now,
            &config,
        );
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
    fn wrist_keeps_only_configured_recent_finals() {
        let now = Instant::now();
        let mut config = VrOverlayWristConfig::default();
        config.max_entries = 3;
        let mut state = WristPresentation::default();
        for id in 1..=5 {
            state.apply(
                PresentationEvent::Final {
                    subtitle: subtitle(id, &format!("line{id}")),
                },
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
        let mut config = VrOverlayWristConfig::default();
        config.max_entries = 5;
        let mut state = WristPresentation::default();
        for id in 1..=5 {
            state.apply(
                PresentationEvent::Final {
                    subtitle: subtitle(id, &format!("line{id}")),
                },
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
        let mut config = VrOverlayWristConfig::default();
        config.include_microphone = true;
        config.include_chatbox = true;
        let mut state = WristPresentation::default();

        for (id, source) in [(1, "speaker"), (2, "microphone"), (3, "chatbox")] {
            let mut item = subtitle(id, source);
            item.source = source.into();
            state.apply(PresentationEvent::Final { subtitle: item }, now, &config);
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
    fn disabled_or_unknown_sources_are_not_presented() {
        let now = Instant::now();
        let config = VrOverlayHeadsetConfig::default();
        let mut state = HeadsetPresentation::default();
        let mut item = subtitle(1, "private");
        item.source = "microphone".into();
        state.apply(PresentationEvent::Final { subtitle: item }, now, &config);
        assert!(state.frame(now, &config).is_none());
    }
}
