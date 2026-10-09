use std::sync::atomic::Ordering;

use crate::config::AppConfig;

use super::super::super::{capture, ApiResult, SettingsContext};

#[derive(Clone, Copy)]
pub(super) struct CaptureChange {
    plan: capture::CaptureReloadPlan,
    pub(super) reload: bool,
    pub(super) resume: bool,
}

impl CaptureChange {
    pub(super) fn between(
        state: &SettingsContext,
        current: &AppConfig,
        candidate: &AppConfig,
    ) -> Self {
        let resume = match candidate.asr.backend.as_str() {
            "local_whisper" => !candidate.asr.local.model.is_empty(),
            crate::config::QWEN_MANAGED_BACKEND => {
                !candidate.asr.managed_qwen.package_id.is_empty()
            }
            _ => true,
        };
        let plan = if resume {
            capture::CaptureReloadPlan::between(current, candidate)
        } else {
            capture::CaptureReloadPlan::all()
        };
        Self {
            plan,
            reload: state.capture.capture_requested.load(Ordering::SeqCst) && !plan.is_empty(),
            resume,
        }
    }

    pub(super) async fn prepare(
        &self,
        state: &SettingsContext,
        candidate: &AppConfig,
    ) -> ApiResult<()> {
        if self.reload && self.resume {
            capture::validate_capture_config(state, candidate).await?;
        }
        Ok(())
    }

    pub(super) async fn stop(&self, state: &SettingsContext) {
        if self.reload {
            capture::stop_pipelines(state, self.plan).await;
        }
    }

    pub(super) async fn stop_for_rollback(&self, state: &SettingsContext) {
        capture::stop_pipelines(state, self.plan).await;
    }

    pub(super) async fn start_candidate(
        &self,
        state: &SettingsContext,
        candidate: &AppConfig,
    ) -> ApiResult<()> {
        if self.reload {
            if self.resume {
                capture::start_pipelines(state, candidate, self.plan).await?;
            } else {
                state
                    .capture
                    .capture_requested
                    .store(false, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    pub(super) async fn restore(
        &self,
        state: &SettingsContext,
        previous: &AppConfig,
    ) -> Result<(), String> {
        if self.reload {
            state
                .capture
                .capture_requested
                .store(true, Ordering::SeqCst);
            capture::start_pipelines(state, previous, self.plan)
                .await
                .map_err(|error| super::api_detail(&error))?;
        }
        Ok(())
    }
}
