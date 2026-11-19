//! Standalone VRChat output-log lifecycle watcher.
//!
//! The watcher runs independently of network capture, discovers the current
//! VRChat process itself, and publishes a small snapshot. Consumers such as the
//! player-audio worker read the snapshot instead of owning file polling.
#[cfg(any(windows, test))]
mod watcher;

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

use crate::vrchat_process::ProcessIdentity;

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(1);

/// Current log-derived state. Extend this type with new fields rather than
/// making consumers read the log file themselves.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub process: Option<ProcessIdentity>,
    pub ready: bool,
    pub room_token: Option<u64>,
    pub world_session_token: Option<u64>,
    pub world_name: Option<String>,
}

#[derive(Clone)]
pub(crate) struct VrchatLog {
    tx: Arc<watch::Sender<Snapshot>>,
}

impl Default for VrchatLog {
    fn default() -> Self {
        let (tx, _) = watch::channel(Snapshot::default());
        Self { tx: Arc::new(tx) }
    }
}

impl VrchatLog {
    pub(crate) fn start(shutdown: watch::Receiver<bool>) -> Self {
        let log = Self::default();
        #[cfg(windows)]
        {
            let tx = Arc::clone(&log.tx);
            if let Err(error) = std::thread::Builder::new()
                .name("vrcs-vrchat-log".into())
                .spawn(move || run(tx, shutdown))
            {
                tracing::warn!(%error, "VRChat log watcher could not start");
            }
        }
        #[cfg(not(windows))]
        let _ = shutdown;
        log
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        self.tx.borrow().clone()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.tx.subscribe()
    }
}

#[cfg(windows)]
fn run(tx: Arc<watch::Sender<Snapshot>>, shutdown: watch::Receiver<bool>) {
    let mut active: Option<(ProcessIdentity, watcher::WorldWatcher)> = None;
    let mut discovered_at = Instant::now()
        .checked_sub(DISCOVERY_INTERVAL)
        .unwrap_or_else(Instant::now);
    loop {
        if *shutdown.borrow() {
            publish(&tx, Snapshot::default());
            return;
        }
        if active
            .as_ref()
            .is_some_and(|(owner, _)| !crate::vrchat_process::is_alive(*owner))
        {
            active = None;
            publish(&tx, Snapshot::default());
        }
        if active.is_none() && discovered_at.elapsed() >= DISCOVERY_INTERVAL {
            match crate::vrchat_process::game() {
                Ok(Some((owner, _))) => {
                    active = Some((owner, watcher::WorldWatcher::for_process(owner)));
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::debug!(%error, "VRChat process discovery failed for the log watcher")
                }
            }
            discovered_at = Instant::now();
        }
        if let Some((owner, watcher)) = active.as_mut() {
            let ready = watcher.poll();
            let room_token = watcher.room_token();
            let world_session_token = watcher.world_session_token();
            let world_name = watcher.world_name().map(str::to_owned);
            publish(
                &tx,
                Snapshot {
                    process: Some(*owner),
                    ready,
                    room_token,
                    world_session_token,
                    world_name,
                },
            );
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn publish(tx: &watch::Sender<Snapshot>, next: Snapshot) {
    tx.send_if_modified(|current| {
        if *current == next {
            false
        } else {
            *current = next;
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_handle_starts_with_an_empty_snapshot() {
        assert_eq!(VrchatLog::default().snapshot(), Snapshot::default());
    }

    #[test]
    fn published_snapshots_replace_the_shared_value() {
        let log = VrchatLog::default();
        let mut rx = log.subscribe();
        let next = Snapshot {
            process: Some(ProcessIdentity { pid: 7, created: 9 }),
            ready: true,
            room_token: Some(11),
            world_session_token: Some(22),
            world_name: Some("Test World".into()),
        };
        publish(&log.tx, next.clone());
        assert_eq!(log.snapshot(), next);
        assert!(rx.has_changed().unwrap());
        assert_eq!(rx.borrow_and_update().clone(), next);
    }
}
