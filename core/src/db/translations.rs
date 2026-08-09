use rusqlite::params;

use super::Database;
use crate::error::AppResult;
use crate::models::SubtitleTranslation;

impl Database {
    pub fn save_translation(
        &self,
        subtitle_id: i64,
        translation: &SubtitleTranslation,
    ) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO subtitle_translations(
                subtitle_id, text, source_language, target_language, provider, model, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(subtitle_id, target_language) DO UPDATE SET
                text = excluded.text,
                source_language = excluded.source_language,
                provider = excluded.provider,
                model = excluded.model,
                created_at = excluded.created_at",
            params![
                subtitle_id,
                translation.text,
                translation.source_language,
                translation.target_language,
                translation.provider,
                translation.model,
                translation.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn translations_for_subtitle(
        &self,
        subtitle_id: i64,
    ) -> AppResult<Vec<SubtitleTranslation>> {
        let mut statement = self.conn.prepare(
            "SELECT text, source_language, target_language, provider, model, created_at
             FROM subtitle_translations WHERE subtitle_id = ? ORDER BY id",
        )?;
        let rows = statement.query_map(params![subtitle_id], |row| {
            Ok(SubtitleTranslation {
                text: row.get(0)?,
                source_language: row.get(1)?,
                target_language: row.get(2)?,
                provider: row.get(3)?,
                model: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}
