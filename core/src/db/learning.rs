use std::io;

use rusqlite::{params, types::Type, OptionalExtension, Row};

use super::Database;
use crate::error::{AppError, AppResult};
use crate::learning::{
    CreateLearningItem, LearningAnalysis, LearningCardDraft, LearningItem, LearningKind,
    LearningStatus, PatchLearningItem,
};
use crate::models::now_iso8601;

impl Database {
    pub(super) fn initialize_learning_storage(&self) -> AppResult<()> {
        self.conn.execute(
            "UPDATE learning_items
             SET status = CASE
                 WHEN anki_note_id IS NOT NULL THEN 'exported'
                 WHEN draft IS NOT NULL THEN 'card_draft'
                 WHEN analysis IS NOT NULL THEN 'analyzed'
                 ELSE 'collected'
             END
             WHERE status <> 'archived'",
            [],
        )?;
        let items = {
            let sql = format!("{} ORDER BY id ASC", learning_item_select());
            let mut statement = self.conn.prepare(&sql)?;
            let rows = statement.query_map([], learning_item_from_row)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for item in items {
            for key in item.capture_keys() {
                self.conn.execute(
                    "INSERT OR IGNORE INTO learning_capture_keys(key, item_id) VALUES (?, ?)",
                    params![key, item.id],
                )?;
            }
        }
        Ok(())
    }

    pub fn create_learning_item(&mut self, input: CreateLearningItem) -> AppResult<LearningItem> {
        input.validate().map_err(AppError::validation)?;
        let capture_keys = input.capture_keys();
        let created_at = now_iso8601();
        let working_text = input
            .working_text
            .clone()
            .unwrap_or_else(|| input.source_text.clone());
        let subtitle_ids = serde_json::to_string(&input.source_subtitle_ids)
            .map_err(|error| AppError::internal(error.to_string()))?;
        let dictionary_entries = serde_json::to_string(&input.dictionary_entries)
            .map_err(|error| AppError::internal(error.to_string()))?;
        let transaction = self.conn.transaction()?;
        let mut existing_id = None;
        for key in &capture_keys {
            existing_id = transaction
                .query_row(
                    "SELECT item_id FROM learning_capture_keys WHERE key = ?",
                    [key],
                    |row| row.get(0),
                )
                .optional()?;
            if existing_id.is_some() {
                break;
            }
        }
        if let Some(id) = existing_id {
            for key in capture_keys {
                transaction.execute(
                    "INSERT OR IGNORE INTO learning_capture_keys(key, item_id) VALUES (?, ?)",
                    params![key, id],
                )?;
            }
            transaction.commit()?;
            return self
                .learning_item(id)?
                .ok_or_else(|| AppError::internal("Captured learning item could not be loaded"));
        }
        transaction.execute(
            "INSERT INTO learning_items(
                 kind, status, source_text, working_text, selected_text,
                 source_translation, source_language, source_subtitle_ids,
                 dictionary_entries, analysis, draft, anki_note_id,
                 created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, ?, ?)",
            params![
                input.kind.as_str(),
                LearningStatus::Collected.as_str(),
                input.source_text,
                working_text,
                input.selected_text,
                input.source_translation,
                input.source_language,
                subtitle_ids,
                dictionary_entries,
                created_at,
                created_at,
            ],
        )?;
        let id = transaction.last_insert_rowid();
        for key in capture_keys {
            transaction.execute(
                "INSERT INTO learning_capture_keys(key, item_id) VALUES (?, ?)",
                params![key, id],
            )?;
        }
        transaction.commit()?;
        self.learning_item(id)?
            .ok_or_else(|| AppError::internal("Created learning item could not be loaded"))
    }

    pub fn learning_capture_keys(&self) -> AppResult<Vec<String>> {
        let mut statement = self
            .conn
            .prepare("SELECT key FROM learning_capture_keys ORDER BY key")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn learning_items(
        &self,
        limit: u32,
        before_id: Option<i64>,
        status: Option<LearningStatus>,
    ) -> AppResult<Vec<LearningItem>> {
        let sql = if status.is_some() {
            format!(
                "{} WHERE (?1 IS NULL OR id < ?1) AND status = ?2 ORDER BY id DESC LIMIT ?3",
                learning_item_select()
            )
        } else {
            format!(
                "{} WHERE (?1 IS NULL OR id < ?1) ORDER BY id DESC LIMIT ?2",
                learning_item_select()
            )
        };
        let mut statement = self.conn.prepare(&sql)?;
        let rows = if let Some(status) = status {
            statement.query_map(
                params![before_id, status.as_str(), limit],
                learning_item_from_row,
            )?
        } else {
            statement.query_map(params![before_id, limit], learning_item_from_row)?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn learning_item(&self, id: i64) -> AppResult<Option<LearningItem>> {
        let sql = format!("{} WHERE id = ?", learning_item_select());
        let mut statement = self.conn.prepare(&sql)?;
        let mut rows = statement.query([id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(learning_item_from_row(row)?))
    }

    pub fn patch_learning_item(
        &self,
        id: i64,
        patch: PatchLearningItem,
    ) -> AppResult<Option<LearningItem>> {
        patch
            .validate()
            .map_err(crate::error::AppError::validation)?;
        let Some(mut item) = self.learning_item(id)? else {
            return Ok(None);
        };
        ensure_learning_item_editable(&item)?;
        if let Some(working_text) = patch.working_text {
            item.working_text = working_text;
        }
        if let Some(draft) = patch.draft {
            item.draft = draft;
        }
        item.status = item.active_status();
        item.updated_at = now_iso8601();
        self.persist_learning_item(&item)?;
        Ok(Some(item))
    }

    pub fn save_learning_analysis(
        &self,
        id: i64,
        analysis: LearningAnalysis,
    ) -> AppResult<Option<LearningItem>> {
        analysis
            .validate()
            .map_err(crate::error::AppError::validation)?;
        let Some(mut item) = self.learning_item(id)? else {
            return Ok(None);
        };
        ensure_learning_item_editable(&item)?;
        item.analysis = Some(analysis);
        item.status = item.active_status();
        item.updated_at = now_iso8601();
        self.persist_learning_item(&item)?;
        Ok(Some(item))
    }

    pub fn save_learning_draft(
        &self,
        id: i64,
        draft: LearningCardDraft,
    ) -> AppResult<Option<LearningItem>> {
        draft
            .validate()
            .map_err(crate::error::AppError::validation)?;
        let Some(mut item) = self.learning_item(id)? else {
            return Ok(None);
        };
        ensure_learning_item_editable(&item)?;
        item.draft = Some(draft);
        item.status = item.active_status();
        item.updated_at = now_iso8601();
        self.persist_learning_item(&item)?;
        Ok(Some(item))
    }

    pub fn save_learning_export(
        &self,
        id: i64,
        anki_note_id: i64,
    ) -> AppResult<Option<LearningItem>> {
        if anki_note_id <= 0 {
            return Err(crate::error::AppError::validation(
                "Anki note ID must be positive",
            ));
        }
        let Some(mut item) = self.learning_item(id)? else {
            return Ok(None);
        };
        ensure_learning_item_editable(&item)?;
        item.anki_note_id = Some(anki_note_id);
        item.status = LearningStatus::Exported;
        item.updated_at = now_iso8601();
        self.persist_learning_item(&item)?;
        Ok(Some(item))
    }

    pub fn archive_learning_item(&self, id: i64) -> AppResult<Option<LearningItem>> {
        let Some(mut item) = self.learning_item(id)? else {
            return Ok(None);
        };
        if item.status != LearningStatus::Archived {
            item.status = LearningStatus::Archived;
            item.updated_at = now_iso8601();
            self.persist_learning_item(&item)?;
        }
        Ok(Some(item))
    }

    pub fn restore_learning_item(&self, id: i64) -> AppResult<Option<LearningItem>> {
        let Some(mut item) = self.learning_item(id)? else {
            return Ok(None);
        };
        if item.status == LearningStatus::Archived {
            item.status = item.active_status();
            item.updated_at = now_iso8601();
            self.persist_learning_item(&item)?;
        }
        Ok(Some(item))
    }

    pub fn delete_learning_item(&self, id: i64) -> AppResult<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM learning_items WHERE id = ?", [id])?
            > 0)
    }

    fn persist_learning_item(&self, item: &LearningItem) -> AppResult<()> {
        item.validate()
            .map_err(crate::error::AppError::validation)?;
        let subtitle_ids = serde_json::to_string(&item.source_subtitle_ids)
            .map_err(|error| crate::error::AppError::internal(error.to_string()))?;
        let dictionary_entries = serde_json::to_string(&item.dictionary_entries)
            .map_err(|error| crate::error::AppError::internal(error.to_string()))?;
        let analysis = item
            .analysis
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| crate::error::AppError::internal(error.to_string()))?;
        let draft = item
            .draft
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| crate::error::AppError::internal(error.to_string()))?;
        self.conn.execute(
            "UPDATE learning_items SET
                 kind = ?, status = ?, source_text = ?, working_text = ?, selected_text = ?,
                 source_translation = ?, source_language = ?, source_subtitle_ids = ?,
                 dictionary_entries = ?, analysis = ?, draft = ?, anki_note_id = ?,
                 created_at = ?, updated_at = ?
             WHERE id = ?",
            params![
                item.kind.as_str(),
                item.status.as_str(),
                item.source_text,
                item.working_text,
                item.selected_text,
                item.source_translation,
                item.source_language,
                subtitle_ids,
                dictionary_entries,
                analysis,
                draft,
                item.anki_note_id,
                item.created_at,
                item.updated_at,
                item.id,
            ],
        )?;
        Ok(())
    }
}

fn ensure_learning_item_editable(item: &LearningItem) -> AppResult<()> {
    if item.status == LearningStatus::Archived {
        return Err(AppError::Conflict(
            "Archived learning items must be restored before editing".into(),
        ));
    }
    Ok(())
}

fn learning_item_select() -> &'static str {
    "SELECT id, kind, status, source_text, working_text, selected_text,
            source_translation, source_language, source_subtitle_ids,
            dictionary_entries, analysis, draft, anki_note_id, created_at, updated_at
     FROM learning_items"
}

fn learning_item_from_row(row: &Row<'_>) -> rusqlite::Result<LearningItem> {
    let kind: String = row.get(1)?;
    let status: String = row.get(2)?;
    let source_subtitle_ids: String = row.get(8)?;
    let dictionary_entries: String = row.get(9)?;
    let analysis: Option<String> = row.get(10)?;
    let draft: Option<String> = row.get(11)?;
    let item = LearningItem {
        id: row.get(0)?,
        kind: LearningKind::parse(&kind).map_err(|error| data_error(1, error))?,
        status: LearningStatus::parse(&status).map_err(|error| data_error(2, error))?,
        source_text: row.get(3)?,
        working_text: row.get(4)?,
        selected_text: row.get(5)?,
        source_translation: row.get(6)?,
        source_language: row.get(7)?,
        source_subtitle_ids: serde_json::from_str(&source_subtitle_ids)
            .map_err(|error| data_error(8, error.to_string()))?,
        dictionary_entries: serde_json::from_str(&dictionary_entries)
            .map_err(|error| data_error(9, error.to_string()))?,
        analysis: analysis
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|error| data_error(10, error.to_string()))?,
        draft: draft
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|error| data_error(11, error.to_string()))?,
        anki_note_id: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    };
    item.validate().map_err(|error| data_error(0, error))?;
    Ok(item)
}

fn data_error(index: usize, detail: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        Type::Text,
        Box::new(io::Error::new(io::ErrorKind::InvalidData, detail)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::{
        AnalysisConfidence, AnalysisTaskType, LearningAnalysis, LearningCardType, LearningKind,
    };
    use crate::models::{now_iso8601, DictionaryEntry, Subtitle};

    fn analysis() -> LearningAnalysis {
        LearningAnalysis {
            task_type: AnalysisTaskType::SentenceAnalysis,
            summary: "A test analysis".into(),
            current_meaning: None,
            base_form: None,
            part_of_speech: None,
            register: None,
            segments: Vec::new(),
            grammar_points: Vec::new(),
            uncertainties: Vec::new(),
            memory_tip: None,
            examples: Vec::new(),
            confidence: AnalysisConfidence::High,
            provider: "test".into(),
            model: "test-model".into(),
            prompt_version: "test-v1".into(),
        }
    }

    fn create_input(subtitle_ids: Vec<i64>) -> CreateLearningItem {
        CreateLearningItem {
            kind: LearningKind::Word,
            source_text: "猫が魚を食べる".into(),
            working_text: None,
            selected_text: Some("食べる".into()),
            source_translation: Some("The cat eats fish".into()),
            source_language: Some("ja".into()),
            source_subtitle_ids: subtitle_ids,
            dictionary_entries: vec![DictionaryEntry {
                term: "食べる".into(),
                language: "ja".into(),
                definition: "to eat".into(),
                reading: Some("たべる".into()),
                dictionary: Some("Test".into()),
            }],
        }
    }

    #[test]
    fn learning_item_crud_and_status_filter_are_typed() {
        let directory = tempfile::tempdir().unwrap();
        let mut database = Database::open(&directory.path().join("learning.db")).unwrap();
        let created = database
            .create_learning_item(create_input(Vec::new()))
            .unwrap();
        assert_eq!(created.working_text, created.source_text);
        assert_eq!(created.status, LearningStatus::Collected);

        let patched = database
            .patch_learning_item(
                created.id,
                PatchLearningItem {
                    working_text: Some("魚を食べる".into()),
                    draft: None,
                },
            )
            .unwrap()
            .unwrap();
        assert_eq!(patched.working_text, "魚を食べる");
        let archived = database.archive_learning_item(created.id).unwrap().unwrap();
        assert_eq!(archived.status, LearningStatus::Archived);
        assert_eq!(
            database
                .learning_items(10, None, Some(LearningStatus::Archived))
                .unwrap()
                .len(),
            1
        );
        assert!(database.delete_learning_item(created.id).unwrap());
        assert!(database.learning_item(created.id).unwrap().is_none());
    }

    #[test]
    fn learning_snapshot_survives_subtitle_deletion() {
        let directory = tempfile::tempdir().unwrap();
        let mut database = Database::open(&directory.path().join("snapshot.db")).unwrap();
        let subtitle = database
            .add_subtitle(&Subtitle {
                id: None,
                conversation_id: None,
                text: "source snapshot".into(),
                language: Some("en".into()),
                started_at: None,
                ended_at: None,
                source: "speaker".into(),
                created_at: now_iso8601(),
                translations: Vec::new(),
            })
            .unwrap();
        let item = database
            .create_learning_item(CreateLearningItem {
                source_text: subtitle.text.clone(),
                ..create_input(vec![subtitle.id.unwrap()])
            })
            .unwrap();

        database.clear_subtitle_history().unwrap();

        let loaded = database.learning_item(item.id).unwrap().unwrap();
        assert_eq!(loaded.source_text, "source snapshot");
        assert_eq!(loaded.source_subtitle_ids, vec![subtitle.id.unwrap()]);
    }

    #[test]
    fn saved_draft_moves_item_to_card_draft() {
        let directory = tempfile::tempdir().unwrap();
        let mut database = Database::open(&directory.path().join("draft.db")).unwrap();
        let item = database
            .create_learning_item(create_input(Vec::new()))
            .unwrap();
        let draft = crate::learning::generate_draft(&item, LearningCardType::Vocabulary).unwrap();
        let saved = database
            .save_learning_draft(item.id, draft)
            .unwrap()
            .unwrap();
        assert_eq!(saved.status, LearningStatus::CardDraft);
        assert!(saved.draft.is_some());
    }

    #[test]
    fn status_transitions_preserve_progress_and_restore_the_derived_stage() {
        let directory = tempfile::tempdir().unwrap();
        let mut database = Database::open(&directory.path().join("status.db")).unwrap();
        let item = database
            .create_learning_item(create_input(Vec::new()))
            .unwrap();

        let analyzed = database
            .save_learning_analysis(item.id, analysis())
            .unwrap()
            .unwrap();
        assert_eq!(analyzed.status, LearningStatus::Analyzed);

        let draft =
            crate::learning::generate_draft(&analyzed, LearningCardType::Vocabulary).unwrap();
        let drafted = database
            .save_learning_draft(item.id, draft)
            .unwrap()
            .unwrap();
        assert_eq!(drafted.status, LearningStatus::CardDraft);
        assert_eq!(
            database
                .save_learning_analysis(item.id, analysis())
                .unwrap()
                .unwrap()
                .status,
            LearningStatus::CardDraft
        );

        let exported = database.save_learning_export(item.id, 42).unwrap().unwrap();
        assert_eq!(exported.status, LearningStatus::Exported);
        assert_eq!(
            database
                .save_learning_analysis(item.id, analysis())
                .unwrap()
                .unwrap()
                .status,
            LearningStatus::Exported
        );

        let archived = database.archive_learning_item(item.id).unwrap().unwrap();
        assert_eq!(archived.status, LearningStatus::Archived);
        assert!(matches!(
            database.save_learning_analysis(item.id, analysis()),
            Err(AppError::Conflict(_))
        ));
        let restored = database.restore_learning_item(item.id).unwrap().unwrap();
        assert_eq!(restored.status, LearningStatus::Exported);
        assert_eq!(restored.anki_note_id, Some(42));
    }

    #[test]
    fn capture_creation_is_idempotent_and_delete_cascades_keys() {
        let directory = tempfile::tempdir().unwrap();
        let mut database = Database::open(&directory.path().join("capture.db")).unwrap();
        let first = database
            .create_learning_item(create_input(vec![8, 9]))
            .unwrap();
        let duplicate = database
            .create_learning_item(create_input(vec![8, 9]))
            .unwrap();

        assert_eq!(duplicate.id, first.id);
        let keys = database.learning_capture_keys().unwrap();
        assert!(keys.contains(&"subtitle:8".into()));
        assert!(keys.contains(&"subtitle:9".into()));
        assert!(keys.contains(&"subtitles:8,9".into()));
        assert!(keys.contains(&"lookup:8:食べる:猫が魚を食べる".into()));

        assert!(database.delete_learning_item(first.id).unwrap());
        assert!(database.learning_capture_keys().unwrap().is_empty());
    }
}
