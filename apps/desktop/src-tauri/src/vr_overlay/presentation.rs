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
    translations: Vec<PresentationTranslation>,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
struct PresentationTranslation {
    target_language: String,
    text: String,
    preferred: bool,
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
            PresentationEvent::TranslationPartial {
                subtitle_id,
                text,
                target_language,
                preferred,
            } if config.show_translation_partials => {
                if let Some(item) = self
                    .current
                    .as_mut()
                    .filter(|item| item.subtitle_id == Some(subtitle_id))
                {
                    update_translation(item, target_language, text, preferred);
                }
            }
            PresentationEvent::TranslationCompleted {
                subtitle_id,
                translation,
                preferred,
            } => {
                if let Some(item) = self
                    .current
                    .as_mut()
                    .filter(|item| item.subtitle_id == Some(subtitle_id))
                {
                    if update_completed_translation(item, translation, preferred) {
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

    #[cfg(test)]
    pub fn frame(
        &self,
        now: Instant,
        config: &VrOverlayHeadsetConfig,
    ) -> Option<PresentationFrame> {
        self.frame_with_translation_display(now, config, "all_languages")
    }

    pub fn frame_with_translation_display(
        &self,
        now: Instant,
        config: &VrOverlayHeadsetConfig,
        translation_display: &str,
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
            display_text(item, &config.content_mode, translation_display, "\n"),
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
            PresentationEvent::TranslationPartial {
                subtitle_id,
                text,
                target_language,
                preferred,
            } if config.show_translation_partials => {
                if let Some(item) = self
                    .entries
                    .iter_mut()
                    .find(|item| item.subtitle_id == Some(subtitle_id))
                {
                    if update_translation(item, target_language, text, preferred) {
                        self.last_activity = Some(now);
                    }
                }
            }
            PresentationEvent::TranslationCompleted {
                subtitle_id,
                translation,
                preferred,
            } => {
                if let Some(item) = self
                    .entries
                    .iter_mut()
                    .find(|item| item.subtitle_id == Some(subtitle_id))
                {
                    if update_completed_translation(item, translation, preferred) {
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

    #[cfg(test)]
    pub fn frame(&self, now: Instant, config: &VrOverlayWristConfig) -> Option<PresentationFrame> {
        self.frame_with_translation_display(now, config, "all_languages")
    }

    pub fn frame_with_translation_display(
        &self,
        now: Instant,
        config: &VrOverlayWristConfig,
        translation_display: &str,
    ) -> Option<PresentationFrame> {
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
            .map(|item| wrist_message(item, &config.content_mode, translation_display))
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
    let translations = subtitle
        .translations
        .iter()
        .enumerate()
        .filter_map(|(index, translation)| {
            visible_translation(subtitle.language.as_deref(), translation).map(|text| {
                PresentationTranslation {
                    target_language: translation.target_language.clone(),
                    text,
                    preferred: index == 0,
                }
            })
        })
        .collect();
    PresentationItem {
        utterance_id,
        subtitle_id: subtitle.id,
        source: subtitle.source,
        language: subtitle.language,
        original: subtitle.text,
        translations,
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
        translations: Vec::new(),
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

fn update_completed_translation(
    item: &mut PresentationItem,
    translation: SubtitleTranslation,
    preferred: bool,
) -> bool {
    let target_language = translation.target_language.clone();
    let Some(text) = visible_translation(item.language.as_deref(), &translation) else {
        item.translations
            .retain(|current| current.target_language != target_language);
        return false;
    };
    update_translation(item, target_language, text, preferred)
}

fn update_translation(
    item: &mut PresentationItem,
    target_language: String,
    text: String,
    preferred: bool,
) -> bool {
    if text.trim().is_empty()
        || item
            .language
            .as_deref()
            .is_some_and(|source| same_translation_language(source, &target_language))
    {
        return false;
    }
    if preferred {
        for translation in &mut item.translations {
            translation.preferred = false;
        }
    }
    if let Some(translation) = item
        .translations
        .iter_mut()
        .find(|translation| translation.target_language == target_language)
    {
        translation.text = text;
        translation.preferred |= preferred;
    } else {
        item.translations.push(PresentationTranslation {
            target_language,
            text,
            preferred,
        });
    }
    item.translations
        .sort_by_key(|translation| !translation.preferred);
    true
}

fn wrist_message(
    item: &PresentationItem,
    content_mode: &str,
    translation_display: &str,
) -> WristMessage {
    WristMessage {
        text: display_text(item, content_mode, translation_display, "\n"),
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

fn display_text(
    item: &PresentationItem,
    mode: &str,
    translation_display: &str,
    separator: &str,
) -> String {
    let translations = if translation_display == "preferred_only" {
        item.translations
            .iter()
            .find(|translation| translation.preferred)
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        item.translations.iter().collect()
    };
    let translation = (!translations.is_empty()).then(|| {
        translations
            .iter()
            .map(|translation| translation.text.as_str())
            .collect::<Vec<_>>()
            .join(separator)
    });
    match mode {
        "translation" => translation.unwrap_or_else(|| item.original.clone()),
        "bilingual" => translation
            .map(|text| format!("{}{separator}{text}", item.original))
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
mod tests;
