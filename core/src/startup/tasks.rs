use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::server::{self, AppState, CaptureContext, CORE_VERSION};

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
        let listener = match tokio::net::TcpListener::bind(requested_address).await {
            Ok(listener) => listener,
            Err(error) => {
                glossary_refresh.abort();
                mute_coordinator.abort();
                return Err(format!("Failed to listen on {requested_address}: {error}"));
            }
        };
        let address = match listener.local_addr() {
            Ok(address) => address,
            Err(error) => {
                glossary_refresh.abort();
                mute_coordinator.abort();
                return Err(format!("Failed to read the listen address: {error}"));
            }
        };
        let server = spawn_http_server(listener, state, shutdown);

        tracing::info!(version = CORE_VERSION, %address, "vrcs-core listening");
        Ok(Self {
            address,
            server: Some(server),
            auxiliaries: vec![glossary_refresh, mute_coordinator],
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
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 60 * 60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if capture.content.glossary.refresh_all().await {
                        if let Err((_, body)) = server::capture::reload_glossary_asr_context(&capture).await {
                            tracing::warn!(detail = ?body, "ASR glossary context could not be reloaded");
                        }
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
