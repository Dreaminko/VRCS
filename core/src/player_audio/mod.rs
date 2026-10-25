//! Experimental local-player network audio, independent of WASAPI and routing.
#[cfg(any(windows, test))]
mod buffer;
#[cfg(any(windows, test))]
mod cache;
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
#[cfg(test)]
mod replay_tests;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod worker;
#[cfg(any(windows, test))]
mod world;

use serde::Serialize;
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
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct PlayerInfo {
    pub speaker: crate::models::SpeakerIdentity,
    pub selected: bool,
    pub active: bool,
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
        }
    }
}
pub(crate) type SharedStatus = Arc<Mutex<Status>>;

pub(crate) enum AudioEvent {
    Speaker(crate::models::SpeakerIdentity),
    Frame {
        speaker: crate::models::SpeakerIdentity,
        samples: Vec<f32>,
        position: u64,
    },
    Closed(String),
    /// Decoder retired after silence; the player/identity is still in the room.
    Idle(String),
}

pub(crate) struct Capture {
    pub events: tokio::sync::mpsc::Receiver<AudioEvent>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Capture {
    pub(crate) fn start(
        config: crate::config::PlayerAudioConfig,
        status: SharedStatus,
        #[cfg(windows)] cache: Arc<SessionCache>,
        settings: Arc<std::sync::RwLock<crate::config::AppConfig>>,
    ) -> Result<Self, String> {
        #[cfg(windows)]
        {
            worker::start(config, status, cache, settings)
        }
        #[cfg(not(windows))]
        {
            let _ = (config, status, settings);
            Err("Player audio is available only on Windows".into())
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
