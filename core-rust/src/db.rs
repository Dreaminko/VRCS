//! SQLite 存储：字幕历史与词典。
//! DDL 与 Python 版 `app/database.py` 完全一致，可直接复用已有的 vrcs.db 数据文件。

use std::path::Path;

use rusqlite::{params, Connection};

use crate::models::{DictionaryEntry, DictionarySource, Subtitle};
use crate::yomitan::YomitanImporter;

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
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| e.to_string())?;
        let db = Self { conn };
        db.initialize()?;
        Ok(db)
    }

    fn initialize(&self) -> Result<(), String> {
        self.conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        // 兼容早期版本的 subtitles 表（没有 source 列）
        let has_source: bool = self
            .conn
            .prepare("PRAGMA table_info(subtitles)")
            .map_err(|e| e.to_string())?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .any(|name| name.as_deref() == Ok("source"));
        if !has_source {
            self.conn
                .execute(
                    "ALTER TABLE subtitles ADD COLUMN source TEXT NOT NULL DEFAULT 'speaker'",
                    [],
                )
                .map_err(|e| e.to_string())?;
        }
        for (term, language, definition) in SEED_ENTRIES {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO dictionary(term, language, definition) VALUES (?, ?, ?)",
                    params![term, language, definition],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// 写入字幕并把历史裁剪到 limit 条，返回带 id 的记录。
    /// 当前由后续阶段的识别管线调用。
    #[allow(dead_code)]
    pub fn add_subtitle(&self, subtitle: &Subtitle, limit: u32) -> Result<Subtitle, String> {
        self.conn
            .execute(
                "INSERT INTO subtitles(text, language, started_at, ended_at, source, created_at)
                 VALUES (?, ?, ?, ?, ?, ?)",
                params![
                    subtitle.text,
                    subtitle.language,
                    subtitle.started_at,
                    subtitle.ended_at,
                    subtitle.source,
                    subtitle.created_at,
                ],
            )
            .map_err(|e| e.to_string())?;
        let id = self.conn.last_insert_rowid();
        self.conn
            .execute(
                "DELETE FROM subtitles WHERE id NOT IN (SELECT id FROM subtitles ORDER BY id DESC LIMIT ?)",
                params![limit],
            )
            .map_err(|e| e.to_string())?;
        let mut saved = subtitle.clone();
        saved.id = Some(id);
        Ok(saved)
    }

    /// 历史按 id 倒序返回（最新的在前）。
    pub fn subtitle_history(&self, limit: u32) -> Result<Vec<Subtitle>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, text, language, started_at, ended_at, source, created_at
                      FROM subtitles ORDER BY id DESC LIMIT ?",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok(Subtitle {
                    id: Some(row.get(0)?),
                    text: row.get(1)?,
                    language: row.get(2)?,
                    started_at: row.get(3)?,
                    ended_at: row.get(4)?,
                    source: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// 分级查词：精确词条 → 内置精确 → 精确读音 → 词条前缀 → 内置前缀 → 读音前缀。
    pub fn lookup(&self, term: &str, limit: u32) -> Result<Vec<DictionaryEntry>, String> {
        let query = term.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(entries) =
            self.lookup_imported("entries.term = ? COLLATE NOCASE", query, limit)?
        {
            return Ok(entries);
        }
        let legacy = self.lookup_legacy("term = ? COLLATE NOCASE", query, limit)?;
        if !legacy.is_empty() {
            return Ok(legacy);
        }
        if let Some(entries) =
            self.lookup_imported("entries.reading = ? COLLATE NOCASE", query, limit)?
        {
            return Ok(entries);
        }
        if let Some(entries) = self.lookup_imported(
            "entries.term LIKE ? COLLATE NOCASE",
            &format!("{query}%"),
            limit,
        )? {
            return Ok(entries);
        }
        let legacy_prefix =
            self.lookup_legacy("term LIKE ? COLLATE NOCASE", &format!("{query}%"), limit)?;
        if !legacy_prefix.is_empty() {
            return Ok(legacy_prefix);
        }
        Ok(self
            .lookup_imported(
                "entries.reading LIKE ? COLLATE NOCASE",
                &format!("{query}%"),
                limit,
            )?
            .unwrap_or_default())
    }

    /// 查询导入的 Yomitan 词条；无结果时返回 None 以便进入下一级回退。
    /// predicate 只允许传入上方lookup中的固定字符串，不做外部拼接。
    fn lookup_imported(
        &self,
        predicate: &str,
        value: &str,
        limit: u32,
    ) -> Result<Option<Vec<DictionaryEntry>>, String> {
        let sql = format!(
            "SELECT entries.term, entries.reading, entries.language, entries.definition,
                    sources.title AS dictionary
             FROM dictionary_entries AS entries
             JOIN dictionary_sources AS sources ON sources.id = entries.source_id
             WHERE {predicate}
             GROUP BY entries.source_id, entries.term, entries.reading,
                      entries.language, entries.definition, sources.title
             ORDER BY MAX(entries.score) DESC, MIN(entries.id)
             LIMIT ?"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![value, limit * 8], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut entries = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for row in rows {
            let (term, reading, language, definition, dictionary) =
                row.map_err(|e| e.to_string())?;
            // 同一词典内按（读音、语言、释义）去重
            if !seen.insert((
                dictionary.clone(),
                reading.clone(),
                language.clone(),
                definition.clone(),
            )) {
                continue;
            }
            entries.push(DictionaryEntry {
                term,
                reading: Some(reading),
                language,
                definition,
                dictionary: Some(dictionary),
            });
            if entries.len() == limit as usize {
                break;
            }
        }
        Ok((!entries.is_empty()).then_some(entries))
    }

    fn lookup_legacy(
        &self,
        predicate: &str,
        value: &str,
        limit: u32,
    ) -> Result<Vec<DictionaryEntry>, String> {
        let sql =
            format!("SELECT term, language, definition FROM dictionary WHERE {predicate} LIMIT ?");
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![value, limit], |row| {
                Ok(DictionaryEntry {
                    term: row.get(0)?,
                    language: row.get(1)?,
                    definition: row.get(2)?,
                    reading: None,
                    dictionary: None,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// 导入 Yomitan 词典包；同名词典整体替换。
    pub fn import_yomitan(&mut self, archive: &[u8]) -> Result<DictionarySource, String> {
        let importer = YomitanImporter::new(archive)?;
        let imported_at = crate::models::now_iso8601();
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO dictionary_sources(
                 title, revision, source_language, target_language, entry_count, imported_at
             ) VALUES (?, ?, ?, ?, 0, ?)
             ON CONFLICT(title) DO UPDATE SET
                 revision = excluded.revision,
                 source_language = excluded.source_language,
                 target_language = excluded.target_language,
                 entry_count = 0,
                 imported_at = excluded.imported_at",
            params![
                importer.metadata.title,
                importer.metadata.revision,
                importer.metadata.source_language,
                importer.metadata.target_language,
                imported_at,
            ],
        )
        .map_err(|e| e.to_string())?;
        let source_id: i64 = tx
            .query_row(
                "SELECT id FROM dictionary_sources WHERE title = ?",
                params![importer.metadata.title],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM dictionary_entries WHERE source_id = ?",
            params![source_id],
        )
        .map_err(|e| e.to_string())?;

        let count = {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO dictionary_entries(source_id, term, reading, language, definition, score)
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .map_err(|e| e.to_string())?;
            importer.for_each_entry(|record| {
                stmt.execute(params![
                    source_id,
                    record.term,
                    record.reading,
                    record.language,
                    record.definition,
                    record.score,
                ])
                .map_err(|e| e.to_string())?;
                Ok(())
            })? as i64
        };
        if count == 0 {
            return Err("Yomitan 词典中没有可导入的文本词条".into());
        }
        tx.execute(
            "UPDATE dictionary_sources SET entry_count = ? WHERE id = ?",
            params![count, source_id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        self.dictionary_source(source_id)
    }

    pub fn dictionary_source(&self, source_id: i64) -> Result<DictionarySource, String> {
        self.conn
            .query_row(
                "SELECT id, title, revision, source_language, target_language, entry_count, imported_at
                 FROM dictionary_sources WHERE id = ?",
                params![source_id],
                |row| {
                    Ok(DictionarySource {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        revision: row.get(2)?,
                        source_language: row.get(3)?,
                        target_language: row.get(4)?,
                        entry_count: row.get(5)?,
                        imported_at: row.get(6)?,
                    })
                },
            )
            .map_err(|e| e.to_string())
    }

    pub fn dictionary_sources(&self) -> Result<Vec<DictionarySource>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, title, revision, source_language, target_language, entry_count, imported_at
                 FROM dictionary_sources ORDER BY imported_at DESC, id DESC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(DictionarySource {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    revision: row.get(2)?,
                    source_language: row.get(3)?,
                    target_language: row.get(4)?,
                    entry_count: row.get(5)?,
                    imported_at: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    pub fn delete_dictionary_source(&self, source_id: i64) -> Result<bool, String> {
        let affected = self
            .conn
            .execute(
                "DELETE FROM dictionary_sources WHERE id = ?",
                params![source_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::now_iso8601;

    fn open_temp_db(name: &str) -> (std::path::PathBuf, Database) {
        let dir = std::env::temp_dir().join(format!("vrcs-db-{}-{}", name, std::process::id()));
        let path = dir.join("vrcs.db");
        let _ = std::fs::remove_file(&path);
        let db = Database::open(&path).unwrap();
        (path, db)
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
        }
    }

    #[test]
    fn subtitles_are_trimmed_and_returned_newest_first() {
        let (_path, db) = open_temp_db("history");
        for i in 0..5 {
            db.add_subtitle(&subtitle(&format!("line {i}")), 3).unwrap();
        }
        let history = db.subtitle_history(500).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].text, "line 4");
        assert_eq!(history[2].text, "line 2");
        assert!(history[0].id.unwrap() > history[1].id.unwrap());
    }

    #[test]
    fn seed_dictionary_lookup_works() {
        let (_path, db) = open_temp_db("seed");
        let entries = db.lookup("こんにちは", 10).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].language, "ja");
        assert!(entries[0].dictionary.is_none());
    }
}
