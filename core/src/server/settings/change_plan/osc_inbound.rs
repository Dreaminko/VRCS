use std::sync::Arc;

use axum::http::StatusCode;

use crate::config::AppConfig;

use super::super::super::{
    ApiResult, ServiceContext, SettingsContext, api_error, osc_inbound_callback,
};

pub(super) struct OscInboundChange {
    reload: bool,
}

impl OscInboundChange {
    pub(super) fn between(current: &AppConfig, candidate: &AppConfig) -> Self {
        Self {
            reload: current.osc.inbound != candidate.osc.inbound
                || (candidate.osc.inbound.enabled && current.osc.port != candidate.osc.port),
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
        let service_ctx = ServiceContext {
            config: Arc::clone(&state.config),
            content: Arc::clone(&state.content),
            integrations: Arc::clone(&state.integrations),
        };
        let mut server = state.integrations.osc_inbound_server.lock().await;
        crate::osc::inbound::reconfigure(
            &mut server,
            &candidate.osc,
            osc_inbound_callback(service_ctx),
            Arc::clone(&state.integrations.osc_inbound_status),
            state.integrations.shutdown.clone(),
        )
        .await
        .map_err(|error| {
            api_error(
                StatusCode::CONFLICT,
                "settings.osc_inbound_reload_failed",
                error,
            )
        })
    }
}
