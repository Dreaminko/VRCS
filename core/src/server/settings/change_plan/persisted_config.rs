use crate::config::{AppConfig, save_config};

use super::super::super::SettingsContext;

#[derive(Default)]
pub(super) struct PersistedConfigChange {
    applied: bool,
}

impl PersistedConfigChange {
    pub(super) fn apply(
        &mut self,
        state: &SettingsContext,
        candidate: &AppConfig,
    ) -> Result<(), String> {
        save_config(&state.config.config_path, candidate)?;
        self.applied = true;
        Ok(())
    }

    pub(super) fn rollback(
        &mut self,
        state: &SettingsContext,
        previous: &AppConfig,
    ) -> Result<(), String> {
        if !self.applied {
            return Ok(());
        }
        save_config(&state.config.config_path, previous)
            .map_err(|error| format!("Previous settings could not be restored: {error}"))?;
        self.applied = false;
        Ok(())
    }
}
