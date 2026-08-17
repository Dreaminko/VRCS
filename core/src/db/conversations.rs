use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use tokio::sync::broadcast;

use super::Database;
use crate::error::AppResult;
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
    pub custom_title: Option<String>,
    pub icon: Option<String>,
    pub subtitle_count: u64,
    pub updated_at: String,
    pub active: bool,
}

impl Database {
    pub fn conversation_catalog(&self) -> AppResult<ConversationCatalog> {
        catalog(&self.conn)
    }

    pub fn create_conversation(&mut self) -> AppResult<ConversationCatalog> {
        let transaction = self.conn.transaction()?;
        let active = active_conversation(&transaction)?;
        if active.subtitle_count > 0 {
            let now = now_iso8601();
            transaction.execute(
                "UPDATE conversations SET ended_at = ?1 WHERE id = ?2",
                params![now, active.id],
            )?;
            insert_active(&transaction, &now)?;
        }
        let result = catalog(&transaction)?;
        transaction.commit()?;
        Ok(result)
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
                c.ended_at IS NULL
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
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ConversationCatalog { conversations })
}

struct ActiveConversation {
    id: String,
    subtitle_count: u64,
}
