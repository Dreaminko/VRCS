use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Dormant,
    Starting,
    AwaitingFrame,
    Live,
    RecoveringCapture,
    RecoveringHost,
    WarmHidden,
    Failed,
    Closing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fault {
    Unsupported,
    Host,
    SourceClosed,
    Session,
    DeviceLost,
    InvalidFrame,
    Submission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    None,
    RestartCapture,
    RecreateDevice,
    RecreateHost,
    Fail,
}

pub(super) struct Observation {
    pub ready: bool,
    pub layout: bool,
    pub has_frame: bool,
    pub pending_since: Option<Instant>,
    pub fault: Option<Fault>,
}

pub(super) struct Recovery {
    pub phase: Phase,
    pub capture_attempts: u8,
    pub host_attempts: u8,
    since: Option<Instant>,
    retry_at: Option<Instant>,
    stable_since: Option<Instant>,
    pub fault_since: Option<Instant>,
    pub fault: Option<Fault>,
}

impl Default for Recovery {
    fn default() -> Self {
        Self {
            phase: Phase::Dormant,
            capture_attempts: 0,
            host_attempts: 0,
            since: None,
            retry_at: None,
            stable_since: None,
            fault_since: None,
            fault: None,
        }
    }
}

impl Recovery {
    pub fn set_active(&mut self, active: bool, now: Instant) {
        if self.phase == Phase::Closing {
            return;
        }
        *self = Self {
            phase: if active {
                Phase::Starting
            } else {
                Phase::WarmHidden
            },
            since: Some(now),
            ..Self::default()
        };
    }
    pub fn retry(&mut self, now: Instant) {
        self.set_active(true, now);
    }
    pub fn submitted(&mut self, now: Instant) {
        if matches!(
            self.phase,
            Phase::Failed | Phase::Closing | Phase::WarmHidden | Phase::Dormant
        ) {
            return;
        }
        self.phase = Phase::Live;
        self.stable_since.get_or_insert(now);
        self.fault = None;
        self.fault_since = None;
    }
    pub fn navigating(&mut self, now: Instant) {
        if !matches!(
            self.phase,
            Phase::Failed | Phase::Closing | Phase::Dormant | Phase::WarmHidden
        ) {
            self.phase = Phase::Starting;
            self.since = Some(now);
            self.stable_since = None;
        }
    }
    pub fn page_ready(&mut self, now: Instant) {
        if self.phase == Phase::Starting {
            self.phase = Phase::AwaitingFrame;
            self.since = Some(now);
        }
    }
    pub fn awaiting_frame(&mut self, now: Instant) {
        if !matches!(self.phase, Phase::Failed | Phase::Closing) {
            self.phase = Phase::AwaitingFrame;
            self.since = Some(now);
            self.stable_since = None;
        }
    }
    pub fn advance(&mut self, observation: Observation, now: Instant) -> Action {
        if self.phase == Phase::Failed {
            return Action::Fail;
        }
        if matches!(
            self.phase,
            Phase::Dormant | Phase::WarmHidden | Phase::Closing
        ) {
            return Action::None;
        }
        if observation.ready && self.phase == Phase::Starting {
            self.phase = Phase::AwaitingFrame;
            self.since = Some(now);
        }
        let timed_out = if !observation.ready {
            self.since.is_some_and(|since| {
                now.saturating_duration_since(since) >= Duration::from_secs(20)
            })
        } else if !observation.has_frame
            || matches!(
                self.phase,
                Phase::AwaitingFrame | Phase::RecoveringCapture | Phase::RecoveringHost
            )
        {
            self.since
                .is_some_and(|since| now.saturating_duration_since(since) >= Duration::from_secs(3))
        } else {
            observation.pending_since.is_some_and(|since| {
                now.saturating_duration_since(since) >= Duration::from_millis(500)
            })
        };
        let fault = observation.fault.or_else(|| {
            timed_out.then_some(if !observation.ready || !observation.layout {
                Fault::Host
            } else {
                Fault::Session
            })
        });
        let Some(fault) = fault else {
            if self.phase == Phase::Live
                && self.stable_since.is_some_and(|since| {
                    now.saturating_duration_since(since) >= Duration::from_secs(5)
                })
            {
                self.capture_attempts = 0;
                self.host_attempts = 0;
                self.retry_at = None;
            }
            return Action::None;
        };
        self.stable_since = None;
        self.fault_since.get_or_insert(now);
        self.fault = Some(fault);
        if matches!(fault, Fault::Unsupported | Fault::Submission) {
            self.phase = Phase::Failed;
            return Action::Fail;
        }
        if self.retry_at.is_some_and(|at| now < at) {
            self.phase = Phase::RecoveringCapture;
            return Action::None;
        }
        let action =
            if !matches!(fault, Fault::Host | Fault::SourceClosed) && self.capture_attempts < 2 {
                self.capture_attempts += 1;
                self.phase = Phase::RecoveringCapture;
                if fault == Fault::DeviceLost {
                    Action::RecreateDevice
                } else {
                    Action::RestartCapture
                }
            } else if self.host_attempts < 1 {
                self.host_attempts += 1;
                self.phase = Phase::RecoveringHost;
                Action::RecreateHost
            } else {
                self.phase = Phase::Failed;
                Action::Fail
            };
        self.since = Some(now);
        self.retry_at = Some(now + Duration::from_secs(1));
        action
    }
    pub fn retains_frame(&self, same_identity: bool, allowed_surface: bool, now: Instant) -> bool {
        same_identity
            && allowed_surface
            && self.phase == Phase::RecoveringCapture
            && matches!(self.fault, Some(Fault::Session | Fault::InvalidFrame))
            && self.fault_since.is_some_and(|since| {
                now.saturating_duration_since(since) < Duration::from_millis(500)
            })
    }

    pub fn input_allowed(&self) -> bool {
        self.phase == Phase::Live && self.fault.is_none()
    }
}

#[derive(Default)]
pub(super) struct PendingPaints(VecDeque<(u32, Instant)>);
impl PendingPaints {
    pub fn accept(&mut self, serial: u32, now: Instant) -> bool {
        if self.0.iter().any(|(previous, _)| *previous == serial) {
            return true;
        }
        if self.0.len() >= 256 {
            return false;
        }
        self.0.push_back((serial, now));
        true
    }
    pub fn committed(&mut self, serial: u32) {
        self.0
            .retain(|(pending, _)| !super::serial_at_least(serial, *pending));
    }
    pub fn oldest(&self) -> Option<Instant> {
        self.0.front().map(|(_, at)| *at)
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observation(ready: bool, has_frame: bool, pending_since: Option<Instant>) -> Observation {
        Observation {
            ready,
            layout: ready,
            has_frame,
            pending_since,
            fault: None,
        }
    }
    #[test]
    fn deadlines_do_not_slide_and_idle_is_not_a_failure() {
        let start = Instant::now();
        let mut recovery = Recovery::default();
        recovery.set_active(true, start);
        assert_eq!(
            recovery.advance(
                observation(false, false, None),
                start + Duration::from_millis(19999)
            ),
            Action::None
        );
        assert_eq!(
            recovery.advance(
                observation(false, false, None),
                start + Duration::from_secs(20)
            ),
            Action::RecreateHost
        );
        recovery.retry(start);
        assert_eq!(
            recovery.advance(observation(true, false, None), start),
            Action::None
        );
        assert_eq!(
            recovery.advance(
                observation(true, false, None),
                start + Duration::from_millis(2999)
            ),
            Action::None
        );
        assert_eq!(
            recovery.advance(
                observation(true, false, None),
                start + Duration::from_secs(3)
            ),
            Action::RestartCapture
        );
        recovery.retry(start);
        recovery.submitted(start);
        assert_eq!(
            recovery.advance(
                observation(true, true, None),
                start + Duration::from_secs(60)
            ),
            Action::None
        );
        assert_eq!(
            recovery.advance(
                observation(true, true, Some(start)),
                start + Duration::from_millis(499)
            ),
            Action::None
        );
        assert_eq!(
            recovery.advance(
                observation(true, true, Some(start)),
                start + Duration::from_millis(500)
            ),
            Action::RestartCapture
        );
    }

    #[test]
    fn retry_budget_survives_brief_success_and_ends_in_failed() {
        let start = Instant::now();
        let mut recovery = Recovery::default();
        recovery.set_active(true, start);
        let fault = || Observation {
            fault: Some(Fault::Session),
            ..observation(true, true, None)
        };
        assert_eq!(recovery.advance(fault(), start), Action::RestartCapture);
        recovery.submitted(start + Duration::from_millis(10));
        assert_eq!(
            recovery.advance(fault(), start + Duration::from_millis(999)),
            Action::None
        );
        assert_eq!(
            recovery.advance(fault(), start + Duration::from_secs(1)),
            Action::RestartCapture
        );
        assert_eq!(
            recovery.advance(fault(), start + Duration::from_secs(2)),
            Action::RecreateHost
        );
        assert_eq!(
            recovery.advance(fault(), start + Duration::from_secs(3)),
            Action::Fail
        );
        assert_eq!(
            recovery.advance(
                observation(true, true, None),
                start + Duration::from_secs(10)
            ),
            Action::Fail
        );
        recovery.retry(start + Duration::from_secs(11));
        assert_eq!(
            recovery.advance(fault(), start + Duration::from_secs(11)),
            Action::RestartCapture
        );
    }

    #[test]
    fn device_and_source_failures_use_the_correct_recovery_scope() {
        let now = Instant::now();
        let mut recovery = Recovery::default();
        recovery.set_active(true, now);
        assert_eq!(
            recovery.advance(
                Observation {
                    fault: Some(Fault::DeviceLost),
                    ..observation(true, true, None)
                },
                now
            ),
            Action::RecreateDevice
        );
        assert!(!recovery.retains_frame(true, true, now));
        recovery.retry(now);
        assert_eq!(
            recovery.advance(
                Observation {
                    fault: Some(Fault::SourceClosed),
                    ..observation(true, true, None)
                },
                now
            ),
            Action::RecreateHost
        );
        assert!(!recovery.retains_frame(true, true, now));
        recovery.retry(now);
        assert_eq!(
            recovery.advance(
                Observation {
                    fault: Some(Fault::Unsupported),
                    ..observation(false, false, None)
                },
                now
            ),
            Action::Fail
        );
    }

    #[test]
    fn retained_frame_expires_and_cannot_cross_identity_or_ocr_boundaries() {
        let now = Instant::now();
        let mut recovery = Recovery::default();
        recovery.set_active(true, now);
        recovery.submitted(now);
        recovery.advance(
            Observation {
                fault: Some(Fault::Session),
                ..observation(true, true, None)
            },
            now,
        );
        assert!(recovery.retains_frame(true, true, now + Duration::from_millis(499)));
        assert!(!recovery.retains_frame(true, true, now + Duration::from_millis(500)));
        assert!(!recovery.retains_frame(false, true, now));
        assert!(!recovery.retains_frame(true, false, now));
        recovery.set_active(false, now);
        assert!(!recovery.retains_frame(true, true, now));
    }

    #[test]
    fn pending_paint_keeps_original_deadlines_through_partial_commits_and_wrap() {
        let now = Instant::now();
        let mut pending = PendingPaints::default();
        assert!(pending.accept(u32::MAX, now));
        assert!(pending.accept(1, now + Duration::from_millis(100)));
        assert!(pending.accept(u32::MAX, now + Duration::from_millis(200)));
        assert_eq!(pending.oldest(), Some(now));
        pending.committed(u32::MAX);
        assert_eq!(pending.oldest(), Some(now + Duration::from_millis(100)));
        pending.committed(1);
        assert_eq!(pending.oldest(), None);
        for serial in 1..=256 {
            assert!(pending.accept(serial, now));
        }
        assert!(pending.accept(1, now + Duration::from_secs(1)));
        assert!(!pending.accept(257, now));
        assert_eq!(pending.oldest(), Some(now));
        pending.clear();
        assert_eq!(pending.oldest(), None);
    }

    #[test]
    fn recovering_host_gets_a_new_startup_deadline_without_resetting_budget() {
        let start = Instant::now();
        let mut recovery = Recovery::default();
        recovery.set_active(true, start);
        let failed = || Observation {
            fault: Some(Fault::Host),
            ..observation(false, false, None)
        };
        assert_eq!(recovery.advance(failed(), start), Action::RecreateHost);
        recovery.navigating(start + Duration::from_secs(1));
        assert_eq!(
            recovery.advance(
                observation(false, false, None),
                start + Duration::from_secs(20)
            ),
            Action::None
        );
        assert_eq!(
            recovery.advance(
                observation(false, false, None),
                start + Duration::from_secs(21)
            ),
            Action::Fail
        );
    }

    #[test]
    fn hidden_reload_does_not_start_recovery_and_close_cannot_be_revived() {
        let now = Instant::now();
        let mut recovery = Recovery::default();
        recovery.set_active(false, now);
        recovery.navigating(now);
        assert_eq!(
            recovery.advance(
                observation(false, false, None),
                now + Duration::from_secs(60)
            ),
            Action::None
        );
        recovery.phase = Phase::Closing;
        recovery.navigating(now);
        recovery.set_active(true, now);
        assert_eq!(recovery.phase, Phase::Closing);
    }

    #[test]
    fn only_stable_live_time_resets_recovery_budget() {
        let start = Instant::now();
        let mut recovery = Recovery::default();
        recovery.set_active(true, start);
        recovery.advance(
            Observation {
                fault: Some(Fault::Session),
                ..observation(true, true, None)
            },
            start,
        );
        recovery.submitted(start + Duration::from_secs(1));
        recovery.advance(
            observation(true, true, None),
            start + Duration::from_millis(5999),
        );
        assert_eq!(recovery.capture_attempts, 1);
        recovery.advance(
            observation(true, true, None),
            start + Duration::from_secs(6),
        );
        assert_eq!(recovery.capture_attempts, 0);
    }

    #[test]
    fn device_fault_remains_nonretaining_during_retry_delay() {
        let now = Instant::now();
        let mut recovery = Recovery::default();
        recovery.set_active(true, now);
        recovery.advance(
            Observation {
                fault: Some(Fault::Session),
                ..observation(true, true, None)
            },
            now,
        );
        assert_eq!(
            recovery.advance(
                Observation {
                    fault: Some(Fault::DeviceLost),
                    ..observation(true, true, None)
                },
                now + Duration::from_millis(100)
            ),
            Action::None
        );
        assert!(!recovery.retains_frame(true, true, now + Duration::from_millis(100)));
        assert_eq!(
            recovery.advance(
                Observation {
                    fault: Some(Fault::DeviceLost),
                    ..observation(true, true, None)
                },
                now + Duration::from_secs(1)
            ),
            Action::RecreateDevice
        );
        assert_eq!(recovery.capture_attempts, 2);
    }

    #[test]
    fn old_capture_deadline_cannot_consume_retries_during_host_startup() {
        let now = Instant::now();
        let mut recovery = Recovery::default();
        recovery.set_active(true, now);
        recovery.advance(
            Observation {
                fault: Some(Fault::Host),
                ..observation(true, true, None)
            },
            now,
        );
        recovery.navigating(now);
        assert_eq!(
            recovery.advance(
                observation(false, false, Some(now)),
                now + Duration::from_secs(4)
            ),
            Action::None
        );
        recovery.page_ready(now + Duration::from_secs(5));
        assert_eq!(
            recovery.advance(
                observation(true, false, Some(now)),
                now + Duration::from_millis(7999)
            ),
            Action::None
        );
        assert_eq!(
            recovery.advance(
                observation(true, false, Some(now)),
                now + Duration::from_secs(8)
            ),
            Action::RestartCapture
        );
    }
}
