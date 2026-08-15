//! SQLite 存储：连接与 schema 初始化。
//! 字幕历史和词典仓储实现分别位于同名子模块中。

use std::path::Path;

use rusqlite::{params, Connection};

use crate::error::AppResult;

mod chatbox;
mod dictionary;
mod subtitles;
mod translation_context;
mod translations;

const SEED_ENTRIES: [(&str, &str, &str); 4] = [
    ("hello", "en", "used as a greeting"),
    ("world", "en", "the earth and all people and things on it"),
    ("こんにちは", "ja", "你好；日间问候语"),
    ("ありがとう", "ja", "谢谢"),
];

const SCHEMA: &str = "
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
";

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let database = Self { conn };
        database.initialize()?;
        Ok(database)
    }

    fn initialize(&self) -> AppResult<()> {
        self.conn.execute_batch(SCHEMA)?;
        // 兼容早期版本的 subtitles 表（没有 source 列）
        let has_source = self
            .conn
            .prepare("PRAGMA table_info(subtitles)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .any(|name| name.as_deref() == Ok("source"));
        if !has_source {
            self.conn.execute(
                "ALTER TABLE subtitles ADD COLUMN source TEXT NOT NULL DEFAULT 'speaker'",
                [],
            )?;
        }
        for (term, language, definition) in SEED_ENTRIES {
            self.conn.execute(
                "INSERT OR IGNORE INTO dictionary(term, language, definition) VALUES (?, ?, ?)",
                params![term, language, definition],
            )?;
        }
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
    fn subtitles_are_trimmed_and_returned_newest_first() {
        let (_path, database) = open_temp_db("history");
        for index in 0..5 {
            database
                .add_subtitle(&subtitle(&format!("line {index}")), 3)
                .unwrap();
        }
        let history = database.subtitle_history(500).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].text, "line 4");
        assert_eq!(history[2].text, "line 2");
        assert!(history[0].id.unwrap() > history[1].id.unwrap());
    }

    #[test]
    fn subtitle_translation_is_saved_and_loaded() {
        let (_path, database) = open_temp_db("translation");
        let saved = database.add_subtitle(&subtitle("hello"), 10).unwrap();
        let translation = SubtitleTranslation {
            text: "你好".into(),
            source_language: Some("en".into()),
            target_language: "zh-Hans".into(),
            provider: "deepl".into(),
            model: None,
            created_at: now_iso8601(),
        };
        database
            .save_translation(saved.id.unwrap(), &translation)
            .unwrap();

        let loaded = database.subtitle(saved.id.unwrap()).unwrap().unwrap();
        assert_eq!(loaded.translations, vec![translation]);
    }

    #[test]
    fn subtitle_history_keeps_batched_translations_with_their_subtitles() {
        let (_path, database) = open_temp_db("history-translations");
        let older = database.add_subtitle(&subtitle("older"), 10).unwrap();
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

        let newer = database.add_subtitle(&subtitle("newer"), 10).unwrap();
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
