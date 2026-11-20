use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::Notify;

const RATE: usize = 16_000;

#[derive(Clone, Default)]
pub(crate) struct MicrophoneTap {
    inner: Arc<Mutex<State>>,
    changed: Arc<Notify>,
}

#[derive(Default)]
struct State {
    running: bool,
    next_id: u64,
    watermark: Option<Instant>,
    window: Option<Window>,
}

struct Window {
    id: u64,
    start: Instant,
    end: Option<Instant>,
    limit: usize,
    deadline: Instant,
    samples: Vec<f32>,
    spans: Vec<(Instant, Instant, usize, usize, usize)>,
    covered_start: bool,
    allow_startup_gap: bool,
    owns_capture: bool,
    complete: bool,
    error: Option<String>,
}

pub(crate) struct AudioWindow {
    tap: MicrophoneTap,
    id: u64,
}

pub(crate) struct AudioReader {
    tap: MicrophoneTap,
    id: u64,
    cursor: usize,
    released_since: Option<Instant>,
}

impl MicrophoneTap {
    #[cfg(test)]
    pub(crate) fn begin(&self, max_seconds: u32) -> Result<AudioWindow, String> {
        self.begin_at(max_seconds, Instant::now())
    }

    pub(crate) fn begin_capture(&self, max_seconds: u32) -> Result<AudioWindow, String> {
        self.create_window(max_seconds, Instant::now(), true)
    }

    #[cfg(test)]
    fn begin_at(&self, max_seconds: u32, start: Instant) -> Result<AudioWindow, String> {
        self.create_window(max_seconds, start, false)
    }

    fn create_window(
        &self,
        max_seconds: u32,
        start: Instant,
        allow_startup_gap: bool,
    ) -> Result<AudioWindow, String> {
        let limit = RATE
            .checked_mul(max_seconds as usize)
            .filter(|n| *n > 0)
            .ok_or("Invalid audio window limit")?;
        let deadline = start
            .checked_add(Duration::from_secs(max_seconds as u64))
            .ok_or("Invalid audio window duration")?;
        let mut state = self.inner.lock().expect("microphone tap lock");
        if !state.running && !allow_startup_gap {
            return Err("Microphone capture is not running".into());
        }
        if state.window.is_some() {
            return Err("An audio window is already active".into());
        }
        state.next_id = state.next_id.wrapping_add(1);
        let id = state.next_id;
        state.window = Some(Window {
            id,
            start,
            end: None,
            limit,
            deadline,
            samples: Vec::new(),
            spans: Vec::new(),
            covered_start: false,
            allow_startup_gap,
            owns_capture: allow_startup_gap,
            complete: false,
            error: None,
        });
        Ok(AudioWindow {
            tap: self.clone(),
            id,
        })
    }

    pub(crate) fn started(&self) {
        let mut state = self.inner.lock().expect("microphone tap lock");
        state.running = true;
        state.watermark = None;
    }

    pub(crate) fn stopped(&self, reason: &str) {
        self.stop_with_error(reason, false);
    }

    pub(crate) fn failed(&self, reason: &str) {
        self.stop_with_error(reason, true);
    }

    fn stop_with_error(&self, reason: &str, terminal: bool) {
        let mut state = self.inner.lock().expect("microphone tap lock");
        state.running = false;
        state.watermark = None;
        if let Some(window) = &mut state.window {
            // Startup attempts may retry before the first PCM chunk. A complete
            // owned window no longer depends on the device or its shutdown.
            let preserve =
                window.owns_capture && (window.complete || (!terminal && !window.covered_start));
            if !preserve {
                if terminal {
                    window.error = Some(reason.to_owned());
                } else {
                    window.error.get_or_insert_with(|| reason.to_owned());
                }
                window.samples.clear();
            }
        }
        self.changed.notify_one();
    }

    pub(crate) fn discontinuity(&self, reason: &str) {
        let mut state = self.inner.lock().expect("microphone tap lock");
        state.watermark = None;
        if let Some(window) = &mut state.window {
            // A later gap cannot invalidate PCM already collected through release.
            if !window.complete {
                window.error.get_or_insert_with(|| reason.to_owned());
                window.samples.clear();
            }
        }
        self.changed.notify_one();
    }

    // The producer supplies a continuous, monotonic interval, not queue arrival time.
    pub(crate) fn push(&self, samples: &[f32], start: Instant, end: Instant) {
        let mut state = self.inner.lock().expect("microphone tap lock");
        if !state.running || end <= start || samples.is_empty() {
            return;
        }
        let discontinuous = state.watermark.is_some_and(|previous| previous != start);
        state.watermark = Some(end);
        if let Some(window) = &mut state.window {
            if window.complete {
                return;
            }
            if discontinuous {
                window.error.get_or_insert_with(|| {
                    "Microphone audio has missing or overlapping frames".into()
                });
            }
            if window.error.is_none() && end > window.start && !window.covered_start {
                // A new device stream cannot cover time spent opening the device.
                // Only its first chunk may move the start; later gaps still fail.
                if window.allow_startup_gap {
                    window.start = window.start.max(start);
                    window.allow_startup_gap = false;
                }
                if start > window.start {
                    window.error = Some("Microphone audio did not cover the window start".into());
                } else {
                    window.covered_start = true;
                }
            }
            if window.error.is_none() {
                let stop = window.end.unwrap_or(window.deadline);
                if stop > window.deadline || (window.end.is_none() && end > window.deadline) {
                    window.error = Some("Audio window exceeded its duration limit".into());
                } else if end > window.start && start < stop {
                    let duration = end.duration_since(start).as_secs_f64();
                    let index = |time: Instant| -> usize {
                        if time <= start {
                            0
                        } else if time >= end {
                            samples.len()
                        } else {
                            // Include sample instants in [press, release).
                            ((time.duration_since(start).as_secs_f64() / duration
                                * samples.len() as f64
                                - 1e-7)
                                .ceil() as usize)
                                .min(samples.len())
                        }
                    };
                    let first = index(window.start);
                    let last = index(stop);
                    if window.samples.len() + last.saturating_sub(first) > window.limit {
                        window.error = Some("Audio window exceeded its sample limit".into());
                    } else {
                        if last > first {
                            window.spans.push((
                                start,
                                end,
                                samples.len(),
                                first,
                                window.samples.len(),
                            ));
                            let additional = last - first;
                            if window.samples.capacity() < window.samples.len() + additional {
                                window.samples.reserve(additional);
                            }
                            window.samples.extend_from_slice(&samples[first..last]);
                        }
                    }
                }
            }
            if window.error.is_some() {
                window.samples.clear();
            } else if window.end.is_some_and(|stop| end >= stop) {
                window.complete = true;
            }
        }
        self.changed.notify_one();
    }
}

impl AudioWindow {
    pub(crate) fn reader(&self) -> AudioReader {
        AudioReader {
            tap: self.tap.clone(),
            id: self.id,
            cursor: 0,
            released_since: None,
        }
    }

    pub(crate) fn released_at(&self) -> Option<Instant> {
        self.tap
            .inner
            .lock()
            .expect("microphone tap lock")
            .window
            .as_ref()
            .filter(|window| window.id == self.id)
            .and_then(|window| window.end)
    }

    pub(crate) fn release(&mut self) {
        self.release_at(Instant::now());
    }

    fn release_at(&mut self, end: Instant) {
        let mut state = self.tap.inner.lock().expect("microphone tap lock");
        let watermark = state.watermark;
        if let Some(window) = state.window.as_mut().filter(|window| window.id == self.id) {
            if window.end.is_none() {
                window.end = Some(end);
                // A producer estimate can be slightly ahead of wall time. Trim
                // samples already collected past the actual release boundary.
                for &(start, stop, count, first, offset) in &window.spans {
                    if end < stop {
                        let last = if end <= start {
                            0
                        } else {
                            ((end.duration_since(start).as_secs_f64()
                                / stop.duration_since(start).as_secs_f64()
                                * count as f64
                                - 1e-7)
                                .ceil() as usize)
                                .min(count)
                        };
                        window.samples.truncate(offset + last.saturating_sub(first));
                        break;
                    }
                }
                if end > window.deadline {
                    window.error = Some("Audio window exceeded its duration limit".into());
                    window.samples.clear();
                }
                window.complete =
                    window.error.is_none() && watermark.is_some_and(|mark| mark >= end);
            }
        }
        self.tap.changed.notify_one();
    }

    #[cfg(test)]
    pub(crate) async fn finish(&mut self) -> Result<Vec<f32>, String> {
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let notified = self.tap.changed.notified();
                {
                    let mut state = self.tap.inner.lock().expect("microphone tap lock");
                    let window = state
                        .window
                        .as_mut()
                        .filter(|window| window.id == self.id)
                        .ok_or("Audio window is no longer active")?;
                    if let Some(error) = &window.error {
                        return Err(error.clone());
                    }
                    window
                        .end
                        .ok_or("Release the audio window before finishing")?;
                    if window.complete {
                        if window.samples.is_empty() {
                            return Err("No microphone audio covered the window".into());
                        }
                        return Ok(std::mem::take(&mut window.samples));
                    }
                }
                notified.await;
            }
        })
        .await
        .unwrap_or_else(|_| Err("Timed out waiting for microphone audio".into()));
        self.cleanup();
        result
    }

    fn cleanup(&self) {
        let mut state = self.tap.inner.lock().expect("microphone tap lock");
        if state
            .window
            .as_ref()
            .is_some_and(|window| window.id == self.id)
        {
            state.window = None;
        }
    }
}

impl AudioReader {
    /// Seal a released window before shutting down its dedicated producer.
    pub(crate) fn finish_capture(&self) -> Result<bool, String> {
        let mut state = self.tap.inner.lock().expect("microphone tap lock");
        let window = state
            .window
            .as_mut()
            .filter(|window| window.id == self.id)
            .ok_or("Audio window is no longer active")?;
        if let Some(error) = &window.error {
            return Err(error.clone());
        }
        if window.complete {
            state.running = false;
            return Ok(true);
        }
        let now = Instant::now();
        if window.end.is_none() && now > window.deadline {
            return Err("Audio window exceeded its duration limit".into());
        }
        if window
            .end
            .is_some_and(|end| now.saturating_duration_since(end) >= Duration::from_secs(1))
        {
            return Err("Timed out waiting for microphone audio".into());
        }
        Ok(false)
    }

    pub(crate) fn check(&self) -> Result<(), String> {
        let state = self.tap.inner.lock().expect("microphone tap lock");
        let window = state
            .window
            .as_ref()
            .filter(|window| window.id == self.id)
            .ok_or("Audio window is no longer active")?;
        match &window.error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    pub(crate) fn released_at(&self) -> Option<Instant> {
        self.tap
            .inner
            .lock()
            .expect("microphone tap lock")
            .window
            .as_ref()
            .filter(|window| window.id == self.id)
            .and_then(|window| window.end)
    }

    /// Only emit sample instants that have passed wall time. Producer estimates
    /// can lead wall time, and transmitted PCM cannot be retracted on release.
    pub(crate) async fn next(&mut self) -> Result<Option<Vec<f32>>, String> {
        loop {
            {
                let state = self.tap.inner.lock().expect("microphone tap lock");
                let window = state
                    .window
                    .as_ref()
                    .filter(|window| window.id == self.id)
                    .ok_or("Audio window is no longer active")?;
                if let Some(error) = &window.error {
                    return Err(error.clone());
                }
                let now = Instant::now();
                if window.end.is_none() && now > window.deadline {
                    return Err("Audio window exceeded its duration limit".into());
                }
                let complete = window.complete;
                if window.end.is_some() {
                    let since = self.released_since.get_or_insert(now);
                    if !complete && since.elapsed() >= Duration::from_secs(1) {
                        return Err("Timed out waiting for microphone audio".into());
                    }
                }
                let stop = window.end.unwrap_or(now).min(now);
                let mut safe = window.samples.len();
                for &(start, end, count, first, offset) in &window.spans {
                    if stop < end {
                        let last = if stop <= start {
                            0
                        } else {
                            ((stop.duration_since(start).as_secs_f64()
                                / end.duration_since(start).as_secs_f64()
                                * count as f64
                                - 1e-7)
                                .ceil() as usize)
                                .min(count)
                        };
                        safe = (offset + last.saturating_sub(first)).min(safe);
                        break;
                    }
                }
                if safe < self.cursor {
                    return Err("Question audio crossed the release boundary".into());
                }
                if safe > self.cursor {
                    let chunk = window.samples[self.cursor..safe].to_vec();
                    self.cursor = safe;
                    return Ok(Some(chunk));
                }
                if complete {
                    return if self.cursor == 0 {
                        Err("No microphone audio covered the window".into())
                    } else {
                        Ok(None)
                    };
                }
            }
            // One reader polls a bounded window; no per-frame tasks or queues.
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

impl Drop for AudioWindow {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> (MicrophoneTap, Instant) {
        let tap = MicrophoneTap::default();
        tap.started();
        (tap, Instant::now())
    }

    #[tokio::test]
    async fn dedicated_capture_allows_startup_and_stops_only_after_release_tail() {
        let tap = MicrophoneTap::default();
        let mut window = tap.begin_capture(1).unwrap();
        let mut reader = window.reader();
        let status = window.reader();
        assert!(!status.finish_capture().unwrap());
        tokio::time::sleep(Duration::from_millis(10)).await;
        tap.started();
        let start = Instant::now();
        tokio::time::sleep(Duration::from_millis(10)).await;
        let first_end = Instant::now();
        tap.push(&[0.1; 160], start, first_end);
        assert_eq!(reader.next().await.unwrap().unwrap().len(), 160);
        tokio::time::sleep(Duration::from_millis(2)).await;
        window.release();
        assert!(!status.finish_capture().unwrap());
        tokio::time::sleep(Duration::from_millis(2)).await;
        tap.push(&[0.2; 64], first_end, Instant::now());
        // Device shutdown can race the control worker after release is covered.
        tap.stopped("Device changed before the control worker stopped capture");
        assert!(status.finish_capture().unwrap());
        tap.stopped("Dedicated microphone capture stopped");
        assert!(reader
            .next()
            .await
            .unwrap()
            .unwrap()
            .iter()
            .all(|sample| *sample == 0.2));
        assert!(reader.next().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn release_before_device_start_does_not_capture_later_speech() {
        let tap = MicrophoneTap::default();
        let mut window = tap.begin_capture(1).unwrap();
        window.release();
        tokio::time::sleep(Duration::from_millis(2)).await;
        tap.started();
        let start = Instant::now();
        tokio::time::sleep(Duration::from_millis(2)).await;
        tap.push(&[0.1; 32], start, Instant::now());
        assert!(window.reader().finish_capture().unwrap());
        tap.stopped("Dedicated microphone capture stopped");
        assert_eq!(
            window.reader().next().await.unwrap_err(),
            "No microphone audio covered the window"
        );
    }

    #[test]
    fn failed_startup_attempt_can_retry_before_the_first_chunk() {
        let tap = MicrophoneTap::default();
        let window = tap.begin_capture(1).unwrap();
        tap.stopped("First startup attempt failed");
        window.reader().check().unwrap();
        tap.started();
        let start = Instant::now();
        tap.push(&[0.1; 160], start, start + Duration::from_millis(10));
        window.reader().check().unwrap();
        tap.stopped("Device failed after capture began");
        assert_eq!(
            window.reader().check().unwrap_err(),
            "Device failed after capture began"
        );
    }

    #[test]
    fn final_startup_error_replaces_the_generic_attempt_stop() {
        let tap = MicrophoneTap::default();
        let window = tap.begin_capture(1).unwrap();
        tap.stopped("Microphone capture stopped");
        tap.failed("Visual question microphone: audio.permission_denied: Access denied");
        assert_eq!(
            window.reader().check().unwrap_err(),
            "Visual question microphone: audio.permission_denied: Access denied"
        );
    }

    #[test]
    fn terminal_capture_error_survives_producer_shutdown() {
        let tap = MicrophoneTap::default();
        let window = tap.begin_capture(1).unwrap();
        tap.failed("Timed out waiting for microphone audio");
        tap.stopped("Microphone capture stopped");
        assert_eq!(
            window.reader().check().unwrap_err(),
            "Timed out waiting for microphone audio"
        );
    }

    #[test]
    fn startup_allowance_does_not_hide_later_capture_gaps() {
        let tap = MicrophoneTap::default();
        let window = tap.begin_capture(1).unwrap();
        tap.started();
        let start = Instant::now();
        let end = start + Duration::from_millis(10);
        tap.push(&[0.1; 160], start, end);
        window.reader().check().unwrap();
        tap.push(
            &[0.1; 160],
            end + Duration::from_millis(1),
            end + Duration::from_millis(11),
        );
        assert!(window
            .reader()
            .check()
            .unwrap_err()
            .contains("missing or overlapping frames"));
    }

    #[tokio::test]
    async fn live_reader_defers_future_pcm_and_preserves_release_tail() {
        let (tap, base) = running();
        let mut window = tap.begin_at(1, base).unwrap();
        let mut reader = window.reader();
        tap.push(&[0.1; 16000], base, base + Duration::from_secs(1));
        tokio::time::sleep(Duration::from_millis(10)).await;
        let first = reader.next().await.unwrap().unwrap();
        assert!(first.len() < 16000);
        window.release();
        let expected = tap
            .inner
            .lock()
            .unwrap()
            .window
            .as_ref()
            .unwrap()
            .samples
            .len();
        let mut total = first.len();
        while let Some(chunk) = reader.next().await.unwrap() {
            total += chunk.len();
        }
        assert_eq!(total, expected);
    }

    #[tokio::test]
    async fn live_reader_waits_for_tail_and_drop_invalidates_it() {
        let (tap, base) = running();
        let mut window = tap.begin_at(1, base).unwrap();
        let mut reader = window.reader();
        tap.push(&[0.1; 16], base, base + Duration::from_millis(1));
        tokio::time::sleep(Duration::from_millis(2)).await;
        assert_eq!(reader.next().await.unwrap().unwrap().len(), 16);
        window.release_at(base + Duration::from_millis(2));
        assert!(
            tokio::time::timeout(Duration::from_millis(10), reader.next())
                .await
                .is_err()
        );
        tap.push(
            &[0.2; 32],
            base + Duration::from_millis(1),
            base + Duration::from_millis(3),
        );
        assert_eq!(reader.next().await.unwrap().unwrap(), vec![0.2; 16]);
        assert!(reader.next().await.unwrap().is_none());
        drop(window);
        assert!(reader.next().await.is_err());
        assert!(tap.begin(1).is_ok());
    }

    #[tokio::test]
    async fn clips_both_boundaries_and_waits_for_late_release_coverage() {
        let (tap, base) = running();
        let mut window = tap.begin_at(1, base + Duration::from_millis(2)).unwrap();
        window.release_at(base + Duration::from_millis(6));
        let producer = tap.clone();
        let task = tokio::spawn(async move { window.finish().await });
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        producer.push(
            &(0..160).map(|n| n as f32).collect::<Vec<_>>(),
            base,
            base + Duration::from_millis(10),
        );
        let audio = task.await.unwrap().unwrap();
        assert_eq!(audio, (32..96).map(|n| n as f32).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn release_trims_samples_already_past_the_boundary() {
        let (tap, base) = running();
        let mut window = tap.begin_at(1, base + Duration::from_millis(2)).unwrap();
        tap.push(
            &(0..160).map(|n| n as f32).collect::<Vec<_>>(),
            base,
            base + Duration::from_millis(10),
        );
        window.release_at(base + Duration::from_millis(6));
        // Repeated release calls must preserve the first release instant.
        window.release_at(base + Duration::from_millis(9));
        assert_eq!(
            window.finish().await.unwrap(),
            (32..96).map(|n| n as f32).collect::<Vec<_>>()
        );
        assert!(tap.begin(1).is_ok());
    }

    #[tokio::test]
    async fn separate_chunks_have_distinct_boundaries() {
        let (tap, base) = running();
        let mut window = tap.begin_at(1, base + Duration::from_millis(5)).unwrap();
        tap.push(&[1.0; 160], base, base + Duration::from_millis(10));
        window.release_at(base + Duration::from_millis(15));
        tap.push(
            &[2.0; 160],
            base + Duration::from_millis(10),
            base + Duration::from_millis(20),
        );
        let audio = window.finish().await.unwrap();
        assert_eq!(audio.len(), 160);
        assert_eq!(&audio[..80], &[1.0; 80]);
        assert_eq!(&audio[80..], &[2.0; 80]);
    }

    #[test]
    fn requires_running_and_rejects_duplicate_until_drop() {
        let tap = MicrophoneTap::default();
        assert!(tap.begin(1).is_err());
        tap.started();
        assert!(tap.begin(0).is_err());
        let window = tap.begin(1).unwrap();
        assert!(tap.begin(1).is_err());
        drop(window);
        assert!(tap.begin(1).is_ok());
    }

    #[tokio::test]
    async fn stop_and_device_reset_cancel_windows() {
        for reason in ["Capture stopped", "Device changed"] {
            let (tap, base) = running();
            let mut window = tap.begin_at(1, base).unwrap();
            window.release_at(base + Duration::from_millis(1));
            tap.stopped(reason);
            tap.started();
            assert_eq!(window.finish().await.unwrap_err(), reason);
        }
    }

    #[tokio::test]
    async fn complete_released_window_survives_later_discontinuities() {
        for release_before_push in [false, true] {
            for explicit_gap in [false, true] {
                let tap = MicrophoneTap::default();
                tap.started();
                let base = Instant::now() - Duration::from_millis(10);
                let mut window = tap.begin_at(1, base).unwrap();
                let mut reader = window.reader();
                if release_before_push {
                    window.release_at(base + Duration::from_millis(1));
                }
                tap.push(&[0.1; 32], base, base + Duration::from_millis(2));
                if !release_before_push {
                    window.release_at(base + Duration::from_millis(1));
                }
                if explicit_gap {
                    tap.discontinuity("Later capture timing gap");
                }
                tap.push(
                    &[0.2; 16],
                    base + Duration::from_millis(3),
                    base + Duration::from_millis(4),
                );
                reader.check().unwrap();
                assert_eq!(reader.next().await.unwrap().unwrap(), vec![0.1; 16]);
                assert!(reader.next().await.unwrap().is_none());
                assert_eq!(window.finish().await.unwrap(), vec![0.1; 16]);
            }
        }
    }

    #[tokio::test]
    async fn active_and_incomplete_released_windows_fail_on_discontinuity() {
        for released in [false, true] {
            for explicit_gap in [false, true] {
                let (tap, base) = running();
                let mut window = tap.begin_at(1, base).unwrap();
                let mut reader = window.reader();
                tap.push(&[0.1; 16], base, base + Duration::from_millis(1));
                if released {
                    window.release_at(base + Duration::from_millis(3));
                }
                if explicit_gap {
                    tap.discontinuity("Capture timing gap");
                }
                tap.push(
                    &[0.2; 16],
                    base + Duration::from_millis(2),
                    base + Duration::from_millis(3),
                );
                assert!(reader.check().is_err());
                assert!(reader.next().await.is_err());
                assert!(tap
                    .inner
                    .lock()
                    .unwrap()
                    .window
                    .as_ref()
                    .unwrap()
                    .samples
                    .is_empty());
                if !released {
                    window.release_at(base + Duration::from_millis(3));
                }
                assert!(window.finish().await.is_err());
            }
        }
    }

    #[tokio::test]
    async fn stopping_capture_still_invalidates_a_complete_released_window() {
        let (tap, base) = running();
        let mut window = tap.begin_at(1, base).unwrap();
        tap.push(&[0.1; 16], base, base + Duration::from_millis(1));
        window.release_at(base + Duration::from_millis(1));
        tap.stopped("Capture stopped");
        assert_eq!(window.finish().await.unwrap_err(), "Capture stopped");
    }

    #[tokio::test]
    async fn duration_and_sample_limits_fail_without_retaining_audio() {
        let (tap, base) = running();
        let mut window = tap.begin_at(1, base).unwrap();
        tap.push(&vec![0.0; RATE + 1], base, base + Duration::from_secs(1));
        window.release_at(base + Duration::from_secs(1));
        assert!(window.finish().await.is_err());
        let mut window = tap.begin_at(1, base).unwrap();
        window.release_at(base + Duration::from_secs(2));
        assert!(window.finish().await.is_err());
    }

    #[tokio::test]
    async fn missing_frames_and_empty_windows_fail() {
        let (tap, base) = running();
        let mut window = tap.begin_at(1, base).unwrap();
        tap.push(&[1.0; 16], base, base + Duration::from_millis(1));
        tap.push(
            &[1.0; 16],
            base + Duration::from_millis(2),
            base + Duration::from_millis(3),
        );
        window.release_at(base + Duration::from_millis(3));
        assert!(window.finish().await.is_err());
        let mut window = tap.begin_at(1, base + Duration::from_millis(3)).unwrap();
        window.release_at(base + Duration::from_millis(3));
        assert!(window.finish().await.is_err());
    }
}
