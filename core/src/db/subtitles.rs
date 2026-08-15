use rusqlite::params;

use super::Database;
use crate::error::AppResult;
use crate::models::{Subtitle, SubtitleTranslation};

impl Database {
    /// 写入字幕并把历史裁剪到 limit 条，返回带 id 的记录。
    /// 当前由后续阶段的识别管线调用。
    #[allow(dead_code)]
    pub fn add_subtitle(&self, subtitle: &Subtitle, limit: u32) -> AppResult<Subtitle> {
        self.conn.execute(
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
        )?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute(
            "DELETE FROM subtitles WHERE id NOT IN (SELECT id FROM subtitles ORDER BY id DESC LIMIT ?)",
            params![limit],
        )?;
        let mut saved = subtitle.clone();
        saved.id = Some(id);
        saved.translations.clear();
        Ok(saved)
    }

    /// 历史按 id 倒序返回（最新的在前）。
    pub fn subtitle_history(&self, limit: u32) -> AppResult<Vec<Subtitle>> {
        let mut statement = self.conn.prepare(
            "SELECT recent.id, recent.text, recent.language, recent.started_at,
                    recent.ended_at, recent.source, recent.created_at,
                    translation.id, translation.text, translation.source_language,
                    translation.target_language, translation.provider,
                    translation.model, translation.created_at
             FROM (
                 SELECT id, text, language, started_at, ended_at, source, created_at
                 FROM subtitles ORDER BY id DESC LIMIT ?
             ) AS recent
             LEFT JOIN subtitle_translations AS translation
               ON translation.subtitle_id = recent.id
             ORDER BY recent.id DESC, translation.id",
        )?;
        let rows = statement.query_map(params![limit], |row| {
            let subtitle = subtitle_from_row(row)?;
            let translation = if row.get::<_, Option<i64>>(7)?.is_some() {
                Some(SubtitleTranslation {
                    text: row.get(8)?,
                    source_language: row.get(9)?,
                    target_language: row.get(10)?,
                    provider: row.get(11)?,
                    model: row.get(12)?,
                    created_at: row.get(13)?,
                })
            } else {
                None
            };
            Ok((subtitle, translation))
        })?;

        let mut subtitles: Vec<Subtitle> = Vec::new();
        for row in rows {
            let (mut subtitle, translation) = row?;
            if let Some(current) = subtitles
                .last_mut()
                .filter(|current| current.id == subtitle.id)
            {
                if let Some(translation) = translation {
                    current.translations.push(translation);
                }
            } else {
                if let Some(translation) = translation {
                    subtitle.translations.push(translation);
                }
                subtitles.push(subtitle);
            }
        }
        Ok(subtitles)
    }

    pub fn subtitle(&self, id: i64) -> AppResult<Option<Subtitle>> {
        let mut statement = self.conn.prepare(
            "SELECT id, text, language, started_at, ended_at, source, created_at
             FROM subtitles WHERE id = ?",
        )?;
        let mut rows = statement.query(params![id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let mut subtitle = subtitle_from_row(row)?;
        drop(rows);
        drop(statement);
        subtitle.translations = self.translations_for_subtitle(id)?;
        Ok(Some(subtitle))
    }
}

fn subtitle_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Subtitle> {
    Ok(Subtitle {
        id: Some(row.get(0)?),
        text: row.get(1)?,
        language: row.get(2)?,
        started_at: row.get(3)?,
        ended_at: row.get(4)?,
        source: row.get(5)?,
        created_at: row.get(6)?,
        translations: Vec::new(),
    })
}
