use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::models::{Subtitle, now_iso8601};
use crate::server::{self, AppState, CORE_VERSION, CaptureContext};

pub(crate) struct RuntimeTasks {
    address: SocketAddr,
    server: Option<JoinHandle<Result<(), String>>>,
    auxiliaries: Vec<JoinHandle<()>>,
}

impl RuntimeTasks {
    pub(crate) async fn spawn(
        requested_address: SocketAddr,
        state: Arc<AppState>,
        shutdown: watch::Receiver<bool>,
    ) -> Result<Self, String> {
        let capture = CaptureContext::from_app(&state);
        let glossary_refresh = spawn_glossary_refresh(capture.clone(), shutdown.clone());
        let mute_coordinator = spawn_mute_coordinator(capture, shutdown.clone());
        let room_context_sync = spawn_room_context_sync(Arc::clone(&state), shutdown.clone());
        let room_markers = spawn_room_marker_recorder(Arc::clone(&state), shutdown.clone());
        let listener = match tokio::net::TcpListener::bind(requested_address).await {
            Ok(listener) => listener,
            Err(error) => {
                glossary_refresh.abort();
                mute_coordinator.abort();
                room_context_sync.abort();
                room_markers.abort();
                return Err(format!("Failed to listen on {requested_address}: {error}"));
            }
        };
        let address = match listener.local_addr() {
            Ok(address) => address,
            Err(error) => {
                glossary_refresh.abort();
                mute_coordinator.abort();
                room_context_sync.abort();
                room_markers.abort();
                return Err(format!("Failed to read the listen address: {error}"));
            }
        };
        let server = spawn_http_server(listener, state, shutdown);

        tracing::info!(version = CORE_VERSION, %address, "vrcs-core listening");
        Ok(Self {
            address,
            server: Some(server),
            auxiliaries: vec![
                glossary_refresh,
                mute_coordinator,
                room_context_sync,
                room_markers,
            ],
        })
    }

    pub(crate) fn address(&self) -> SocketAddr {
        self.address
    }

    pub(crate) async fn wait_server(&mut self) -> Result<(), String> {
        let task = self.server.take().expect("runtime server task");
        task.await
            .map_err(|error| format!("Core task failed: {error}"))?
    }

    pub(crate) async fn stop_auxiliaries(&mut self) {
        for task in &self.auxiliaries {
            task.abort();
        }
        for task in self.auxiliaries.drain(..) {
            let _ = task.await;
        }
    }
}

fn spawn_http_server(
    listener: tokio::net::TcpListener,
    state: Arc<AppState>,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<Result<(), String>> {
    tokio::spawn(async move {
        axum::serve(listener, server::router(state))
            .with_graceful_shutdown(async move {
                if !*shutdown.borrow() {
                    let _ = shutdown.changed().await;
                }
            })
            .await
            .map_err(|error| format!("Server failed: {error}"))
    })
}

fn spawn_glossary_refresh(
    capture: CaptureContext,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let mut features = capture.config.features_tx.subscribe();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if !features.borrow().glossary { continue; }
                    let refreshed = tokio::select! {
                        biased;
                        _ = features.wait_for(|features| !features.glossary) => false,
                        _ = shutdown.changed() => break,
                        refreshed = capture.content.glossary.refresh_all() => refreshed,
                    };
                    if refreshed
                        && let Err((_, body)) = server::capture::reload_glossary_asr_context(&capture).await {
                            tracing::warn!(detail = ?body, "ASR glossary context could not be reloaded");
                        }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    })
}

fn spawn_mute_coordinator(
    capture: CaptureContext,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let mut mute_updates = capture.integrations.vrchat_mute_sync.subscribe();
    tokio::spawn(async move {
        loop {
            let snapshot = mute_updates.borrow().clone();
            capture.integrations.osc.update_mute_status(snapshot.muted);
            if snapshot.muted == Some(true) {
                let _control = capture.capture.capture_control.lock().await;
                capture
                    .capture
                    .microphone_pipeline
                    .lock()
                    .await
                    .stop_discarding_results()
                    .await;
            } else if snapshot.muted == Some(false) || !snapshot.enabled {
                let _control = capture.capture.capture_control.lock().await;
                if let Err(error) = server::capture::resume_microphone(&capture).await {
                    tracing::warn!(%error, "failed to resume microphone after VRChat unmuted");
                }
            }

            tokio::select! {
                changed = mute_updates.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    })
}

fn spawn_room_context_sync(
    state: Arc<AppState>,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let mut rx = state.capture.vrchat_log.subscribe();
    tokio::spawn(async move {
        let initial = rx.borrow_and_update().clone();
        state.capture.room_context.sync_log(&initial);
        loop {
            tokio::select! {
                changed = rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let snapshot = rx.borrow_and_update().clone();
                    state.capture.room_context.sync_log(&snapshot);
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    })
}

fn spawn_room_marker_recorder(
    state: Arc<AppState>,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut log_rx = state.capture.vrchat_log.subscribe();
        let mut recording_rx = state.capture.subscribe_capture_requested();
        let mut last_name: Option<String> = None;
        let mut was_recording = false;
        loop {
            let recording = *recording_rx.borrow_and_update();
            let name = log_rx.borrow_and_update().world_name.clone();
            let switched = name != last_name;
            if recording
                && name.is_some()
                && (switched || !was_recording)
                && let Err(error) =
                    record_room_marker(&state, name.as_deref().unwrap_or_default()).await
            {
                tracing::warn!(%error, "VRChat room marker could not be stored");
            }
            last_name = name;
            was_recording = recording;
            tokio::select! {
                changed = log_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                changed = recording_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    })
}

async fn record_room_marker(state: &AppState, name: &str) -> crate::error::AppResult<()> {
    let subtitle = Subtitle {
        id: None,
        conversation_id: None,
        text: name.trim().to_owned(),
        speaker: None,
        language: None,
        started_at: None,
        ended_at: None,
        source: "world".into(),
        created_at: now_iso8601(),
        translations: Vec::new(),
    };
    let catalog = state.content.conversation_catalog_tx.clone();
    let saved = server::db_call(Arc::clone(&state.content.db), move |db| {
        let saved = db.add_subtitle(&subtitle)?;
        crate::db::conversations::publish_latest_catalog(db, &catalog);
        Ok(saved)
    })
    .await?;
    state.content.subtitle_output.room_marker_recorded(saved);
    Ok(())
}
