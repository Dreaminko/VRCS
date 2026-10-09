use super::{tasks::TranslationProgress, Phase};
use serde::Serialize;

/// Counts scan-owned requests, rather than translated result slots or elapsed time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PipelineProgress {
    pub scan_id: u64,
    pub recognition_phases: Vec<Phase>,
    pub images_total: usize,
    pub images_done: usize,
    pub recognition_done: bool,
    pub translation_started: bool,
    pub translation_total: usize,
    pub translation_completed: usize,
    pub translation_failed: usize,
    pub translation_total_final: bool,
}

pub(super) struct Reporter<F> {
    snapshot: PipelineProgress,
    phases: [Option<Phase>; 2],
    done: [bool; 2],
    callback: F,
}

impl<F: FnMut(PipelineProgress)> Reporter<F> {
    pub fn new(scan_id: u64, images_total: usize, callback: F) -> Self {
        Self {
            snapshot: PipelineProgress {
                scan_id,
                images_total,
                ..Default::default()
            },
            phases: [None; 2],
            done: [false; 2],
            callback,
        }
    }

    pub fn phase(&mut self, eye: Option<usize>, phase: Phase) {
        for index in 0..self.snapshot.images_total {
            if !self.done[index] && eye.is_none_or(|eye| eye == index) {
                self.phases[index] = Some(phase);
            }
        }
        self.publish();
    }

    pub fn translation(&mut self, event: TranslationProgress) {
        match event {
            TranslationProgress::ImageDone(eye) => {
                self.done[eye] = true;
                self.phases[eye] = None;
                self.snapshot.images_done += 1;
            }
            TranslationProgress::InputDone => {
                self.snapshot.recognition_done = true;
                self.snapshot.translation_total_final = true;
            }
            TranslationProgress::Registered => {
                self.snapshot.translation_started = true;
                self.snapshot.translation_total += 1;
            }
            TranslationProgress::Finished(failed) => {
                self.snapshot.translation_completed += 1;
                self.snapshot.translation_failed += usize::from(failed);
            }
        }
        self.publish();
    }

    fn publish(&mut self) {
        self.snapshot.recognition_phases.clear();
        for phase in self.phases.iter().flatten() {
            if !self.snapshot.recognition_phases.contains(phase) {
                self.snapshot.recognition_phases.push(*phase);
            }
        }
        (self.callback)(self.snapshot.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_does_not_hide_the_other_eyes_recognition() {
        let mut snapshots = Vec::new();
        {
            let mut report = Reporter::new(42, 2, |snapshot| snapshots.push(snapshot));
            report.phase(Some(0), Phase::Downloading);
            report.phase(Some(1), Phase::Pending);
            report.translation(TranslationProgress::Registered);
            report.translation(TranslationProgress::ImageDone(0));
        }
        let progress = snapshots.last().unwrap();
        assert_eq!(progress.recognition_phases, [Phase::Pending]);
        assert_eq!(
            (
                progress.scan_id,
                progress.images_total,
                progress.images_done
            ),
            (42, 2, 1)
        );
        assert!(progress.translation_started);
        assert!(!progress.translation_total_final);
    }

    #[test]
    fn input_end_fixes_the_total_without_completing_pending_requests() {
        let mut snapshots = Vec::new();
        {
            let mut report = Reporter::new(3, 1, |snapshot| snapshots.push(snapshot));
            report.phase(None, Phase::Recognizing);
            report.translation(TranslationProgress::Registered);
            report.translation(TranslationProgress::Registered);
            report.translation(TranslationProgress::Finished(true));
            report.translation(TranslationProgress::ImageDone(0));
            report.translation(TranslationProgress::InputDone);
        }
        let progress = snapshots.last().unwrap();
        assert!(progress.recognition_done && progress.translation_total_final);
        assert_eq!(
            (
                progress.translation_total,
                progress.translation_completed,
                progress.translation_failed
            ),
            (2, 1, 1)
        );
        assert!(progress.recognition_phases.is_empty());
    }
}
