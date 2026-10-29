//! Opt-in, bounded diagnostics. Counts describe application work, not GPU duration.
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, OnceLock,
};
use std::time::{Duration, Instant};

pub(super) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("VRCS_VR_SURFACE_METRICS").as_deref() == Ok("1"))
}

#[derive(Default)]
pub struct CaptureCounters {
    pub source_frames: AtomicU64,
    pub copies: AtomicU64,
    pub copy_bytes: AtomicU64,
    pub submissions: AtomicU64,
    pub rejected_markers: AtomicU64,
    pub rejected_frames: AtomicU64,
    last_frame: AtomicU64,
    last_submit: AtomicU64,
}

impl CaptureCounters {
    pub fn increment(counter: &AtomicU64, amount: u64) {
        if enabled() {
            counter.fetch_add(amount, Ordering::Relaxed);
        }
    }

    pub fn frame(&self) {
        if enabled() {
            self.source_frames.fetch_add(1, Ordering::Relaxed);
            self.last_frame.store(clock_ms(), Ordering::Relaxed);
        }
    }

    pub fn submitted(&self) {
        if enabled() {
            self.submissions.fetch_add(1, Ordering::Relaxed);
            self.last_submit.store(clock_ms(), Ordering::Relaxed);
        }
    }
}

fn clock_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

fn age_ms(at: &AtomicU64) -> Option<u64> {
    let at = at.load(Ordering::Relaxed);
    (at != 0).then(|| clock_ms().saturating_sub(at))
}

pub(super) struct Metrics {
    pub capture: Arc<CaptureCounters>,
    pub publications: u64,
    pub layouts: u64,
    pub paints: u64,
    pub pauses: u64,
    pub resumes: u64,
    pub cold_releases: u64,
    pub last_paint: Option<Instant>,
    wake_ms: [u64; 32],
    wake_count: usize,
    report_at: Instant,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            capture: Arc::default(),
            publications: 0,
            layouts: 0,
            paints: 0,
            pauses: 0,
            resumes: 0,
            cold_releases: 0,
            last_paint: None,
            wake_ms: [0; 32],
            wake_count: 0,
            report_at: Instant::now(),
        }
    }
}

impl Metrics {
    pub fn wake(&mut self, elapsed: Duration) {
        if !enabled() {
            return;
        }
        self.wake_ms[self.wake_count % 32] = elapsed.as_millis() as u64;
        self.wake_count = self.wake_count.saturating_add(1);
    }

    pub fn report(&mut self, surface: &str, state: &str, host: bool, source: bool) {
        if !enabled() || self.report_at.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.report_at = Instant::now();
        let mut samples = self.wake_ms;
        let count = self.wake_count.min(32);
        samples[..count].sort_unstable();
        let wake_p95_ms = if count == 0 {
            0
        } else {
            samples[(count * 95).div_ceil(100) - 1]
        };
        tracing::info!(target: "vr_surface_metrics", surface, state, host_count = u8::from(host),
            active_sources = u8::from(source), publications = self.publications,
            layouts = self.layouts, paint_requests = self.paints,
            source_frames = self.capture.source_frames.load(Ordering::Relaxed),
            full_copies = self.capture.copies.load(Ordering::Relaxed),
            estimated_copy_bytes = self.capture.copy_bytes.load(Ordering::Relaxed),
            submissions = self.capture.submissions.load(Ordering::Relaxed),
            rejected_markers = self.capture.rejected_markers.load(Ordering::Relaxed),
            rejected_frames = self.capture.rejected_frames.load(Ordering::Relaxed),
            paint_age_ms = self.last_paint.map(|at| at.elapsed().as_millis() as u64),
            frame_age_ms = age_ms(&self.capture.last_frame),
            submit_age_ms = age_ms(&self.capture.last_submit),
            pauses = self.pauses, resumes = self.resumes, cold_releases = self.cold_releases,
            wake_samples = count, wake_p95_ms, "VR surface work totals");
    }
}
