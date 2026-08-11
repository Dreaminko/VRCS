use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::audio::{AudioCapture, AudioError, CaptureSource};
use crate::models::{AudioDevice, LiveTranscription};
use crate::pipeline::audio_level_dbfs;

pub struct MicrophoneMonitor {
    device: Option<AudioDevice>,
    stop: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<()>>,
}

impl MicrophoneMonitor {
    pub fn new() -> Self {
        Self {
            device: None,
            stop: None,
            task: None,
        }
    }

    pub fn running(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }

    pub fn device(&self) -> Option<&AudioDevice> {
        if self.running() {
            self.device.as_ref()
        } else {
            None
        }
    }

    pub async fn start(
        &mut self,
        sample_rate: u32,
        device_id: Option<i64>,
        live_tx: broadcast::Sender<LiveTranscription>,
    ) -> Result<AudioDevice, AudioError> {
        if self.running() {
            return Err(AudioError::with_code(
                "audio.microphone_test_already_running",
                "Microphone test is already running",
            ));
        }
        self.stop().await;

        let mut capture = AudioCapture::new(sample_rate, CaptureSource::Microphone);
        let device = capture.start(device_id, None)?;
        let (stop_tx, mut stop_rx) = watch::channel(false);
        self.device = Some(device.clone());
        self.stop = Some(stop_tx);
        self.task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    changed = stop_rx.changed() => {
                        if changed.is_err() || *stop_rx.borrow() {
                            break;
                        }
                    }
                    chunk = capture.read() => {
                        let Ok(chunk) = chunk else {
                            break;
                        };
                        let (rms_dbfs, peak_dbfs) = audio_level_dbfs(&chunk);
                        let _ = live_tx.send(LiveTranscription::AudioLevel {
                            source: "microphone".into(),
                            rms_dbfs,
                            peak_dbfs,
                            speech: false,
                        });
                    }
                }
            }
            capture.shutdown().await;
        }));
        Ok(device)
    }

    pub async fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(true);
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        self.device = None;
    }
}

impl Default for MicrophoneMonitor {
    fn default() -> Self {
        Self::new()
    }
}
