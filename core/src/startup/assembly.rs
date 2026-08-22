use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::db::Database;
use crate::server::{
    AppState, CaptureRuntime, CaptureRuntimeInput, ConfigRuntime, ConfigRuntimeInput,
    ContentServices, ContentServicesInput, IntegrationRuntime, IntegrationRuntimeInput,
};
use crate::{
    asr, credentials, domain_events, external_api, glossary, learning, osc, subtitle_output,
    translation, vad, vrchat_mute_sync, vrcx,
};

use super::plan::StartupPlan;

pub(crate) struct RuntimeAssembly {
    pub(crate) requested_address: std::net::SocketAddr,
    pub(crate) session_token: String,
    pub(crate) shutdown_tx: watch::Sender<bool>,
    pub(crate) shutdown_rx: watch::Receiver<bool>,
    pub(crate) state: Arc<AppState>,
    pub(crate) model_manager: Arc<asr::ModelManager>,
    pub(crate) vad_runtime: vad::VadRuntimeState,
    pub(crate) vad_prepare_task: Option<JoinHandle<()>>,
}

impl RuntimeAssembly {
    pub(crate) async fn build(plan: StartupPlan) -> Result<Self, String> {
        let mut database = Database::open(&plan.database_path).map_err(|error| {
            format!(
                "Failed to open database {}: {error}",
                plan.database_path.display()
            )
        })?;
        database
            .set_subtitle_history_max_bytes(plan.config.storage.subtitle_history_max_bytes)
            .map_err(|error| format!("Failed to apply subtitle history storage quota: {error}"))?;

        let (vad_runtime, vad_prepare_task) = prepare_vad(&plan).await;
        let model_manager = Arc::new(asr::ModelManager::new(plan.asr_model_dir.clone())?);
        let asr_config = plan.config.asr.clone();

        let (subtitles_tx, _) = broadcast::channel(50);
        let (live_tx, _) = broadcast::channel(100);
        let (conversation_catalog_tx, _) = broadcast::channel(50);
        let (translation_tx, _) = broadcast::channel(100);
        let domain_events = domain_events::DomainEventHub::new();
        let db = Arc::new(Mutex::new(database));
        let osc = osc::OscChatboxDispatcher::new_with_db_and_events(
            plan.config.osc.clone(),
            Arc::clone(&db),
            domain_events.clone(),
        );
        let subtitle_output = subtitle_output::SubtitleLifecyclePublisher::with_domain_events(
            subtitles_tx,
            translation_tx,
            osc.clone(),
            domain_events.clone(),
        );
        let glossary = Arc::new(glossary::GlossaryStore::new(
            plan.glossary_cache_path.clone(),
            plan.config.glossary.clone(),
        )?);
        let translation_service = Arc::new(translation::TranslationService::with_glossary(
            Arc::clone(&glossary),
        )?);
        let learning_service = Arc::new(learning::LearningService::new()?);

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let vrcx = vrcx::VrcxIntegration::new(shutdown_rx.clone());
        let vrcx_token = match credentials::read_vrcx_token() {
            Ok(token) => token,
            Err(error) => {
                tracing::warn!(%error, "VRCX-0 token could not be read");
                None
            }
        };
        vrcx.reconfigure(plan.config.vrcx.clone(), vrcx_token).await;
        let translation_dispatcher = translation::TranslationDispatcher::new(
            Arc::clone(&translation_service),
            Arc::clone(&db),
            conversation_catalog_tx.clone(),
            subtitle_output.clone(),
            vrcx.clone(),
        );
        let asr_service = asr::AsrService::new(asr_config, plan.asr_model_dir.clone());
        let asr_runtime = asr_service.runtime_state();
        let asr = Arc::new(Mutex::new(asr_service));

        let (external_api_server, external_api_status) =
            start_external_api(&plan, domain_events.clone(), shutdown_rx.clone()).await;
        let vrchat_mute_sync = vrchat_mute_sync::VrchatMuteSync::new(
            plan.config.osc.mute_sync_enabled,
            shutdown_rx.clone(),
        );
        let (vr_overlay_config_tx, _) = watch::channel(plan.config.vr_overlay.clone());

        let config_runtime = ConfigRuntime::new(ConfigRuntimeInput {
            config_path: plan.config_path,
            asr_model_dir_override: plan.asr_model_dir_override,
            config: plan.config,
        });
        let capture_runtime = CaptureRuntime::new(CaptureRuntimeInput {
            live_tx,
            vad_runtime: vad_runtime.clone(),
            asr,
            asr_runtime,
            model_manager: Arc::clone(&model_manager),
            vad_model_path: plan.vad_model_path,
            shutdown: shutdown_rx.clone(),
        });
        let content_services = ContentServices::new(ContentServicesInput {
            db,
            conversation_catalog_tx,
            subtitle_output,
            translation_service,
            learning_service,
            translation_dispatcher,
            glossary,
        });
        let integration_runtime = IntegrationRuntime::new(IntegrationRuntimeInput {
            vr_overlay_config_tx,
            osc,
            session_token: plan.session_token.clone(),
            domain_events,
            external_api_server,
            external_api_status,
            shutdown: shutdown_rx.clone(),
            vrchat_mute_sync,
            vrcx,
        });
        let state = Arc::new(AppState::new(
            config_runtime,
            capture_runtime,
            content_services,
            integration_runtime,
        ));

        Ok(Self {
            requested_address: plan.requested_address,
            session_token: plan.session_token,
            shutdown_tx,
            shutdown_rx,
            state,
            model_manager,
            vad_runtime,
            vad_prepare_task,
        })
    }
}

async fn prepare_vad(plan: &StartupPlan) -> (vad::VadRuntimeState, Option<JoinHandle<()>>) {
    let prepare_task = if plan.managed_vad_model && plan.defer_managed_vad {
        let path = plan.vad_model_path.clone();
        Some(tokio::spawn(async move {
            let started = Instant::now();
            match vad::ensure_model(&path).await {
                Ok(()) => tracing::info!(
                    elapsed_ms = started.elapsed().as_millis(),
                    "Silero VAD model prepared in background"
                ),
                Err(error) => tracing::warn!(
                    %error,
                    elapsed_ms = started.elapsed().as_millis(),
                    "Silero VAD model unavailable; energy fallback remains active"
                ),
            }
        }))
    } else {
        None
    };
    if plan.managed_vad_model && !plan.defer_managed_vad {
        if let Err(error) = vad::ensure_model(&plan.vad_model_path).await {
            tracing::warn!(%error, "Silero VAD model unavailable; using energy fallback");
        }
    }
    let runtime = vad::VadRuntimeState::default();
    if !plan.defer_managed_vad {
        let _ = vad::VoiceDetector::load_with_runtime(&plan.vad_model_path, runtime.clone());
    }
    (runtime, prepare_task)
}

async fn start_external_api(
    plan: &StartupPlan,
    domain_events: domain_events::DomainEventHub,
    shutdown: watch::Receiver<bool>,
) -> (
    Option<external_api::ExternalApiServer>,
    external_api::ExternalApiRuntimeStatus,
) {
    let result = if plan.config.external_api.enabled {
        match credentials::read_external_api_token() {
            Ok(token) => {
                external_api::start(&plan.config.external_api, domain_events, token, shutdown).await
            }
            Err(error) => Err(format!("Failed to read the External API token: {error}")),
        }
    } else {
        Ok(None)
    };
    match result {
        Ok(Some(server)) => {
            let status = external_api::ExternalApiRuntimeStatus::running(server.address);
            (Some(server), status)
        }
        Ok(None) => (None, external_api::ExternalApiRuntimeStatus::disabled()),
        Err(error) => {
            tracing::error!(%error, "External API listener could not start");
            (None, external_api::ExternalApiRuntimeStatus::failed(error))
        }
    }
}
