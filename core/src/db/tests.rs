use super::dictionary::{like_prefix, INSERT_BATCH_SIZE};
use super::*;
use crate::models::{now_iso8601, Subtitle, SubtitleTranslation};
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
            text: "新".into(),
            source_language: Some("en".into()),
            target_language: "zh-Hans".into(),
            provider: "deepl".into(),
            model: None,
            created_at: now_iso8601(),
        },
        SubtitleTranslation {
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
fn dictionary_import_flushes_full_and_partial_batches() {
    let (_path, mut database) = open_temp_db("batch-import");
    let archive = dictionary_archive(INSERT_BATCH_SIZE + 1);

    let mut progress = Vec::new();
    let imported = database
        .import_yomitan_with_progress(&archive, |value| progress.push(value))
        .unwrap();
    assert_eq!(imported.entry_count, 501);
    assert_eq!(progress.first(), Some(&0.0));
    assert_eq!(progress.last(), Some(&1.0));
    assert!(progress.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(
        database
            .conn
            .query_row("SELECT COUNT(*) FROM dictionary_entries", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        501
    );

    database.import_yomitan(&archive).unwrap();
    assert_eq!(
        database
            .conn
            .query_row("SELECT COUNT(*) FROM dictionary_entries", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        501
    );
}
