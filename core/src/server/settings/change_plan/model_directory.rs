use std::path::PathBuf;
use std::sync::Arc;

use crate::config::AppConfig;

use super::super::super::SettingsContext;

pub(super) struct ModelDirectoryChange {
    pub(super) changed: bool,
    pub(super) candidate_path: PathBuf,
    previous_path: PathBuf,
    applied: bool,
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
            applied: false,
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
            if let Err(error) =
                move_translation_models(state, self.candidate_path.clone(), lifecycle).await
            {
                let _ = move_model_directory(state, self.previous_path.clone()).await;
                return Err(error);
            }
            self.applied = true;
        }
        Ok(())
    }

    pub(super) async fn rollback(&mut self, state: &SettingsContext) -> Result<(), String> {
        if self.applied {
            let lifecycle = state
                .content
                .local_translation_models
                .lock_exclusive()
                .await;
            state.capture.qwen_runtime.stop().await?;
            state.content.plamo_runtime.stop().await?;
            move_model_directory(state, self.previous_path.clone()).await?;
            move_translation_models(state, self.previous_path.clone(), lifecycle).await?;
            self.applied = false;
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
