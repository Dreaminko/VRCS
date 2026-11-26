use std::sync::atomic::Ordering;

use crate::config::AppConfig;

use super::super::super::{ApiResult, SettingsContext, capture};

#[derive(Clone, Copy)]
pub(super) struct CaptureChange {
    plan: capture::CaptureReloadPlan,
    pub(super) reload: bool,
    pub(super) resume: bool,
    stop_network: bool,
    standby_network_restart: bool,
}

impl CaptureChange {
    pub(super) fn include_credential_change(&mut self, state: &SettingsContext) {
        self.plan = capture::CaptureReloadPlan::all();
        self.reload = state.capture.capture_requested.load(Ordering::SeqCst);
    }

    pub(super) fn between(
        state: &SettingsContext,
        current: &AppConfig,
        candidate: &AppConfig,
    ) -> Self {
        let capture_requested = state.capture.capture_requested.load(Ordering::SeqCst);
        let network_requested = state
            .capture
            .network_capture_requested
            .load(Ordering::SeqCst);
        let resume = candidate.vrchat_chatbox.enabled
            || (capture::has_audio_sources(candidate)
                && match candidate.asr.backend.as_str() {
                    crate::config::QWEN_MANAGED_BACKEND => {
                        !candidate.asr.managed_qwen.package_id.is_empty()
                    }
                    _ => true,
                });
        let pagefile_drive_changed = current.audio.output.player_audio.pagefile_drive
            != candidate.audio.output.player_audio.pagefile_drive;
        let plan = if resume {
            capture::CaptureReloadPlan::between(current, candidate)
        } else {
            capture::CaptureReloadPlan {
                network_restart: !network_requested || pagefile_drive_changed,
                ..capture::CaptureReloadPlan::all()
            }
        };
        let stop_network = network_requested && !candidate.audio.output.player_audio.enabled;
        let standby_network_restart = network_requested
            && (!capture_requested || !resume)
            && candidate.audio.output.player_audio.enabled
            && pagefile_drive_changed;
        Self {
            plan,
            reload: capture_requested && !plan.is_empty(),
            resume,
            stop_network,
            standby_network_restart,
        }
    }

    pub(super) async fn prepare(
        &self,
        state: &SettingsContext,
        candidate: &AppConfig,
    ) -> ApiResult<()> {
        if self.reload && self.resume && self.plan.audio_changed() {
            capture::validate_capture_config(state, candidate).await?;
        }
        Ok(())
    }

    pub(super) async fn stop(&self, state: &SettingsContext) {
        if self.reload {
            capture::stop_pipelines(state, self.plan).await;
        } else if self.stop_network || self.standby_network_restart {
            state.capture.network_session.lock().await.stop().await;
        }
        if self.stop_network {
            state.capture.set_network_capture_requested(false);
            if self.reload {
                state.capture.network_session.lock().await.stop().await;
            }
        }
    }

    pub(super) async fn stop_for_rollback(&self, state: &SettingsContext) {
        if self.reload {
            capture::stop_pipelines(state, self.plan).await;
        } else if self.stop_network || self.standby_network_restart {
            state.capture.network_session.lock().await.stop().await;
        }
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
                state.capture.set_capture_requested(false);
                if self.standby_network_restart {
                    capture::reconcile_network_with(state, candidate, true, Default::default())
                        .await?;
                }
            }
        } else if self.standby_network_restart {
            capture::reconcile_network_with(state, candidate, true, Default::default()).await?;
        }
        Ok(())
    }

    pub(super) async fn restore(
        &self,
        state: &SettingsContext,
        previous: &AppConfig,
    ) -> Result<(), String> {
        if self.stop_network {
            state.capture.set_network_capture_requested(true);
        }
        if self.reload {
            state.capture.set_capture_requested(true);
            let effective = state
                .config
                .language_session
                .read()
                .expect("language session lock")
                .apply_to(previous);
            capture::start_pipelines(state, &effective, self.plan)
                .await
                .map_err(|error| super::api_detail(&error))?;
        } else if self.stop_network || self.standby_network_restart {
            let effective = state
                .config
                .language_session
                .read()
                .expect("language session lock")
                .apply_to(previous);
            capture::reconcile_network_with(state, &effective, true, Default::default())
                .await
                .map(|_| ())
                .map_err(|error| super::api_detail(&error))?;
        }
        Ok(())
    }
}
