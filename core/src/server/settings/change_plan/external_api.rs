use axum::http::StatusCode;

use crate::config::{AppConfig, ExternalApiConfig};
use crate::credentials;

use super::super::super::{api_error, ApiResult, SettingsContext};

pub(super) struct ExternalApiChange {
    reload: bool,
}

impl ExternalApiChange {
    pub(super) fn between(current: &AppConfig, candidate: &AppConfig) -> Self {
        Self {
            reload: current.external_api != candidate.external_api,
        }
    }

    pub(super) async fn apply(
        &self,
        state: &SettingsContext,
        candidate: &AppConfig,
    ) -> ApiResult<()> {
        if !self.reload {
            return Ok(());
        }
        let token = if candidate.external_api.enabled && candidate.external_api.require_token {
            credentials::read_external_api_token().map_err(|error| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "external_api.token_status_failed",
                    error,
                )
            })?
        } else {
            None
        };
        reload_external_api_runtime(state, &candidate.external_api, token)
            .await
            .map_err(|error| {
                api_error(
                    StatusCode::CONFLICT,
                    "settings.external_api_reload_failed",
                    error,
                )
            })
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
