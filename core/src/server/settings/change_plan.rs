use std::sync::atomic::Ordering;

use axum::http::StatusCode;
use axum::Json;
use serde_json::Value;

use crate::config::AppConfig;

use super::super::{api_error, ApiResult, SettingsContext};

mod asr_runtime;
mod capture;
mod external_api;
mod model_directory;
mod persisted_config;
mod post_commit;
mod validation;

use asr_runtime::AsrRuntimeChange;
use capture::CaptureChange;
use external_api::ExternalApiChange;
use model_directory::ModelDirectoryChange;
use persisted_config::PersistedConfigChange;
use post_commit::PostCommitUpdates;

type ApiError = (StatusCode, Json<Value>);

pub(super) struct SettingsCommitResult {
    pub(super) revision: u64,
    pub(super) config: AppConfig,
}

pub(super) struct SettingsChangePlan {
    current: AppConfig,
    candidate: AppConfig,
    effective_candidate: AppConfig,
    capture: CaptureChange,
    model_directory: ModelDirectoryChange,
    asr_runtime: AsrRuntimeChange,
    persisted_config: PersistedConfigChange,
    external_api: ExternalApiChange,
    post_commit: PostCommitUpdates,
}

impl SettingsChangePlan {
    pub(super) async fn prepare_update(
        state: &SettingsContext,
        mut candidate: AppConfig,
        has_current_revision: bool,
    ) -> ApiResult<Self> {
        let current = state.config.config.read().expect("config lock").clone();
        super::protect_profile_owned_settings(&mut candidate, &current, has_current_revision);
        validation::validate_candidate(state, &mut candidate, &current).await?;
        Self::build(state, candidate, current).await
    }

    pub(super) async fn prepare(state: &SettingsContext, candidate: AppConfig) -> ApiResult<Self> {
        let current = state.config.config.read().expect("config lock").clone();
        Self::build(state, candidate, current).await
    }

    async fn build(
        state: &SettingsContext,
        candidate: AppConfig,
        current: AppConfig,
    ) -> ApiResult<Self> {
        let language_session = state
            .config
            .language_session
            .read()
            .expect("language session lock")
            .clone();
        let effective_current = language_session.apply_to(&current);
        let effective_candidate = language_session.apply_to(&candidate);
        let capture = CaptureChange::between(state, &effective_current, &effective_candidate);
        capture.prepare(state, &effective_candidate).await?;

        let model_directory = ModelDirectoryChange::between(state, &current, &candidate);
        let asr_runtime = AsrRuntimeChange::between(
            &current,
            &candidate,
            model_directory.changed,
            capture.reload,
        );
        let external_api = ExternalApiChange::between(&current, &candidate);
        let post_commit = PostCommitUpdates::between(&current, &candidate);

        Ok(Self {
            current,
            candidate,
            effective_candidate,
            capture,
            model_directory,
            asr_runtime,
            persisted_config: PersistedConfigChange::default(),
            external_api,
            post_commit,
        })
    }

    pub(super) async fn apply(
        mut self,
        state: &SettingsContext,
    ) -> ApiResult<SettingsCommitResult> {
        self.capture.stop(state).await;

        if let Err(detail) = self.model_directory.apply(state).await {
            let error = api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "settings.model_directory_migration_failed",
                detail,
            );
            return Err(self.rollback_or(state, error).await);
        }

        if let Err(detail) = self
            .asr_runtime
            .prepare(&self.candidate, self.model_directory.candidate_path.clone())
            .await
        {
            let error = api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "settings.asr_preload_failed",
                detail,
            );
            return Err(self.rollback_or(state, error).await);
        }

        if let Err(detail) = self.persisted_config.apply(state, &self.candidate) {
            let error = api_error(StatusCode::UNPROCESSABLE_ENTITY, "settings.invalid", detail);
            return Err(self.rollback_or(state, error).await);
        }

        if let Err(detail) = self
            .asr_runtime
            .apply(
                state,
                &self.candidate,
                self.model_directory.candidate_path.clone(),
            )
            .await
        {
            let error = api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "settings.asr_update_failed",
                detail,
            );
            return Err(self.rollback_or(state, error).await);
        }

        if let Err(error) = self
            .capture
            .start_candidate(state, &self.effective_candidate)
            .await
        {
            return Err(self.rollback_or(state, error).await);
        }

        if let Err(error) = self.external_api.apply(state, &self.candidate).await {
            return Err(self.rollback_or(state, error).await);
        }

        // The durable and fallible runtime changes are complete. Updates below are
        // best-effort projections of the committed configuration and are not rolled back.
        self.post_commit
            .apply(state, &self.candidate, &self.effective_candidate)
            .await;
        let revision = state.config.config_revision.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(SettingsCommitResult {
            revision,
            config: self.candidate,
        })
    }

    async fn rollback_or(&mut self, state: &SettingsContext, error: ApiError) -> ApiError {
        let detail = api_detail(&error);
        match self.rollback(state).await {
            Ok(()) => error,
            Err(recovery) => rollback_error(detail, recovery),
        }
    }

    async fn rollback(&mut self, state: &SettingsContext) -> Result<(), String> {
        let mut errors = Vec::new();
        self.capture.stop_for_rollback(state).await;
        if let Err(error) = self.model_directory.rollback(state).await {
            errors.push(error);
        }
        if let Err(error) = self.persisted_config.rollback(state, &self.current) {
            errors.push(error);
        }
        if let Err(error) = self.asr_runtime.rollback(state, &self.current).await {
            errors.push(error);
        }
        if let Err(error) = self.capture.restore(state, &self.current).await {
            errors.push(error);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

pub(super) async fn reload_external_api_runtime(
    state: &SettingsContext,
    config: &crate::config::ExternalApiConfig,
    token: Option<String>,
) -> Result<(), String> {
    external_api::reload_external_api_runtime(state, config, token).await
}

fn api_detail(error: &ApiError) -> String {
    error
        .1
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or("Capture reconfiguration failed")
        .to_string()
}

fn rollback_error(error: impl std::fmt::Display, recovery: impl std::fmt::Display) -> ApiError {
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "settings.rollback_failed",
        format!("{error}; rollback failed: {recovery}"),
    )
}
