use std::collections::{HashSet, VecDeque};

const MAX_RECENT_TERMINATIONS: usize = 32;

#[derive(Default)]
pub(super) struct RecognitionLifecycle {
    active: HashSet<String>,
    terminated: HashSet<String>,
    termination_order: VecDeque<String>,
}

impl RecognitionLifecycle {
    pub(super) fn begin(&mut self, utterance_id: &str) -> bool {
        if self.terminated.contains(utterance_id) {
            return false;
        }
        self.active.insert(utterance_id.to_owned());
        true
    }

    pub(super) fn accept_partial(&mut self, utterance_id: &str) -> bool {
        self.begin(utterance_id)
    }

    pub(super) fn failure_id(&self, utterance_id: Option<&str>) -> Option<String> {
        utterance_id.map(str::to_owned).or_else(|| {
            (self.active.len() == 1)
                .then(|| self.active.iter().next().cloned())
                .flatten()
        })
    }

    pub(super) fn terminate(&mut self, utterance_id: &str) {
        self.active.remove(utterance_id);
        self.remember_termination(utterance_id);
    }

    pub(super) fn terminate_all(&mut self) -> Vec<String> {
        let active = std::mem::take(&mut self.active);
        for utterance_id in &active {
            self.remember_termination(utterance_id);
        }
        active.into_iter().collect()
    }

    pub(super) fn reset(&mut self) {
        self.active.clear();
        self.terminated.clear();
        self.termination_order.clear();
    }

    fn remember_termination(&mut self, utterance_id: &str) {
        if !self.terminated.insert(utterance_id.to_owned()) {
            return;
        }
        self.termination_order.push_back(utterance_id.to_owned());
        if self.termination_order.len() > MAX_RECENT_TERMINATIONS {
            if let Some(expired) = self.termination_order.pop_front() {
                self.terminated.remove(&expired);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_late_partial_cannot_reactivate_a_completed_utterance() {
        let mut lifecycle = RecognitionLifecycle::default();
        assert!(lifecycle.accept_partial("utterance-1"));
        lifecycle.terminate("utterance-1");
        assert!(!lifecycle.accept_partial("utterance-1"));
    }

    #[test]
    fn final_only_utterance_can_be_tracked_before_completion() {
        let mut lifecycle = RecognitionLifecycle::default();
        assert!(lifecycle.begin("utterance-1"));
        assert_eq!(lifecycle.failure_id(None).as_deref(), Some("utterance-1"));
        lifecycle.terminate("utterance-1");
        assert!(!lifecycle.begin("utterance-1"));
    }

    #[test]
    fn completing_one_utterance_does_not_end_another() {
        let mut lifecycle = RecognitionLifecycle::default();
        assert!(lifecycle.accept_partial("utterance-1"));
        assert!(lifecycle.accept_partial("utterance-2"));
        lifecycle.terminate("utterance-1");
        assert!(lifecycle.accept_partial("utterance-2"));
        assert_eq!(lifecycle.failure_id(None).as_deref(), Some("utterance-2"));
    }

    #[test]
    fn the_termination_cache_is_bounded() {
        let mut lifecycle = RecognitionLifecycle::default();
        for index in 0..=MAX_RECENT_TERMINATIONS {
            lifecycle.terminate(&format!("utterance-{index}"));
        }
        assert!(lifecycle.accept_partial("utterance-0"));
        assert!(!lifecycle.accept_partial(&format!("utterance-{MAX_RECENT_TERMINATIONS}")));
    }
}
