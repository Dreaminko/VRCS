use std::path::PathBuf;
use std::sync::Arc;

use crate::config::AppConfig;

use super::super::super::{capture, SettingsContext};

pub(super) struct AsrRuntimeChange {
    changed: bool,
    preload: bool,
    prepared_candidate: Option<Box<dyn crate::asr::AsrEngine>>,
    previous_engine: Option<Box<dyn crate::asr::AsrEngine>>,
    attempted: bool,
}

impl AsrRuntimeChange {
    pub(super) fn between(
        current: &AppConfig,
        candidate: &AppConfig,
        model_directory_changed: bool,
        reload_capture: bool,
    ) -> Self {
        let current_local_required = local_asr_required(current);
        let local_required = local_asr_required(candidate);
        let local_runtime_changed = model_directory_changed
            || current.asr.local != candidate.asr.local
            || (!current_local_required && local_required);
        Self {
            changed: capture::asr_runtime_changed(current, candidate) || model_directory_changed,
            preload: reload_capture && local_runtime_changed && local_required,
            prepared_candidate: None,
            previous_engine: None,
            attempted: false,
        }
    }

    pub(super) async fn prepare(
        &mut self,
        candidate: &AppConfig,
        model_directory: PathBuf,
    ) -> Result<(), String> {
        if self.preload {
            self.prepared_candidate = Some(prepare_asr_runtime(candidate, model_directory).await?);
        }
        Ok(())
    }

    pub(super) async fn apply(
        &mut self,
        state: &SettingsContext,
        candidate: &AppConfig,
        model_directory: PathBuf,
    ) -> Result<(), String> {
        if self.changed {
            self.attempted = true;
            self.previous_engine = update_asr_runtime(
                state,
                candidate,
                model_directory,
                self.prepared_candidate.take(),
            )
            .await?;
        }
        Ok(())
    }

    pub(super) async fn rollback(
        &mut self,
        state: &SettingsContext,
        previous: &AppConfig,
    ) -> Result<(), String> {
        if !self.attempted {
            return Ok(());
        }
        let model_directory = state
            .config
            .asr_model_dir_override
            .clone()
            .unwrap_or_else(|| {
                crate::resolve_config_path(
                    &state.config.config_path,
                    &previous.storage.model_directory,
                )
            });
        update_asr_runtime(
            state,
            previous,
            model_directory,
            self.previous_engine.take(),
        )
        .await?;
        self.attempted = false;
        Ok(())
    }
}

fn local_asr_required(config: &AppConfig) -> bool {
    config.asr.backend == "local_whisper" || config.asr.cloud_failure_policy == "local"
}

async fn prepare_asr_runtime(
    config: &AppConfig,
    model_directory: PathBuf,
) -> Result<Box<dyn crate::asr::AsrEngine>, String> {
    let asr_config = config.asr.clone();
    tokio::task::spawn_blocking(move || {
        crate::asr::prepare_local_engine(&asr_config, &model_directory)
    })
    .await
    .map_err(|error| format!("ASR model preload task failed: {error}"))?
}

async fn update_asr_runtime(
    state: &SettingsContext,
    config: &AppConfig,
    model_directory: PathBuf,
    prepared_engine: Option<Box<dyn crate::asr::AsrEngine>>,
) -> Result<Option<Box<dyn crate::asr::AsrEngine>>, String> {
    let asr = Arc::clone(&state.capture.asr);
    let asr_config = config.asr.clone();
    tokio::task::spawn_blocking(move || {
        let previous_engine = asr
            .lock()
            .map_err(|_| "The ASR inference lock is unavailable".to_string())?
            .update(asr_config, model_directory, prepared_engine);
        Ok::<_, String>(previous_engine)
    })
    .await
    .map_err(|error| format!("ASR configuration update task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_runtime_preload_depends_on_capture_activity() {
        let mut current = AppConfig::default();
        current.asr.backend = "local_whisper".into();
        let mut candidate = current.clone();
        candidate.asr.local.model = "medium".into();

        let inactive = AsrRuntimeChange::between(&current, &candidate, false, false);
        assert!(inactive.changed);
        assert!(!inactive.preload);

        let active = AsrRuntimeChange::between(&current, &candidate, false, true);
        assert!(active.changed);
        assert!(active.preload);
    }
}
