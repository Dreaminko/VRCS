use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, RwLock};

use tokio::sync::watch;

use crate::config::{AppConfig, AsrConfig};
use crate::glossary::GlossaryStore;
use crate::models::SpeakerIdentity;
use crate::providers;
use crate::room_context::{
    RoomContextAvailability, RoomContextReader, RoomContextSnapshot, sanitize_display_name,
};
use crate::vrcx::VrcxIntegration;

const MAX_ASR_ROOM_TERMS: usize = 24;
const MAX_QWEN_LIVE_PHRASES: usize = 500;

#[derive(Clone, Default)]
pub(crate) struct AsrEchoGuard {
    signatures: Vec<String>,
}

impl AsrEchoGuard {
    pub(crate) fn new(signatures: Vec<String>) -> Self {
        let mut signatures = signatures
            .into_iter()
            .map(|text| normalize_echo_text(&text))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>();
        signatures.sort_unstable();
        signatures.dedup();
        Self { signatures }
    }

    pub(crate) fn is_echo(&self, text: &str) -> bool {
        let text = normalize_echo_text(text);
        !text.is_empty() && self.signatures.iter().any(|signature| signature == &text)
    }

    pub(crate) fn suppresses_partial(&self, text: &str) -> bool {
        let text = normalize_echo_text(text);
        text.contains(':')
            && self
                .signatures
                .iter()
                .any(|signature| signature.starts_with(&text))
    }
}

fn normalize_echo_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', '。', '!', '！', '?', '？'])
        .to_lowercase()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AsrContextCapability {
    PromptText,
    LivePhrases,
    Unsupported,
}

pub(crate) fn capability_for_backend(backend: &str) -> AsrContextCapability {
    if backend == providers::SERVICE_QWEN_LIVE_TRANSLATE {
        return AsrContextCapability::LivePhrases;
    }
    if providers::recognition_service(backend)
        .and_then(|(_, service)| service.context_max_chars)
        .is_some()
    {
        return AsrContextCapability::PromptText;
    }
    AsrContextCapability::Unsupported
}

#[derive(Clone)]
pub(crate) struct PreparedAsrContext {
    pub fingerprint: u64,
    pub scope_id: Option<String>,
    pub asr_config: AsrConfig,
    pub echo_guard: AsrEchoGuard,
}

#[derive(Clone)]
pub(crate) struct AsrContextSource {
    config: Arc<RwLock<AppConfig>>,
    room_context: RoomContextReader,
    glossary: Arc<GlossaryStore>,
    vrcx: VrcxIntegration,
    speaker: Option<SpeakerIdentity>,
}

impl AsrContextSource {
    pub(crate) fn new(
        config: Arc<RwLock<AppConfig>>,
        room_context: RoomContextReader,
        glossary: Arc<GlossaryStore>,
        vrcx: VrcxIntegration,
    ) -> Self {
        Self {
            config,
            room_context,
            glossary,
            vrcx,
            speaker: None,
        }
    }

    pub(crate) fn for_player(&self, speaker: SpeakerIdentity) -> Self {
        let mut next = self.clone();
        next.speaker = Some(speaker);
        next
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<RoomContextSnapshot> {
        self.room_context.subscribe()
    }

    pub(crate) fn prepare(&self, base_asr: &AsrConfig) -> PreparedAsrContext {
        let (vrchat_context, glossary_cfg, vrcx_active) = {
            let config = self.config.read().expect("config lock");
            (
                config.vrchat_context.clone(),
                config.glossary.clone(),
                config.features.vrcx && config.vrcx.enabled && config.vrcx.include_in_asr_context,
            )
        };
        let snapshot = self.room_context.snapshot();
        let mut asr = base_asr.clone();

        let capability = capability_for_backend(&asr.backend);
        if capability == AsrContextCapability::LivePhrases {
            asr.live_translation_phrases =
                self.glossary.phrases_for_live_translation(&glossary_cfg);
        } else if capability == AsrContextCapability::PromptText {
            let glossary_terms = self.glossary.terms_for_asr(&glossary_cfg).join("\n");
            crate::glossary::append_asr_context(&mut asr, &glossary_terms);
        }

        let native_active = vrchat_context.include_in_asr_context
            && snapshot.availability != RoomContextAvailability::Unavailable;
        let mut signatures = Vec::new();
        if native_active {
            match capability {
                AsrContextCapability::PromptText => {
                    let terms = self.prioritized_prompt_terms(
                        &snapshot,
                        vrchat_context.include_world_name,
                        MAX_ASR_ROOM_TERMS,
                    );
                    if !terms.is_empty() {
                        let appended =
                            crate::glossary::append_asr_context(&mut asr, &terms.join("\n"));
                        if !appended.is_empty() {
                            signatures.extend(crate::vrcx::asr_echo_signatures(&appended));
                        }
                    }
                }
                AsrContextCapability::LivePhrases => {
                    let player_names = self.prioritized_player_names(&snapshot, MAX_ASR_ROOM_TERMS);
                    let mut added = Vec::new();
                    for name in player_names {
                        if asr.live_translation_phrases.len() >= MAX_QWEN_LIVE_PHRASES {
                            break;
                        }
                        if !asr.live_translation_phrases.contains_key(&name) {
                            asr.live_translation_phrases
                                .insert(name.clone(), name.clone());
                            added.push(name);
                        }
                    }
                    if !added.is_empty() {
                        signatures.extend(crate::vrcx::asr_echo_signatures(&added.join("\n")));
                    }
                }
                AsrContextCapability::Unsupported => {}
            }
        } else if vrcx_active {
            signatures = self.vrcx.apply_asr_context(&mut asr);
        }

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        capability.hash(&mut hasher);
        signatures.hash(&mut hasher);
        asr.service_settings
            .get(&asr.backend)
            .map(|settings| settings.context.as_str())
            .hash(&mut hasher);
        asr.live_translation_phrases.hash(&mut hasher);

        PreparedAsrContext {
            fingerprint: hasher.finish(),
            scope_id: snapshot.scope_id,
            asr_config: asr,
            echo_guard: AsrEchoGuard::new(signatures),
        }
    }

    fn prioritized_prompt_terms(
        &self,
        snapshot: &RoomContextSnapshot,
        include_world_name: bool,
        max_terms: usize,
    ) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut terms = Vec::new();
        if let Some(name) = self
            .speaker
            .as_ref()
            .and_then(|speaker| speaker.display_name.as_deref())
            .and_then(sanitize_display_name)
            && seen.insert(name.to_lowercase())
            && terms.len() < max_terms
        {
            terms.push(name);
        }
        for term in snapshot.asr_terms(include_world_name, max_terms) {
            if terms.len() >= max_terms {
                break;
            }
            if seen.insert(term.to_lowercase()) {
                terms.push(term);
            }
        }
        terms
    }

    fn prioritized_player_names(
        &self,
        snapshot: &RoomContextSnapshot,
        max_names: usize,
    ) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut names = Vec::new();
        if let Some(name) = self
            .speaker
            .as_ref()
            .and_then(|speaker| speaker.display_name.as_deref())
            .and_then(sanitize_display_name)
            && seen.insert(name.to_lowercase())
            && names.len() < max_names
        {
            names.push(name);
        }
        for name in snapshot.member_names(max_names) {
            if names.len() >= max_names {
                break;
            }
            if seen.insert(name.to_lowercase()) {
                names.push(name);
            }
        }
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GlossaryCategory, GlossaryEntry, GlossarySource, QWEN_MANAGED_BACKEND};
    use crate::player_audio::PlayerInfo;
    use crate::player_audio::session::NetworkConsumers;
    use crate::room_context::RoomContextStore;
    use crate::vrchat_log;
    use crate::vrchat_process::ProcessIdentity;

    fn test_source(
        include_asr: bool,
        include_world: bool,
        store: &RoomContextStore,
    ) -> (AsrContextSource, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let glossary = Arc::new(
            GlossaryStore::new(
                dir.path().join("glossary.json"),
                crate::config::GlossaryConfig {
                    sources: vec![GlossarySource::Local {
                        id: "local".into(),
                        name: "Local".into(),
                        enabled: true,
                        entries: vec![GlossaryEntry {
                            source: "Alice".into(),
                            target: Some("アリス".into()),
                            category: GlossaryCategory::Person,
                            case_sensitive: false,
                        }],
                    }],
                    ..Default::default()
                },
            )
            .unwrap(),
        );
        let mut config = AppConfig::default();
        config.vrchat_context.include_in_asr_context = include_asr;
        config.vrchat_context.include_world_name = include_world;
        config.glossary.sources = vec![GlossarySource::Local {
            id: "local".into(),
            name: "Local".into(),
            enabled: true,
            entries: vec![GlossaryEntry {
                source: "Alice".into(),
                target: Some("アリス".into()),
                category: GlossaryCategory::Person,
                case_sensitive: false,
            }],
        }];
        let (_, shutdown_rx) = watch::channel(false);
        (
            AsrContextSource::new(
                Arc::new(RwLock::new(config)),
                store.reader(),
                glossary,
                VrcxIntegration::new(shutdown_rx),
            ),
            dir,
        )
    }

    fn populate_room(store: &RoomContextStore) {
        store.sync_log(&vrchat_log::Snapshot {
            process: Some(ProcessIdentity {
                pid: 7,
                created: 11,
            }),
            ready: true,
            room_token: Some(1),
            world_session_token: Some(1),
            world_name: Some("Starlight Bar".into()),
            error: None,
        });
        let publisher = store.network_publisher();
        publisher.set_consumers(NetworkConsumers {
            voice: true,
            chatbox: false,
        });
        publisher.set_stream_ready(true);
        publisher.replace_members(&[PlayerInfo {
            speaker: SpeakerIdentity {
                id: "track-bob".into(),
                index: 1,
                display_name: Some("Bob".into()),
                user_id: Some("usr_b".into()),
            },
            selected: true,
            active: true,
            distance_m: Some(1.0),
        }]);
    }

    #[test]
    fn prompt_text_injects_player_priority_world_setting_and_echo_guard() {
        let store = RoomContextStore::default();
        populate_room(&store);
        let (base_source, _dir) = test_source(true, false, &store);
        let source = base_source.for_player(SpeakerIdentity {
            id: "track-charlie".into(),
            index: 2,
            display_name: Some("Charlie".into()),
            user_id: Some("usr_c".into()),
        });
        let asr = AsrConfig {
            backend: providers::SERVICE_FUN_ASR_REALTIME.into(),
            ..Default::default()
        };

        let prepared = source.prepare(&asr);
        let ctx =
            &prepared.asr_config.service_settings[providers::SERVICE_FUN_ASR_REALTIME].context;
        assert!(ctx.contains("Charlie"));
        assert!(ctx.contains("Bob"));
        assert!(!ctx.contains("Starlight Bar"));
        assert!(prepared.echo_guard.is_echo("Names: Charlie, Bob"));
    }

    #[test]
    fn qwen_live_translate_receives_player_names_only_and_keeps_glossary_priority() {
        let store = RoomContextStore::default();
        populate_room(&store);
        let (source, _dir) = test_source(true, true, &store);
        let asr = AsrConfig {
            backend: providers::SERVICE_QWEN_LIVE_TRANSLATE.into(),
            ..Default::default()
        };

        let prepared = source.prepare(&asr);
        assert_eq!(
            prepared.asr_config.live_translation_phrases["Alice"],
            "アリス"
        );
        assert_eq!(prepared.asr_config.live_translation_phrases["Bob"], "Bob");
        assert!(
            !prepared
                .asr_config
                .live_translation_phrases
                .contains_key("Starlight Bar")
        );
    }

    #[test]
    fn managed_qwen_stays_unsupported_for_room_context() {
        let store = RoomContextStore::default();
        populate_room(&store);
        let (source, _dir) = test_source(true, true, &store);
        let asr = AsrConfig {
            backend: QWEN_MANAGED_BACKEND.into(),
            ..Default::default()
        };
        let prepared = source.prepare(&asr);
        assert_eq!(prepared.asr_config, asr);
    }

    #[test]
    fn vrcx_echo_guard_blocks_generated_context_without_blocking_real_mentions() {
        let guard = AsrEchoGuard::new(vec!["Names: Alice, Bob".into(), "Alice\nBob".into()]);

        assert!(guard.is_echo("Names: Alice, Bob."));
        assert!(guard.is_echo("names: alice, bob"));
        assert!(guard.is_echo("Alice\nBob"));
        assert!(guard.suppresses_partial("Names: Al"));
        assert!(!guard.is_echo("Alice"));
        assert!(!guard.is_echo("Hi Alice"));
        assert!(!guard.is_echo("Club_GoH2.0"));
        assert!(!guard.is_echo("We are in Club_GoH2.0"));
        assert!(!guard.is_echo("Club_GoH2.0 is a nice world"));
        assert!(!guard.is_echo("Club_GoH2.0 Club_GoH2.0"));
        assert!(!guard.suppresses_partial("Club_GoH2.0"));
    }
}
