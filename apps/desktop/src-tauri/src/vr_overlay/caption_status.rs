use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CaptionLabels {
    pub me: String,
    pub other: String,
}

impl Default for CaptionLabels {
    fn default() -> Self {
        Self {
            me: "Me".into(),
            other: "Others".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CaptionNotice {
    pub key: String,
    pub text: String,
    pub duration_ms: Option<u64>,
}

#[derive(Default)]
pub struct CaptionStatus {
    notice: Option<CaptionNotice>,
    expires: Option<Instant>,
}

impl CaptionStatus {
    pub fn update(&mut self, notice: Option<CaptionNotice>, now: Instant) {
        if self.notice.as_ref().map(|value| &value.key) != notice.as_ref().map(|value| &value.key) {
            self.expires = notice
                .as_ref()
                .and_then(|value| value.duration_ms)
                .map(|ms| now + Duration::from_millis(ms));
        }
        self.notice = notice;
    }

    pub fn text(&self, now: Instant) -> Option<String> {
        self.notice
            .as_ref()
            .filter(|_| self.expires.is_none_or(|expires| now < expires))
            .map(|notice| notice.text.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_persists_success_expires_and_health_refresh_does_not_replay_it() {
        let now = Instant::now();
        let mut state = CaptionStatus::default();
        let notice = |key: &str, duration_ms| {
            Some(CaptionNotice {
                key: key.into(),
                text: key.into(),
                duration_ms,
            })
        };
        state.update(notice("waiting_game", None), now);
        assert_eq!(
            state.text(now + Duration::from_secs(100)).as_deref(),
            Some("waiting_game")
        );
        state.update(notice("ready", Some(5000)), now);
        state.update(notice("ready", Some(5000)), now + Duration::from_secs(4));
        assert!(state.text(now + Duration::from_secs(6)).is_none());
        state.update(notice("failed", None), now + Duration::from_secs(7));
        assert_eq!(
            state.text(now + Duration::from_secs(100)).as_deref(),
            Some("failed")
        );
        state.update(None, now);
        assert!(state.text(now).is_none());
    }
}
