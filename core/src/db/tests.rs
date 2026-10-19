use super::dictionary::{like_prefix, INSERT_BATCH_SIZE};
use super::*;
use crate::models::{now_iso8601, DictionarySource, Subtitle, SubtitleTranslation};
use std::io::{Cursor, Write};

fn open_temp_db(name: &str) -> (std::path::PathBuf, Database) {
    let dir = std::env::temp_dir().join(format!("vrcs-db-{}-{}", name, std::process::id()));
    let path = dir.join("vrcs.db");
    let _ = std::fs::remove_file(&path);
    let database = Database::open(&path).unwrap();
    (path, database)
}

fn subtitle(text: &str) -> Subtitle {
    Subtitle {
        speaker: None,
        id: None,
        conversation_id: None,
        text: text.into(),
        language: Some("ja".into()),
        started_at: None,
        ended_at: None,
        source: "speaker".into(),
        created_at: now_iso8601(),
        translations: Vec::new(),
    }
}

fn import_yomitan(db: &std::sync::Mutex<Database>, archive: &[u8]) -> DictionarySource {
    Database::import_yomitan_with_progress(db, archive, |_| {}, || false).unwrap()
}

fn dictionary_archive(count: usize) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("index.json", options).unwrap();
    writer
        .write_all(br#"{"title":"BatchDict","revision":"1","format":3}"#)
        .unwrap();
    writer.start_file("term_bank_1.json", options).unwrap();
    let entries = (0..count)
        .map(|index| serde_json::json!([format!("term-{index}"), "", "", "", 0, ["definition"]]))
        .collect::<Vec<_>>();
    writer
        .write_all(serde_json::to_string(&entries).unwrap().as_bytes())
        .unwrap();
    writer.finish().unwrap().into_inner()
}

#[test]
fn subtitles_are_trimmed_by_database_usage_and_returned_newest_first() {
    let (_path, mut database) = open_temp_db("history");
    let baseline = database.storage_stats().unwrap().used_bytes;
    database
        .set_subtitle_history_max_bytes(baseline + 4 * 1024)
        .unwrap();
    for index in 0..5 {
        let text = format!("line {index} {}", "x".repeat(20_000));
        database.add_subtitle(&subtitle(&text)).unwrap();
    }
    let history = database.subtitle_history(500).unwrap();
    assert_eq!(history.len(), 1);
    assert!(history[0].text.starts_with("line 4 "));
}

#[test]
fn subtitle_history_can_load_older_pages() {
    let (_path, database) = open_temp_db("history-pages");
    for index in 0..5 {
        database
            .add_subtitle(&subtitle(&format!("line {index}")))
            .unwrap();
    }

    let latest = database.subtitle_history(2).unwrap();
    assert_eq!(
        latest
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["line 4", "line 3"]
    );

    let older = database
        .subtitle_history_before(2, latest.last().and_then(|item| item.id).unwrap())
        .unwrap();
    assert_eq!(
        older
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["line 2", "line 1"]
    );
}

#[test]
fn version_3_clears_old_conversations_but_preserves_learning_and_dictionary_data() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("version-1.db");
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch(MIGRATION_1_SCHEMA).unwrap();
    connection
        .execute(
            "INSERT INTO subtitles(text, source, created_at)
             VALUES ('old subtitle', 'speaker', '2026-01-01T00:00:00.000000Z')",
            [],
        )
        .unwrap();
    let subtitle_id = connection.last_insert_rowid();
    connection
        .execute(
            "INSERT INTO subtitle_translations(
                subtitle_id, text, target_language, provider, created_at
             ) VALUES (?1, '旧字幕', 'zh-Hans', 'local', '2026-01-01T00:00:01.000000Z')",
            [subtitle_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO dictionary(term, language, definition)
             VALUES ('preserved-term', 'en', 'preserved definition')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO learning_items(
                kind, status, source_text, working_text, source_language,
                source_subtitle_ids, dictionary_entries, created_at, updated_at
             ) VALUES (
                'sentence', 'collected', 'preserved learning item',
                'preserved learning item', 'en', ?1, '[]',
                '2026-01-01T00:00:02.000000Z', '2026-01-01T00:00:02.000000Z'
             )",
            [format!("[{subtitle_id}]")],
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    drop(connection);

    let database = Database::open(&path).unwrap();
    let version = database
        .conn
        .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
        .unwrap();
    let subtitle_count = database
        .conn
        .query_row("SELECT COUNT(*) FROM subtitles", [], |row| {
            row.get::<_, u64>(0)
        })
        .unwrap();
    let translation_count = database
        .conn
        .query_row("SELECT COUNT(*) FROM subtitle_translations", [], |row| {
            row.get::<_, u64>(0)
        })
        .unwrap();
    let metadata_table_count = database
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'conversation_metadata'",
            [],
            |row| row.get::<_, u64>(0),
        )
        .unwrap();
    let definition = database
        .conn
        .query_row(
            "SELECT definition FROM dictionary WHERE term = 'preserved-term'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    let learning_snapshot = database
        .conn
        .query_row(
            "SELECT source_text, source_subtitle_ids FROM learning_items LIMIT 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .unwrap();
    let catalog = database.conversation_catalog().unwrap();

    assert_eq!(version, LATEST_SCHEMA_VERSION);
    assert_eq!(subtitle_count, 0);
    assert_eq!(translation_count, 0);
    assert_eq!(metadata_table_count, 0);
    assert_eq!(definition, "preserved definition");
    assert_eq!(learning_snapshot.0, "preserved learning item");
    assert_eq!(learning_snapshot.1, format!("[{subtitle_id}]"));
    assert_eq!(catalog.conversations.len(), 1);
    assert!(catalog.conversations[0].active);
    assert_eq!(catalog.conversations[0].subtitle_count, 0);
    assert_eq!(catalog.conversations[0].automatic_title, None);
}

#[test]
fn conversation_title_freezes_and_subtitles_use_keyset_pages() {
    let (_path, database) = open_temp_db("conversation-pages");
    let mut first = subtitle("  hello   world from elsewhere  ");
    first.created_at = "2026-01-01T00:00:00.000000Z".into();
    let first = database.add_subtitle(&first).unwrap();
    for index in 1..5 {
        let mut item = subtitle(&format!("line {index}"));
        item.created_at = format!("2026-01-01T00:00:0{index}.000000Z");
        database.add_subtitle(&item).unwrap();
    }

    let catalog = database.conversation_catalog().unwrap();
    let conversation = &catalog.conversations[0];
    assert_eq!(
        conversation.automatic_title.as_deref(),
        Some("hello world fr")
    );
    assert_eq!(conversation.subtitle_count, 5);
    assert_eq!(
        first.conversation_id.as_deref(),
        Some(conversation.id.as_str())
    );

    let latest = database
        .conversation_subtitles(&conversation.id, 2, None)
        .unwrap()
        .unwrap();
    assert_eq!(
        latest
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["line 4", "line 3"]
    );
    assert!(latest.has_more);
    let older = database
        .conversation_subtitles(&conversation.id, 2, latest.next_before_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        older
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["line 2", "line 1"]
    );
    assert!(older.has_more);
}

#[test]
fn creating_a_conversation_switches_subsequent_subtitle_ownership() {
    let (_path, mut database) = open_temp_db("conversation-switch");
    let initial_id = database
        .conversation_catalog()
        .unwrap()
        .conversations
        .into_iter()
        .find(|conversation| conversation.active)
        .unwrap()
        .id;
    let reused = database.create_conversation().unwrap();
    assert_eq!(
        reused
            .conversations
            .iter()
            .find(|conversation| conversation.active)
            .unwrap()
            .id,
        initial_id
    );

    let first = database.add_subtitle(&subtitle("first")).unwrap();
    let first_id = first.conversation_id.unwrap();
    let catalog = database.create_conversation().unwrap();
    let active = catalog
        .conversations
        .iter()
        .find(|conversation| conversation.active)
        .unwrap();
    assert_ne!(active.id, first_id);
    let second = database.add_subtitle(&subtitle("second")).unwrap();
    assert_eq!(second.conversation_id.as_deref(), Some(active.id.as_str()));
}

#[test]
fn conversation_titles_are_deduplicated_and_preserve_custom_titles_and_previous_results() {
    let (_path, mut database) = open_temp_db("conversation-titles");
    assert!(database
        .create_conversation_with_title(true)
        .unwrap()
        .1
        .is_none());
    database
        .add_subtitle(&subtitle("Japanese interview practice"))
        .unwrap();
    let (_, job) = database.create_conversation_with_title(true).unwrap();
    let job = job.unwrap();
    let id = &job.conversation_id;
    assert!(database
        .request_conversation_title(id, true, true)
        .unwrap()
        .unwrap()
        .1
        .is_none());
    assert!(database.start_conversation_title(&job).unwrap());
    assert!(!database.start_conversation_title(&job).unwrap());
    database
        .update_conversation(id, Some(Some("My title")), Some(Some("star")))
        .unwrap();
    assert!(database
        .finish_conversation_title(&job, Ok("Interview practice"))
        .unwrap());
    let catalog = database.conversation_catalog().unwrap();
    let saved = catalog
        .conversations
        .iter()
        .find(|item| &item.id == id)
        .unwrap();
    assert_eq!(saved.custom_title.as_deref(), Some("My title"));
    assert_eq!(saved.summary_title.as_deref(), Some("Interview practice"));
    assert_eq!(saved.icon.as_deref(), Some("star"));
    assert!(matches!(
        database.request_conversation_title(id, true, true),
        Err(crate::error::AppError::Conflict(_))
    ));
    database.update_conversation(id, Some(None), None).unwrap();
    assert!(database
        .request_conversation_title(id, false, false)
        .unwrap()
        .unwrap()
        .1
        .is_none());
    let retry = database
        .request_conversation_title(id, true, true)
        .unwrap()
        .unwrap()
        .1
        .unwrap();
    assert!(database.start_conversation_title(&retry).unwrap());
    assert!(!database
        .finish_conversation_title(&job, Ok("Stale result"))
        .unwrap());
    assert!(database
        .finish_conversation_title(&retry, Err("conversations.title_timeout"))
        .unwrap());
    let catalog = database.conversation_catalog().unwrap();
    let saved = catalog
        .conversations
        .iter()
        .find(|item| &item.id == id)
        .unwrap();
    assert_eq!(saved.summary_title.as_deref(), Some("Interview practice"));
    assert_eq!(saved.title_generation_status, "failed");

    // Tasks that fail before starting must not leave the conversation pending.
    let queued = database
        .request_conversation_title(id, true, true)
        .unwrap()
        .unwrap()
        .1
        .unwrap();
    assert!(database
        .finish_conversation_title(&queued, Err("conversations.title_service_failed"))
        .unwrap());
    let catalog = database.conversation_catalog().unwrap();
    let saved = catalog
        .conversations
        .iter()
        .find(|item| &item.id == id)
        .unwrap();
    assert_eq!(saved.title_generation_status, "failed");
    assert!(database
        .request_conversation_title(id, true, true)
        .unwrap()
        .unwrap()
        .1
        .is_some());
}

#[test]
fn user_deletions_invalidate_title_jobs_but_quota_trimming_keeps_them() {
    for deletion in ["subtitle", "conversation", "clear", "quota"] {
        let (_path, mut database) = open_temp_db("title-deletion");
        let saved = database.add_subtitle(&subtitle("First excerpt")).unwrap();
        database.add_subtitle(&subtitle("Last excerpt")).unwrap();
        let (_, job) = database.create_conversation_with_title(true).unwrap();
        let job = job.unwrap();
        assert!(database.start_conversation_title(&job).unwrap());
        database
            .finish_conversation_title(&job, Ok("Saved title"))
            .unwrap();
        let job = database
            .request_conversation_title(&job.conversation_id, true, true)
            .unwrap()
            .unwrap()
            .1
            .unwrap();
        assert!(database.start_conversation_title(&job).unwrap());
        match deletion {
            "conversation" => {
                database.delete_conversation(&job.conversation_id).unwrap();
            }
            "clear" => {
                database.clear_subtitle_history().unwrap();
            }
            "quota" => {
                assert!(database.set_subtitle_history_max_bytes(1).unwrap());
            }
            _ => {
                database
                    .delete_subtitle_range(&saved.created_at, None)
                    .unwrap();
            }
        }
        if matches!(deletion, "subtitle" | "quota") {
            let catalog = database.conversation_catalog().unwrap();
            let item = catalog
                .conversations
                .iter()
                .find(|item| item.id == job.conversation_id)
                .unwrap();
            let kept = deletion == "quota";
            assert_eq!(
                item.title_generation_status,
                if kept { "running" } else { "idle" }
            );
            assert_eq!(item.summary_title.is_some(), kept);
        }
        assert_eq!(
            database
                .finish_conversation_title(&job, Ok("Late title"))
                .unwrap(),
            deletion == "quota"
        );
    }
}

#[test]
fn title_input_samples_first_and_last_originals_with_a_strict_unicode_budget() {
    let (_path, mut database) = open_temp_db("title-input");
    let mut source = subtitle("");
    source.speaker = Some(crate::models::SpeakerIdentity {
        id: "private-session-id".into(),
        index: 0,
    });
    for index in 0..250 {
        source.text = format!("excerpt-{index} {}", "界".repeat(400));
        database.add_subtitle(&source).unwrap();
    }
    let (_, job) = database.create_conversation_with_title(true).unwrap();
    let input = database
        .conversation_title_input(&job.unwrap().conversation_id)
        .unwrap();
    assert_eq!(input.lines().count(), 200);
    assert!(input.contains("excerpt-0 ") && input.contains("excerpt-249 "));
    assert!(input.contains("[speaker speaker-1]"));
    assert!(!input.contains("private-session-id"));
    assert!(input.chars().count() <= 24_000);
}

#[test]
fn title_input_ignores_malformed_speaker_indexes() {
    let (_path, mut database) = open_temp_db("title-speakers");
    for speaker in [
        r#"{"id":"a","index":-1}"#,
        r#"{"id":"b","index":1.5}"#,
        r#"{"id":"c","index":"2"}"#,
        r#"{"id":"d","index":3}"#,
    ] {
        let id = database
            .add_subtitle(&subtitle(speaker))
            .unwrap()
            .id
            .unwrap();
        database
            .conn
            .execute(
                "UPDATE subtitles SET speaker = ?1 WHERE id = ?2",
                params![speaker, id],
            )
            .unwrap();
    }
    let (_, job) = database.create_conversation_with_title(true).unwrap();
    let input = database
        .conversation_title_input(&job.unwrap().conversation_id)
        .unwrap();
    assert_eq!(input.matches("[speaker] ").count(), 3);
    assert!(input.contains("[speaker speaker-4] "));
}

#[test]
fn empty_transcripts_never_register_title_requests() {
    let (_path, mut database) = open_temp_db("title-empty");
    database.add_subtitle(&subtitle(" \t\n\r ")).unwrap();
    let (catalog, job) = database.create_conversation_with_title(true).unwrap();
    assert!(job.is_none());
    let ended = catalog
        .conversations
        .iter()
        .find(|item| !item.active)
        .unwrap();
    assert!(database
        .conversation_title_input(&ended.id)
        .unwrap()
        .is_empty());
    assert!(matches!(
        database.request_conversation_title(&ended.id, false, true),
        Err(crate::error::AppError::Validation(_))
    ));
}

#[test]
fn version_8_adds_idle_title_state_without_changing_existing_conversations() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("version-7.db");
    let mut database = Database::open(&path).unwrap();
    let saved = database
        .add_subtitle(&subtitle("Original transcript"))
        .unwrap();
    let id = saved.conversation_id.as_deref().unwrap();
    database
        .update_conversation(id, Some(Some("My title")), Some(Some("star")))
        .unwrap();
    database.create_conversation().unwrap();
    let before = database.conversation_catalog().unwrap();
    database
        .conn
        .execute_batch(
            "ALTER TABLE conversations DROP COLUMN summary_title;
         ALTER TABLE conversations DROP COLUMN title_generation_status;
         ALTER TABLE conversations DROP COLUMN title_generation_error_code;
         ALTER TABLE conversations DROP COLUMN title_generation_request_id;
         PRAGMA user_version = 7;",
        )
        .unwrap();
    drop(database);

    let database = Database::open(&path).unwrap();
    let after = database.conversation_catalog().unwrap();
    assert_eq!(after.conversations.len(), before.conversations.len());
    for (after, before) in after.conversations.iter().zip(&before.conversations) {
        assert_eq!(after.id, before.id);
        assert_eq!(after.custom_title, before.custom_title);
        assert_eq!(after.automatic_title, before.automatic_title);
        assert_eq!(after.icon, before.icon);
        assert_eq!(after.ended_at, before.ended_at);
        assert_eq!(after.subtitle_count, before.subtitle_count);
        assert!(after.summary_title.is_none());
        assert_eq!(after.title_generation_status, "idle");
        assert!(after.title_generation_error_code.is_none());
    }
    assert_eq!(
        database.subtitle(saved.id.unwrap()).unwrap().unwrap().text,
        saved.text
    );
    assert_eq!(
        database
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        super::LATEST_SCHEMA_VERSION
    );
}

#[test]
fn restarting_marks_title_jobs_interrupted_without_removing_successful_titles() {
    let (path, mut database) = open_temp_db("title-restart");
    database.add_subtitle(&subtitle("Original")).unwrap();
    let (_, job) = database.create_conversation_with_title(true).unwrap();
    let job = job.unwrap();
    database.start_conversation_title(&job).unwrap();
    database
        .finish_conversation_title(&job, Ok("Saved title"))
        .unwrap();
    database
        .request_conversation_title(&job.conversation_id, true, true)
        .unwrap();
    drop(database);
    let database = Database::open(&path).unwrap();
    let catalog = database.conversation_catalog().unwrap();
    let item = catalog
        .conversations
        .iter()
        .find(|item| item.id == job.conversation_id)
        .unwrap();
    assert_eq!(item.summary_title.as_deref(), Some("Saved title"));
    assert_eq!(item.title_generation_status, "failed");
    assert_eq!(
        item.title_generation_error_code.as_deref(),
        Some("conversations.title_interrupted")
    );
}

#[test]
fn deleting_active_creates_replacement_before_waiting_writes_continue() {
    let (_path, database) = open_temp_db("conversation-delete-active");
    let shared = std::sync::Arc::new(std::sync::Mutex::new(database));
    let mut database = shared.lock().unwrap();
    let saved = database.add_subtitle(&subtitle("old active")).unwrap();
    let deleted_id = saved.conversation_id.unwrap();
    let writer_database = std::sync::Arc::clone(&shared);
    let writer = std::thread::spawn(move || {
        writer_database
            .lock()
            .unwrap()
            .add_subtitle(&subtitle("after delete"))
            .unwrap()
    });

    database.delete_conversation(&deleted_id).unwrap().unwrap();
    drop(database);
    let saved = writer.join().unwrap();
    let database = shared.lock().unwrap();
    let active_id = database
        .conversation_catalog()
        .unwrap()
        .conversations
        .into_iter()
        .find(|conversation| conversation.active)
        .unwrap()
        .id;
    assert_ne!(saved.conversation_id.as_deref(), Some(deleted_id.as_str()));
    assert_eq!(saved.conversation_id.as_deref(), Some(active_id.as_str()));
}

#[test]
fn subtitle_history_deletes_only_the_requested_time_range() {
    let (_path, database) = open_temp_db("history-range-delete");
    for (text, created_at) in [
        ("older", "2026-08-16T00:00:00.000000Z"),
        ("target one", "2026-08-16T01:00:00.000000Z"),
        ("target two", "2026-08-16T01:30:00.000000Z"),
        ("newer", "2026-08-16T02:00:00.000000Z"),
    ] {
        let mut item = subtitle(text);
        item.created_at = created_at.into();
        database.add_subtitle(&item).unwrap();
    }

    let deleted = database
        .delete_subtitle_range(
            "2026-08-16T01:00:00.000000Z",
            Some("2026-08-16T02:00:00.000000Z"),
        )
        .unwrap();
    assert_eq!(deleted, 2);
    assert_eq!(
        database
            .subtitle_history(10)
            .unwrap()
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["newer", "older"]
    );
}

#[test]
fn subtitle_quota_ignores_dictionary_data() {
    let (_path, mut database) = open_temp_db("history-dictionary");
    database
        .conn
        .execute(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000)
             INSERT INTO dictionary (term, language, definition)
             SELECT 'term-' || i, 'ja', printf('%.2000c', 'x') FROM n",
            [],
        )
        .unwrap();
    let quota = 512 * 1024;
    assert!(database.storage_stats().unwrap().allocated_bytes > quota);
    database.set_subtitle_history_max_bytes(quota).unwrap();
    for index in 0..5 {
        database
            .add_subtitle(&subtitle(&format!("line {index}")))
            .unwrap();
    }

    let stats = database.storage_stats().unwrap();
    assert_eq!(database.subtitle_history(10).unwrap().len(), 5);
    assert!(!stats.over_limit);
    assert!(stats.used_bytes < quota);
}

#[test]
fn subtitle_history_clear_reclaims_database_pages() {
    let (_path, mut database) = open_temp_db("history-clear");
    for index in 0..4 {
        let text = format!("line {index} {}", "x".repeat(20_000));
        database.add_subtitle(&subtitle(&text)).unwrap();
    }
    database.create_conversation().unwrap();
    let before = database.storage_stats().unwrap();
    let after = database.clear_subtitle_history().unwrap();
    let catalog = database.conversation_catalog().unwrap();

    assert!(database.subtitle_history(10).unwrap().is_empty());
    assert_eq!(catalog.conversations.len(), 1);
    assert!(catalog.conversations[0].active);
    assert_eq!(catalog.conversations[0].automatic_title, None);
    assert_eq!(catalog.conversations[0].subtitle_count, 0);
    assert!(after.allocated_bytes < before.allocated_bytes);
    assert!(after.used_bytes <= after.allocated_bytes);
}

#[test]
fn subtitle_translation_is_saved_and_loaded() {
    let (_path, database) = open_temp_db("translation");
    let saved = database.add_subtitle(&subtitle("hello")).unwrap();
    let translation = SubtitleTranslation {
        source_group: None,
        text: "你好".into(),
        source_language: Some("en".into()),
        target_language: "zh-Hans".into(),
        provider: "deepl".into(),
        model: None,
        created_at: now_iso8601(),
    };
    let catalog_changed = database
        .save_translation(saved.id.unwrap(), &translation)
        .unwrap();

    assert!(!catalog_changed);
    let loaded = database.subtitle(saved.id.unwrap()).unwrap().unwrap();
    assert_eq!(loaded.translations, vec![translation]);
}

#[test]
fn subtitle_search_finds_originals_and_translations_without_duplicates() {
    let (_path, database) = open_temp_db("subtitle-search");
    let first = database
        .add_subtitle(&subtitle("今日は Virtual Market に行きます"))
        .unwrap();
    let second = database
        .add_subtitle(&subtitle("Virtual Market starts soon"))
        .unwrap();
    database
        .save_translation(
            first.id.unwrap(),
            &SubtitleTranslation {
                source_group: None,
                text: "I am going to the virtual market today".into(),
                source_language: Some("ja".into()),
                target_language: "en".into(),
                provider: "local".into(),
                model: None,
                created_at: now_iso8601(),
            },
        )
        .unwrap();

    let page = database.search_subtitles("Virtual Market", 50, 0).unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(!page.has_more);
    assert_eq!(
        page.items
            .iter()
            .map(|hit| hit.subtitle.id.unwrap())
            .collect::<std::collections::HashSet<_>>(),
        [first.id.unwrap(), second.id.unwrap()]
            .into_iter()
            .collect()
    );
    let translated = database.search_subtitles("going to the", 50, 0).unwrap();
    assert_eq!(translated.items.len(), 1);
    assert_eq!(translated.items[0].matched_field, "translation");

    let single_character = database.search_subtitles("今", 50, 0).unwrap();
    assert_eq!(single_character.items.len(), 1);
    assert_eq!(single_character.items[0].subtitle.id, first.id);

    let single_character_translation = database.search_subtitles("g", 50, 0).unwrap();
    assert_eq!(single_character_translation.items.len(), 1);
    assert_eq!(
        single_character_translation.items[0].matched_field,
        "translation"
    );
}

#[test]
fn subtitle_search_index_tracks_updates_and_deletes() {
    let (_path, database) = open_temp_db("subtitle-search-lifecycle");
    let saved = database
        .add_subtitle(&subtitle("searchable phrase"))
        .unwrap();
    let subtitle_id = saved.id.unwrap();
    assert_eq!(
        database
            .search_subtitles("searchable", 10, 0)
            .unwrap()
            .items
            .len(),
        1
    );

    database
        .conn
        .execute(
            "UPDATE subtitles SET text = 'replacement phrase' WHERE id = ?1",
            [subtitle_id],
        )
        .unwrap();
    assert!(database
        .search_subtitles("searchable", 10, 0)
        .unwrap()
        .items
        .is_empty());
    assert_eq!(
        database
            .search_subtitles("replacement", 10, 0)
            .unwrap()
            .items
            .len(),
        1
    );

    database
        .conn
        .execute("DELETE FROM subtitles WHERE id = ?1", [subtitle_id])
        .unwrap();
    assert!(database
        .search_subtitles("replacement", 10, 0)
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn conversation_context_centers_the_target_and_reports_both_directions() {
    let (_path, database) = open_temp_db("subtitle-context");
    let saved = (0..7)
        .map(|index| {
            database
                .add_subtitle(&subtitle(&format!("context line {index}")))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let conversation_id = saved[0].conversation_id.as_deref().unwrap();
    let target_id = saved[3].id.unwrap();

    let context = database
        .conversation_subtitle_context(conversation_id, target_id, 2)
        .unwrap()
        .unwrap();
    assert_eq!(
        context
            .items
            .iter()
            .map(|item| item.id.unwrap())
            .collect::<Vec<_>>(),
        saved[1..=5]
            .iter()
            .rev()
            .map(|item| item.id.unwrap())
            .collect::<Vec<_>>()
    );
    assert!(context.has_older);
    assert_eq!(context.target_id, target_id);
    assert!(database
        .conversation_subtitle_context(conversation_id, saved[6].id.unwrap() + 100, 2)
        .unwrap()
        .is_none());
}

#[test]
fn subtitle_history_keeps_batched_translations_with_their_subtitles() {
    let (_path, database) = open_temp_db("history-translations");
    let older = database.add_subtitle(&subtitle("older")).unwrap();
    let older_translation = SubtitleTranslation {
        source_group: None,
        text: "旧".into(),
        source_language: Some("en".into()),
        target_language: "zh-Hans".into(),
        provider: "deepl".into(),
        model: None,
        created_at: now_iso8601(),
    };
    database
        .save_translation(older.id.unwrap(), &older_translation)
        .unwrap();

    let newer = database.add_subtitle(&subtitle("newer")).unwrap();
    let newer_translations = [
        SubtitleTranslation {
            source_group: None,
            text: "新".into(),
            source_language: Some("en".into()),
            target_language: "zh-Hans".into(),
            provider: "deepl".into(),
            model: None,
            created_at: now_iso8601(),
        },
        SubtitleTranslation {
            source_group: None,
            text: "nouveau".into(),
            source_language: Some("en".into()),
            target_language: "fr".into(),
            provider: "openai".into(),
            model: Some("test-model".into()),
            created_at: now_iso8601(),
        },
    ];
    for translation in &newer_translations {
        database
            .save_translation(newer.id.unwrap(), translation)
            .unwrap();
    }

    let history = database.subtitle_history(10).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].text, "newer");
    assert_eq!(history[0].translations, newer_translations);
    assert_eq!(history[1].text, "older");
    assert_eq!(history[1].translations, vec![older_translation]);
}

#[test]
fn seed_dictionary_lookup_works() {
    let (_path, database) = open_temp_db("seed");
    let entries = database.lookup("こんにちは", 10).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].language, "ja");
    assert!(entries[0].dictionary.is_none());
}

#[test]
fn dictionary_prefix_treats_like_wildcards_as_text() {
    let (_path, database) = open_temp_db("like-wildcards");
    database
        .conn
        .execute(
            "INSERT INTO dictionary(term, language, definition) VALUES (?, 'en', 'test')",
            ["%literal"],
        )
        .unwrap();
    database
        .conn
        .execute(
            "INSERT INTO dictionary(term, language, definition) VALUES (?, 'en', 'test')",
            ["_literal"],
        )
        .unwrap();

    let percent = database.lookup("%", 10).unwrap();
    let underscore = database.lookup("_", 10).unwrap();

    assert_eq!(
        percent
            .iter()
            .map(|entry| entry.term.as_str())
            .collect::<Vec<_>>(),
        ["%literal"]
    );
    assert_eq!(
        underscore
            .iter()
            .map(|entry| entry.term.as_str())
            .collect::<Vec<_>>(),
        ["_literal"]
    );
}

#[test]
fn dictionary_lookup_ranks_shorter_prefixes_first_and_deinflects_queries() {
    let (_path, database) = open_temp_db("deinflect-lookup");
    let db = std::sync::Mutex::new(database);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("index.json", options).unwrap();
    writer
        .write_all(br#"{"title":"LookupDict","revision":"1","format":3,"sourceLanguage":"ja","targetLanguage":"en"}"#)
        .unwrap();
    writer.start_file("term_bank_1.json", options).unwrap();
    let entries = serde_json::json!([
        ["食べ放題", "たべほうだい", "", "", 100, ["all-you-can-eat"]],
        ["食べる", "たべる", "", "", 10, ["to eat"]],
        ["走る", "はしる", "", "", 10, ["to run"]],
        ["書く", "かく", "", "", 10, ["to write"]],
        ["高い", "たかい", "", "", 10, ["high; expensive"]],
        ["猫", "ねこ", "", "", 10, ["cat"]],
        ["食", "しょく", "", "", 10, ["meal"]],
        ["書き", "かき", "", "", 10, ["writing"]],
        ["高く", "たかく", "", "", 10, ["highly"]],
        ["話", "はなし", "", "", 10, ["story"]],
        ["話す", "はなす", "", "", 10, ["to speak"]],
        ["今日は", "こんにちは", "", "", 10, ["imported greeting"]],
        ["run", "run", "", "", 10, ["to move quickly"]]
    ]);
    writer.write_all(entries.to_string().as_bytes()).unwrap();
    let archive = writer.finish().unwrap().into_inner();
    import_yomitan(&db, &archive);
    let database = db.into_inner().unwrap();

    let prefix_hits = database.lookup("食べ", 10).unwrap();
    assert_eq!(prefix_hits[0].term, "食べる");
    assert_eq!(prefix_hits[1].term, "食べ放題");

    let imported_reading_hit = &database.lookup("こんにちは", 5).unwrap()[0];
    assert_eq!(imported_reading_hit.term, "今日は");
    assert_eq!(
        imported_reading_hit.dictionary.as_deref(),
        Some("LookupDict")
    );
    assert_eq!(database.lookup("こんに", 5).unwrap()[0].term, "今日は");

    assert_eq!(database.lookup("食べました", 5).unwrap()[0].term, "食べる");
    assert_eq!(database.lookup("走っている", 5).unwrap()[0].term, "走る");
    assert_eq!(database.lookup("書きました", 5).unwrap()[0].term, "書く");
    assert_eq!(database.lookup("話しました", 5).unwrap()[0].term, "話す");
    assert_eq!(database.lookup("高かった", 5).unwrap()[0].term, "高い");
    assert_eq!(database.lookup("猫が", 5).unwrap()[0].term, "猫");
    assert_eq!(database.lookup("running", 5).unwrap()[0].term, "run");
}

#[test]
fn escaped_prefix_lookup_uses_the_term_index() {
    let (_path, database) = open_temp_db("like-plan");
    let plan = database
        .conn
        .prepare(
            "EXPLAIN QUERY PLAN SELECT term FROM dictionary_entries
             WHERE term LIKE ? ESCAPE '\\' COLLATE NOCASE",
        )
        .unwrap()
        .query_map([like_prefix("term")], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join(" ");

    assert!(plan.contains("dictionary_entries_term_idx"), "{plan}");
}

#[test]
fn feature_switches_cancel_dictionary_replacement_without_losing_existing_entries() {
    let (_path, database) = open_temp_db("cancel-import");
    let db = std::sync::Mutex::new(database);
    let archive = dictionary_archive(INSERT_BATCH_SIZE + 1);
    let original = import_yomitan(&db, &archive);
    let cancelled = std::cell::Cell::new(false);
    let result = Database::import_yomitan_with_progress(
        &db,
        &archive,
        |progress| {
            if progress > 0.0 {
                cancelled.set(true);
            }
        },
        || cancelled.get(),
    );
    assert!(matches!(result, Err(crate::error::AppError::Conflict(_))));
    let database = db.into_inner().unwrap();
    let sources = database.dictionary_sources().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, original.id);
    assert_eq!(sources[0].entry_count, original.entry_count);
    assert_eq!(
        database
            .conn
            .query_row("SELECT COUNT(*) FROM dictionary_entries", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        501
    );
}

#[test]
fn subtitles_can_be_saved_while_a_dictionary_imports() {
    let (_path, database) = open_temp_db("import-concurrency");
    let db = std::sync::Mutex::new(database);
    let archive = dictionary_archive(INSERT_BATCH_SIZE * 3);
    Database::import_yomitan_with_progress(
        &db,
        &archive,
        |progress| {
            let database = db
                .try_lock()
                .expect("import must not hold the database lock");
            database.add_subtitle(&subtitle("during import")).unwrap();
            if progress < 1.0 {
                assert!(database.dictionary_sources().unwrap().is_empty());
            }
        },
        || false,
    )
    .unwrap();
    let database = db.into_inner().unwrap();
    assert!(!database.subtitle_history(100).unwrap().is_empty());
    assert_eq!(database.dictionary_sources().unwrap().len(), 1);
}

#[test]
fn dictionary_import_flushes_full_and_partial_batches() {
    let (_path, database) = open_temp_db("batch-import");
    let db = std::sync::Mutex::new(database);
    let archive = dictionary_archive(INSERT_BATCH_SIZE + 1);

    let mut progress = Vec::new();
    let imported = Database::import_yomitan_with_progress(
        &db,
        &archive,
        |value| progress.push(value),
        || false,
    )
    .unwrap();
    assert_eq!(imported.entry_count, 501);
    assert_eq!(progress.first(), Some(&0.0));
    assert_eq!(progress.last(), Some(&1.0));
    assert!(progress.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(
        db.lock()
            .unwrap()
            .conn
            .query_row("SELECT COUNT(*) FROM dictionary_entries", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        501
    );

    let reimported = import_yomitan(&db, &archive);
    assert_eq!(reimported.title, imported.title);
    assert_eq!(db.lock().unwrap().dictionary_sources().unwrap().len(), 1);
    assert_eq!(
        db.lock()
            .unwrap()
            .conn
            .query_row("SELECT COUNT(*) FROM dictionary_entries", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        501
    );
}

#[test]
fn grouped_translation_roundtrips_and_rolls_back_all_members_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join("group.db")).unwrap();
    let first = db.add_subtitle(&subtitle("First.")).unwrap();
    let second = db.add_subtitle(&subtitle("Second.")).unwrap();
    let ids = vec![first.id.unwrap(), second.id.unwrap()];
    let mut translation = SubtitleTranslation {
        text: "Combined translation".into(),
        source_language: Some("en".into()),
        target_language: "zh-Hans".into(),
        provider: "openai".into(),
        model: None,
        created_at: now_iso8601(),
        source_group: Some(crate::models::TranslationSourceGroup {
            subtitle_ids: ids.clone(),
            text: "First. Second.".into(),
        }),
    };
    db.save_translation_group(&ids, &translation).unwrap();
    for row in db.subtitle_history(10).unwrap() {
        assert_eq!(row.translations, vec![translation.clone()]);
    }
    assert_eq!(
        db.subtitle(ids[0]).unwrap().unwrap().translations,
        vec![translation.clone()]
    );
    db.conn.execute_batch(&format!("CREATE TRIGGER fail_second BEFORE INSERT ON subtitle_translations WHEN NEW.subtitle_id = {} BEGIN SELECT RAISE(FAIL, 'test failure'); END;", ids[1])).unwrap();
    translation.text = "Must roll back".into();
    assert!(db.save_translation_group(&ids, &translation).is_err());
    for row in db.subtitle_history(10).unwrap() {
        assert_eq!(row.translations[0].text, "Combined translation");
    }
}

#[test]
fn version_4_preserves_existing_version_3_subtitles() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("migration.db");
    let db = Database::open(&path).unwrap();
    let original = db.add_subtitle(&subtitle("Keep existing history")).unwrap();
    db.conn
        .execute_batch(
            "ALTER TABLE subtitle_translations DROP COLUMN source_group; ALTER TABLE subtitles DROP COLUMN speaker; ALTER TABLE learning_items DROP COLUMN anki_sync_pending; ALTER TABLE learning_items DROP COLUMN analysis_stale; ALTER TABLE learning_items DROP COLUMN draft_stale; ALTER TABLE conversations DROP COLUMN summary_title; ALTER TABLE conversations DROP COLUMN title_generation_status; ALTER TABLE conversations DROP COLUMN title_generation_error_code; ALTER TABLE conversations DROP COLUMN title_generation_request_id; PRAGMA user_version = 3;",
        )
        .unwrap();
    drop(db);
    let db = Database::open(&path).unwrap();
    assert_eq!(
        db.subtitle(original.id.unwrap()).unwrap().unwrap().text,
        original.text
    );
    assert_eq!(
        db.conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        LATEST_SCHEMA_VERSION
    );
}

#[test]
fn version_6_adds_anki_sync_pending_without_touching_exported_items() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("anki-sync.db");
    let db = Database::open(&path).unwrap();
    db.conn
        .execute(
            "INSERT INTO learning_items(
                kind, status, source_text, working_text, anki_note_id, created_at, updated_at
             ) VALUES ('word', 'exported', '学ぶ', '学ぶ', 42, 'now', 'now')",
            [],
        )
        .unwrap();
    db.conn
        .execute_batch(
            "ALTER TABLE learning_items DROP COLUMN anki_sync_pending; ALTER TABLE learning_items DROP COLUMN analysis_stale; ALTER TABLE learning_items DROP COLUMN draft_stale; ALTER TABLE conversations DROP COLUMN summary_title; ALTER TABLE conversations DROP COLUMN title_generation_status; ALTER TABLE conversations DROP COLUMN title_generation_error_code; ALTER TABLE conversations DROP COLUMN title_generation_request_id; PRAGMA user_version = 5;",
        )
        .unwrap();
    drop(db);
    let db = Database::open(&path).unwrap();
    let (note_id, pending) = db
        .conn
        .query_row(
            "SELECT anki_note_id, anki_sync_pending FROM learning_items",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?)),
        )
        .unwrap();
    assert_eq!(note_id, 42);
    assert!(!pending);
}

#[test]
fn version_5_preserves_history_and_round_trips_session_scoped_speakers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("speakers.db");
    let db = Database::open(&path).unwrap();
    let legacy = db.add_subtitle(&subtitle("Legacy history")).unwrap();
    db.conn
        .execute_batch(
            "ALTER TABLE subtitles DROP COLUMN speaker; ALTER TABLE learning_items DROP COLUMN anki_sync_pending; ALTER TABLE learning_items DROP COLUMN analysis_stale; ALTER TABLE learning_items DROP COLUMN draft_stale; ALTER TABLE conversations DROP COLUMN summary_title; ALTER TABLE conversations DROP COLUMN title_generation_status; ALTER TABLE conversations DROP COLUMN title_generation_error_code; ALTER TABLE conversations DROP COLUMN title_generation_request_id; PRAGMA user_version = 4;",
        )
        .unwrap();
    drop(db);
    let db = Database::open(&path).unwrap();
    assert!(db
        .subtitle(legacy.id.unwrap())
        .unwrap()
        .unwrap()
        .speaker
        .is_none());
    let mut item = subtitle("Two speakers");
    item.speaker = Some(crate::models::SpeakerIdentity {
        id: "qwen-session-1".into(),
        index: 1,
    });
    let saved = db.add_subtitle(&item).unwrap();
    let id = saved.id.unwrap();
    let conversation = saved.conversation_id.as_deref().unwrap();
    assert_eq!(db.subtitle(id).unwrap().unwrap().speaker, item.speaker);
    assert_eq!(db.subtitle_history(10).unwrap()[0].speaker, item.speaker);
    assert_eq!(
        db.conversation_subtitles(conversation, 10, None)
            .unwrap()
            .unwrap()
            .items[0]
            .speaker,
        item.speaker
    );
    assert_eq!(
        db.conversation_subtitle_context(conversation, id, 1)
            .unwrap()
            .unwrap()
            .items[0]
            .speaker,
        item.speaker
    );
    assert_eq!(
        db.search_subtitles("speakers", 10, 0).unwrap().items[0]
            .subtitle
            .speaker,
        item.speaker
    );
    drop(db);
    assert_eq!(
        Database::open(&path)
            .unwrap()
            .subtitle(id)
            .unwrap()
            .unwrap()
            .speaker,
        item.speaker
    );
}
