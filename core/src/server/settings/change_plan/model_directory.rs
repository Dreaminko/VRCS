use std::path::PathBuf;
use std::sync::Arc;

use crate::config::AppConfig;

use super::super::super::SettingsContext;

pub(super) struct ModelDirectoryChange {
    pub(super) changed: bool,
    pub(super) candidate_path: PathBuf,
    previous_path: PathBuf,
    previous_ocr_path: PathBuf,
    models_moved: bool,
    translation_moved: bool,
    ocr_moved: bool,
}

impl ModelDirectoryChange {
    pub(super) fn between(
        state: &SettingsContext,
        current: &AppConfig,
        candidate: &AppConfig,
    ) -> Self {
        let changed = candidate.storage.model_directory != current.storage.model_directory;
        let candidate_path = state
            .config
            .asr_model_dir_override
            .clone()
            .unwrap_or_else(|| {
                crate::resolve_config_path(
                    &state.config.config_path,
                    &candidate.storage.model_directory,
                )
            });
        Self {
            changed,
            candidate_path,
            previous_path: state.capture.model_manager.model_dir(),
            previous_ocr_path: state.config.local_ocr.model_dir(),
            models_moved: false,
            translation_moved: false,
            ocr_moved: false,
        }
    }

    pub(super) async fn apply(&mut self, state: &SettingsContext) -> Result<(), String> {
        if self.changed {
            state.content.local_translation_models.cancel().await;
            let lifecycle = state
                .content
                .local_translation_models
                .lock_exclusive()
                .await;
            state.capture.qwen_runtime.stop().await?;
            state.content.plamo_runtime.stop().await?;
            move_model_directory(state, self.candidate_path.clone()).await?;
            self.models_moved = true;
            move_translation_models(state, self.candidate_path.clone(), lifecycle).await?;
            self.translation_moved = true;
            move_ocr_directory(state, crate::ocr::model_directory(&self.candidate_path)).await?;
            self.ocr_moved = true;
        }
        Ok(())
    }

    pub(super) async fn rollback(&mut self, state: &SettingsContext) -> Result<(), String> {
        if !self.models_moved && !self.translation_moved && !self.ocr_moved {
            return Ok(());
        }
        state.content.local_translation_models.cancel().await;
        let lifecycle = state
            .content
            .local_translation_models
            .lock_exclusive()
            .await;
        let mut errors = Vec::new();
        if let Err(error) = state.capture.qwen_runtime.stop().await {
            errors.push(error);
        }
        if let Err(error) = state.content.plamo_runtime.stop().await {
            errors.push(error);
        }
        if self.ocr_moved {
            match move_ocr_directory(state, self.previous_ocr_path.clone()).await {
                Ok(()) => self.ocr_moved = false,
                Err(error) => errors.push(error),
            }
        }
        if self.translation_moved {
            match move_translation_models(state, self.previous_path.clone(), lifecycle).await {
                Ok(()) => self.translation_moved = false,
                Err(error) => errors.push(error),
            }
        }
        if self.models_moved {
            match move_model_directory(state, self.previous_path.clone()).await {
                Ok(()) => self.models_moved = false,
                Err(error) => errors.push(error),
            }
        }
        if !errors.is_empty() {
            return Err(errors.join("; "));
        }
        Ok(())
    }
}

async fn move_model_directory(state: &SettingsContext, path: PathBuf) -> Result<(), String> {
    let manager = Arc::clone(&state.capture.model_manager);
    tokio::task::spawn_blocking(move || manager.move_model_dir(path))
        .await
        .map_err(|error| format!("Model directory migration task failed: {error}"))?
}

async fn move_translation_models(
    state: &SettingsContext,
    path: PathBuf,
    lifecycle: tokio::sync::OwnedRwLockWriteGuard<()>,
) -> Result<(), String> {
    let models = Arc::clone(&state.content.local_translation_models);
    tokio::task::spawn_blocking(move || models.move_root_locked(path, &lifecycle))
        .await
        .map_err(|error| format!("Translation model directory migration task failed: {error}"))?
}

async fn move_ocr_directory(state: &SettingsContext, path: PathBuf) -> Result<(), String> {
    state.config.local_ocr.move_model_dir(path).await
}
