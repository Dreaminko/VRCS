use std::collections::HashSet;

use rusqlite::{params, params_from_iter, Statement, ToSql};

use std::sync::Mutex;

use super::Database;
use crate::error::{AppError, AppResult};
use crate::models::{DictionaryEntry, DictionarySource};
use crate::yomitan::{DictionaryMetadata, DictionaryRecord, YomitanImporter};

pub(super) const INSERT_BATCH_SIZE: usize = 500;
const STAGED_TITLE_PREFIX: &str = "\0importing:";

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
    /// 分级查词：精确词条 → 内置精确 → 精确读音 → 词形还原精确 → 词条前缀 → 内置前缀 → 读音前缀。
    pub fn lookup(&self, term: &str, limit: u32) -> AppResult<Vec<DictionaryEntry>> {
        let query = term.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(entries) = self.lookup_exact(query, limit)? {
            return Ok(entries);
        }
        for candidate in deinflect_candidates(query) {
            if let Some(entries) = self.lookup_exact(&candidate, limit)? {
                return Ok(entries);
            }
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

    fn lookup_exact(&self, query: &str, limit: u32) -> AppResult<Option<Vec<DictionaryEntry>>> {
        if let Some(entries) =
            self.lookup_imported("entries.term = ? COLLATE NOCASE", query, limit)?
        {
            return Ok(Some(entries));
        }
        let legacy = self.lookup_legacy("term = ? COLLATE NOCASE", query, limit)?;
        if !legacy.is_empty() {
            return Ok(Some(legacy));
        }
        self.lookup_imported("entries.reading = ? COLLATE NOCASE", query, limit)
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
             WHERE sources.entry_count > 0 AND ({predicate})
             GROUP BY entries.source_id, entries.term, entries.reading,
                      entries.language, entries.definition, sources.title
             ORDER BY LENGTH(entries.term) ASC, MAX(entries.score) DESC, MIN(entries.id)
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
        let sql = format!(
            "SELECT term, language, definition FROM dictionary
             WHERE {predicate}
             ORDER BY LENGTH(term) ASC
             LIMIT ?"
        );
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
    /// 词条分批写入隐藏的暂存来源，每批只短暂持锁，避免长时间阻塞字幕写入。
    pub fn import_yomitan_with_progress(
        db: &Mutex<Database>,
        archive: &[u8],
        mut report_progress: impl FnMut(f64),
        should_cancel: impl Fn() -> bool,
    ) -> AppResult<DictionarySource> {
        let check_cancelled = || {
            if should_cancel() {
                Err(AppError::Conflict("Dictionary import was cancelled".into()))
            } else {
                Ok(())
            }
        };
        let lock = || {
            db.lock()
                .map_err(|_| AppError::internal("Database lock is unavailable"))
        };
        check_cancelled()?;
        report_progress(0.0);
        let importer = YomitanImporter::new(archive).map_err(AppError::validation)?;
        let staged_id = lock()?.stage_dictionary_source(&importer.metadata)?;
        let result = (|| {
            let mut records = Vec::with_capacity(INSERT_BATCH_SIZE);
            let count = importer.for_each_entry_with_progress(
                |record| {
                    check_cancelled()?;
                    records.push(record);
                    if records.len() == INSERT_BATCH_SIZE {
                        lock()?.insert_dictionary_records(staged_id, &mut records)?;
                    }
                    Ok(())
                },
                |progress| report_progress(progress * 0.99),
            )? as i64;
            if !records.is_empty() {
                lock()?.insert_dictionary_records(staged_id, &mut records)?;
            }
            if count == 0 {
                return Err(AppError::validation(
                    "The Yomitan dictionary contains no importable text entries",
                ));
            }
            check_cancelled()?;
            lock()?.publish_dictionary_source(staged_id, &importer.metadata.title, count)
        })();
        match &result {
            Ok(_) => report_progress(1.0),
            Err(_) => {
                if let Ok(db) = lock() {
                    let _ = db.delete_dictionary_source(staged_id);
                }
            }
        }
        result
    }

    /// 暂存来源的 entry_count 为 0，对查询和列表不可见。
    fn stage_dictionary_source(&self, metadata: &DictionaryMetadata) -> AppResult<i64> {
        self.conn.execute(
            "INSERT INTO dictionary_sources(
                 title, revision, source_language, target_language, entry_count, imported_at
             ) VALUES (?, ?, ?, ?, 0, ?)",
            params![
                format!("{STAGED_TITLE_PREFIX}{}", uuid::Uuid::new_v4()),
                metadata.revision,
                metadata.source_language,
                metadata.target_language,
                crate::models::now_iso8601(),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn insert_dictionary_records(
        &self,
        source_id: i64,
        records: &mut Vec<DictionaryRecord>,
    ) -> AppResult<()> {
        let mut statement = self.conn.prepare_cached(&insert_sql(records.len()))?;
        insert_batch(&mut statement, source_id, records)
    }

    fn publish_dictionary_source(
        &mut self,
        staged_id: i64,
        title: &str,
        entry_count: i64,
    ) -> AppResult<DictionarySource> {
        let transaction = self.conn.transaction()?;
        transaction.execute(
            "DELETE FROM dictionary_sources WHERE title = ?",
            params![title],
        )?;
        transaction.execute(
            "UPDATE dictionary_sources SET title = ?, entry_count = ? WHERE id = ?",
            params![title, entry_count, staged_id],
        )?;
        transaction.commit()?;
        self.dictionary_source(staged_id)
    }

    /// 清理进程中断时遗留的暂存来源。
    pub(super) fn remove_staged_dictionary_sources(&self) -> AppResult<()> {
        self.conn
            .execute("DELETE FROM dictionary_sources WHERE entry_count = 0", [])?;
        Ok(())
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
                 FROM dictionary_sources WHERE entry_count > 0
                 ORDER BY imported_at DESC, id DESC",
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

fn deinflect_candidates(query: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    seen.insert(query.to_owned());
    let mut push = |candidate: String| {
        if !candidate.is_empty() && seen.insert(candidate.clone()) {
            candidates.push(candidate);
        }
    };

    for (suffix, replacements) in [
        ("ませんでした", &["る", ""][..]),
        ("ました", &["る", ""]),
        ("ません", &["る", ""]),
        ("ます", &["る", ""]),
        ("ないで", &["る", ""]),
        ("なかった", &["る", "い", ""]),
        ("ない", &["る", "い", ""]),
        ("たかった", &["る", ""]),
        ("たい", &["る", ""]),
        ("ている", &["る", ""]),
        ("てる", &["る", ""]),
        ("ていた", &["る", ""]),
        ("てた", &["る", ""]),
        ("てしまう", &["る", ""]),
        ("ちゃう", &["る", ""]),
        ("られる", &["る", ""]),
        ("させる", &["る", ""]),
        ("って", &["う", "つ", "る"]),
        ("った", &["う", "つ", "る"]),
        ("いて", &["く", "る"]),
        ("いた", &["く", "る"]),
        ("いで", &["ぐ", "る"]),
        ("いだ", &["ぐ", "る"]),
        ("んで", &["む", "ぶ", "ぬ"]),
        ("んだ", &["む", "ぶ", "ぬ"]),
        ("して", &["す", "する", ""]),
        ("した", &["す", "する", ""]),
        ("て", &["る", ""]),
        ("た", &["る", ""]),
        ("かった", &["い"]),
        ("くない", &["い"]),
        ("くて", &["い"]),
        ("ければ", &["い"]),
    ] {
        if let Some(stem) = query.strip_suffix(suffix) {
            if !stem.is_empty() {
                for replacement in replacements {
                    push(format!("{stem}{replacement}"));
                }
                push_godan_bases(stem, &mut push);
            }
        }
    }

    if query.is_ascii() && query.len() >= 4 {
        let lower = query.to_ascii_lowercase();
        for (suffix, replacements) in [
            ("ies", &["y"][..]),
            ("ied", &["y"]),
            ("ing", &["", "e"]),
            ("ed", &["", "e"]),
            ("es", &["", "e"]),
            ("s", &[""]),
        ] {
            if let Some(stem) = lower.strip_suffix(suffix) {
                if stem.len() >= 2 {
                    for replacement in replacements {
                        push(format!("{stem}{replacement}"));
                    }
                    if matches!(suffix, "ing" | "ed") && has_doubled_trailing_consonant(stem) {
                        push(stem[..stem.len() - 1].to_owned());
                    }
                }
            }
        }
    } else if !query.is_ascii() {
        let chars: Vec<char> = query.chars().collect();
        for len in (1..chars.len()).rev().take(3) {
            let prefix: String = chars[..len].iter().collect();
            if len >= 2 || prefix.chars().all(is_kanji) {
                push(prefix);
            }
        }
    }

    candidates
}

fn push_godan_bases(stem: &str, push: &mut impl FnMut(String)) {
    if let Some(root) = stem.strip_suffix('っ') {
        if !root.is_empty() {
            for ending in ['る', 'う', 'つ'] {
                push(format!("{root}{ending}"));
            }
        }
        return;
    }
    let Some(last) = stem.chars().last() else {
        return;
    };
    let root = &stem[..stem.len() - last.len_utf8()];
    if root.is_empty() {
        return;
    }
    if last == 'し' {
        push(format!("{root}する"));
        push(root.to_owned());
    }
    let mapped = match last {
        'き' | 'か' => Some('く'),
        'ぎ' | 'が' => Some('ぐ'),
        'し' | 'さ' => Some('す'),
        'ち' | 'た' => Some('つ'),
        'に' | 'な' => Some('ぬ'),
        'び' | 'ば' => Some('ぶ'),
        'み' | 'ま' => Some('む'),
        'り' | 'ら' => Some('る'),
        'い' | 'わ' => Some('う'),
        _ => None,
    };
    if let Some(ending) = mapped {
        push(format!("{root}{ending}"));
    }
}

fn has_doubled_trailing_consonant(stem: &str) -> bool {
    let bytes = stem.as_bytes();
    if bytes.len() < 3 {
        return false;
    }
    let last = bytes[bytes.len() - 1];
    last == bytes[bytes.len() - 2]
        && last.is_ascii_alphabetic()
        && !matches!(last, b'a' | b'e' | b'i' | b'o' | b'u')
}

fn is_kanji(character: char) -> bool {
    matches!(character, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}')
}
