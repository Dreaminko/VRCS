use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::Database;
use crate::error::AppResult;
use crate::models::Subtitle;

const FTS_MIN_QUERY_CHARS: usize = 3;

const SEARCH_SCHEMA: &str = "
CREATE VIRTUAL TABLE subtitle_search_fts USING fts5(
    text,
    content='',
    columnsize=0,
    tokenize='trigram'
);
CREATE TRIGGER subtitle_search_subtitles_ai AFTER INSERT ON subtitles BEGIN
    INSERT INTO subtitle_search_fts(rowid, text) VALUES (new.id * 2, new.text);
END;
CREATE TRIGGER subtitle_search_subtitles_ad AFTER DELETE ON subtitles BEGIN
    INSERT INTO subtitle_search_fts(subtitle_search_fts, rowid, text)
    VALUES ('delete', old.id * 2, old.text);
END;
CREATE TRIGGER subtitle_search_subtitles_au AFTER UPDATE OF text ON subtitles BEGIN
    INSERT INTO subtitle_search_fts(subtitle_search_fts, rowid, text)
    VALUES ('delete', old.id * 2, old.text);
    INSERT INTO subtitle_search_fts(rowid, text) VALUES (new.id * 2, new.text);
END;
CREATE TRIGGER subtitle_search_translations_ai AFTER INSERT ON subtitle_translations BEGIN
    INSERT INTO subtitle_search_fts(rowid, text) VALUES (new.id * 2 + 1, new.text);
END;
CREATE TRIGGER subtitle_search_translations_ad AFTER DELETE ON subtitle_translations BEGIN
    INSERT INTO subtitle_search_fts(subtitle_search_fts, rowid, text)
    VALUES ('delete', old.id * 2 + 1, old.text);
END;
CREATE TRIGGER subtitle_search_translations_au AFTER UPDATE OF text ON subtitle_translations BEGIN
    INSERT INTO subtitle_search_fts(subtitle_search_fts, rowid, text)
    VALUES ('delete', old.id * 2 + 1, old.text);
    INSERT INTO subtitle_search_fts(rowid, text) VALUES (new.id * 2 + 1, new.text);
END;
INSERT INTO subtitle_search_fts(rowid, text)
SELECT id * 2, text FROM subtitles;
INSERT INTO subtitle_search_fts(rowid, text)
SELECT id * 2 + 1, text FROM subtitle_translations;
";

#[derive(Debug, Clone, Serialize)]
pub struct SubtitleSearchPage {
    pub items: Vec<SubtitleSearchHit>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubtitleSearchHit {
    pub subtitle: Subtitle,
    pub matched_field: &'static str,
    pub matched_text: String,
}

impl Database {
    pub(super) fn initialize_subtitle_search(&mut self) -> AppResult<()> {
        let exists = self.conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_schema
                WHERE type = 'table' AND name = 'subtitle_search_fts'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        if exists {
            return Ok(());
        }

        let transaction = self.conn.transaction()?;
        transaction.execute_batch(SEARCH_SCHEMA)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn search_subtitles(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> AppResult<SubtitleSearchPage> {
        let candidates = if query.chars().count() < FTS_MIN_QUERY_CHARS {
            self.search_short_subtitle_candidates(query, limit, offset)?
        } else {
            self.search_fts_subtitle_candidates(query, limit, offset)?
        };
        let has_more = candidates.len() > limit as usize;
        let mut items = Vec::with_capacity(limit as usize);
        for (subtitle_id, search_rowid) in candidates.into_iter().take(limit as usize) {
            let Some(subtitle) = self.subtitle(subtitle_id)? else {
                continue;
            };
            let (matched_field, matched_text) = if search_rowid % 2 == 0 {
                ("original", subtitle.text.clone())
            } else {
                let translation_id = (search_rowid - 1) / 2;
                let text = self
                    .conn
                    .query_row(
                        "SELECT text FROM subtitle_translations WHERE id = ?1",
                        [translation_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                ("translation", text.unwrap_or_else(|| subtitle.text.clone()))
            };
            items.push(SubtitleSearchHit {
                subtitle,
                matched_field,
                matched_text,
            });
        }
        Ok(SubtitleSearchPage { items, has_more })
    }

    fn search_fts_subtitle_candidates(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<(i64, i64)>> {
        let expression = format!("\"{}\"", query.replace('"', "\"\""));
        let fetch_limit = limit.saturating_add(1);
        let mut statement = self.conn.prepare(
            "WITH matches AS (
                SELECT
                    CASE
                        WHEN subtitle_search_fts.rowid % 2 = 0 THEN subtitle_search_fts.rowid / 2
                        ELSE translation.subtitle_id
                    END AS subtitle_id,
                    subtitle_search_fts.rowid AS search_rowid,
                    subtitle_search_fts.rank AS search_rank,
                    ROW_NUMBER() OVER (
                        PARTITION BY CASE
                            WHEN subtitle_search_fts.rowid % 2 = 0 THEN subtitle_search_fts.rowid / 2
                            ELSE translation.subtitle_id
                        END
                        ORDER BY subtitle_search_fts.rank, subtitle_search_fts.rowid
                    ) AS match_order
                FROM subtitle_search_fts(?1)
                LEFT JOIN subtitle_translations AS translation
                  ON subtitle_search_fts.rowid % 2 = 1
                 AND translation.id = (subtitle_search_fts.rowid - 1) / 2
            )
            SELECT subtitle_id, search_rowid
            FROM matches
            WHERE subtitle_id IS NOT NULL AND match_order = 1
            ORDER BY search_rank, subtitle_id DESC
            LIMIT ?2 OFFSET ?3",
        )?;
        let candidates = statement
            .query_map(params![expression, fetch_limit, offset], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(candidates)
    }

    fn search_short_subtitle_candidates(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<(i64, i64)>> {
        let pattern = format!("%{}%", escape_like(query));
        let fetch_limit = limit.saturating_add(1);
        let mut statement = self.conn.prepare(
            "WITH matches AS (
                SELECT id AS subtitle_id, id * 2 AS search_rowid, 0 AS match_priority
                FROM subtitles WHERE text LIKE ?1 ESCAPE '\\'
                UNION ALL
                SELECT subtitle_id, id * 2 + 1, 1
                FROM subtitle_translations WHERE text LIKE ?1 ESCAPE '\\'
             ), ranked AS (
                SELECT subtitle_id, search_rowid,
                    ROW_NUMBER() OVER (
                        PARTITION BY subtitle_id ORDER BY match_priority, search_rowid
                    ) AS match_order
                FROM matches
             )
             SELECT subtitle_id, search_rowid
             FROM ranked
             WHERE match_order = 1
             ORDER BY subtitle_id DESC
             LIMIT ?2 OFFSET ?3",
        )?;
        let candidates = statement
            .query_map(params![pattern, fetch_limit, offset], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(candidates)
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
