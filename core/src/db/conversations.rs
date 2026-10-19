use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tokio::sync::broadcast;

use super::Database;
use crate::error::{AppError, AppResult};
use crate::models::now_iso8601;

#[derive(Debug, Clone, Serialize)]
pub struct ConversationCatalog {
    pub conversations: Vec<Conversation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Conversation {
    pub id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub automatic_title: Option<String>,
    pub summary_title: Option<String>,
    pub title_generation_status: String,
    pub title_generation_error_code: Option<String>,
    pub custom_title: Option<String>,
    pub icon: Option<String>,
    pub subtitle_count: u64,
    pub updated_at: String,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TitleJob {
    pub conversation_id: String,
    pub request_id: String,
}

impl Database {
    pub fn conversation_catalog(&self) -> AppResult<ConversationCatalog> {
        catalog(&self.conn)
    }

    #[cfg(test)]
    pub fn create_conversation(&mut self) -> AppResult<ConversationCatalog> {
        self.create_conversation_with_title(false)
            .map(|(catalog, _)| catalog)
    }

    pub(crate) fn create_conversation_with_title(
        &mut self,
        auto_generate: bool,
    ) -> AppResult<(ConversationCatalog, Option<TitleJob>)> {
        let transaction = self.conn.transaction()?;
        let active = active_conversation(&transaction)?;
        let mut job = None;
        if active.subtitle_count > 0 {
            let now = now_iso8601();
            transaction.execute(
                "UPDATE conversations SET ended_at = ?1 WHERE id = ?2",
                params![now, active.id],
            )?;
            if auto_generate {
                job = register_title(&transaction, &active.id, false)?;
            }
            insert_active(&transaction, &now)?;
        }
        let result = catalog(&transaction)?;
        transaction.commit()?;
        Ok((result, job))
    }

    pub(crate) fn request_conversation_title(
        &mut self,
        id: &str,
        regenerate: bool,
        configured: bool,
    ) -> AppResult<Option<(ConversationCatalog, Option<TitleJob>)>> {
        let transaction = self.conn.transaction()?;
        let state = transaction.query_row(
            "SELECT ended_at IS NOT NULL, custom_title IS NOT NULL, summary_title IS NOT NULL,
                    title_generation_status IN ('pending', 'running'),
                    EXISTS(SELECT 1 FROM subtitles WHERE conversation_id = ?1 AND trim(text, char(9, 10, 13, 32)) <> '')
             FROM conversations WHERE id = ?1",
            [id],
            |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?, row.get::<_, bool>(2)?, row.get::<_, bool>(3)?, row.get::<_, bool>(4)?)),
        ).optional()?;
        let Some((ended, custom, ready, busy, has_text)) = state else {
            return Ok(None);
        };
        if !ended || custom {
            return Err(AppError::Conflict(
                "End the conversation and clear its custom title first".into(),
            ));
        }
        let job = if busy || (ready && !regenerate) {
            None
        } else {
            if !has_text {
                return Err(AppError::validation(
                    "The conversation has no text to summarize",
                ));
            }
            if !configured {
                return Err(AppError::validation(
                    "Select a valid text generation profile and model in conversation settings",
                ));
            }
            register_title(&transaction, id, regenerate)?
        };
        let catalog = catalog(&transaction)?;
        transaction.commit()?;
        Ok(Some((catalog, job)))
    }

    pub(crate) fn start_conversation_title(&self, job: &TitleJob) -> AppResult<bool> {
        Ok(self.conn.execute(
            "UPDATE conversations SET title_generation_status = 'running'
             WHERE id = ?1 AND title_generation_request_id = ?2
               AND title_generation_status = 'pending' AND ended_at IS NOT NULL",
            params![job.conversation_id, job.request_id],
        )? > 0)
    }

    pub(crate) fn finish_conversation_title(
        &self,
        job: &TitleJob,
        result: Result<&str, &str>,
    ) -> AppResult<bool> {
        let (title, status, error) = match result {
            Ok(title) => (Some(title), "ready", None),
            Err(error) => (None, "failed", Some(error)),
        };
        Ok(self.conn.execute(
            "UPDATE conversations SET summary_title = COALESCE(?3, summary_title),
                 title_generation_status = ?4, title_generation_error_code = ?5,
                 title_generation_request_id = NULL
             WHERE id = ?1 AND title_generation_request_id = ?2
               AND title_generation_status IN ('pending', 'running') AND ended_at IS NOT NULL",
            params![job.conversation_id, job.request_id, title, status, error],
        )? > 0)
    }

    pub(crate) fn cancel_conversation_title(&self, job: &TitleJob) -> AppResult<()> {
        self.conn.execute(
            "UPDATE conversations SET title_generation_status = CASE WHEN summary_title IS NULL THEN 'idle' ELSE 'ready' END,
                 title_generation_request_id = NULL, title_generation_error_code = NULL
             WHERE id = ?1 AND title_generation_request_id = ?2 AND title_generation_status = 'pending'",
            params![job.conversation_id, job.request_id],
        )?;
        Ok(())
    }

    pub(crate) fn conversation_title_input(&self, id: &str) -> AppResult<String> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM subtitles WHERE conversation_id = ?1 AND trim(text, char(9, 10, 13, 32)) <> ''",
            [id], |row| row.get(0),
        )?;
        if count == 0 {
            return Ok(String::new());
        }
        let samples = count.min(200);
        let line_budget = 24_000 / samples as usize;
        let mut statement = self.conn.prepare(
            "WITH RECURSIVE samples(n) AS (
                 VALUES(0) UNION ALL SELECT n + 1 FROM samples WHERE n + 1 < ?2
             ), ranked AS (
                 SELECT id, ROW_NUMBER() OVER (ORDER BY id) - 1 AS rank
                 FROM subtitles WHERE conversation_id = ?1 AND trim(text, char(9, 10, 13, 32)) <> ''
             )
             SELECT substr(s.source, 1, 16),
                    CASE WHEN json_valid(s.speaker) THEN json_extract(s.speaker, '$.index') END,
                    substr(s.text, 1, ?4)
             FROM subtitles AS s JOIN ranked AS r ON r.id = s.id
             WHERE r.rank IN (SELECT n * (?3 - 1) / max(?2 - 1, 1) FROM samples)
             ORDER BY s.id",
        )?;
        let rows = statement.query_map(params![id, samples, count, line_budget], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<u64>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut input = String::new();
        for row in rows {
            let (source, speaker, text) = row?;
            if text.trim().is_empty() {
                continue;
            }
            let source = source.split_whitespace().collect::<Vec<_>>().join(" ");
            let label = speaker
                .map(|index| format!(" speaker-{}", index.saturating_add(1)))
                .unwrap_or_default();
            let prefix = format!("[{source}{label}] ");
            let text_budget = line_budget.saturating_sub(prefix.chars().count() + 1);
            input.push_str(&prefix);
            input.extend(text.chars().take(text_budget));
            input.push('\n');
        }
        Ok(input)
    }

    pub fn update_conversation(
        &mut self,
        id: &str,
        custom_title: Option<Option<&str>>,
        icon: Option<Option<&str>>,
    ) -> AppResult<Option<ConversationCatalog>> {
        let transaction = self.conn.transaction()?;
        let updated = transaction.execute(
            "UPDATE conversations
             SET custom_title = CASE WHEN ?1 THEN ?2 ELSE custom_title END,
                 icon = CASE WHEN ?3 THEN ?4 ELSE icon END
             WHERE id = ?5",
            params![
                custom_title.is_some(),
                custom_title.flatten(),
                icon.is_some(),
                icon.flatten(),
                id
            ],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        let result = catalog(&transaction)?;
        transaction.commit()?;
        Ok(Some(result))
    }

    pub fn delete_conversation(&mut self, id: &str) -> AppResult<Option<ConversationCatalog>> {
        let transaction = self.conn.transaction()?;
        let was_active = transaction
            .query_row(
                "SELECT ended_at IS NULL FROM conversations WHERE id = ?1",
                [id],
                |row| row.get::<_, bool>(0),
            )
            .optional()?;
        let Some(was_active) = was_active else {
            return Ok(None);
        };
        transaction.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
        if was_active {
            insert_active(&transaction, &now_iso8601())?;
        }
        let result = catalog(&transaction)?;
        transaction.commit()?;
        Ok(Some(result))
    }
}

pub(crate) fn publish_catalog(
    sender: &broadcast::Sender<ConversationCatalog>,
    catalog: &ConversationCatalog,
) {
    let _ = sender.send(catalog.clone());
}

pub(crate) fn publish_latest_catalog(
    database: &Database,
    sender: &broadcast::Sender<ConversationCatalog>,
) {
    match database.conversation_catalog() {
        Ok(catalog) => publish_catalog(sender, &catalog),
        Err(error) => tracing::warn!(%error, "conversation catalog could not be published"),
    }
}

const CATALOG_REFRESH_DELAY: Duration = Duration::from_millis(250);

/// Coalesces catalog rebuilds so realtime writes do not scan history under the database lock.
#[derive(Clone)]
pub(crate) struct CatalogRefresh {
    database: Arc<Mutex<Database>>,
    sender: broadcast::Sender<ConversationCatalog>,
    scheduled: Arc<AtomicBool>,
}

impl CatalogRefresh {
    pub(crate) fn new(
        database: Arc<Mutex<Database>>,
        sender: broadcast::Sender<ConversationCatalog>,
    ) -> Self {
        Self {
            database,
            sender,
            scheduled: Arc::default(),
        }
    }

    pub(crate) fn request(&self) {
        if self.scheduled.swap(true, Ordering::AcqRel) {
            return;
        }
        let refresh = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(CATALOG_REFRESH_DELAY).await;
            let _ = tokio::task::spawn_blocking(move || {
                // Clear first so a write committed during this rebuild schedules another.
                refresh.scheduled.store(false, Ordering::Release);
                match refresh.database.lock() {
                    Ok(database) => publish_latest_catalog(&database, &refresh.sender),
                    Err(_) => tracing::warn!("conversation catalog database lock is unavailable"),
                }
            })
            .await;
        });
    }
}

pub(super) fn initialize_conversations(conn: &Connection) -> AppResult<()> {
    let active_count = conn.query_row(
        "SELECT COUNT(*) FROM conversations WHERE ended_at IS NULL",
        [],
        |row| row.get::<_, u64>(0),
    )?;
    if active_count == 0 {
        insert_active(conn, &now_iso8601())?;
    }
    Ok(())
}

pub(super) fn active_conversation_id(conn: &Connection) -> AppResult<String> {
    Ok(active_conversation(conn)?.id)
}

pub(super) fn reset_after_history_clear(conn: &Connection) -> AppResult<()> {
    conn.execute("DELETE FROM conversations WHERE ended_at IS NOT NULL", [])?;
    initialize_conversations(conn)?;
    conn.execute(
        "UPDATE conversations
         SET automatic_title = NULL, updated_at = started_at
         WHERE ended_at IS NULL",
        [],
    )?;
    Ok(())
}

pub(super) fn cleanup_empty_ended(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "DELETE FROM conversations
         WHERE ended_at IS NOT NULL
           AND NOT EXISTS (
               SELECT 1 FROM subtitles WHERE subtitles.conversation_id = conversations.id
           )",
        [],
    )?;
    Ok(())
}

pub(super) fn automatic_title(text: &str) -> Option<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = normalized.chars().take(14).collect::<String>();
    (!title.is_empty()).then_some(title)
}

pub(super) fn new_public_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().simple())
}

fn active_conversation(conn: &Connection) -> AppResult<ActiveConversation> {
    Ok(conn.query_row(
        "SELECT c.id, COUNT(s.id)
         FROM conversations AS c
         LEFT JOIN subtitles AS s ON s.conversation_id = c.id
         WHERE c.ended_at IS NULL
         GROUP BY c.id",
        [],
        |row| {
            Ok(ActiveConversation {
                id: row.get(0)?,
                subtitle_count: row.get(1)?,
            })
        },
    )?)
}

pub(super) fn insert_active(conn: &Connection, started_at: &str) -> AppResult<String> {
    let id = new_public_id("conversation");
    conn.execute(
        "INSERT INTO conversations(
            id, started_at, ended_at, automatic_title, custom_title, icon,
            updated_at, provisional
         ) VALUES (?1, ?2, NULL, NULL, NULL, NULL, ?2, 0)",
        params![id, started_at],
    )?;
    Ok(id)
}

fn catalog(conn: &Connection) -> AppResult<ConversationCatalog> {
    let mut statement = conn.prepare(
        "SELECT c.id, c.started_at, c.ended_at, c.automatic_title, c.custom_title,
                c.icon, COUNT(s.id), COALESCE(MAX(s.created_at), c.started_at),
                c.ended_at IS NULL, c.summary_title, c.title_generation_status,
                c.title_generation_error_code
         FROM conversations AS c
         LEFT JOIN subtitles AS s ON s.conversation_id = c.id
         GROUP BY c.id
         ORDER BY c.started_at DESC, c.rowid DESC",
    )?;
    let conversations = statement
        .query_map([], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                started_at: row.get(1)?,
                ended_at: row.get(2)?,
                automatic_title: row.get(3)?,
                custom_title: row.get(4)?,
                icon: row.get(5)?,
                subtitle_count: row.get(6)?,
                updated_at: row.get(7)?,
                active: row.get(8)?,
                summary_title: row.get(9)?,
                title_generation_status: row.get(10)?,
                title_generation_error_code: row.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ConversationCatalog { conversations })
}

fn register_title(conn: &Connection, id: &str, regenerate: bool) -> AppResult<Option<TitleJob>> {
    let request_id = new_public_id("title");
    let updated = conn.execute(
        "UPDATE conversations SET title_generation_status = 'pending',
             title_generation_error_code = NULL, title_generation_request_id = ?2
         WHERE id = ?1 AND ended_at IS NOT NULL AND custom_title IS NULL
           AND (summary_title IS NULL OR ?3)
           AND title_generation_status NOT IN ('pending', 'running')
           AND EXISTS(SELECT 1 FROM subtitles WHERE conversation_id = ?1 AND trim(text, char(9, 10, 13, 32)) <> '')",
        params![id, request_id, regenerate],
    )?;
    Ok((updated > 0).then(|| TitleJob {
        conversation_id: id.into(),
        request_id,
    }))
}

struct ActiveConversation {
    id: String,
    subtitle_count: u64,
}
