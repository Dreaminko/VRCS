//! Microphone capture owned by one visual question, not by transcription.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::audio::{AudioCapture, CaptureSource};
use crate::config::AppConfig;

use super::audio::{AudioReader, AudioWindow, MicrophoneTap};

pub(super) fn device_id(config: &AppConfig) -> Option<i64> {
    let microphone = &config.audio.microphone;
    (microphone.mode == "device")
        .then_some(microphone.device_id)
        .flatten()
}

pub(super) struct QuestionMicrophoneGuard(Arc<AtomicBool>);

impl QuestionMicrophoneGuard {
    pub(super) fn acquire(occupied: Arc<AtomicBool>) -> Result<Arc<Self>, String> {
        occupied
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "Another visual question is using the microphone".to_owned())?;
        Ok(Arc::new(Self(occupied)))
    }
}

impl Drop for QuestionMicrophoneGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub(super) struct QuestionCapture {
    window: AudioWindow,
    cancelled: Arc<AtomicBool>,
    microphone: Option<Arc<QuestionMicrophoneGuard>>,
}

impl QuestionCapture {
    pub(super) fn begin(
        device_id: Option<i64>,
        device_key: Option<String>,
        max_seconds: u32,
        runtime: &tokio::runtime::Handle,
        microphone: Arc<QuestionMicrophoneGuard>,
    ) -> Result<Self, String> {
        let tap = MicrophoneTap::default();
        let window = tap.begin_capture(max_seconds)?;
        let status = window.reader();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        // Device startup and shutdown can block. The VR thread must keep polling
        // the real release edge even while the device is opening.
        let worker_microphone = microphone.clone();
        runtime.spawn_blocking(move || {
            // Keep exclusion until the physical device has stopped, even on cancellation.
            let _microphone = worker_microphone;
            let result = run_capture(device_id, device_key.as_deref(), &tap, status, &stop);
            if let Err(detail) = result {
                tracing::warn!(%detail, "visual question microphone capture failed");
                tap.failed(&detail);
            }
        });
        Ok(Self {
            window,
            cancelled,
            microphone: Some(microphone),
        })
    }

    pub(super) fn reader(&self) -> AudioReader {
        self.window.reader()
    }

    pub(super) fn release(&mut self) {
        self.window.release();
    }

    pub(super) fn released_at(&self) -> Option<std::time::Instant> {
        self.window.released_at()
    }

    pub(super) fn stop(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.microphone.take();
    }

    #[cfg(test)]
    pub(super) fn from_window(window: AudioWindow) -> Self {
        Self {
            window,
            cancelled: Arc::new(AtomicBool::new(false)),
            microphone: None,
        }
    }

    #[cfg(test)]
    pub(super) fn with_microphone(mut self, microphone: Arc<QuestionMicrophoneGuard>) -> Self {
        self.microphone = Some(microphone);
        self
    }
}

impl Drop for QuestionCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_capture(
    device_id: Option<i64>,
    device_key: Option<&str>,
    tap: &MicrophoneTap,
    status: AudioReader,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if cancelled.load(Ordering::SeqCst) {
        return Ok(());
    }
    let mut capture =
        AudioCapture::new(16_000, CaptureSource::Microphone).with_microphone_tap(tap.clone());
    let device = capture
        .start(device_id, device_key, None)
        .map_err(|error| format!("Visual question microphone: {}: {error}", error.code()))?;
    tracing::info!(device_name = %device.name, "visual question microphone capture started");
    let result = loop {
        if cancelled.load(Ordering::SeqCst) {
            break Ok(());
        }
        match status.finish_capture() {
            Ok(true) => break Ok(()),
            Ok(false) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                tap.failed(&error);
                break Err(error);
            }
        }
    };
    capture.stop();
    tracing::info!("visual question microphone capture stopped");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microphone_exclusion_lasts_until_the_last_owner_stops() {
        let occupied = Arc::new(AtomicBool::new(false));
        let session = QuestionMicrophoneGuard::acquire(occupied.clone()).unwrap();
        let worker = session.clone();
        assert!(QuestionMicrophoneGuard::acquire(occupied.clone()).is_err());
        drop(session);
        assert!(QuestionMicrophoneGuard::acquire(occupied.clone()).is_err());
        drop(worker);
        assert!(!occupied.load(Ordering::SeqCst));
        let retry = QuestionMicrophoneGuard::acquire(occupied.clone()).unwrap();
        drop(retry);
        assert!(!occupied.load(Ordering::SeqCst));
    }

    #[test]
    fn transcription_disable_and_threshold_do_not_disable_question_input() {
        let mut config = AppConfig::default();
        config.audio.microphone.mode = "device".into();
        config.audio.microphone.device_id = Some(42);
        assert_eq!(device_id(&config), Some(42));
        config.audio.microphone.trigger_threshold_dbfs = 0.0;
        assert_eq!(device_id(&config), Some(42));
        for mode in ["disabled", "default"] {
            config.audio.microphone.mode = mode.into();
            assert_eq!(device_id(&config), None);
        }
        config.audio.microphone.mode = "device".into();
        config.audio.microphone.device_id = None;
        assert_eq!(device_id(&config), None);
    }

    #[test]
    fn dropping_a_question_cancels_capture_and_invalidates_its_reader() {
        let tap = MicrophoneTap::default();
        let capture = QuestionCapture::from_window(tap.begin_capture(1).unwrap());
        let cancelled = capture.cancelled.clone();
        let reader = capture.reader();
        drop(capture);
        assert!(cancelled.load(Ordering::SeqCst));
        assert!(reader.check().is_err());
    }
}
