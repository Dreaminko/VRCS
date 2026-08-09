use std::collections::HashSet;

use rusqlite::{params, params_from_iter, Statement, ToSql};

use super::Database;
use crate::error::{AppError, AppResult};
use crate::models::{DictionaryEntry, DictionarySource};
use crate::yomitan::{DictionaryRecord, YomitanImporter};

pub(super) const INSERT_BATCH_SIZE: usize = 500;

pub(super) fn like_prefix(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 1);
    for character in value.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped.push('%');
    escaped
}

fn insert_sql(record_count: usize) -> String {
    let placeholders = (0..record_count)
        .map(|_| "(?, ?, ?, ?, ?, ?)")
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "INSERT INTO dictionary_entries(source_id, term, reading, language, definition, score) VALUES {placeholders}"
    )
}

fn insert_batch(
    statement: &mut Statement<'_>,
    source_id: i64,
    records: &mut Vec<DictionaryRecord>,
) -> AppResult<()> {
    let parameters = records.iter().flat_map(|record| {
        [
            &source_id as &dyn ToSql,
            &record.term as &dyn ToSql,
            &record.reading as &dyn ToSql,
            &record.language as &dyn ToSql,
            &record.definition as &dyn ToSql,
            &record.score as &dyn ToSql,
        ]
    });
    statement.execute(params_from_iter(parameters))?;
    records.clear();
    Ok(())
}

impl Database {
    /// 分级查词：精确词条 → 内置精确 → 精确读音 → 词条前缀 → 内置前缀 → 读音前缀。
    pub fn lookup(&self, term: &str, limit: u32) -> AppResult<Vec<DictionaryEntry>> {
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
        let prefix = like_prefix(query);
        if let Some(entries) = self.lookup_imported(
            "entries.term LIKE ? ESCAPE '\\' COLLATE NOCASE",
            &prefix,
            limit,
        )? {
            return Ok(entries);
        }
        let legacy_prefix =
            self.lookup_legacy("term LIKE ? ESCAPE '\\' COLLATE NOCASE", &prefix, limit)?;
        if !legacy_prefix.is_empty() {
            return Ok(legacy_prefix);
        }
        Ok(self
            .lookup_imported(
                "entries.reading LIKE ? ESCAPE '\\' COLLATE NOCASE",
                &prefix,
                limit,
            )?
            .unwrap_or_default())
    }

    /// 查询导入的 Yomitan 词条；无结果时返回 None 以便进入下一级回退。
    /// predicate 只允许传入 lookup 中的固定字符串，不做外部拼接。
    fn lookup_imported(
        &self,
        predicate: &str,
        value: &str,
        limit: u32,
    ) -> AppResult<Option<Vec<DictionaryEntry>>> {
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
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params![value, limit * 8], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        for row in rows {
            let (term, reading, language, definition, dictionary) = row?;
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
    ) -> AppResult<Vec<DictionaryEntry>> {
        let sql =
            format!("SELECT term, language, definition FROM dictionary WHERE {predicate} LIMIT ?");
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params![value, limit], |row| {
            Ok(DictionaryEntry {
                term: row.get(0)?,
                language: row.get(1)?,
                definition: row.get(2)?,
                reading: None,
                dictionary: None,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 导入 Yomitan 词典包；同名词典整体替换。
    #[cfg(test)]
    pub fn import_yomitan(&mut self, archive: &[u8]) -> AppResult<DictionarySource> {
        self.import_yomitan_with_progress(archive, |_| {})
    }

    pub fn import_yomitan_with_progress(
        &mut self,
        archive: &[u8],
        mut report_progress: impl FnMut(f64),
    ) -> AppResult<DictionarySource> {
        report_progress(0.0);
        let importer = YomitanImporter::new(archive).map_err(AppError::validation)?;
        let imported_at = crate::models::now_iso8601();
        let transaction = self.conn.transaction()?;
        transaction.execute(
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
        )?;
        let source_id: i64 = transaction.query_row(
            "SELECT id FROM dictionary_sources WHERE title = ?",
            params![importer.metadata.title],
            |row| row.get(0),
        )?;
        transaction.execute(
            "DELETE FROM dictionary_entries WHERE source_id = ?",
            params![source_id],
        )?;

        let mut records = Vec::with_capacity(INSERT_BATCH_SIZE);
        let count = {
            let sql = insert_sql(INSERT_BATCH_SIZE);
            let mut statement = transaction.prepare(&sql)?;
            importer.for_each_entry_with_progress(
                |record| {
                    records.push(record);
                    if records.len() == INSERT_BATCH_SIZE {
                        insert_batch(&mut statement, source_id, &mut records)?;
                    }
                    Ok(())
                },
                |progress| report_progress(progress * 0.99),
            )? as i64
        };
        if !records.is_empty() {
            let sql = insert_sql(records.len());
            let mut statement = transaction.prepare(&sql)?;
            insert_batch(&mut statement, source_id, &mut records)?;
        }
        if count == 0 {
            return Err(AppError::validation(
                "The Yomitan dictionary contains no importable text entries",
            ));
        }
        transaction.execute(
            "UPDATE dictionary_sources SET entry_count = ? WHERE id = ?",
            params![count, source_id],
        )?;
        transaction.commit()?;
        report_progress(1.0);
        self.dictionary_source(source_id)
    }

    pub fn dictionary_source(&self, source_id: i64) -> AppResult<DictionarySource> {
        Ok(self.conn.query_row(
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
        )?)
    }

    pub fn dictionary_sources(&self) -> AppResult<Vec<DictionarySource>> {
        let mut statement = self.conn.prepare(
            "SELECT id, title, revision, source_language, target_language, entry_count, imported_at
                 FROM dictionary_sources ORDER BY imported_at DESC, id DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(DictionarySource {
                id: row.get(0)?,
                title: row.get(1)?,
                revision: row.get(2)?,
                source_language: row.get(3)?,
                target_language: row.get(4)?,
                entry_count: row.get(5)?,
                imported_at: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn delete_dictionary_source(&self, source_id: i64) -> AppResult<bool> {
        let affected = self.conn.execute(
            "DELETE FROM dictionary_sources WHERE id = ?",
            params![source_id],
        )?;
        Ok(affected > 0)
    }
}
