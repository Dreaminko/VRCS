use rusqlite::params;

use super::Database;
use crate::config::TranslationPromptConfig;
use crate::error::AppResult;
use crate::translation::TranslationContextEntry;

impl Database {
    pub fn recent_translation_context(
        &self,
        config: &TranslationPromptConfig,
        exclude_subtitle_id: Option<i64>,
    ) -> AppResult<Vec<TranslationContextEntry>> {
        self.recent_translation_context_scoped(config, exclude_subtitle_id, None)
    }

    pub(crate) fn recent_translation_context_scoped(
        &self,
        config: &TranslationPromptConfig,
        exclude_subtitle_id: Option<i64>,
        scope_id: Option<&str>,
    ) -> AppResult<Vec<TranslationContextEntry>> {
        if !config.context_enabled {
            return Ok(Vec::new());
        }
        let map_row = |row: &rusqlite::Row<'_>| {
            let speaker_json: Option<String> = row.get(3)?;
            let speaker = speaker_json.as_deref().and_then(|raw| {
                serde_json::from_str(raw)
                    .map_err(|error| {
                        tracing::warn!(%error, "Malformed speaker JSON in translation context");
                        error
                    })
                    .ok()
            });
            Ok(TranslationContextEntry {
                source: row.get(0)?,
                text: row.get(1)?,
                created_at: row.get(2)?,
                speaker,
            })
        };
        let mut entries = if let Some(scope_id) = scope_id {
            let mut statement = self.conn.prepare_cached(
                "SELECT source, text, created_at, speaker FROM (
                     SELECT * FROM (
                         SELECT source, text, created_at, speaker, 0 AS source_order, id
                         FROM subtitles
                         WHERE context_scope_id = ?6
                           AND source IN ('speaker', 'microphone', 'vrchat_chatbox', 'chatbox')
                           AND (
                               (source IN ('speaker', 'vrchat_chatbox') AND ?1)
                               OR (source = 'microphone' AND ?2)
                               OR (source = 'chatbox' AND ?3 AND NOT EXISTS (
                                   SELECT 1 FROM chatbox_messages cm
                                   WHERE cm.created_at = subtitles.created_at AND cm.status = 'sent'
                               ))
                           )
                           AND (?4 IS NULL OR id != ?4)
                         ORDER BY created_at DESC, id DESC
                         LIMIT ?5
                     )
                     UNION ALL
                     SELECT * FROM (
                         SELECT 'chatbox' AS source, original AS text, created_at, NULL AS speaker, 1 AS source_order, id
                         FROM chatbox_messages
                         WHERE context_scope_id = ?6
                           AND ?3 AND status = 'sent'
                         ORDER BY created_at DESC, id DESC
                         LIMIT ?5
                     )
                 )
                 ORDER BY created_at DESC, source_order DESC, id DESC
                 LIMIT ?5",
            )?;
            let rows = statement.query_map(
                params![
                    config.include_speaker,
                    config.include_microphone,
                    config.include_chatbox,
                    exclude_subtitle_id,
                    config.max_messages,
                    scope_id,
                ],
                map_row,
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        } else {
            let mut statement = self.conn.prepare_cached(
                "SELECT source, text, created_at, speaker FROM (
                     SELECT * FROM (
                         SELECT source, text, created_at, speaker, 0 AS source_order, id
                         FROM subtitles
                         WHERE source IN ('speaker', 'microphone', 'vrchat_chatbox', 'chatbox')
                           AND (
                               (source IN ('speaker', 'vrchat_chatbox') AND ?1)
                               OR (source = 'microphone' AND ?2)
                               OR (source = 'chatbox' AND ?3 AND NOT EXISTS (
                                   SELECT 1 FROM chatbox_messages cm
                                   WHERE cm.created_at = subtitles.created_at AND cm.status = 'sent'
                               ))
                           )
                           AND (?4 IS NULL OR id != ?4)
                         ORDER BY created_at DESC, id DESC
                         LIMIT ?5
                     )
                     UNION ALL
                     SELECT * FROM (
                         SELECT 'chatbox' AS source, original AS text, created_at, NULL AS speaker, 1 AS source_order, id
                         FROM chatbox_messages
                         WHERE ?3 AND status = 'sent'
                         ORDER BY created_at DESC, id DESC
                         LIMIT ?5
                     )
                 )
                 ORDER BY created_at DESC, source_order DESC, id DESC
                 LIMIT ?5",
            )?;
            let rows = statement.query_map(
                params![
                    config.include_speaker,
                    config.include_microphone,
                    config.include_chatbox,
                    exclude_subtitle_id,
                    config.max_messages,
                ],
                map_row,
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        entries.reverse();
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Subtitle;

    #[test]
    fn remote_chatbox_is_other_player_context_independent_of_local_sends() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("remote-context.db")).unwrap();
        let mut current = None;
        for text in ["remote history", "current remote text"] {
            current = database
                .add_subtitle(&Subtitle {
                    id: None,
                    speaker: None,
                    conversation_id: None,
                    text: text.into(),
                    language: None,
                    started_at: None,
                    ended_at: None,
                    source: "vrchat_chatbox".into(),
                    created_at: "2026-11-04T00:00:00Z".into(),
                    translations: Vec::new(),
                })
                .unwrap()
                .id;
        }
        let mut config = TranslationPromptConfig {
            context_enabled: true,
            include_speaker: true,
            include_microphone: false,
            include_chatbox: false,
            ..Default::default()
        };
        let entries = database
            .recent_translation_context(&config, current)
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, "vrchat_chatbox");
        assert_eq!(entries[0].text, "remote history");
        config.include_speaker = false;
        config.include_chatbox = true;
        assert!(
            database
                .recent_translation_context(&config, None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn context_filters_sources_orders_messages_and_excludes_current_subtitle() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("context.db")).unwrap();
        let add = |text: &str, source: &str, created_at: &str| {
            database
                .add_subtitle(&Subtitle {
                    speaker: None,
                    id: None,
                    conversation_id: None,
                    text: text.into(),
                    language: None,
                    started_at: None,
                    ended_at: None,
                    source: source.into(),
                    created_at: created_at.into(),
                    translations: Vec::new(),
                })
                .unwrap()
        };
        add("speaker old", "speaker", "2026-08-14T00:00:01Z");
        add("microphone", "microphone", "2026-08-14T00:00:02Z");
        add("chatbox translation", "chatbox", "2026-08-14T00:00:03Z");
        database
            .conn
            .execute(
                "INSERT INTO chatbox_messages(source, original, translation, source_language,
                    target_language, send_mode, message_format, rendered_text, char_count,
                    truncated, status, created_at)
                 VALUES ('manual', 'chatbox original', 'chatbox translation', 'en', 'ja',
                    'translation', 'slash_separated', 'chatbox translation', 19, 0, 'sent',
                    '2026-08-14T00:00:03Z')",
                [],
            )
            .unwrap();
        let current = add("current", "speaker", "2026-08-14T00:00:04Z");
        let config = TranslationPromptConfig {
            context_enabled: true,
            max_messages: 3,
            ..TranslationPromptConfig::default()
        };

        let entries = database
            .recent_translation_context(&config, current.id)
            .unwrap();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].text, "speaker old");
        assert_eq!(entries[1].source, "microphone");
        assert_eq!(entries[2].text, "chatbox original");
        assert!(
            entries
                .iter()
                .all(|entry| entry.text != "chatbox translation")
        );
        assert!(entries.iter().all(|entry| entry.text != "current"));
    }

    #[test]
    fn disabled_recent_context_returns_no_history() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("disabled-context.db")).unwrap();
        database
            .add_subtitle(&Subtitle {
                speaker: None,
                id: None,
                conversation_id: None,
                text: "history".into(),
                language: None,
                started_at: None,
                ended_at: None,
                source: "speaker".into(),
                created_at: "2026-08-14T00:00:01Z".into(),
                translations: Vec::new(),
            })
            .unwrap();

        let config = TranslationPromptConfig {
            context_enabled: false,
            ..Default::default()
        };
        let entries = database.recent_translation_context(&config, None).unwrap();

        assert!(entries.is_empty());
    }

    #[test]
    fn context_respects_independent_source_switches_and_message_limit() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("sources.db")).unwrap();
        for index in 0..3 {
            database
                .add_subtitle(&Subtitle {
                    speaker: None,
                    id: None,
                    conversation_id: None,
                    text: format!("speaker {index}"),
                    language: None,
                    started_at: None,
                    ended_at: None,
                    source: "speaker".into(),
                    created_at: format!("2026-08-14T00:00:0{index}Z"),
                    translations: Vec::new(),
                })
                .unwrap();
        }
        let config = TranslationPromptConfig {
            context_enabled: true,
            include_speaker: true,
            include_microphone: false,
            include_chatbox: false,
            max_messages: 2,
            ..TranslationPromptConfig::default()
        };

        let entries = database.recent_translation_context(&config, None).unwrap();

        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>(),
            ["speaker 1", "speaker 2"]
        );
    }

    #[test]
    fn scoped_context_isolates_rooms_and_decodes_speaker_identity() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("scoped.db")).unwrap();
        let speaker = crate::models::SpeakerIdentity {
            id: "track-1".into(),
            index: 0,
            display_name: Some("Alice".into()),
            user_id: Some("usr_11111111-1111-1111-1111-111111111111".into()),
        };
        database
            .add_subtitle_in_scope(
                &Subtitle {
                    id: None,
                    conversation_id: None,
                    text: "room A line".into(),
                    speaker: Some(speaker.clone()),
                    language: None,
                    started_at: None,
                    ended_at: None,
                    source: "speaker".into(),
                    created_at: "2026-08-14T00:00:01Z".into(),
                    translations: Vec::new(),
                },
                Some("scope-a"),
                Some(1),
            )
            .unwrap();
        let saved_b = database
            .add_subtitle_in_scope(
                &Subtitle {
                    id: None,
                    conversation_id: None,
                    text: "room B line".into(),
                    speaker: Some(speaker.clone()),
                    language: None,
                    started_at: None,
                    ended_at: None,
                    source: "speaker".into(),
                    created_at: "2026-08-14T00:00:02Z".into(),
                    translations: Vec::new(),
                },
                Some("scope-b"),
                Some(2),
            )
            .unwrap();
        assert_eq!(
            database.subtitle_context_meta(saved_b.id.unwrap()).unwrap(),
            (Some("scope-b".into()), Some(2))
        );

        let config = TranslationPromptConfig {
            context_enabled: true,
            include_speaker: true,
            include_microphone: true,
            include_chatbox: true,
            max_messages: 10,
            ..Default::default()
        };
        let scoped_a = database
            .recent_translation_context_scoped(&config, None, Some("scope-a"))
            .unwrap();
        assert_eq!(scoped_a.len(), 1);
        assert_eq!(scoped_a[0].text, "room A line");
        assert_eq!(scoped_a[0].speaker, Some(speaker));
    }

    #[test]
    fn captured_self_chatbox_subtitles_are_included_when_chatbox_context_is_enabled() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("self-chatbox-ctx.db")).unwrap();
        let add = |text: &str, source: &str, created_at: &str| {
            database
                .add_subtitle_in_scope(
                    &Subtitle {
                        speaker: None,
                        id: None,
                        conversation_id: None,
                        text: text.into(),
                        language: None,
                        started_at: None,
                        ended_at: None,
                        source: source.into(),
                        created_at: created_at.into(),
                        translations: Vec::new(),
                    },
                    Some("scope-self"),
                    None,
                )
                .unwrap()
        };

        let captured_self = add("captured self line", "chatbox", "2026-08-14T00:00:01Z");
        add("manual translation", "chatbox", "2026-08-14T00:00:02Z");
        database
            .conn
            .execute(
                "INSERT INTO chatbox_messages(source, original, translation, source_language,
                    target_language, send_mode, message_format, rendered_text, char_count,
                    truncated, status, created_at, context_scope_id)
                 VALUES ('manual', 'manual original', 'manual translation', 'en', 'ja',
                    'translation', 'slash_separated', 'manual translation', 18, 0, 'sent',
                    '2026-08-14T00:00:02Z', 'scope-self')",
                [],
            )
            .unwrap();
        let current_self = add("current self line", "chatbox", "2026-08-14T00:00:03Z");

        let mut config = TranslationPromptConfig {
            context_enabled: true,
            include_speaker: false,
            include_microphone: false,
            include_chatbox: true,
            max_messages: 10,
            ..Default::default()
        };

        let scoped = database
            .recent_translation_context_scoped(&config, current_self.id, Some("scope-self"))
            .unwrap();
        assert_eq!(
            scoped.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["captured self line", "manual original"]
        );
        assert!(scoped.iter().all(|e| e.source == "chatbox"));

        let unscoped = database
            .recent_translation_context(&config, current_self.id)
            .unwrap();
        assert_eq!(
            unscoped.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["captured self line", "manual original"]
        );

        let excluding_captured = database
            .recent_translation_context_scoped(&config, captured_self.id, Some("scope-self"))
            .unwrap();
        assert_eq!(
            excluding_captured
                .iter()
                .map(|e| e.text.as_str())
                .collect::<Vec<_>>(),
            ["manual original", "current self line"]
        );

        config.include_chatbox = false;
        assert!(
            database
                .recent_translation_context_scoped(&config, current_self.id, Some("scope-self"))
                .unwrap()
                .is_empty()
        );
    }
}
