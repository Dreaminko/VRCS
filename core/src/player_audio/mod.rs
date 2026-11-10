//! Experimental Windows network audio, independent of device capture and routing.
//! Portable parsers and session algorithms remain available to cross-platform tests.
#[cfg(any(windows, test))]
mod buffer;
#[cfg(any(windows, test))]
mod cache;
#[cfg(any(windows, test))]
mod chatbox;
#[cfg(windows)]
pub(crate) use cache::SessionCache;
#[cfg(any(windows, test))]
mod crypto;
#[cfg(any(windows, test))]
mod key;
#[cfg(any(windows, test))]
mod names;
#[cfg(any(windows, test))]
mod ntfs;
#[cfg(any(windows, test))]
mod opus;
#[cfg(any(windows, test))]
mod protocol;
#[cfg(any(windows, test))]
mod proximity;
#[cfg(test)]
mod replay_tests;
pub(crate) mod session;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod worker;
#[cfg(any(windows, test))]
mod world;

use serde::Serialize;
#[cfg(windows)]
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Status {
    pub phase: &'static str,
    pub captured_packets: u64,
    pub decrypted_packets: u64,
    pub pcm_frames: u64,
    pub active_players: usize,
    pub dropped_frames: u64,
    pub decode_errors: u64,
    pub key_scan: Option<KeyScanProgress>,
    pub detail: Option<String>,
    pub players: Vec<PlayerInfo>,
    pub proximity: ProximityStatus,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct ProximityStatus {
    pub enabled: bool,
    pub local_position: bool,
    pub tracked_players: usize,
    pub excluded_packets: u64,
    pub unknown_packets: u64,
    #[cfg(windows)]
    #[serde(skip)]
    pub blocked_players: HashSet<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PlayerInfo {
    pub speaker: crate::models::SpeakerIdentity,
    pub selected: bool,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_m: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct KeyScanProgress {
    pub stage: &'static str,
    pub scanned_bytes: u64,
    pub total_bytes: u64,
    pub candidates_tested: u64,
    pub elapsed_ms: u64,
    pub scan_elapsed_ms: u64,
    pub workers: usize,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            phase: "waiting_game",
            captured_packets: 0,
            decrypted_packets: 0,
            pcm_frames: 0,
            active_players: 0,
            dropped_frames: 0,
            decode_errors: 0,
            key_scan: None,
            detail: None,
            players: Vec::new(),
            proximity: ProximityStatus::default(),
        }
    }
}
pub(crate) type SharedStatus = Arc<Mutex<Status>>;

pub(crate) enum AudioEvent {
    #[cfg(any(windows, test))]
    Speaker(crate::models::SpeakerIdentity),
    #[cfg(windows)]
    Frame {
        speaker: crate::models::SpeakerIdentity,
        samples: Vec<f32>,
        position: u64,
    },
    #[cfg(windows)]
    Closed(String),
    #[cfg(windows)]
    /// The player left the configured proximity window during recognition.
    Excluded(String),
    #[cfg(windows)]
    /// A prior exclusion barrier has drained; new voice frames may start a fresh batch.
    Admitted(String),
    #[cfg(windows)]
    /// Decoder retired after silence; the player/identity is still in the room.
    Idle(String),
}

pub(crate) struct Capture {
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Capture {
    pub(crate) fn finished(&self) -> bool {
        self.thread
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished)
    }
    fn start(
        config: crate::config::PlayerAudioConfig,
        status: SharedStatus,
        outputs: session::SharedOutputs,
        #[cfg(windows)] cache: Arc<SessionCache>,
        settings: Arc<std::sync::RwLock<crate::config::AppConfig>>,
    ) -> Result<Self, String> {
        #[cfg(windows)]
        {
            worker::start(config, status, outputs, cache, settings)
        }
        #[cfg(not(windows))]
        {
            let _ = (config, status, outputs, settings);
            Err("VRChat network capture is available only on Windows".into())
        }
    }
    pub(crate) async fn shutdown(mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = tokio::task::spawn_blocking(move || thread.join()).await;
        }
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
