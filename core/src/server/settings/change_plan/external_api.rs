use axum::http::StatusCode;

use crate::config::{AppConfig, ExternalApiConfig};
use crate::credentials;

use super::super::super::{ApiResult, SettingsContext, api_error};

pub(super) struct ExternalApiChange {
    reload: bool,
    applied: bool,
}

impl ExternalApiChange {
    pub(super) fn between(current: &AppConfig, candidate: &AppConfig) -> Self {
        Self {
            reload: current.external_api != candidate.external_api,
            applied: false,
        }
    }

    pub(super) async fn apply(
        &mut self,
        state: &SettingsContext,
        candidate: &AppConfig,
    ) -> ApiResult<()> {
        if !self.reload {
            return Ok(());
        }
        let token = read_external_api_token(&candidate.external_api).await?;
        reload_external_api_runtime(state, &candidate.external_api, token)
            .await
            .map_err(|error| {
                api_error(
                    StatusCode::CONFLICT,
                    "settings.external_api_reload_failed",
                    error,
                )
            })?;
        self.applied = true;
        Ok(())
    }

    pub(super) async fn rollback(
        &mut self,
        state: &SettingsContext,
        previous: &AppConfig,
    ) -> Result<(), String> {
        if !self.applied {
            return Ok(());
        }
        let token = if previous.external_api.enabled && previous.external_api.require_token {
            credentials::blocking(credentials::read_external_api_token).await?
        } else {
            None
        };
        reload_external_api_runtime(state, &previous.external_api, token).await?;
        self.applied = false;
        Ok(())
    }
}

async fn read_external_api_token(config: &ExternalApiConfig) -> ApiResult<Option<String>> {
    if config.enabled && config.require_token {
        credentials::blocking(credentials::read_external_api_token)
            .await
            .map_err(|error| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    credentials::error_reason_code(&error)
                        .unwrap_or("external_api.token_status_failed"),
                    error,
                )
            })
    } else {
        Ok(None)
    }
}

pub(super) async fn reload_external_api_runtime(
    state: &SettingsContext,
    config: &ExternalApiConfig,
    token: Option<String>,
) -> Result<(), String> {
    let mut server = state.integrations.external_api_server.lock().await;
    let result = crate::external_api::reconfigure(
        &mut server,
        config,
        state.integrations.domain_events.clone(),
        token,
        state.integrations.shutdown.clone(),
    )
    .await;
    let status = match server.as_ref() {
        Some(server) => crate::external_api::ExternalApiRuntimeStatus::running(server.address),
        None if config.enabled => crate::external_api::ExternalApiRuntimeStatus::failed(
            result
                .as_ref()
                .err()
                .cloned()
                .unwrap_or_else(|| "External API listener is unavailable".into()),
        ),
        None => crate::external_api::ExternalApiRuntimeStatus::disabled(),
    };
    *state
        .integrations
        .external_api_status
        .write()
        .expect("External API status lock") = status;
    result
}
