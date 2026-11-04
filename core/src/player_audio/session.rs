//! One network owner with independent bounded voice and text subscriptions.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use serde::Serialize;
use tokio::sync::mpsc;

use super::{AudioEvent, Capture, SharedStatus, Status};
use crate::config::AppConfig;
use crate::models::SpeakerIdentity;
use crate::vrchat_chatbox::{ChatboxEvent, TaskValidity};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NetworkConsumers {
    pub voice: bool,
    pub chatbox: bool,
}

impl NetworkConsumers {
    pub(crate) fn from_config(config: &AppConfig) -> Self {
        Self {
            voice: config.audio.output.mode == "vrchat_players",
            chatbox: config.vrchat_chatbox.enabled,
        }
    }

    fn any(self) -> bool {
        self.voice || self.chatbox
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ChatboxStatus {
    pub phase: &'static str,
    pub received: u64,
    pub accepted: u64,
    pub duplicates: u64,
    pub dropped: u64,
    pub detail: Option<String>,
}

impl Default for ChatboxStatus {
    fn default() -> Self {
        Self {
            phase: "stopped",
            received: 0,
            accepted: 0,
            duplicates: 0,
            dropped: 0,
            detail: None,
        }
    }
}

#[derive(Default)]
pub(crate) struct Subscriptions {
    pub voice: Option<mpsc::Receiver<AudioEvent>>,
    pub chatbox: Option<mpsc::Receiver<ChatboxEvent>>,
}

#[derive(Default)]
pub(super) struct Outputs {
    voice: Option<mpsc::Sender<AudioEvent>>,
    pub(super) chatbox: Option<mpsc::Sender<ChatboxEvent>>,
    pub(super) voice_generation: u64,
    pub(super) text_generation: u64,
    pub(super) opus_library_path: String,
    pub(super) max_players: usize,
    voice_error: Option<String>,
    pub(super) chatbox_status: ChatboxStatus,
    scopes: HashMap<String, (TaskValidity, Instant)>,
    pending_speakers: HashMap<String, SpeakerIdentity>,
}

pub(super) type SharedOutputs = Arc<Mutex<Outputs>>;

impl Outputs {
    #[cfg(test)]
    pub(super) fn with_voice(sender: mpsc::Sender<AudioEvent>) -> Self {
        Self {
            voice: Some(sender),
            ..Self::default()
        }
    }
    pub(super) fn voice_enabled(&self) -> bool {
        self.voice.is_some()
    }

    pub(super) fn chatbox_enabled(&self) -> bool {
        self.chatbox.is_some()
    }

    pub(super) fn voice_failed(&mut self, error: String) {
        self.voice_error = Some(error);
        self.voice.take();
    }

    pub(super) fn audio(&self, event: AudioEvent) -> bool {
        self.voice
            .as_ref()
            .is_some_and(|tx| tx.try_send(event).is_ok())
    }

    pub(super) fn speaker(&mut self, speaker: SpeakerIdentity) {
        self.audio(AudioEvent::Speaker(speaker.clone()));
        if let Some(tx) = &self.chatbox {
            if tx.try_send(ChatboxEvent::Speaker(speaker.clone())).is_err() {
                if self.pending_speakers.len() < 512
                    || self.pending_speakers.contains_key(&speaker.id)
                {
                    self.pending_speakers.insert(speaker.id.clone(), speaker);
                }
            }
        }
    }

    pub(super) fn flush_speakers(&mut self) {
        let Some(tx) = &self.chatbox else { return };
        self.pending_speakers
            .retain(|_, speaker| tx.try_send(ChatboxEvent::Speaker(speaker.clone())).is_err());
    }

    pub(super) fn invalidate_scope(&mut self, id: &str) {
        if let Some((validity, _)) = self.scopes.remove(id) {
            validity.invalidate();
        }
        self.pending_speakers.remove(id);
    }

    pub(super) fn invalidate_text(&mut self) {
        for (_, (validity, _)) in self.scopes.drain() {
            validity.invalidate();
        }
        self.pending_speakers.clear();
    }

    pub(super) fn validity(&mut self, id: &str, now: Instant) -> TaskValidity {
        if self.scopes.len() >= 512 && !self.scopes.contains_key(id) {
            if let Some(oldest) = self
                .scopes
                .iter()
                .min_by_key(|(_, (_, at))| *at)
                .map(|(id, _)| id.clone())
            {
                self.invalidate_scope(&oldest);
            }
        }
        let (validity, at) = self
            .scopes
            .entry(id.into())
            .or_insert_with(|| (TaskValidity::new(), now));
        *at = now;
        validity.clone()
    }
}

pub(crate) struct VrchatNetworkSession {
    capture: Option<Capture>,
    status: SharedStatus,
    outputs: SharedOutputs,
    consumers: NetworkConsumers,
    #[cfg(windows)]
    cache: Arc<super::SessionCache>,
}

impl Default for VrchatNetworkSession {
    fn default() -> Self {
        Self {
            capture: None,
            status: Arc::default(),
            outputs: Arc::default(),
            consumers: NetworkConsumers::default(),
            #[cfg(windows)]
            cache: Arc::default(),
        }
    }
}

impl VrchatNetworkSession {
    pub(crate) fn running(&self) -> bool {
        self.capture
            .as_ref()
            .is_some_and(|capture| !capture.finished())
    }

    pub(crate) fn shared_status(&self) -> SharedStatus {
        Arc::clone(&self.status)
    }

    pub(crate) fn voice_status(&self) -> Option<Status> {
        self.consumers.voice.then(|| {
            let mut status = self.status.lock().expect("network status").clone();
            if let Some(error) = &self.outputs.lock().expect("network outputs").voice_error {
                status.phase = "failed";
                status.detail = Some(error.clone());
            }
            status
        })
    }

    pub(crate) fn chatbox_status(&self) -> Option<ChatboxStatus> {
        self.consumers.chatbox.then(|| {
            let status = self.status.lock().expect("network status").clone();
            let mut text = self
                .outputs
                .lock()
                .expect("network outputs")
                .chatbox_status
                .clone();
            text.phase = status.phase;
            text.detail = status.detail;
            text
        })
    }

    pub(crate) async fn reconcile(
        &mut self,
        config: &AppConfig,
        consumers: NetworkConsumers,
        settings: Arc<RwLock<AppConfig>>,
    ) -> Result<Subscriptions, String> {
        let options = config.audio.output.player_audio.clone();
        {
            let mut outputs = self.outputs.lock().expect("network outputs");
            outputs
                .opus_library_path
                .clone_from(&options.opus_library_path);
            outputs.max_players = options.max_players;
        }
        #[cfg(windows)]
        let cache = Arc::clone(&self.cache);
        self.reconcile_with(consumers, |status, outputs| {
            Capture::start(
                options,
                status,
                outputs,
                #[cfg(windows)]
                cache,
                settings,
            )
        })
        .await
    }

    async fn reconcile_with(
        &mut self,
        consumers: NetworkConsumers,
        start: impl FnOnce(SharedStatus, SharedOutputs) -> Result<Capture, String>,
    ) -> Result<Subscriptions, String> {
        if self.capture.as_ref().is_some_and(Capture::finished) {
            self.stop().await;
        }
        let subscriptions = self.attach(consumers);
        if consumers.any() && self.capture.is_none() {
            *self.status.lock().expect("network status") = Status::default();
            let result = start(Arc::clone(&self.status), Arc::clone(&self.outputs));
            match result {
                Ok(capture) => self.capture = Some(capture),
                Err(error) => {
                    self.stop().await;
                    return Err(error);
                }
            }
        } else if !consumers.any() {
            self.stop().await;
        }
        Ok(subscriptions)
    }

    fn attach(&mut self, consumers: NetworkConsumers) -> Subscriptions {
        let mut outputs = self.outputs.lock().expect("network outputs");
        let mut subscriptions = Subscriptions::default();
        if consumers.voice && !self.consumers.voice {
            let (tx, rx) = mpsc::channel(2048);
            outputs.voice = Some(tx);
            outputs.voice_error = None;
            outputs.voice_generation += 1;
            subscriptions.voice = Some(rx);
        } else if !consumers.voice {
            outputs.voice.take();
        }
        if consumers.chatbox && !self.consumers.chatbox {
            let (tx, rx) = mpsc::channel(64);
            outputs.chatbox = Some(tx);
            outputs.text_generation += 1;
            outputs.chatbox_status = ChatboxStatus::default();
            subscriptions.chatbox = Some(rx);
        } else if !consumers.chatbox {
            outputs.invalidate_text();
            outputs.chatbox.take();
        }
        self.consumers = consumers;
        subscriptions
    }

    pub(crate) async fn stop(&mut self) {
        self.attach(NetworkConsumers::default());
        if let Some(capture) = self.capture.take() {
            capture.shutdown().await;
        }
    }

    pub(crate) async fn detach(&mut self, consumers: NetworkConsumers) {
        {
            let mut outputs = self.outputs.lock().expect("network outputs");
            if !consumers.voice {
                outputs.voice.take();
                self.consumers.voice = false;
            }
            if !consumers.chatbox {
                outputs.invalidate_text();
                outputs.chatbox.take();
                self.consumers.chatbox = false;
            }
        }
        if !self.consumers.any() {
            self.stop().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscriptions_are_independent_and_old_text_is_invalidated() {
        let mut session = VrchatNetworkSession::default();
        let mut first = session.attach(NetworkConsumers {
            voice: true,
            chatbox: true,
        });
        assert!(first.voice.is_some() && first.chatbox.is_some());
        assert!(session
            .attach(NetworkConsumers {
                voice: true,
                chatbox: true
            })
            .voice
            .is_none());
        let validity = session
            .outputs
            .lock()
            .unwrap()
            .validity("scope", Instant::now());
        session.attach(NetworkConsumers {
            voice: true,
            chatbox: false,
        });
        assert!(!validity.is_current());
        assert!(first.chatbox.as_mut().unwrap().recv().await.is_none());
        assert!(!first.voice.as_ref().unwrap().is_closed());
        session.attach(NetworkConsumers::default());
        assert!(first.voice.as_mut().unwrap().recv().await.is_none());
    }

    #[tokio::test]
    async fn both_consumers_share_one_worker_until_the_last_detaches() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let starts = AtomicUsize::new(0);
        let exited = Arc::new(AtomicBool::new(false));
        let starter = |_: SharedStatus, _: SharedOutputs| {
            starts.fetch_add(1, Ordering::SeqCst);
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = Arc::clone(&stop);
            let exited = Arc::clone(&exited);
            let thread = std::thread::spawn(move || {
                while !worker_stop.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                exited.store(true, Ordering::SeqCst);
            });
            Ok(Capture {
                stop,
                thread: Some(thread),
            })
        };
        let mut session = VrchatNetworkSession::default();
        let text = session
            .reconcile_with(
                NetworkConsumers {
                    voice: false,
                    chatbox: true,
                },
                starter,
            )
            .await
            .unwrap();
        let voice = session
            .reconcile_with(
                NetworkConsumers {
                    voice: true,
                    chatbox: true,
                },
                starter,
            )
            .await
            .unwrap();
        assert!(text.chatbox.is_some() && voice.voice.is_some());
        session
            .reconcile_with(
                NetworkConsumers {
                    voice: false,
                    chatbox: true,
                },
                starter,
            )
            .await
            .unwrap();
        assert!(session.running());
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        session
            .reconcile_with(NetworkConsumers::default(), starter)
            .await
            .unwrap();
        assert!(exited.load(Ordering::SeqCst));
        assert!(!session.running());
    }

    #[tokio::test]
    async fn detaching_after_worker_failure_does_not_consume_replacement_subscriptions() {
        let mut session = VrchatNetworkSession::default();
        let subscriptions = session.attach(NetworkConsumers {
            voice: true,
            chatbox: true,
        });
        let thread = std::thread::spawn(|| {});
        while !thread.is_finished() {
            std::thread::yield_now();
        }
        session.capture = Some(Capture {
            stop: Arc::default(),
            thread: Some(thread),
        });
        session
            .detach(NetworkConsumers {
                voice: true,
                chatbox: false,
            })
            .await;
        assert!(!session.running());
        assert!(!subscriptions.voice.as_ref().unwrap().is_closed());
        let replacements = session
            .reconcile_with(
                NetworkConsumers {
                    voice: true,
                    chatbox: false,
                },
                |_, _| {
                    Ok(Capture {
                        stop: Arc::default(),
                        thread: Some(std::thread::spawn(|| {})),
                    })
                },
            )
            .await
            .unwrap();
        assert!(replacements.voice.is_some());
        session.stop().await;
    }

    #[test]
    fn name_updates_retry_after_text_backpressure() {
        let (tx, mut rx) = mpsc::channel(1);
        let mut outputs = Outputs::default();
        outputs.chatbox = Some(tx);
        let speaker = |name: &str| SpeakerIdentity {
            id: "scope".into(),
            index: 0,
            display_name: Some(name.into()),
            user_id: None,
        };
        outputs.speaker(speaker("Before"));
        outputs.speaker(speaker("After"));
        assert!(rx.try_recv().is_ok());
        outputs.flush_speakers();
        let ChatboxEvent::Speaker(speaker) = rx.try_recv().unwrap() else {
            panic!("metadata")
        };
        assert_eq!(speaker.display_name.as_deref(), Some("After"));
    }
}
