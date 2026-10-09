#[cfg(any(windows, test))]
use super::ocr_status::OcrState;
use serde::Serialize;
#[cfg(any(windows, test))]
use std::time::{Duration, Instant};
#[cfg(any(windows, test))]
use vrcs_core::ocr::{Phase, PipelineProgress};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Pending,
    Active,
    Complete,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProgressView {
    pub scan_id: u64,
    pub status: String,
    pub detail: String,
    pub elapsed_seconds: u64,
    pub stages: [Stage; 4],
    pub translation_fraction: Option<(usize, usize)>,
}

#[cfg(any(windows, test))]
pub(super) struct Feedback {
    scan_id: u64,
    source_only: bool,
    started: Instant,
    phase_started: Instant,
    pipeline: Option<PipelineProgress>,
    terminal: Option<(OcrState, Instant)>,
    displayed: bool,
    error: Option<&'static str>,
}

#[cfg(any(windows, test))]
impl Feedback {
    pub fn new(scan_id: u64, source_only: bool, started: Instant) -> Self {
        Self {
            scan_id,
            source_only,
            started,
            phase_started: started,
            pipeline: None,
            terminal: None,
            displayed: false,
            error: None,
        }
    }

    pub fn apply(&mut self, snapshot: PipelineProgress, now: Instant) {
        if snapshot.scan_id != self.scan_id || self.terminal.is_some() {
            return;
        }
        if self.pipeline.as_ref().is_none_or(|old| {
            old.recognition_phases != snapshot.recognition_phases
                || old.recognition_done != snapshot.recognition_done
                || old.translation_started != snapshot.translation_started
        }) {
            self.phase_started = now;
        }
        self.pipeline = Some(snapshot);
    }

    pub fn displayed(&mut self) {
        self.displayed = true;
    }
    pub fn results_changed(&mut self) {
        self.displayed = false;
    }

    pub fn finish(&mut self, state: OcrState, now: Instant) {
        if self.terminal.is_none() {
            self.terminal = Some((state, now));
        }
    }

    pub fn fail(&mut self, state: OcrState, code: &str, now: Instant) {
        self.error = Some(match code {
            "authentication" => "Authentication failed",
            "credentials_missing" => "OCR credentials are missing",
            "rate_limit" => "Service rate limit reached",
            "models_missing" => "Local OCR models are missing",
            "network" => "Network request failed",
            "local_failed" => "Local OCR failed",
            "tracking_lost" => "Headset tracking unavailable",
            "view_changed" => "View changed; scan again",
            "configuration_changed" => "Settings changed; scan again",
            _ => "Try scanning again",
        });
        self.terminal = Some((state, now));
    }

    pub fn animation(&self, now: Instant) -> u8 {
        if self.terminal.is_some() {
            0
        } else {
            ((now.saturating_duration_since(self.started).as_millis() / 250) % 4) as u8
        }
    }

    pub fn view(&self, now: Instant) -> Option<ProgressView> {
        let end = self.terminal.map(|(_, ended)| ended).unwrap_or(now);
        let elapsed_seconds = end.saturating_duration_since(self.started).as_secs();
        let mut stages = [
            Stage::Active,
            Stage::Pending,
            if self.source_only {
                Stage::Skipped
            } else {
                Stage::Pending
            },
            Stage::Pending,
        ];
        let mut status = "Preparing image…";
        let mut detail = format!("Elapsed {elapsed_seconds}s");
        let mut translation_fraction = None;
        if let Some(pipeline) = &self.pipeline {
            stages[0] = Stage::Complete;
            stages[1] = if pipeline.recognition_done {
                Stage::Complete
            } else {
                Stage::Active
            };
            if !self.source_only && pipeline.translation_started {
                stages[2] = if pipeline.translation_total_final
                    && pipeline.translation_completed == pipeline.translation_total
                {
                    Stage::Complete
                } else {
                    Stage::Active
                };
                if pipeline.translation_total_final && pipeline.translation_total > 0 {
                    translation_fraction =
                        Some((pipeline.translation_completed, pipeline.translation_total));
                    detail.push_str(&format!(
                        " · Tasks {} / {}",
                        pipeline.translation_completed, pipeline.translation_total
                    ));
                } else {
                    detail.push_str(&format!(
                        " · {} tasks processed",
                        pipeline.translation_completed
                    ));
                }
                if pipeline.translation_failed > 0 {
                    detail.push_str(&format!(" · {} failed", pipeline.translation_failed));
                }
            }
            status = if !pipeline.recognition_done {
                if pipeline.translation_started {
                    "Recognizing and translating…"
                } else if pipeline.recognition_phases.contains(&Phase::WaitingWorker) {
                    "Waiting for local OCR…"
                } else if pipeline.recognition_phases.contains(&Phase::LoadingModel) {
                    "Loading local models…"
                } else if pipeline.recognition_phases.contains(&Phase::Pending) {
                    "Waiting for cloud OCR…"
                } else if pipeline.recognition_phases.contains(&Phase::Submitting) {
                    "Uploading image…"
                } else if pipeline.recognition_phases.contains(&Phase::Downloading) {
                    "Fetching OCR results…"
                } else {
                    "Recognizing text…"
                }
            } else if stages[2] == Stage::Active {
                "Translating text…"
            } else {
                "Displaying results…"
            };
            if self.terminal.is_none()
                && now.saturating_duration_since(self.phase_started) >= Duration::from_secs(5)
            {
                if pipeline.recognition_phases.contains(&Phase::LoadingModel) {
                    detail.push_str(" · First scan may take longer");
                } else if pipeline.recognition_phases.contains(&Phase::Pending) {
                    detail.push_str(" · Waiting for the OCR service");
                }
            }
        }
        if self.displayed {
            stages[3] = Stage::Active;
        }
        if let Some((state, ended)) = self.terminal {
            let successful =
                matches!(state, OcrState::Visible | OcrState::SourceVisible) && self.displayed;
            let duration = if successful {
                Duration::from_millis(1200)
            } else {
                Duration::from_secs(3)
            };
            if now.saturating_duration_since(ended) >= duration {
                return None;
            }
            match state {
                OcrState::NoText | OcrState::LowConfidence => {
                    stages = [
                        Stage::Complete,
                        Stage::Complete,
                        Stage::Skipped,
                        Stage::Skipped,
                    ];
                    status = if state == OcrState::NoText {
                        "No text detected"
                    } else {
                        "No readable text detected"
                    };
                }
                OcrState::Visible
                | OcrState::Recognized
                | OcrState::SourceVisible
                | OcrState::PartialVisible
                | OcrState::TranslationFailed => {
                    stages = [
                        Stage::Complete,
                        Stage::Complete,
                        if self.source_only {
                            Stage::Skipped
                        } else if matches!(
                            state,
                            OcrState::PartialVisible | OcrState::TranslationFailed
                        ) {
                            Stage::Failed
                        } else {
                            Stage::Complete
                        },
                        if self.displayed {
                            Stage::Complete
                        } else {
                            Stage::Failed
                        },
                    ];
                    status = if state == OcrState::TranslationFailed {
                        "Translation failed"
                    } else if state == OcrState::PartialVisible {
                        "Some translations unavailable"
                    } else if !self.displayed {
                        "Results ready; display unavailable"
                    } else if self.source_only {
                        "Original text displayed"
                    } else {
                        "Complete · Results displayed"
                    };
                }
                _ => {
                    let failed = stages
                        .iter()
                        .position(|stage| *stage == Stage::Active)
                        .unwrap_or(3);
                    stages[failed] = Stage::Failed;
                    for stage in &mut stages {
                        if *stage == Stage::Active {
                            *stage = Stage::Skipped;
                        }
                    }
                    if self.displayed {
                        stages[3] = Stage::Complete;
                    }
                    status = match state {
                        OcrState::TimedOut if self.displayed => {
                            "Timed out · Partial results displayed"
                        }
                        OcrState::TimedOut => "Processing timed out",
                        OcrState::Invalid => "View changed; scan again",
                        _ => "OCR unavailable",
                    };
                }
            }
            if let Some(error) = self.error {
                detail.push_str(&format!(" · {error}"));
            }
        }
        Some(ProgressView {
            scan_id: self.scan_id,
            status: status.into(),
            detail,
            elapsed_seconds,
            stages,
            translation_fraction,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_results_require_display_confirmation_before_success() {
        let now = Instant::now();
        let mut feedback = Feedback::new(8, false, now);
        feedback.finish(OcrState::Visible, now);
        assert_eq!(feedback.view(now).unwrap().stages[3], Stage::Failed);
        feedback.displayed();
        assert_eq!(feedback.view(now).unwrap().stages[3], Stage::Complete);
    }

    #[test]
    fn late_snapshots_cannot_restart_finished_feedback() {
        let now = Instant::now();
        let mut feedback = Feedback::new(7, false, now);
        feedback.apply(
            PipelineProgress {
                scan_id: 6,
                ..Default::default()
            },
            now,
        );
        assert_eq!(feedback.view(now).unwrap().stages[0], Stage::Active);
        feedback.finish(OcrState::TimedOut, now);
        feedback.apply(
            PipelineProgress {
                scan_id: 7,
                recognition_phases: vec![Phase::Recognizing],
                ..Default::default()
            },
            now,
        );
        let view = feedback.view(now).unwrap();
        assert!(view.stages.contains(&Stage::Failed));
        assert!(!view.stages.contains(&Stage::Active));
        assert!(feedback.view(now + Duration::from_secs(3)).is_none());
    }

    #[test]
    fn streamed_results_keep_recognition_and_translation_active() {
        let now = Instant::now();
        let mut feedback = Feedback::new(1, false, now);
        feedback.apply(
            PipelineProgress {
                scan_id: 1,
                images_total: 2,
                images_done: 1,
                recognition_phases: vec![Phase::Recognizing],
                translation_started: true,
                translation_total: 4,
                translation_completed: 1,
                ..Default::default()
            },
            now,
        );
        feedback.displayed();
        let view = feedback.view(now + Duration::from_secs(8)).unwrap();
        assert_eq!(
            view.stages,
            [Stage::Complete, Stage::Active, Stage::Active, Stage::Active]
        );
        assert_eq!(view.translation_fraction, None);
        assert_eq!(view.elapsed_seconds, 8);
    }

    #[test]
    fn fixed_translation_total_and_actual_display_are_required_for_completion() {
        let now = Instant::now();
        let mut feedback = Feedback::new(2, false, now);
        feedback.apply(
            PipelineProgress {
                scan_id: 2,
                images_total: 1,
                images_done: 1,
                recognition_done: true,
                translation_started: true,
                translation_total: 2,
                translation_completed: 1,
                translation_total_final: true,
                ..Default::default()
            },
            now,
        );
        assert_eq!(
            feedback.view(now).unwrap().translation_fraction,
            Some((1, 2))
        );
        feedback.finish(OcrState::Visible, now);
        let view = feedback.view(now).unwrap();
        assert_eq!(view.stages[3], Stage::Failed);
        assert!(!view.status.contains("Complete"));
    }

    #[test]
    fn source_only_skips_translation_and_success_expires_without_animation() {
        let now = Instant::now();
        let mut feedback = Feedback::new(3, true, now);
        feedback.displayed();
        feedback.finish(OcrState::SourceVisible, now);
        assert_eq!(
            feedback.view(now).unwrap().stages,
            [
                Stage::Complete,
                Stage::Complete,
                Stage::Skipped,
                Stage::Complete
            ]
        );
        assert_eq!(feedback.animation(now + Duration::from_millis(300)), 0);
        assert!(feedback.view(now + Duration::from_millis(1200)).is_none());
    }

    #[test]
    fn animation_updates_at_most_four_times_per_second() {
        let now = Instant::now();
        let feedback = Feedback::new(4, false, now);
        assert_eq!(
            feedback.animation(now),
            feedback.animation(now + Duration::from_millis(249))
        );
        assert_ne!(
            feedback.animation(now),
            feedback.animation(now + Duration::from_millis(250))
        );
    }
}
