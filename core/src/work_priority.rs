//! Bounded admission at shared model slots and connection-rate limits.
//! Priority changes affect waiting work only; permits never preempt running work.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::Instant;

#[derive(Clone, Default)]
pub(crate) struct Context {
    player: Option<Arc<str>>,
}

tokio::task_local! {
    static CURRENT: Context;
}

impl Context {
    pub(crate) fn for_player(id: &str) -> Self {
        Self {
            player: id.starts_with("vrc:").then(|| Arc::from(id)),
        }
    }

    pub(crate) fn current() -> Self {
        CURRENT.try_with(Clone::clone).unwrap_or_default()
    }

    pub(crate) fn is_priority(&self) -> bool {
        self.player
            .as_deref()
            .is_some_and(crate::player_audio::is_view_priority)
    }

    pub(crate) async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }
}

#[derive(Clone)]
pub(crate) struct Gate(Arc<Inner>);

struct Inner {
    state: Mutex<State>,
    changed: watch::Sender<()>,
    interval: Option<Duration>,
    max_waiting: usize,
}

struct State {
    available: usize,
    next_id: u64,
    next_start: Instant,
    waiting: Vec<Waiter>,
    priority_burst: u8,
    last_player: Option<Arc<str>>,
}

struct Waiter {
    id: u64,
    context: Context,
    queued_at: Instant,
}

pub(crate) struct Permit(Arc<Inner>);

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.state.lock().expect("work admission").available += 1;
        // Return capacity before waking waiters, including on cancellation/panic.
        self.0.changed.send_replace(());
    }
}

struct Waiting {
    inner: Arc<Inner>,
    id: u64,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        let mut state = self.inner.state.lock().expect("work admission");
        if let Some(index) = state.waiting.iter().position(|waiter| waiter.id == self.id) {
            state.waiting.remove(index);
            drop(state);
            self.inner.changed.send_replace(());
        }
    }
}

impl Gate {
    pub(crate) fn new(capacity: usize, max_waiting: usize) -> Self {
        assert!(capacity > 0);
        Self(Arc::new(Inner {
            state: Mutex::new(State {
                available: capacity,
                next_id: 0,
                next_start: Instant::now(),
                waiting: Vec::new(),
                priority_burst: 0,
                last_player: None,
            }),
            changed: watch::channel(()).0,
            interval: None,
            max_waiting,
        }))
    }

    pub(crate) fn rate_limited(interval: Duration, max_waiting: usize) -> Self {
        let mut gate = Self::new(1, max_waiting);
        Arc::get_mut(&mut gate.0)
            .expect("new admission gate")
            .interval = Some(interval);
        gate
    }

    pub(crate) fn defer_until(&self, until: Instant) {
        let mut state = self.0.state.lock().expect("work admission");
        state.next_start = state.next_start.max(until);
        drop(state);
        self.0.changed.send_replace(());
    }

    pub(crate) async fn acquire(&self) -> Result<Permit, &'static str> {
        // Subscribe before publishing the waiter so a release cannot be lost.
        let mut changed = self.0.changed.subscribe();
        let id = {
            let mut state = self.0.state.lock().expect("work admission");
            if state.waiting.len() >= self.0.max_waiting {
                return Err("Work admission queue is full");
            }
            state.next_id = state.next_id.wrapping_add(1);
            let id = state.next_id;
            state.waiting.push(Waiter {
                id,
                context: Context::current(),
                queued_at: Instant::now(),
            });
            id
        };
        let _waiting = Waiting {
            inner: Arc::clone(&self.0),
            id,
        };
        self.0.changed.send_replace(());
        loop {
            let deadline = {
                let mut state = self.0.state.lock().expect("work admission");
                let now = Instant::now();
                if state.available > 0 && state.next_start <= now {
                    if let Some(index) = next_waiter(&state, now) {
                        if state.waiting[index].id == id {
                            let waiter = state.waiting.remove(index);
                            state.priority_burst = if waiter.context.is_priority() {
                                (state.priority_burst + 1).min(3)
                            } else {
                                0
                            };
                            state.last_player = waiter.context.player;
                            state.available -= 1;
                            if let Some(interval) = self.0.interval {
                                state.next_start = now + interval;
                            }
                            drop(state);
                            self.0.changed.send_replace(());
                            return Ok(Permit(Arc::clone(&self.0)));
                        }
                    }
                }
                (state.available > 0).then_some(if state.next_start > now {
                    state.next_start
                } else {
                    now + Duration::from_millis(50)
                })
            };
            if let Some(deadline) = deadline {
                tokio::select! {
                    _ = changed.changed() => {}
                    _ = tokio::time::sleep_until(deadline) => {}
                }
            } else {
                let _ = changed.changed().await;
            }
        }
    }
}

fn next_waiter(state: &State, now: Instant) -> Option<usize> {
    let priority = state
        .waiting
        .iter()
        .any(|waiter| waiter.context.is_priority());
    let ordinary = state
        .waiting
        .iter()
        .any(|waiter| !waiter.context.is_priority());
    let choose_priority = priority && (!ordinary || state.priority_burst < 3);
    let candidates = || {
        state
            .waiting
            .iter()
            .enumerate()
            .filter(|(_, waiter)| waiter.context.is_priority() == choose_priority)
    };
    // Old work wins inside its class; the 3:1 service budget still gives both
    // classes progress even when a rate limit makes every waiter old.
    if let Some((index, _)) = candidates()
        .filter(|(_, waiter)| now.duration_since(waiter.queued_at) >= Duration::from_secs(2))
        .min_by_key(|(_, waiter)| waiter.id)
    {
        return Some(index);
    }
    candidates()
        .find(|(_, waiter)| waiter.context.player != state.last_player)
        .or_else(|| candidates().next())
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelled_waiters_release_the_queue_and_permits_wake_after_capacity_returns() {
        let gate = Gate::new(1, 1);
        let running = gate.acquire().await.unwrap();
        let waiting = tokio::spawn({
            let gate = gate.clone();
            async move { gate.acquire().await }
        });
        tokio::task::yield_now().await;
        assert!(gate.acquire().await.is_err());
        waiting.abort();
        let _ = waiting.await;
        let next = tokio::spawn({
            let gate = gate.clone();
            async move { gate.acquire().await }
        });
        tokio::task::yield_now().await;
        drop(running);
        let permit = tokio::time::timeout(Duration::from_secs(1), next)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        drop(permit);
        assert_eq!(gate.0.state.lock().unwrap().available, 1);
    }
}
