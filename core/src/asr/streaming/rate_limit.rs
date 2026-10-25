//! Admission control applies to connections, never to audio on an open socket.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::time::Instant;

use crate::{
    config::{ApiProfile, AsrConfig},
    providers,
};

// Qwen AI / Model Studio LiveTranslate publish 10 RPM. Space starts slightly
// beyond six seconds so sliding minute windows do not include eleven starts.
const START_INTERVAL: Duration = Duration::from_millis(6_100);
const BACKOFF_RESET: Duration = Duration::from_secs(300);

#[derive(Hash, PartialEq, Eq)]
struct QuotaKey {
    provider: String,
    region: String,
    model: String,
}

fn quota_key(config: &AsrConfig, profile: &ApiProfile) -> Option<QuotaKey> {
    if config.backend != providers::SERVICE_QWEN_LIVE_TRANSLATE
        || !matches!(
            profile.provider.as_str(),
            providers::QWEN_AI_PROVIDER | providers::ALIBABA_PROVIDER
        )
    {
        return None;
    }
    Some(QuotaKey {
        provider: profile.provider.clone(),
        region: if profile.provider == providers::QWEN_AI_PROVIDER {
            String::new()
        } else {
            profile.region.clone().unwrap_or_default()
        },
        model: config.service_settings.get(&config.backend)?.model.clone(),
    })
}

pub(super) fn gate(config: &AsrConfig, profile: &ApiProfile) -> Option<Arc<ConnectionGate>> {
    static GATES: OnceLock<Mutex<HashMap<QuotaKey, Arc<ConnectionGate>>>> = OnceLock::new();
    // Account identity is unavailable locally. Share across keys/workspaces on
    // the same service/model rather than accidentally granting each player RPM.
    let key = quota_key(config, profile)?;
    Some(
        GATES
            .get_or_init(Default::default)
            .lock()
            .expect("connection gates")
            .entry(key)
            .or_default()
            .clone(),
    )
}

pub(crate) fn controlled(config: &AsrConfig) -> bool {
    config
        .api_profiles
        .iter()
        .find(|p| Some(&p.id) == config.active_profile_id.as_ref())
        .is_some_and(|p| quota_key(config, p).is_some())
}

#[derive(Default)]
struct State {
    next_start: Option<Instant>,
    cooldown: Option<Instant>,
    last_limit: Option<Instant>,
    failures: u32,
}

#[derive(Default)]
pub(super) struct ConnectionGate {
    queue: tokio::sync::Mutex<()>,
    state: Mutex<State>,
}

impl ConnectionGate {
    pub(super) async fn acquire(&self) {
        // FIFO, cancellation-safe: only an actual admission consumes a slot.
        let _turn = self.queue.lock().await;
        loop {
            let until = {
                let mut state = self.state.lock().expect("connection quota");
                let now = Instant::now();
                let until = state
                    .next_start
                    .into_iter()
                    .chain(state.cooldown)
                    .max()
                    .unwrap_or(now);
                if until <= now {
                    state.next_start = Some(now + START_INTERVAL);
                    return;
                }
                until
            };
            // Recheck shared cooldown: another active socket may be limited
            // while this request is queued. Never sleep with the state locked.
            tokio::time::sleep_until(until).await;
        }
    }

    pub(super) fn failed(&self, detail: &str) {
        if !is_rate_limit(detail) {
            return;
        }
        let now = Instant::now();
        let mut state = self.state.lock().expect("connection quota");
        // A burst of failures is one incident, not N backoff increments.
        if state.cooldown.is_some_and(|until| until > now) {
            return;
        }
        if state
            .last_limit
            .is_none_or(|last| now.duration_since(last) >= BACKOFF_RESET)
        {
            state.failures = 0;
        }
        let seconds = 5 * (u64::from(state.failures.min(2)) + 1);
        state.failures = state.failures.saturating_add(1);
        state.last_limit = Some(now);
        state.cooldown = Some(now + Duration::from_secs(seconds));
        tracing::warn!(
            retry_after_seconds = seconds,
            "Qwen connection quota reached; shared cooldown before new sessions"
        );
    }
}

fn is_rate_limit(detail: &str) -> bool {
    let text = detail.to_ascii_lowercase();
    [
        "rate limit",
        "ratelimit",
        "throttl",
        "too many requests",
        "too many concurrent",
        "exceeded your request",
        "exceeded your current request",
        "http error: 429",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn first_start_is_immediate_and_all_other_starts_share_a_minute_budget() {
        let gate = ConnectionGate::default();
        let start = Instant::now();
        gate.acquire().await;
        assert_eq!(Instant::now(), start);
        for n in 1..=10 {
            gate.acquire().await;
            assert_eq!(Instant::now().duration_since(start), START_INTERVAL * n);
        }
        assert!(Instant::now().duration_since(start) > Duration::from_secs(60));
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_wait_does_not_reserve_a_slot_and_cooldown_is_rechecked() {
        let gate = Arc::new(ConnectionGate::default());
        gate.acquire().await;
        let waiting = {
            let gate = gate.clone();
            tokio::spawn(async move { gate.acquire().await })
        };
        tokio::task::yield_now().await;
        waiting.abort();
        let _ = waiting.await;
        tokio::time::advance(START_INTERVAL).await;
        let start = Instant::now();
        gate.failed("1007 Requests rate limit exceeded, please try again later.");
        gate.failed("Requests rate limit exceeded");
        gate.acquire().await;
        assert_eq!(Instant::now().duration_since(start), Duration::from_secs(5));
        gate.failed("429 Too many requests");
        gate.acquire().await;
        assert_eq!(
            Instant::now().duration_since(start),
            Duration::from_secs(15)
        );
        gate.failed("Throttling.Concurrency");
        gate.acquire().await;
        assert_eq!(
            Instant::now().duration_since(start),
            Duration::from_secs(30)
        );
        gate.failed("Requests rate limit exceeded");
        gate.acquire().await;
        assert_eq!(
            Instant::now().duration_since(start),
            Duration::from_secs(45)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn failures_on_other_sockets_extend_a_wait_without_delaying_existing_audio() {
        let gate = Arc::new(ConnectionGate::default());
        gate.acquire().await;
        let start = Instant::now();
        let waiting = {
            let gate = gate.clone();
            tokio::spawn(async move { gate.acquire().await })
        };
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(3)).await;
        gate.failed("Requests rate limit exceeded");
        waiting.await.unwrap();
        assert_eq!(Instant::now().duration_since(start), Duration::from_secs(8));
        tokio::time::advance(BACKOFF_RESET).await;
        gate.failed("Requests rate limit exceeded");
        let reset = Instant::now();
        gate.acquire().await;
        assert_eq!(Instant::now().duration_since(reset), Duration::from_secs(5));
    }

    #[test]
    fn profiles_and_workspaces_share_model_quota_but_unrelated_services_do_not() {
        let config = AsrConfig {
            backend: providers::SERVICE_QWEN_LIVE_TRANSLATE.into(),
            ..Default::default()
        };
        let a = ApiProfile {
            id: "one".into(),
            provider: providers::QWEN_AI_PROVIDER.into(),
            ..Default::default()
        };
        let b = ApiProfile {
            id: "two".into(),
            workspace_id: Some("another".into()),
            ..a.clone()
        };
        assert!(Arc::ptr_eq(
            &gate(&config, &a).unwrap(),
            &gate(&config, &b).unwrap()
        ));
        assert!(gate(&AsrConfig::default(), &a).is_none());
        assert!(!is_rate_limit("1007 malformed audio"));
        assert!(!is_rate_limit("network timeout"));
    }
}
