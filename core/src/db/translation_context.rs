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
        if !config.context_enabled {
            return Ok(Vec::new());
        }
        let mut statement = self.conn.prepare(
            "SELECT source, text, created_at FROM (
                 SELECT source, text, created_at, 0 AS source_order, id
                 FROM subtitles
                 WHERE source IN ('speaker', 'microphone')
                   AND ((source = 'speaker' AND ?1) OR (source = 'microphone' AND ?2))
                   AND (?4 IS NULL OR id != ?4)
                 UNION ALL
                 SELECT 'chatbox' AS source, original AS text, created_at, 1 AS source_order, id
                 FROM chatbox_messages
                 WHERE ?3 AND status = 'sent'
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
            |row| {
                Ok(TranslationContextEntry {
                    source: row.get(0)?,
                    text: row.get(1)?,
                    created_at: row.get(2)?,
                })
            },
        )?;
        let mut entries = rows.collect::<Result<Vec<_>, _>>()?;
        entries.reverse();
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Subtitle;

    #[test]
    fn context_filters_sources_orders_messages_and_excludes_current_subtitle() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("context.db")).unwrap();
        let add = |text: &str, source: &str, created_at: &str| {
            database
                .add_subtitle(&Subtitle {
                    id: None,
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
        assert!(entries
            .iter()
            .all(|entry| entry.text != "chatbox translation"));
        assert!(entries.iter().all(|entry| entry.text != "current"));
    }

    #[test]
    fn disabled_recent_context_returns_no_history() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("disabled-context.db")).unwrap();
        database
            .add_subtitle(&Subtitle {
                id: None,
                text: "history".into(),
                language: None,
                started_at: None,
                ended_at: None,
                source: "speaker".into(),
                created_at: "2026-08-14T00:00:01Z".into(),
                translations: Vec::new(),
            })
            .unwrap();

        let entries = database
            .recent_translation_context(&TranslationPromptConfig::default(), None)
            .unwrap();

        assert!(entries.is_empty());
    }

    #[test]
    fn context_respects_independent_source_switches_and_message_limit() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&directory.path().join("sources.db")).unwrap();
        for index in 0..3 {
            database
                .add_subtitle(&Subtitle {
                    id: None,
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
}
