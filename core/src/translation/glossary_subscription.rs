use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config::{
    validate_glossary_source_url, GlossaryCategory, GlossaryEntry, GlossarySource,
    TranslationPromptConfig,
};

const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_GLOSSARY_ENTRIES: usize = 500;

#[derive(Debug)]
pub struct GlossarySubscriptionError {
    pub code: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GlossaryStatus {
    pub id: String,
    #[serde(rename = "type")]
    pub source_type: String,
    pub name: String,
    pub enabled: bool,
    pub url: Option<String>,
    pub state: String,
    pub entry_count: usize,
    pub effective_entry_count: usize,
    pub omitted_entry_count: usize,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub error_code: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LegacyGlossarySubscriptionStatus {
    pub configured: bool,
    pub url: Option<String>,
    pub state: String,
    pub source_name: Option<String>,
    pub entry_count: usize,
    pub effective_entry_count: usize,
    pub omitted_entry_count: usize,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub error_code: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheFile {
    subscriptions: Vec<CacheEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    id: String,
    url: String,
    source_name: Option<String>,
    entries: Vec<GlossaryEntry>,
    etag: Option<String>,
    last_modified: Option<String>,
    last_success_at: String,
}

#[derive(Debug, Deserialize)]
struct LegacyCacheFile {
    url: String,
    source_name: Option<String>,
    entries: Vec<GlossaryEntry>,
    etag: Option<String>,
    last_modified: Option<String>,
    last_success_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CacheDocument {
    Multiple(CacheFile),
    Single(LegacyCacheFile),
}

#[derive(Debug, Clone)]
struct SubscriptionState {
    id: String,
    url: String,
    display_name: Option<String>,
    enabled: bool,
    source_name: Option<String>,
    entries: Vec<GlossaryEntry>,
    etag: Option<String>,
    last_modified: Option<String>,
    state: String,
    last_attempt_at: Option<String>,
    last_success_at: Option<String>,
    error_code: Option<String>,
    detail: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteGlossary {
    version: u32,
    #[serde(default)]
    name: Option<String>,
    entries: Vec<RemoteGlossaryEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteGlossaryEntry {
    source: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    category: GlossaryCategory,
    #[serde(default)]
    case_sensitive: bool,
}

enum FetchResult {
    NotModified,
    Updated {
        source_name: Option<String>,
        entries: Vec<GlossaryEntry>,
        etag: Option<String>,
        last_modified: Option<String>,
    },
}

pub struct GlossarySubscriptionStore {
    cache_path: PathBuf,
    client: reqwest::Client,
    subscriptions: RwLock<Vec<SubscriptionState>>,
    refresh_control: Mutex<()>,
}

impl GlossarySubscriptionStore {
    pub fn new(cache_path: PathBuf, glossary_sources: Vec<GlossarySource>) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 3
                    || validate_glossary_source_url(attempt.url().as_str()).is_err()
                {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|error| format!("Failed to create glossary HTTP client: {error}"))?;
        let cached = load_cache(&cache_path, &glossary_sources);
        let subscriptions = subscription_configs(&glossary_sources)
            .into_iter()
            .map(|(id, url, display_name, enabled)| {
                if let Some(cache) = cached.get(&id).filter(|cache| cache.url == url) {
                    SubscriptionState {
                        id,
                        url,
                        display_name,
                        enabled,
                        source_name: cache.source_name.clone(),
                        entries: cache.entries.clone(),
                        etag: cache.etag.clone(),
                        last_modified: cache.last_modified.clone(),
                        state: if enabled { "ready" } else { "disabled" }.into(),
                        last_attempt_at: None,
                        last_success_at: Some(cache.last_success_at.clone()),
                        error_code: None,
                        detail: None,
                    }
                } else {
                    empty_subscription_state(id, url, display_name, enabled)
                }
            })
            .collect();
        Ok(Self {
            cache_path,
            client,
            subscriptions: RwLock::new(subscriptions),
            refresh_control: Mutex::new(()),
        })
    }

    pub fn set_sources(&self, glossary_sources: Vec<GlossarySource>) -> Vec<String> {
        let mut current = self
            .subscriptions
            .write()
            .expect("glossary subscription lock");
        let mut previous = current
            .drain(..)
            .map(|state| (state.id.clone(), state))
            .collect::<HashMap<_, _>>();
        let mut refresh_ids = Vec::new();
        let mut next = Vec::new();
        for (id, url, display_name, enabled) in subscription_configs(&glossary_sources) {
            let state = match previous.remove(&id) {
                Some(mut state) if state.url == url => {
                    let was_enabled = state.enabled;
                    state.display_name = display_name;
                    state.enabled = enabled;
                    if !enabled {
                        state.state = "disabled".into();
                    } else if !was_enabled {
                        state.state = if state.last_success_at.is_some() {
                            "ready"
                        } else {
                            "idle"
                        }
                        .into();
                        state.error_code = None;
                        state.detail = None;
                    }
                    state
                }
                _ => {
                    if enabled {
                        refresh_ids.push(id.clone());
                    }
                    empty_subscription_state(id, url, display_name, enabled)
                }
            };
            next.push(state);
        }
        *current = next;
        drop(current);
        self.save_cache();
        refresh_ids
    }

    pub fn configured_ids(&self) -> Vec<String> {
        self.subscriptions
            .read()
            .expect("glossary subscription lock")
            .iter()
            .map(|state| state.id.clone())
            .collect()
    }

    pub fn merged_prompt(&self, prompt: &TranslationPromptConfig) -> TranslationPromptConfig {
        let states = self.subscription_snapshot();
        let mut merged = prompt.clone();
        merged.glossary.clear();
        let mut keys = HashSet::new();
        for source in &prompt.glossary_sources {
            let entries = effective_source_entries(source, &states);
            append_effective_entries(&mut merged.glossary, &mut keys, entries);
            if merged.glossary.len() >= MAX_GLOSSARY_ENTRIES {
                break;
            }
        }
        merged
    }

    pub fn statuses(&self, prompt: &TranslationPromptConfig) -> Vec<GlossaryStatus> {
        let states = self.subscription_snapshot();
        let mut keys = HashSet::new();
        let mut effective_total = 0;
        prompt
            .glossary_sources
            .iter()
            .map(|source| match source {
                GlossarySource::Local {
                    id,
                    name,
                    enabled,
                    entries,
                } => {
                    let effective_entry_count = if *enabled {
                        count_effective_entries(entries, &mut keys, &mut effective_total)
                    } else {
                        0
                    };
                    GlossaryStatus {
                        id: id.clone(),
                        source_type: "local".into(),
                        name: name.clone(),
                        enabled: *enabled,
                        url: None,
                        state: if *enabled { "ready" } else { "disabled" }.into(),
                        entry_count: entries.len(),
                        effective_entry_count,
                        omitted_entry_count: if *enabled {
                            entries.len().saturating_sub(effective_entry_count)
                        } else {
                            0
                        },
                        last_attempt_at: None,
                        last_success_at: None,
                        error_code: None,
                        detail: None,
                    }
                }
                GlossarySource::Subscription {
                    id,
                    url,
                    display_name,
                    enabled,
                } => {
                    let state = states.get(id).filter(|state| state.url == url.trim());
                    let entries = state.map(|state| state.entries.as_slice()).unwrap_or(&[]);
                    let effective_entry_count = if *enabled {
                        count_effective_entries(entries, &mut keys, &mut effective_total)
                    } else {
                        0
                    };
                    GlossaryStatus {
                        id: id.clone(),
                        source_type: "subscription".into(),
                        name: subscription_name(display_name.as_deref(), state),
                        enabled: *enabled,
                        url: Some(url.clone()),
                        state: if !enabled {
                            "disabled".into()
                        } else {
                            state
                                .map(|state| state.state.clone())
                                .unwrap_or_else(|| "idle".into())
                        },
                        entry_count: entries.len(),
                        effective_entry_count,
                        omitted_entry_count: if *enabled {
                            entries.len().saturating_sub(effective_entry_count)
                        } else {
                            0
                        },
                        last_attempt_at: state.and_then(|state| state.last_attempt_at.clone()),
                        last_success_at: state.and_then(|state| state.last_success_at.clone()),
                        error_code: state.and_then(|state| state.error_code.clone()),
                        detail: state.and_then(|state| state.detail.clone()),
                    }
                }
            })
            .collect()
    }

    pub fn legacy_status(
        &self,
        prompt: &TranslationPromptConfig,
    ) -> LegacyGlossarySubscriptionStatus {
        let status = self
            .statuses(prompt)
            .into_iter()
            .find(|status| status.source_type == "subscription");
        match status {
            Some(status) => LegacyGlossarySubscriptionStatus {
                configured: true,
                url: status.url,
                state: status.state,
                source_name: Some(status.name),
                entry_count: status.entry_count,
                effective_entry_count: status.effective_entry_count,
                omitted_entry_count: status.omitted_entry_count,
                last_attempt_at: status.last_attempt_at,
                last_success_at: status.last_success_at,
                error_code: status.error_code,
                detail: status.detail,
            },
            None => LegacyGlossarySubscriptionStatus {
                configured: false,
                url: None,
                state: "unconfigured".into(),
                source_name: None,
                entry_count: 0,
                effective_entry_count: 0,
                omitted_entry_count: 0,
                last_attempt_at: None,
                last_success_at: None,
                error_code: None,
                detail: None,
            },
        }
    }

    pub async fn refresh(&self, id: &str) -> Result<(), GlossarySubscriptionError> {
        let _refresh = self.refresh_control.lock().await;
        let (url, etag, last_modified) = {
            let mut subscriptions = self
                .subscriptions
                .write()
                .expect("glossary subscription lock");
            let Some(state) = subscriptions.iter_mut().find(|state| state.id == id) else {
                return Err(subscription_error(
                    "glossary_subscription.not_found",
                    format!("Glossary subscription was not found: {id}"),
                ));
            };
            if !state.enabled {
                return Err(subscription_error(
                    "glossary_subscription.disabled",
                    format!("Glossary subscription is disabled: {id}"),
                ));
            }
            state.state = "refreshing".into();
            state.last_attempt_at = Some(now());
            state.error_code = None;
            state.detail = None;
            (
                state.url.clone(),
                state.etag.clone(),
                state.last_modified.clone(),
            )
        };
        let result = self
            .fetch(&url, etag.as_deref(), last_modified.as_deref())
            .await;
        match result {
            Ok(FetchResult::NotModified) => {
                let mut subscriptions = self
                    .subscriptions
                    .write()
                    .expect("glossary subscription lock");
                let Some(state) = matching_state_mut(&mut subscriptions, id, &url) else {
                    return Ok(());
                };
                state.state = "ready".into();
                state.last_success_at = Some(now());
                drop(subscriptions);
                self.save_cache();
                Ok(())
            }
            Ok(FetchResult::Updated {
                source_name,
                entries,
                etag,
                last_modified,
            }) => {
                let mut subscriptions = self
                    .subscriptions
                    .write()
                    .expect("glossary subscription lock");
                let Some(state) = matching_state_mut(&mut subscriptions, id, &url) else {
                    return Ok(());
                };
                state.source_name = source_name;
                state.entries = entries;
                state.etag = etag;
                state.last_modified = last_modified;
                state.state = "ready".into();
                state.last_success_at = Some(now());
                drop(subscriptions);
                self.save_cache();
                Ok(())
            }
            Err(error) => {
                let mut subscriptions = self
                    .subscriptions
                    .write()
                    .expect("glossary subscription lock");
                let Some(state) = matching_state_mut(&mut subscriptions, id, &url) else {
                    return Ok(());
                };
                state.state = if state.last_success_at.is_some() {
                    "stale".into()
                } else {
                    "error".into()
                };
                state.error_code = Some(error.code.into());
                state.detail = Some(error.detail.clone());
                Err(error)
            }
        }
    }

    pub async fn refresh_all(&self) {
        let ids = self
            .subscriptions
            .read()
            .expect("glossary subscription lock")
            .iter()
            .filter(|state| state.enabled)
            .map(|state| state.id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            if let Err(error) = self.refresh(&id).await {
                tracing::warn!(subscription_id = %id, code = error.code, detail = %error.detail, "glossary subscription refresh failed");
            }
        }
    }

    async fn fetch(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<FetchResult, GlossarySubscriptionError> {
        validate_glossary_source_url(url)
            .map_err(|detail| subscription_error("glossary_subscription.invalid_url", detail))?;
        let mut request = self.client.get(url);
        if let Some(etag) = etag {
            request = request.header(IF_NONE_MATCH, etag);
        }
        if let Some(last_modified) = last_modified {
            request = request.header(IF_MODIFIED_SINCE, last_modified);
        }
        let response = request.send().await.map_err(|error| {
            subscription_error("glossary_subscription.fetch_failed", error.to_string())
        })?;
        validate_glossary_source_url(response.url().as_str()).map_err(|detail| {
            subscription_error("glossary_subscription.invalid_redirect", detail)
        })?;
        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(FetchResult::NotModified);
        }
        if !response.status().is_success() {
            return Err(subscription_error(
                "glossary_subscription.http_error",
                format!("Glossary source returned HTTP {}", response.status()),
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_SOURCE_BYTES as u64)
        {
            return Err(subscription_error(
                "glossary_subscription.too_large",
                "Glossary source exceeds 1 MiB",
            ));
        }
        let headers = response.headers().clone();
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                subscription_error("glossary_subscription.fetch_failed", error.to_string())
            })?;
            if body.len() + chunk.len() > MAX_SOURCE_BYTES {
                return Err(subscription_error(
                    "glossary_subscription.too_large",
                    "Glossary source exceeds 1 MiB",
                ));
            }
            body.extend_from_slice(&chunk);
        }
        let document: RemoteGlossary = serde_json::from_slice(&body).map_err(|error| {
            subscription_error(
                "glossary_subscription.invalid_json",
                format!("Glossary JSON is invalid: {error}"),
            )
        })?;
        if document.version != 1 {
            return Err(subscription_error(
                "glossary_subscription.unsupported_version",
                format!("Unsupported glossary version: {}", document.version),
            ));
        }
        let entries = document
            .entries
            .into_iter()
            .map(|entry| GlossaryEntry {
                source: entry.source,
                target: entry.target,
                category: entry.category,
                case_sensitive: entry.case_sensitive,
            })
            .collect::<Vec<_>>();
        validate_remote_entries(&entries)?;
        let source_name = document
            .name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        if source_name
            .as_ref()
            .is_some_and(|name| name.chars().count() > 100)
        {
            return Err(subscription_error(
                "glossary_subscription.invalid_json",
                "Glossary name cannot exceed 100 characters",
            ));
        }
        Ok(FetchResult::Updated {
            source_name,
            entries,
            etag: header_text(&headers, ETAG),
            last_modified: header_text(&headers, LAST_MODIFIED),
        })
    }

    fn subscription_snapshot(&self) -> HashMap<String, SubscriptionState> {
        self.subscriptions
            .read()
            .expect("glossary subscription lock")
            .iter()
            .cloned()
            .map(|state| (state.id.clone(), state))
            .collect()
    }

    fn save_cache(&self) {
        let cache = CacheFile {
            subscriptions: self
                .subscriptions
                .read()
                .expect("glossary subscription lock")
                .iter()
                .filter_map(|state| {
                    Some(CacheEntry {
                        id: state.id.clone(),
                        url: state.url.clone(),
                        source_name: state.source_name.clone(),
                        entries: state.entries.clone(),
                        etag: state.etag.clone(),
                        last_modified: state.last_modified.clone(),
                        last_success_at: state.last_success_at.clone()?,
                    })
                })
                .collect(),
        };
        if cache.subscriptions.is_empty() {
            let _ = fs::remove_file(&self.cache_path);
            return;
        }
        let Ok(payload) = serde_json::to_vec_pretty(&cache) else {
            return;
        };
        if let Some(parent) = self.cache_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(error) = fs::write(&self.cache_path, payload) {
            tracing::warn!(%error, "glossary subscription cache could not be saved");
        }
    }
}

fn subscription_configs(
    glossary_sources: &[GlossarySource],
) -> Vec<(String, String, Option<String>, bool)> {
    glossary_sources
        .iter()
        .filter_map(|source| match source {
            GlossarySource::Subscription {
                id,
                url,
                display_name,
                enabled,
            } => Some((
                id.clone(),
                url.trim().to_owned(),
                display_name.clone(),
                *enabled,
            )),
            GlossarySource::Local { .. } => None,
        })
        .collect()
}

fn empty_subscription_state(
    id: String,
    url: String,
    display_name: Option<String>,
    enabled: bool,
) -> SubscriptionState {
    SubscriptionState {
        id,
        url,
        display_name,
        enabled,
        source_name: None,
        entries: Vec::new(),
        etag: None,
        last_modified: None,
        state: if enabled { "idle" } else { "disabled" }.into(),
        last_attempt_at: None,
        last_success_at: None,
        error_code: None,
        detail: None,
    }
}

fn matching_state_mut<'a>(
    subscriptions: &'a mut [SubscriptionState],
    id: &str,
    url: &str,
) -> Option<&'a mut SubscriptionState> {
    subscriptions
        .iter_mut()
        .find(|state| state.id == id && state.url == url)
}

fn effective_source_entries<'a>(
    source: &'a GlossarySource,
    states: &'a HashMap<String, SubscriptionState>,
) -> &'a [GlossaryEntry] {
    match source {
        GlossarySource::Local {
            enabled: true,
            entries,
            ..
        } => entries,
        GlossarySource::Subscription {
            id,
            url,
            enabled: true,
            ..
        } => states
            .get(id)
            .filter(|state| state.url == url.trim() && state.enabled)
            .map(|state| state.entries.as_slice())
            .unwrap_or(&[]),
        _ => &[],
    }
}

fn append_effective_entries(
    destination: &mut Vec<GlossaryEntry>,
    keys: &mut HashSet<(String, bool)>,
    entries: &[GlossaryEntry],
) {
    for entry in entries {
        if destination.len() >= MAX_GLOSSARY_ENTRIES {
            break;
        }
        if keys.insert(entry_key(entry)) {
            destination.push(entry.clone());
        }
    }
}

fn count_effective_entries(
    entries: &[GlossaryEntry],
    keys: &mut HashSet<(String, bool)>,
    effective_total: &mut usize,
) -> usize {
    let mut applied = 0;
    for entry in entries {
        if *effective_total >= MAX_GLOSSARY_ENTRIES {
            break;
        }
        if keys.insert(entry_key(entry)) {
            *effective_total += 1;
            applied += 1;
        }
    }
    applied
}

fn subscription_name(display_name: Option<&str>, state: Option<&SubscriptionState>) -> String {
    display_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .or_else(|| state.and_then(|state| state.source_name.clone()))
        .unwrap_or_else(|| "Subscription glossary".into())
}

fn load_cache(path: &PathBuf, glossary_sources: &[GlossarySource]) -> HashMap<String, CacheEntry> {
    let Ok(bytes) = fs::read(path) else {
        return HashMap::new();
    };
    let Ok(document) = serde_json::from_slice::<CacheDocument>(&bytes) else {
        return HashMap::new();
    };
    let configured = subscription_configs(glossary_sources);
    let entries = match document {
        CacheDocument::Multiple(cache) => cache.subscriptions,
        CacheDocument::Single(cache) => {
            let Some((id, _, _, _)) = configured.iter().find(|(_, url, _, _)| *url == cache.url)
            else {
                return HashMap::new();
            };
            vec![CacheEntry {
                id: id.clone(),
                url: cache.url,
                source_name: cache.source_name,
                entries: cache.entries,
                etag: cache.etag,
                last_modified: cache.last_modified,
                last_success_at: cache.last_success_at,
            }]
        }
    };
    let configured = configured
        .into_iter()
        .map(|(id, url, _, _)| (id, url))
        .collect::<HashMap<_, _>>();
    entries
        .into_iter()
        .filter(|cache| {
            configured.get(&cache.id) == Some(&cache.url)
                && validate_remote_entries(&cache.entries).is_ok()
        })
        .map(|cache| (cache.id.clone(), cache))
        .collect()
}

fn validate_remote_entries(entries: &[GlossaryEntry]) -> Result<(), GlossarySubscriptionError> {
    if entries.len() > MAX_GLOSSARY_ENTRIES {
        return Err(subscription_error(
            "glossary_subscription.too_many_entries",
            "Glossary source cannot exceed 500 entries",
        ));
    }
    let mut keys = HashSet::new();
    for entry in entries {
        let source = entry.source.trim();
        if source.is_empty() || source.chars().count() > 200 || contains_control(source) {
            return Err(subscription_error(
                "glossary_subscription.invalid_entry",
                "Each glossary source term must contain 1 to 200 single-line characters",
            ));
        }
        if entry
            .target
            .as_deref()
            .is_some_and(|target| target.chars().count() > 200 || contains_control(target))
        {
            return Err(subscription_error(
                "glossary_subscription.invalid_entry",
                "Each glossary target must contain at most 200 single-line characters",
            ));
        }
        if !keys.insert(entry_key(entry)) {
            return Err(subscription_error(
                "glossary_subscription.duplicate_entry",
                format!("Glossary source contains a duplicate term: {source}"),
            ));
        }
    }
    Ok(())
}

fn contains_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

fn entry_key(entry: &GlossaryEntry) -> (String, bool) {
    let source = entry.source.trim();
    (
        if entry.case_sensitive {
            source.to_owned()
        } else {
            source.to_lowercase()
        },
        entry.case_sensitive,
    )
}

fn header_text(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn subscription_error(code: &'static str, detail: impl Into<String>) -> GlossarySubscriptionError {
    GlossarySubscriptionError {
        code,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Json, Router};
    use serde_json::json;

    fn entry(source: &str, target: Option<&str>) -> GlossaryEntry {
        GlossaryEntry {
            source: source.into(),
            target: target.map(str::to_owned),
            category: GlossaryCategory::Custom,
            case_sensitive: false,
        }
    }

    fn local(id: &str, entries: Vec<GlossaryEntry>) -> GlossarySource {
        GlossarySource::Local {
            id: id.into(),
            name: id.into(),
            enabled: true,
            entries,
        }
    }

    fn subscription(id: &str, url: String, enabled: bool) -> GlossarySource {
        GlossarySource::Subscription {
            id: id.into(),
            url,
            display_name: None,
            enabled,
        }
    }

    #[test]
    fn validates_remote_entries_and_rejects_duplicates() {
        assert!(validate_remote_entries(&[entry("VRChat", None)]).is_ok());
        let error =
            validate_remote_entries(&[entry("VRChat", None), entry("vrchat", Some("VRChat"))])
                .unwrap_err();
        assert_eq!(error.code, "glossary_subscription.duplicate_entry");
    }

    #[test]
    fn merges_sources_in_priority_order_and_reports_omissions() {
        let directory = tempfile::tempdir().unwrap();
        let sources = vec![
            local("local", vec![entry("VRChat", Some("local"))]),
            subscription("remote", "https://example.com/glossary.json".into(), true),
            local("last", vec![entry("Udon", Some("last"))]),
        ];
        let store =
            GlossarySubscriptionStore::new(directory.path().join("cache.json"), sources.clone())
                .unwrap();
        store.subscriptions.write().unwrap()[0].entries = vec![
            entry("vrchat", Some("remote")),
            entry("Udon", Some("remote")),
        ];
        let prompt = TranslationPromptConfig {
            glossary_sources: sources,
            glossary: vec![entry("runtime", None)],
            ..TranslationPromptConfig::default()
        };

        let merged = store.merged_prompt(&prompt);
        let statuses = store.statuses(&prompt);

        assert_eq!(merged.glossary.len(), 2);
        assert_eq!(merged.glossary[0].target.as_deref(), Some("local"));
        assert_eq!(merged.glossary[1].target.as_deref(), Some("remote"));
        assert_eq!(statuses[0].effective_entry_count, 1);
        assert_eq!(statuses[1].effective_entry_count, 1);
        assert_eq!(statuses[1].omitted_entry_count, 1);
        assert_eq!(statuses[2].effective_entry_count, 0);
        assert_eq!(statuses[2].omitted_entry_count, 1);
    }

    #[test]
    fn set_sources_returns_enabled_new_and_changed_subscription_ids() {
        let directory = tempfile::tempdir().unwrap();
        let store = GlossarySubscriptionStore::new(
            directory.path().join("cache.json"),
            vec![subscription(
                "same",
                "https://example.com/one.json".into(),
                true,
            )],
        )
        .unwrap();

        let refresh = store.set_sources(vec![
            subscription("same", "https://example.com/two.json".into(), true),
            subscription(
                "disabled",
                "https://example.com/disabled.json".into(),
                false,
            ),
            subscription("new", "https://example.com/new.json".into(), true),
        ]);

        assert_eq!(refresh, ["same", "new"]);
        assert_eq!(store.configured_ids(), ["same", "disabled", "new"]);
    }

    #[tokio::test]
    async fn refresh_all_caches_multiple_subscriptions() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route(
                        "/one.json",
                        get(|| async {
                            Json(json!({
                                "version": 1,
                                "name": "One",
                                "entries": [{"source": "VRChat"}]
                            }))
                        }),
                    )
                    .route(
                        "/two.json",
                        get(|| async {
                            Json(json!({
                                "version": 1,
                                "name": "Two",
                                "entries": [{"source": "Udon"}]
                            }))
                        }),
                    ),
            )
            .await
            .unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("cache.json");
        let sources = vec![
            subscription("one", format!("http://{address}/one.json"), true),
            subscription("two", format!("http://{address}/two.json"), true),
        ];
        let store = GlossarySubscriptionStore::new(cache_path.clone(), sources.clone()).unwrap();

        store.refresh_all().await;

        let prompt = TranslationPromptConfig {
            glossary_sources: sources.clone(),
            ..TranslationPromptConfig::default()
        };
        let statuses = store.statuses(&prompt);
        assert_eq!(statuses.len(), 2);
        assert!(statuses.iter().all(|status| status.state == "ready"));
        let cache: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache_path).unwrap()).unwrap();
        assert_eq!(cache["subscriptions"].as_array().unwrap().len(), 2);

        let reloaded = GlossarySubscriptionStore::new(cache_path, sources).unwrap();
        assert!(reloaded
            .statuses(&prompt)
            .iter()
            .all(|status| status.state == "ready" && status.entry_count == 1));
        server.abort();
    }

    #[test]
    fn loads_the_legacy_single_subscription_cache() {
        let directory = tempfile::tempdir().unwrap();
        let cache_path = directory.path().join("cache.json");
        fs::write(
            &cache_path,
            serde_json::to_vec(&json!({
                "url": "https://example.com/glossary.json",
                "source_name": "Legacy",
                "entries": [{
                    "source": "VRChat",
                    "target": null,
                    "category": "custom",
                    "case_sensitive": false
                }],
                "etag": null,
                "last_modified": null,
                "last_success_at": "2026-08-15T00:00:00Z"
            }))
            .unwrap(),
        )
        .unwrap();
        let source = subscription(
            "legacy-subscription",
            "https://example.com/glossary.json".into(),
            true,
        );
        let store = GlossarySubscriptionStore::new(cache_path, vec![source.clone()]).unwrap();
        let prompt = TranslationPromptConfig {
            glossary_sources: vec![source],
            ..TranslationPromptConfig::default()
        };

        let status = store.statuses(&prompt).remove(0);

        assert_eq!(status.state, "ready");
        assert_eq!(status.name, "Legacy");
        assert_eq!(status.entry_count, 1);
    }
}
