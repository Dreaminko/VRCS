use rusqlite::params;

use super::Database;
use crate::error::AppResult;
use crate::models::SubtitleTranslation;

impl Database {
    pub fn save_translation(
        &self,
        subtitle_id: i64,
        translation: &SubtitleTranslation,
    ) -> AppResult<bool> {
        self.save_translation_group(&[subtitle_id], translation)
    }

    pub fn save_translation_group(
        &self,
        subtitle_ids: &[i64],
        translation: &SubtitleTranslation,
    ) -> AppResult<bool> {
        let source_group = translation
            .source_group
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        let transaction = self.conn.unchecked_transaction()?;
        for subtitle_id in subtitle_ids {
            transaction.execute(
            "INSERT INTO subtitle_translations(
                subtitle_id, text, source_language, target_language, provider, model, created_at, source_group
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(subtitle_id, target_language) DO UPDATE SET
                text = excluded.text,
                source_language = excluded.source_language,
                provider = excluded.provider,
                model = excluded.model,
                created_at = excluded.created_at,
                source_group = excluded.source_group",
            params![
                subtitle_id,
                translation.text,
                translation.source_language,
                translation.target_language,
                translation.provider,
                translation.model,
                translation.created_at,
                source_group,
            ],
        )?;
        }
        transaction.commit()?;
        let write_estimate = (translation.text.len() as u64)
            .saturating_mul(subtitle_ids.len() as u64)
            .saturating_mul(4)
            .saturating_add(4096);
        match self.trim_subtitle_history_after_write(write_estimate) {
            Ok(catalog_changed) => Ok(catalog_changed),
            Err(error) => {
                tracing::warn!(%error, "translation saved but history quota maintenance failed");
                Ok(true)
            }
        }
    }

    pub fn translations_for_subtitle(
        &self,
        subtitle_id: i64,
    ) -> AppResult<Vec<SubtitleTranslation>> {
        let mut statement = self.conn.prepare(
            "SELECT text, source_language, target_language, provider, model, created_at, source_group
             FROM subtitle_translations WHERE subtitle_id = ? ORDER BY id",
        )?;
        let rows = statement.query_map(params![subtitle_id], |row| {
            Ok(SubtitleTranslation {
                source_group: read_source_group(row, 6)?,
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

pub(super) fn read_source_group(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<crate::models::TranslationSourceGroup>> {
    row.get::<_, Option<String>>(index)?
        .map(|value| {
            serde_json::from_str(&value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()
}
