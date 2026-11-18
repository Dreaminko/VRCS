//! Process-wide ranking for the one active network capture.
//!
//! The capture worker publishes the current horizontal-view set; ASR startup and
//! translation admission read it at contention points. Track identifiers carry
//! session, flow, epoch, actor and generation, so stale entries cannot match a
//! later track.
//! Rankings expire at the earliest contributing position or heading deadline,
//! even when capture stops receiving packets or its worker is not scheduled.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

#[derive(Default)]
struct State {
    enabled: bool,
    visible: HashSet<String>,
    valid_until: Option<Instant>,
}

fn state() -> &'static Mutex<State> {
    static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);
    &STATE
}

pub(crate) fn replace(enabled: bool, visible: HashSet<String>, valid_until: Option<Instant>) {
    *state().lock().expect("view priority") = State {
        enabled,
        visible,
        valid_until,
    };
}

pub(crate) fn clear() {
    *state().lock().expect("view priority") = State::default();
}

pub(crate) fn is_visible(id: &str) -> bool {
    let state = state().lock().expect("view priority");
    state.is_visible_at(id, Instant::now())
}

impl State {
    fn is_visible_at(&self, id: &str, now: Instant) -> bool {
        self.enabled
            && self.valid_until.is_some_and(|until| now < until)
            && self.visible.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_expires_without_worker_refresh() {
        let now = Instant::now();
        let until = now + std::time::Duration::from_secs(1);
        let mut state = State {
            enabled: true,
            visible: HashSet::from(["speaker".into()]),
            valid_until: Some(until),
        };
        assert!(state.is_visible_at("speaker", now));
        assert!(!state.is_visible_at("speaker", until));
        state.valid_until = None;
        assert!(!state.is_visible_at("speaker", now));
    }
}
