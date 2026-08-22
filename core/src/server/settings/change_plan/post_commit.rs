use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::config::AppConfig;
use crate::credentials;
use crate::db::conversations::publish_latest_catalog;

use super::super::super::{capture, db_call, SettingsContext};

pub(super) struct PostCommitUpdates {
    storage_quota_changed: bool,
    vrcx_changed: bool,
    vr_overlay_changed: bool,
}

impl PostCommitUpdates {
    pub(super) fn between(current: &AppConfig, candidate: &AppConfig) -> Self {
        Self {
            storage_quota_changed: candidate.storage.subtitle_history_max_bytes
                != current.storage.subtitle_history_max_bytes,
            vrcx_changed: candidate.vrcx != current.vrcx,
            vr_overlay_changed: candidate.vr_overlay != current.vr_overlay,
        }
    }

    pub(super) async fn apply(
        &self,
        state: &SettingsContext,
        candidate: &AppConfig,
        effective_candidate: &AppConfig,
    ) {
        let glossary_refresh_ids = state
            .content
            .glossary
            .set_config(candidate.glossary.clone());
        *state.config.config.write().expect("config lock") = candidate.clone();

        if self.storage_quota_changed {
            apply_storage_quota(state, candidate.storage.subtitle_history_max_bytes).await;
        }
        state.integrations.osc.update_config(
            if state.capture.capture_requested.load(Ordering::SeqCst) {
                effective_candidate.osc.clone()
            } else {
                candidate.osc.clone()
            },
        );
        state
            .integrations
            .vrchat_mute_sync
            .update_enabled(candidate.osc.mute_sync_enabled);
        if self.vrcx_changed {
            let token = credentials::read_vrcx_token().unwrap_or_else(|error| {
                tracing::warn!(%error, "VRCX-0 token could not be read after settings update");
                None
            });
            state
                .integrations
                .vrcx
                .reconfigure(candidate.vrcx.clone(), token)
                .await;
        }
        refresh_glossaries(state, glossary_refresh_ids);
        if self.vr_overlay_changed {
            state
                .integrations
                .vr_overlay_config_tx
                .send_replace(candidate.vr_overlay.clone());
        }
    }
}

async fn apply_storage_quota(state: &SettingsContext, max_bytes: u64) {
    let conversation_catalog = state.content.conversation_catalog_tx.clone();
    if let Err(error) = db_call(Arc::clone(&state.content.db), move |db| {
        if db.set_subtitle_history_max_bytes(max_bytes)? {
            publish_latest_catalog(db, &conversation_catalog);
        }
        Ok(())
    })
    .await
    {
        tracing::warn!(%error, "subtitle history storage quota could not be enforced immediately");
    }
}

fn refresh_glossaries(state: &SettingsContext, glossary_refresh_ids: Vec<String>) {
    if glossary_refresh_ids.is_empty() {
        return;
    }
    let glossary = Arc::clone(&state.content.glossary);
    let state = state.clone();
    tokio::spawn(async move {
        let mut refreshed = false;
        for id in glossary_refresh_ids {
            match glossary.refresh(&id).await {
                Ok(updated) => refreshed |= updated,
                Err(error) => {
                    tracing::warn!(subscription_id = %id, code = error.code, detail = %error.detail, "glossary subscription refresh failed");
                }
            }
        }
        if refreshed {
            if let Err((_, body)) = capture::reload_glossary_asr_context(&state).await {
                tracing::warn!(detail = ?body, "ASR glossary context could not be reloaded");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_changed_runtime_notifications_are_marked() {
        let current = AppConfig::default();
        let mut candidate = current.clone();
        candidate.storage.subtitle_history_max_bytes += 1;
        candidate.vrcx.enabled = !candidate.vrcx.enabled;

        let updates = PostCommitUpdates::between(&current, &candidate);

        assert!(updates.storage_quota_changed);
        assert!(updates.vrcx_changed);
        assert!(!updates.vr_overlay_changed);
    }
}
