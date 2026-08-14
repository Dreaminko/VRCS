use rusqlite::{params, OptionalExtension};

use super::Database;
use crate::chatbox::{ChatboxMessage, NewChatboxMessage};
use crate::error::AppResult;

const MESSAGE_HISTORY_LIMIT: i64 = 200;

impl Database {
    pub fn add_chatbox_message(&self, message: &NewChatboxMessage) -> AppResult<ChatboxMessage> {
        self.conn.execute(
            "INSERT INTO chatbox_messages(
                source, original, translation, source_language, target_language,
                send_mode, message_format, custom_format, rendered_text, char_count,
                truncated, status, error_code, error_detail, resent_from_id, created_at, sent_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                message.source,
                message.original,
                message.translation,
                message.source_language,
                message.target_language,
                message.send_mode,
                message.message_format,
                message.custom_format,
                message.rendered_text,
                message.char_count as i64,
                message.truncated,
                message.status,
                message.error_code,
                message.error_detail,
                message.resent_from_id,
                message.created_at,
                message.sent_at,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute(
            "DELETE FROM chatbox_messages
             WHERE id NOT IN (SELECT id FROM chatbox_messages ORDER BY id DESC LIMIT ?)",
            [MESSAGE_HISTORY_LIMIT],
        )?;
        self.chatbox_message(id)?.ok_or_else(|| {
            crate::error::AppError::internal("Saved Chatbox message could not be loaded")
        })
    }

    fn chatbox_message(&self, id: i64) -> AppResult<Option<ChatboxMessage>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, source, original, translation, source_language, target_language,
                        send_mode, message_format, custom_format, rendered_text, char_count,
                        truncated, status, error_code, error_detail, resent_from_id, created_at, sent_at
                 FROM chatbox_messages WHERE id = ?",
                [id],
                chatbox_message_from_row,
            )
            .optional()?)
    }
}

fn chatbox_message_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatboxMessage> {
    Ok(ChatboxMessage {
        id: row.get(0)?,
        source: row.get(1)?,
        original: row.get(2)?,
        translation: row.get(3)?,
        source_language: row.get(4)?,
        target_language: row.get(5)?,
        send_mode: row.get(6)?,
        message_format: row.get(7)?,
        custom_format: row.get(8)?,
        rendered_text: row.get(9)?,
        char_count: row.get::<_, i64>(10)? as usize,
        truncated: row.get(11)?,
        status: row.get(12)?,
        error_code: row.get(13)?,
        error_detail: row.get(14)?,
        resent_from_id: row.get(15)?,
        created_at: row.get(16)?,
        sent_at: row.get(17)?,
    })
}
