use serde::Serialize;

use super::Database;
use crate::error::AppResult;

const DELETE_BATCH_SIZE: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DatabaseStorageStats {
    pub used_bytes: u64,
    pub allocated_bytes: u64,
    pub reclaimable_bytes: u64,
    pub max_bytes: u64,
    pub over_limit: bool,
}

impl Database {
    pub fn set_subtitle_history_max_bytes(&mut self, max_bytes: u64) -> AppResult<bool> {
        self.subtitle_history_max_bytes = max_bytes;
        self.trim_subtitle_history_to_size()
    }

    pub fn storage_stats(&self) -> AppResult<DatabaseStorageStats> {
        let page_size = self.pragma_u64("page_size")?;
        let page_count = self.pragma_u64("page_count")?;
        let freelist_count = self.pragma_u64("freelist_count")?;
        let allocated_bytes = page_count.saturating_mul(page_size);
        let reclaimable_bytes = freelist_count.saturating_mul(page_size);
        let used_bytes = allocated_bytes.saturating_sub(reclaimable_bytes);
        Ok(DatabaseStorageStats {
            used_bytes,
            allocated_bytes,
            reclaimable_bytes,
            max_bytes: self.subtitle_history_max_bytes,
            over_limit: used_bytes > self.subtitle_history_max_bytes,
        })
    }

    pub fn clear_subtitle_history(&self) -> AppResult<DatabaseStorageStats> {
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute("DELETE FROM subtitles", [])?;
        super::conversations::reset_after_history_clear(&transaction)?;
        transaction.commit()?;
        self.conn.execute_batch("VACUUM")?;
        self.storage_stats()
    }

    pub(super) fn trim_subtitle_history_to_size(&self) -> AppResult<bool> {
        let mut stats = self.storage_stats()?;
        let mut catalog_changed = false;
        while stats.over_limit {
            let subtitle_count =
                self.conn
                    .query_row("SELECT COUNT(*) FROM subtitles", [], |row| {
                        row.get::<_, u64>(0)
                    })?;
            if subtitle_count <= 1 {
                break;
            }
            let delete_count = DELETE_BATCH_SIZE.min(subtitle_count - 1);
            let deleted = self.conn.execute(
                "DELETE FROM subtitles
                 WHERE id IN (SELECT id FROM subtitles ORDER BY id ASC LIMIT ?)",
                [delete_count],
            )?;
            if deleted == 0 {
                break;
            }
            catalog_changed = true;
            super::conversations::cleanup_empty_ended(&self.conn)?;
            stats = self.storage_stats()?;
        }
        Ok(catalog_changed)
    }

    fn pragma_u64(&self, name: &str) -> AppResult<u64> {
        let value = self
            .conn
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get::<_, i64>(0))?;
        Ok(value.max(0) as u64)
    }
}
