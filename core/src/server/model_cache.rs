use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config::ApiProfile;

const TTL: Duration = Duration::from_secs(600);
const MAX_ENTRIES: usize = 128;

struct Entry {
    profile: ApiProfile,
    credential_hash: [u8; 32],
    created: Instant,
    catalog: Value,
}

#[derive(Default)]
pub(super) struct ModelCatalogCache(Mutex<HashMap<(String, String), Entry>>);

impl ModelCatalogCache {
    pub(super) fn get(&self, profile: &ApiProfile, service: &str, api_key: &str) -> Option<Value> {
        let mut entries = self.0.lock().expect("model catalog cache lock");
        entries.retain(|_, entry| entry.created.elapsed() < TTL);
        let entry = entries.get(&(profile.id.clone(), service.to_owned()))?;
        (entry.profile == *profile
            && entry.credential_hash == <[u8; 32]>::from(Sha256::digest(api_key)))
        .then(|| entry.catalog.clone())
    }

    pub(super) fn insert(
        &self,
        profile: &ApiProfile,
        service: &str,
        api_key: &str,
        catalog: Value,
    ) {
        let mut entries = self.0.lock().expect("model catalog cache lock");
        entries.retain(|_, entry| entry.created.elapsed() < TTL);
        if entries.len() >= MAX_ENTRIES {
            if let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.created)
                .map(|(key, _)| key.clone())
            {
                entries.remove(&oldest);
            }
        }
        entries.insert(
            (profile.id.clone(), service.to_owned()),
            Entry {
                profile: profile.clone(),
                credential_hash: Sha256::digest(api_key).into(),
                created: Instant::now(),
                catalog,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cached_models_require_the_same_connection_service_and_credential() {
        let cache = ModelCatalogCache::default();
        let mut profile = ApiProfile {
            id: "profile".into(),
            ..Default::default()
        };
        cache.insert(
            &profile,
            "chat",
            "first-key",
            json!({ "models": ["model"] }),
        );
        assert!(cache.get(&profile, "chat", "first-key").is_some());
        assert!(cache.get(&profile, "chat", "second-key").is_none());
        assert!(cache.get(&profile, "other", "first-key").is_none());
        profile.base_url = Some("https://other.example/v1".into());
        assert!(cache.get(&profile, "chat", "first-key").is_none());
    }

    #[test]
    fn expired_catalogs_are_not_reused() {
        let cache = ModelCatalogCache::default();
        let profile = ApiProfile {
            id: "profile".into(),
            ..Default::default()
        };
        cache.insert(&profile, "chat", "", json!({ "models": ["model"] }));
        cache.0.lock().unwrap().values_mut().next().unwrap().created -= TTL;
        assert!(cache.get(&profile, "chat", "").is_none());
    }
}
