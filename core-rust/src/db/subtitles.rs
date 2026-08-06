use rusqlite::params;

use super::Database;
use crate::error::AppResult;
use crate::models::Subtitle;

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
        Ok(saved)
    }

    /// 历史按 id 倒序返回（最新的在前）。
    pub fn subtitle_history(&self, limit: u32) -> AppResult<Vec<Subtitle>> {
        let mut statement = self.conn.prepare(
            "SELECT id, text, language, started_at, ended_at, source, created_at
                      FROM subtitles ORDER BY id DESC LIMIT ?",
        )?;
        let rows = statement.query_map(params![limit], |row| {
            Ok(Subtitle {
                id: Some(row.get(0)?),
                text: row.get(1)?,
                language: row.get(2)?,
                started_at: row.get(3)?,
                ended_at: row.get(4)?,
                source: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}
