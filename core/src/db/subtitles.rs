use rusqlite::params;
use serde::Serialize;

use super::conversations::{active_conversation_id, automatic_title};
use super::Database;
use crate::error::AppResult;
use crate::models::{Subtitle, SubtitleTranslation};

const HISTORY_SQL: &str = "SELECT recent.id, recent.conversation_id, recent.text, recent.language,
            recent.started_at, recent.ended_at, recent.source, recent.created_at,
            translation.id, translation.text, translation.source_language,
            translation.target_language, translation.provider,
            translation.model, translation.created_at
     FROM (
         SELECT id, conversation_id, text, language, started_at, ended_at, source, created_at
         FROM subtitles
         ORDER BY id DESC LIMIT ?1
     ) AS recent
     LEFT JOIN subtitle_translations AS translation
       ON translation.subtitle_id = recent.id
     ORDER BY recent.id DESC, translation.id";

const HISTORY_BEFORE_SQL: &str =
    "SELECT recent.id, recent.conversation_id, recent.text, recent.language,
            recent.started_at, recent.ended_at, recent.source, recent.created_at,
            translation.id, translation.text, translation.source_language,
            translation.target_language, translation.provider,
            translation.model, translation.created_at
     FROM (
         SELECT id, conversation_id, text, language, started_at, ended_at, source, created_at
         FROM subtitles
         WHERE id < ?1
         ORDER BY id DESC LIMIT ?2
     ) AS recent
     LEFT JOIN subtitle_translations AS translation
       ON translation.subtitle_id = recent.id
     ORDER BY recent.id DESC, translation.id";

const CONVERSATION_HISTORY_SQL: &str =
    "SELECT recent.id, recent.conversation_id, recent.text, recent.language,
            recent.started_at, recent.ended_at, recent.source, recent.created_at,
            translation.id, translation.text, translation.source_language,
            translation.target_language, translation.provider,
            translation.model, translation.created_at
     FROM (
         SELECT id, conversation_id, text, language, started_at, ended_at, source, created_at
         FROM subtitles
         WHERE conversation_id = ?1
         ORDER BY id DESC LIMIT ?2
     ) AS recent
     LEFT JOIN subtitle_translations AS translation
       ON translation.subtitle_id = recent.id
     ORDER BY recent.id DESC, translation.id";

const CONVERSATION_HISTORY_BEFORE_SQL: &str =
    "SELECT recent.id, recent.conversation_id, recent.text, recent.language,
            recent.started_at, recent.ended_at, recent.source, recent.created_at,
            translation.id, translation.text, translation.source_language,
            translation.target_language, translation.provider,
            translation.model, translation.created_at
     FROM (
         SELECT id, conversation_id, text, language, started_at, ended_at, source, created_at
         FROM subtitles
         WHERE conversation_id = ?1 AND id < ?2
         ORDER BY id DESC LIMIT ?3
     ) AS recent
     LEFT JOIN subtitle_translations AS translation
       ON translation.subtitle_id = recent.id
     ORDER BY recent.id DESC, translation.id";

#[derive(Debug, Clone, Serialize)]
pub struct ConversationSubtitlePage {
    pub items: Vec<Subtitle>,
    pub has_more: bool,
    pub next_before_id: Option<i64>,
}

impl Database {
    /// 写入字幕，并在数据库锁内确定当前 active 会话。
    #[allow(dead_code)]
    pub fn add_subtitle(&self, subtitle: &Subtitle) -> AppResult<Subtitle> {
        let transaction = self.conn.unchecked_transaction()?;
        let conversation_id = active_conversation_id(&transaction)?;
        let existing_count = transaction.query_row(
            "SELECT COUNT(*) FROM subtitles WHERE conversation_id = ?1",
            [&conversation_id],
            |row| row.get::<_, u64>(0),
        )?;
        transaction.execute(
            "INSERT INTO subtitles(
                conversation_id, text, language, started_at, ended_at, source, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                conversation_id,
                subtitle.text,
                subtitle.language,
                subtitle.started_at,
                subtitle.ended_at,
                subtitle.source,
                subtitle.created_at,
            ],
        )?;
        let id = transaction.last_insert_rowid();
        if existing_count == 0 {
            transaction.execute(
                "UPDATE conversations
                 SET automatic_title = ?1, updated_at = ?2
                 WHERE id = ?3",
                params![
                    automatic_title(&subtitle.text),
                    subtitle.created_at,
                    conversation_id
                ],
            )?;
        } else {
            transaction.execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                params![subtitle.created_at, conversation_id],
            )?;
        }
        transaction.commit()?;
        if let Err(error) = self.trim_subtitle_history_to_size() {
            tracing::warn!(%error, "subtitle saved but history quota maintenance failed");
        }

        let mut saved = subtitle.clone();
        saved.id = Some(id);
        saved.conversation_id = Some(conversation_id);
        saved.translations.clear();
        Ok(saved)
    }

    /// 历史按 id 倒序返回（最新的在前）。
    pub fn subtitle_history(&self, limit: u32) -> AppResult<Vec<Subtitle>> {
        let mut statement = self.conn.prepare(HISTORY_SQL)?;
        collect_subtitles(&mut statement, [limit])
    }

    pub fn subtitle_history_before(&self, limit: u32, before_id: i64) -> AppResult<Vec<Subtitle>> {
        let mut statement = self.conn.prepare(HISTORY_BEFORE_SQL)?;
        collect_subtitles(&mut statement, params![before_id, limit])
    }

    pub fn conversation_subtitles(
        &self,
        conversation_id: &str,
        limit: u32,
        before_id: Option<i64>,
    ) -> AppResult<Option<ConversationSubtitlePage>> {
        let exists = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1)",
            [conversation_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Ok(None);
        }

        let fetch_limit = limit.saturating_add(1);
        let mut items = match before_id {
            Some(before_id) => {
                let mut statement = self.conn.prepare(CONVERSATION_HISTORY_BEFORE_SQL)?;
                collect_subtitles(
                    &mut statement,
                    params![conversation_id, before_id, fetch_limit],
                )?
            }
            None => {
                let mut statement = self.conn.prepare(CONVERSATION_HISTORY_SQL)?;
                collect_subtitles(&mut statement, params![conversation_id, fetch_limit])?
            }
        };
        let has_more = items.len() > limit as usize;
        if has_more {
            items.truncate(limit as usize);
        }
        let next_before_id = has_more
            .then(|| items.last().and_then(|item| item.id))
            .flatten();
        Ok(Some(ConversationSubtitlePage {
            items,
            has_more,
            next_before_id,
        }))
    }

    pub fn delete_subtitle_range(
        &self,
        started_at: &str,
        ended_at: Option<&str>,
    ) -> AppResult<u64> {
        let deleted = match ended_at {
            Some(ended_at) => self.conn.execute(
                "DELETE FROM subtitles WHERE created_at >= ?1 AND created_at < ?2",
                params![started_at, ended_at],
            )?,
            None => self
                .conn
                .execute("DELETE FROM subtitles WHERE created_at >= ?1", [started_at])?,
        };
        Ok(deleted as u64)
    }

    pub fn subtitle(&self, id: i64) -> AppResult<Option<Subtitle>> {
        let mut statement = self.conn.prepare(
            "SELECT id, conversation_id, text, language, started_at, ended_at, source, created_at
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

fn collect_subtitles<P>(
    statement: &mut rusqlite::Statement<'_>,
    params: P,
) -> AppResult<Vec<Subtitle>>
where
    P: rusqlite::Params,
{
    let rows = statement.query_map(params, |row| {
        let subtitle = subtitle_from_row(row)?;
        let translation = if row.get::<_, Option<i64>>(8)?.is_some() {
            Some(SubtitleTranslation {
                text: row.get(9)?,
                source_language: row.get(10)?,
                target_language: row.get(11)?,
                provider: row.get(12)?,
                model: row.get(13)?,
                created_at: row.get(14)?,
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

fn subtitle_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Subtitle> {
    Ok(Subtitle {
        id: Some(row.get(0)?),
        conversation_id: Some(row.get(1)?),
        text: row.get(2)?,
        language: row.get(3)?,
        started_at: row.get(4)?,
        ended_at: row.get(5)?,
        source: row.get(6)?,
        created_at: row.get(7)?,
        translations: Vec::new(),
    })
}
