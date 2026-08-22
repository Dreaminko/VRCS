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
mod tests;
