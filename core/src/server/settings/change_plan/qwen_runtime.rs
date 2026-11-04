use crate::config::{AppConfig, QWEN_MANAGED_BACKEND};

use super::super::super::SettingsContext;

pub(super) struct QwenRuntimeChange {
    preload: bool,
    stop: bool,
}

impl QwenRuntimeChange {
    pub(super) fn between(
        current: &AppConfig,
        candidate: &AppConfig,
        model_directory_changed: bool,
        reload_capture: bool,
    ) -> Self {
        Self {
            preload: reload_capture
                && crate::server::capture::has_audio_sources(candidate)
                && (model_directory_changed
                    || crate::server::capture::CaptureReloadPlan::between(current, candidate)
                        .audio_changed())
                && candidate.asr.backend == QWEN_MANAGED_BACKEND,
            stop: current.asr.backend == QWEN_MANAGED_BACKEND
                && (model_directory_changed
                    || candidate.asr.backend != QWEN_MANAGED_BACKEND
                    || current.asr.managed_qwen != candidate.asr.managed_qwen),
        }
    }

    pub(super) async fn prepare(
        &self,
        state: &SettingsContext,
        candidate: &AppConfig,
    ) -> Result<(), String> {
        if self.stop {
            state.capture.qwen_runtime.stop().await?;
        }
        if self.preload {
            crate::asr::prepare_managed_qwen(
                &candidate.asr,
                &state.capture.qwen_runtime,
                &state.capture.model_manager,
            )
            .await?;
        }
        Ok(())
    }

    pub(super) async fn rollback(&self, state: &SettingsContext) -> Result<(), String> {
        if self.preload {
            state.capture.qwen_runtime.stop().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_qwen_preload_depends_on_capture_activity() {
        let current = AppConfig::default();
        let mut candidate = current.clone();
        candidate.asr.backend = QWEN_MANAGED_BACKEND.into();
        candidate.asr.managed_qwen.device = "cpu".into();
        assert!(!QwenRuntimeChange::between(&current, &candidate, false, false).preload);
        assert!(QwenRuntimeChange::between(&current, &candidate, false, true).preload);
    }

    #[test]
    fn chatbox_toggle_and_text_only_do_not_preload_qwen() {
        let mut current = AppConfig::default();
        current.asr.backend = QWEN_MANAGED_BACKEND.into();
        let mut candidate = current.clone();
        candidate.vrchat_chatbox.enabled = true;
        assert!(!QwenRuntimeChange::between(&current, &candidate, false, true).preload);
        candidate.audio.output.mode = "disabled".into();
        candidate.audio.microphone.mode = "disabled".into();
        assert!(!QwenRuntimeChange::between(&current, &candidate, false, true).preload);
    }

    #[test]
    fn changing_the_qwen_storage_or_device_stops_the_old_runtime() {
        let mut current = AppConfig::default();
        current.asr.backend = QWEN_MANAGED_BACKEND.into();
        assert!(QwenRuntimeChange::between(&current, &current, true, false).stop);
        let mut candidate = current.clone();
        candidate.asr.managed_qwen.device = "cpu".into();
        assert!(QwenRuntimeChange::between(&current, &candidate, false, false).stop);
        assert!(!QwenRuntimeChange::between(&current, &current, false, false).stop);
    }
}
