//! SQLite 存储：连接与 schema 初始化。
//! 字幕历史和词典仓储实现分别位于同名子模块中。

use std::path::Path;

use rusqlite::{params, Connection};

use crate::error::AppResult;

mod chatbox;
pub(crate) mod conversations;
mod dictionary;
mod learning;
mod storage;
mod subtitles;
mod translation_context;
mod translations;

const SEED_ENTRIES: [(&str, &str, &str); 4] = [
    ("hello", "en", "used as a greeting"),
    ("world", "en", "the earth and all people and things on it"),
    ("こんにちは", "ja", "你好；日间问候语"),
    ("ありがとう", "ja", "谢谢"),
];

const LATEST_SCHEMA_VERSION: u32 = 3;

const MIGRATION_1_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS subtitles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    text TEXT NOT NULL,
    language TEXT,
    started_at REAL,
    ended_at REAL,
    source TEXT NOT NULL DEFAULT 'speaker',
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS subtitle_translations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subtitle_id INTEGER NOT NULL REFERENCES subtitles(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    source_language TEXT,
    target_language TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(subtitle_id, target_language)
);
CREATE TABLE IF NOT EXISTS chatbox_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    original TEXT NOT NULL,
    translation TEXT,
    source_language TEXT,
    target_language TEXT,
    send_mode TEXT NOT NULL,
    message_format TEXT NOT NULL,
    custom_format TEXT,
    rendered_text TEXT NOT NULL,
    char_count INTEGER NOT NULL,
    truncated INTEGER NOT NULL,
    status TEXT NOT NULL,
    error_code TEXT,
    error_detail TEXT,
    resent_from_id INTEGER REFERENCES chatbox_messages(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    sent_at TEXT
);
CREATE TABLE IF NOT EXISTS dictionary (
    term TEXT NOT NULL,
    language TEXT NOT NULL,
    definition TEXT NOT NULL,
    PRIMARY KEY (term, language)
);
CREATE VIRTUAL TABLE IF NOT EXISTS dictionary_fts USING fts5(
    term, definition, content='dictionary', content_rowid='rowid'
);
CREATE TRIGGER IF NOT EXISTS dictionary_ai AFTER INSERT ON dictionary BEGIN
    INSERT INTO dictionary_fts(rowid, term, definition)
    VALUES (new.rowid, new.term, new.definition);
END;
CREATE TABLE IF NOT EXISTS dictionary_sources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL UNIQUE,
    revision TEXT NOT NULL,
    source_language TEXT NOT NULL,
    target_language TEXT,
    entry_count INTEGER NOT NULL DEFAULT 0,
    imported_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS dictionary_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id INTEGER NOT NULL REFERENCES dictionary_sources(id) ON DELETE CASCADE,
    term TEXT NOT NULL,
    reading TEXT NOT NULL DEFAULT '',
    language TEXT NOT NULL,
    definition TEXT NOT NULL,
    score REAL NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS dictionary_entries_term_idx
    ON dictionary_entries(term COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS dictionary_entries_reading_idx
    ON dictionary_entries(reading COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS learning_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    source_text TEXT NOT NULL,
    working_text TEXT NOT NULL,
    selected_text TEXT,
    source_translation TEXT,
    source_language TEXT,
    source_subtitle_ids TEXT NOT NULL DEFAULT '[]',
    dictionary_entries TEXT NOT NULL DEFAULT '[]',
    analysis TEXT,
    draft TEXT,
    anki_note_id INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS learning_items_status_id_idx
    ON learning_items(status, id DESC);
CREATE TABLE IF NOT EXISTS learning_capture_keys (
    key TEXT PRIMARY KEY,
    item_id INTEGER NOT NULL REFERENCES learning_items(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS learning_capture_keys_item_id_idx
    ON learning_capture_keys(item_id);
";

pub struct Database {
    conn: Connection,
    subtitle_history_max_bytes: u64,
}

impl Database {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut database = Self {
            conn,
            subtitle_history_max_bytes: u64::MAX,
        };
        database.initialize()?;
        Ok(database)
    }

    fn initialize(&mut self) -> AppResult<()> {
        let mut version = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))?;
        if version > LATEST_SCHEMA_VERSION {
            return Err(crate::error::AppError::internal(format!(
                "Database schema version {version} is newer than supported version {LATEST_SCHEMA_VERSION}"
            )));
        }
        if version < 1 {
            self.migrate_to_version_1()?;
            version = 1;
        }
        if version < 2 {
            self.migrate_to_version_2()?;
            version = 2;
        }
        if version < 3 {
            self.migrate_to_version_3()?;
        }

        self.initialize_learning_storage()?;
        conversations::initialize_conversations(&self.conn)?;
        for (term, language, definition) in SEED_ENTRIES {
            self.conn.execute(
                "INSERT OR IGNORE INTO dictionary(term, language, definition) VALUES (?, ?, ?)",
                params![term, language, definition],
            )?;
        }
        Ok(())
    }

    fn migrate_to_version_1(&mut self) -> AppResult<()> {
        let transaction = self.conn.transaction()?;
        transaction.execute_batch(MIGRATION_1_SCHEMA)?;
        let has_source = transaction
            .prepare("PRAGMA table_info(subtitles)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .any(|name| name.as_deref() == Ok("source"));
        if !has_source {
            transaction.execute(
                "ALTER TABLE subtitles ADD COLUMN source TEXT NOT NULL DEFAULT 'speaker'",
                [],
            )?;
        }
        transaction.pragma_update(None, "user_version", 1)?;
        transaction.commit()?;
        Ok(())
    }

    fn migrate_to_version_2(&mut self) -> AppResult<()> {
        let transaction = self.conn.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE conversations (
                id TEXT PRIMARY KEY,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                automatic_title TEXT,
                custom_title TEXT,
                icon TEXT,
                updated_at TEXT NOT NULL,
                provisional INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE conversation_metadata (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                legacy_imported INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO conversation_metadata(singleton, legacy_imported) VALUES (1, 0);",
        )?;

        let subtitle_count =
            transaction.query_row("SELECT COUNT(*) FROM subtitles", [], |row| {
                row.get::<_, u64>(0)
            })?;
        let started_at = transaction
            .query_row("SELECT MIN(created_at) FROM subtitles", [], |row| {
                row.get::<_, Option<String>>(0)
            })?
            .unwrap_or_else(crate::models::now_iso8601);
        let conversation_id = conversations::new_public_id(if subtitle_count > 0 {
            "legacy"
        } else {
            "conversation"
        });
        transaction.execute(
            "INSERT INTO conversations(
                id, started_at, ended_at, automatic_title, custom_title, icon,
                updated_at, provisional
             ) VALUES (?1, ?2, NULL, NULL, NULL, NULL, ?2, ?3)",
            params![conversation_id, started_at, subtitle_count > 0],
        )?;
        transaction.execute(
            "ALTER TABLE subtitles
             ADD COLUMN conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE",
            [],
        )?;
        transaction.execute(
            "UPDATE subtitles SET conversation_id = ?1",
            [&conversation_id],
        )?;
        if subtitle_count > 0 {
            let first_text = transaction.query_row(
                "SELECT text FROM subtitles ORDER BY id ASC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )?;
            let updated_at =
                transaction.query_row("SELECT MAX(created_at) FROM subtitles", [], |row| {
                    row.get::<_, String>(0)
                })?;
            transaction.execute(
                "UPDATE conversations SET automatic_title = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    conversations::automatic_title(&first_text),
                    updated_at,
                    conversation_id
                ],
            )?;
        }
        transaction.execute_batch(
            "CREATE INDEX subtitles_conversation_id_id_idx
                 ON subtitles(conversation_id, id DESC);
             CREATE UNIQUE INDEX conversations_single_active_idx
                 ON conversations((1)) WHERE ended_at IS NULL;
             CREATE TRIGGER subtitles_conversation_required_insert
             BEFORE INSERT ON subtitles
             WHEN NEW.conversation_id IS NULL
             BEGIN
                 SELECT RAISE(ABORT, 'subtitles.conversation_id is required');
             END;
             CREATE TRIGGER subtitles_conversation_required_update
             BEFORE UPDATE OF conversation_id ON subtitles
             WHEN NEW.conversation_id IS NULL
             BEGIN
                 SELECT RAISE(ABORT, 'subtitles.conversation_id is required');
             END;",
        )?;
        transaction.pragma_update(None, "user_version", 2)?;
        transaction.commit()?;
        Ok(())
    }

    fn migrate_to_version_3(&mut self) -> AppResult<()> {
        let transaction = self.conn.transaction()?;
        transaction.execute("DELETE FROM subtitles", [])?;
        transaction.execute("DELETE FROM conversations", [])?;
        transaction.execute("DROP TABLE IF EXISTS conversation_metadata", [])?;
        conversations::insert_active(&transaction, &crate::models::now_iso8601())?;
        transaction.pragma_update(None, "user_version", 3)?;
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
            .map(|index| {
                serde_json::json!([format!("term-{index}"), "", "", "", 0, ["definition"]])
            })
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
}
