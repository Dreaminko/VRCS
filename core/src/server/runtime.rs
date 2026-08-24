use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, RwLock};

use axum::extract::FromRef;
use tokio::sync::{broadcast, watch, Mutex as AsyncMutex};

use crate::config::{AppConfig, VrOverlayConfig};
use crate::db::conversations::ConversationCatalog;
use crate::db::Database;
use crate::microphone_monitor::MicrophoneMonitor;
use crate::models::LiveTranscription;
use crate::osc::OscChatboxDispatcher;
use crate::pipeline::TranscriptionPipeline;
use crate::subtitle_output::SubtitleLifecyclePublisher;
use crate::translation::{TranslationDispatcher, TranslationService};
use crate::{asr, smart_turn, vad};

pub(crate) struct ConfigRuntime {
    pub(crate) config_path: PathBuf,
    pub(crate) asr_model_dir_override: Option<PathBuf>,
    pub(crate) config: Arc<RwLock<AppConfig>>,
    pub(crate) language_session: Arc<RwLock<crate::language_session::ActiveLanguageSession>>,
    pub(crate) config_epoch: String,
    pub(crate) config_revision: AtomicU64,
    pub(crate) config_control: AsyncMutex<()>,
}

pub(crate) struct ConfigRuntimeInput {
    pub(crate) config_path: PathBuf,
    pub(crate) asr_model_dir_override: Option<PathBuf>,
    pub(crate) config: AppConfig,
}

impl ConfigRuntime {
    pub(crate) fn new(input: ConfigRuntimeInput) -> Self {
        Self {
            config_path: input.config_path,
            asr_model_dir_override: input.asr_model_dir_override,
            config: Arc::new(RwLock::new(input.config)),
            language_session: Arc::new(RwLock::new(
                crate::language_session::ActiveLanguageSession::Global,
            )),
            config_epoch: uuid::Uuid::new_v4().to_string(),
            config_revision: AtomicU64::new(0),
            config_control: AsyncMutex::new(()),
        }
    }
}

pub(crate) struct CaptureRuntime {
    pub(crate) live_tx: broadcast::Sender<LiveTranscription>,
    pub(crate) vad_runtime: vad::VadRuntimeState,
    pub(crate) asr: Arc<Mutex<asr::AsrService>>,
    pub(crate) asr_runtime: asr::AsrRuntimeState,
    pub(crate) model_manager: Arc<asr::ModelManager>,
    pub(crate) capture_control: AsyncMutex<()>,
    pub(crate) capture_requested: AtomicBool,
    pub(crate) speaker_pipeline: AsyncMutex<TranscriptionPipeline>,
    pub(crate) microphone_pipeline: AsyncMutex<TranscriptionPipeline>,
    pub(crate) microphone_monitor: AsyncMutex<MicrophoneMonitor>,
}

pub(crate) struct CaptureRuntimeInput {
    pub(crate) live_tx: broadcast::Sender<LiveTranscription>,
    pub(crate) vad_runtime: vad::VadRuntimeState,
    pub(crate) asr: Arc<Mutex<asr::AsrService>>,
    pub(crate) asr_runtime: asr::AsrRuntimeState,
    pub(crate) model_manager: Arc<asr::ModelManager>,
    pub(crate) vad_model_path: PathBuf,
    pub(crate) shutdown: watch::Receiver<bool>,
}

impl CaptureRuntime {
    pub(crate) fn new(input: CaptureRuntimeInput) -> Self {
        let smart_turn_runtime = smart_turn::SmartTurnRuntime::new(
            input
                .vad_model_path
                .with_file_name(smart_turn::MODEL_FILENAME),
        );
        Self {
            live_tx: input.live_tx,
            vad_runtime: input.vad_runtime.clone(),
            asr: input.asr,
            asr_runtime: input.asr_runtime,
            model_manager: input.model_manager,
            capture_control: AsyncMutex::new(()),
            capture_requested: AtomicBool::new(false),
            speaker_pipeline: AsyncMutex::new(TranscriptionPipeline::new(
                crate::audio::CaptureSource::Speaker,
                "speaker",
                input.vad_model_path.clone(),
                input.vad_runtime.clone(),
                smart_turn_runtime.clone(),
                input.shutdown.clone(),
            )),
            microphone_pipeline: AsyncMutex::new(TranscriptionPipeline::new(
                crate::audio::CaptureSource::Microphone,
                "microphone",
                input.vad_model_path,
                input.vad_runtime,
                smart_turn_runtime,
                input.shutdown,
            )),
            microphone_monitor: AsyncMutex::new(MicrophoneMonitor::new()),
        }
    }
}

pub(crate) struct ContentServices {
    pub(crate) db: Arc<Mutex<Database>>,
    pub(crate) conversation_catalog_tx: broadcast::Sender<ConversationCatalog>,
    pub(crate) subtitle_output: SubtitleLifecyclePublisher,
    pub(crate) translation_service: Arc<TranslationService>,
    pub(crate) learning_service: Arc<crate::learning::LearningService>,
    pub(crate) translation_dispatcher: TranslationDispatcher,
    pub(crate) glossary: Arc<crate::glossary::GlossaryStore>,
}

pub(crate) struct ContentServicesInput {
    pub(crate) db: Arc<Mutex<Database>>,
    pub(crate) conversation_catalog_tx: broadcast::Sender<ConversationCatalog>,
    pub(crate) subtitle_output: SubtitleLifecyclePublisher,
    pub(crate) translation_service: Arc<TranslationService>,
    pub(crate) learning_service: Arc<crate::learning::LearningService>,
    pub(crate) translation_dispatcher: TranslationDispatcher,
    pub(crate) glossary: Arc<crate::glossary::GlossaryStore>,
}

impl ContentServices {
    pub(crate) fn new(input: ContentServicesInput) -> Self {
        Self {
            db: input.db,
            conversation_catalog_tx: input.conversation_catalog_tx,
            subtitle_output: input.subtitle_output,
            translation_service: input.translation_service,
            learning_service: input.learning_service,
            translation_dispatcher: input.translation_dispatcher,
            glossary: input.glossary,
        }
    }
}

pub(crate) struct IntegrationRuntime {
    pub(crate) vr_overlay_config_tx: watch::Sender<VrOverlayConfig>,
    pub(crate) osc: OscChatboxDispatcher,
    pub(crate) http: reqwest::Client,
    pub(crate) session_token: String,
    pub(crate) domain_events: crate::domain_events::DomainEventHub,
    pub(crate) external_api_server: AsyncMutex<Option<crate::external_api::ExternalApiServer>>,
    pub(crate) external_api_status: RwLock<crate::external_api::ExternalApiRuntimeStatus>,
    pub(crate) shutdown: watch::Receiver<bool>,
    pub(crate) vrchat_mute_sync: crate::vrchat_mute_sync::VrchatMuteSync,
    pub(crate) vrcx: crate::vrcx::VrcxIntegration,
}

pub(crate) struct IntegrationRuntimeInput {
    pub(crate) vr_overlay_config_tx: watch::Sender<VrOverlayConfig>,
    pub(crate) osc: OscChatboxDispatcher,
    pub(crate) session_token: String,
    pub(crate) domain_events: crate::domain_events::DomainEventHub,
    pub(crate) external_api_server: Option<crate::external_api::ExternalApiServer>,
    pub(crate) external_api_status: crate::external_api::ExternalApiRuntimeStatus,
    pub(crate) shutdown: watch::Receiver<bool>,
    pub(crate) vrchat_mute_sync: crate::vrchat_mute_sync::VrchatMuteSync,
    pub(crate) vrcx: crate::vrcx::VrcxIntegration,
}

impl IntegrationRuntime {
    pub(crate) fn new(input: IntegrationRuntimeInput) -> Self {
        Self {
            vr_overlay_config_tx: input.vr_overlay_config_tx,
            osc: input.osc,
            http: crate::anki::client(),
            session_token: input.session_token,
            domain_events: input.domain_events,
            external_api_server: AsyncMutex::new(input.external_api_server),
            external_api_status: RwLock::new(input.external_api_status),
            shutdown: input.shutdown,
            vrchat_mute_sync: input.vrchat_mute_sync,
            vrcx: input.vrcx,
        }
    }
}

pub(crate) struct AppState {
    pub(crate) config: Arc<ConfigRuntime>,
    pub(crate) capture: Arc<CaptureRuntime>,
    pub(crate) content: Arc<ContentServices>,
    pub(crate) integrations: Arc<IntegrationRuntime>,
}

impl AppState {
    pub(crate) fn new(
        config: ConfigRuntime,
        capture: CaptureRuntime,
        content: ContentServices,
        integrations: IntegrationRuntime,
    ) -> Self {
        Self {
            config: Arc::new(config),
            capture: Arc::new(capture),
            content: Arc::new(content),
            integrations: Arc::new(integrations),
        }
    }
}

#[derive(Clone)]
pub(crate) struct ContentState(pub(crate) Arc<ContentServices>);

impl Deref for ContentState {
    type Target = ContentServices;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromRef<Arc<AppState>> for ContentState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self(Arc::clone(&state.content))
    }
}

#[derive(Clone)]
pub(crate) struct IntegrationState(pub(crate) Arc<IntegrationRuntime>);

impl Deref for IntegrationState {
    type Target = IntegrationRuntime;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromRef<Arc<AppState>> for IntegrationState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self(Arc::clone(&state.integrations))
    }
}

#[derive(Clone)]
pub(crate) struct CaptureContext {
    pub(crate) config: Arc<ConfigRuntime>,
    pub(crate) capture: Arc<CaptureRuntime>,
    pub(crate) content: Arc<ContentServices>,
    pub(crate) integrations: Arc<IntegrationRuntime>,
}

impl CaptureContext {
    pub(crate) fn from_app(state: &AppState) -> Self {
        Self {
            config: Arc::clone(&state.config),
            capture: Arc::clone(&state.capture),
            content: Arc::clone(&state.content),
            integrations: Arc::clone(&state.integrations),
        }
    }
}

impl FromRef<Arc<AppState>> for CaptureContext {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self::from_app(state)
    }
}

#[derive(Clone)]
pub(crate) struct SettingsContext(CaptureContext);

impl SettingsContext {
    pub(crate) fn from_app(state: &AppState) -> Self {
        Self(CaptureContext::from_app(state))
    }
}

impl Deref for SettingsContext {
    type Target = CaptureContext;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromRef<Arc<AppState>> for SettingsContext {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self::from_app(state)
    }
}

#[derive(Clone)]
pub(crate) struct ServiceContext {
    pub(crate) config: Arc<ConfigRuntime>,
    pub(crate) content: Arc<ContentServices>,
    pub(crate) integrations: Arc<IntegrationRuntime>,
}

impl ServiceContext {
    pub(crate) fn from_app(state: &AppState) -> Self {
        Self {
            config: Arc::clone(&state.config),
            content: Arc::clone(&state.content),
            integrations: Arc::clone(&state.integrations),
        }
    }
}

impl FromRef<Arc<AppState>> for ServiceContext {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self::from_app(state)
    }
}

#[derive(Clone)]
pub(crate) struct ModelContext {
    pub(crate) config: Arc<ConfigRuntime>,
    pub(crate) capture: Arc<CaptureRuntime>,
}

impl FromRef<Arc<AppState>> for ModelContext {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            config: Arc::clone(&state.config),
            capture: Arc::clone(&state.capture),
        }
    }
}

#[derive(Clone)]
pub(crate) struct OutputContext {
    pub(crate) content: Arc<ContentServices>,
    pub(crate) integrations: Arc<IntegrationRuntime>,
}

impl FromRef<Arc<AppState>> for OutputContext {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            content: Arc::clone(&state.content),
            integrations: Arc::clone(&state.integrations),
        }
    }
}

#[derive(Clone)]
pub(crate) struct RealtimeContext {
    pub(crate) capture: Arc<CaptureRuntime>,
    pub(crate) content: Arc<ContentServices>,
    pub(crate) integrations: Arc<IntegrationRuntime>,
}

impl FromRef<Arc<AppState>> for RealtimeContext {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            capture: Arc::clone(&state.capture),
            content: Arc::clone(&state.content),
            integrations: Arc::clone(&state.integrations),
        }
    }
}

#[derive(Clone)]
pub(crate) struct HealthContext {
    pub(crate) config: Arc<ConfigRuntime>,
    pub(crate) capture: Arc<CaptureRuntime>,
    pub(crate) integrations: Arc<IntegrationRuntime>,
}

impl FromRef<Arc<AppState>> for HealthContext {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            config: Arc::clone(&state.config),
            capture: Arc::clone(&state.capture),
            integrations: Arc::clone(&state.integrations),
        }
    }
}
