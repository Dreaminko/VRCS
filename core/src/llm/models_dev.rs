//! Public model facts. This cache never owns connection settings or credentials.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::watch;

use super::model_catalog::{
    modalities, positive_limit, ModelCatalog, ModelDetails, ReasoningControl, ReasoningOptions,
};
use crate::config::ApiProfile;

const URL: &str = "https://models.dev/api.json";
const MAX_BYTES: usize = 16 * 1024 * 1024;
const PROVIDERS: &[&str] = &[
    "openai",
    "google",
    "groq",
    "deepseek",
    "openrouter",
    "alibaba",
    "alibaba-cn",
    "alibaba-token-plan-cn",
];
static STORE: OnceLock<Arc<Store>> = OnceLock::new();

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn provider_id(profile: &ApiProfile) -> Option<&'static str> {
    match profile.provider.as_str() {
        "openai" => Some("openai"),
        "gemini" => Some("google"),
        "groq" => Some("groq"),
        "deepseek" => Some("deepseek"),
        "openrouter" => Some("openrouter"),
        "alibaba_cloud" => match profile.region.as_deref() {
            Some("singapore") => Some("alibaba"),
            Some("china_beijing") => Some("alibaba-cn"),
            _ => None,
        },
        "alibaba_token_plan" => Some("alibaba-token-plan-cn"),
        _ => None,
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Snapshot {
    version: u8,
    updated_at: i64,
    checked_at: i64,
    etag: Option<String>,
    providers: BTreeMap<String, BTreeMap<String, ModelDetails>>,
}

impl Snapshot {
    fn parse(value: &Value, timestamp: i64, etag: Option<String>) -> Result<Self, String> {
        let root = value
            .get("providers")
            .unwrap_or(value)
            .as_object()
            .ok_or("Invalid catalog root")?;
        let mut providers = BTreeMap::new();
        let mut count = 0;
        for (id, provider) in root {
            let Some(models) = provider.get("models").and_then(Value::as_object) else {
                continue;
            };
            count += models.len();
            if count > 100_000 {
                return Err("Catalog entry limit exceeded".into());
            }
            if !PROVIDERS.contains(&id.as_str()) {
                continue;
            }
            let mut details = BTreeMap::new();
            for (model, entry) in models {
                if model.is_empty() || model.len() > 200 || !entry.is_object() {
                    continue;
                }
                let info = ModelDetails {
                    input_modalities: modalities(entry.pointer("/modalities/input")),
                    output_modalities: modalities(entry.pointer("/modalities/output")),
                    context_tokens: positive_limit(entry.pointer("/limit/context")),
                    max_output_tokens: positive_limit(entry.pointer("/limit/output")),
                    temperature: entry.get("temperature").and_then(Value::as_bool),
                    structured_output: entry.get("structured_output").and_then(Value::as_bool),
                    tool_call: entry.get("tool_call").and_then(Value::as_bool),
                    status: entry
                        .get("status")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    canonical_model_id: entry
                        .get("canonical_model_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    reasoning: entry
                        .get("reasoning")
                        .and_then(Value::as_bool)
                        .map(|supported| {
                            let controls: Vec<ReasoningControl> = if supported {
                                entry
                                    .get("reasoning_options")
                                    .and_then(Value::as_array)
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|control| {
                                        serde_json::from_value(control.clone()).ok()
                                    })
                                    .filter(|control| match control {
                                        ReasoningControl::Toggle => true,
                                        ReasoningControl::Effort { values } => {
                                            !values.is_empty()
                                                && values.iter().all(|value| {
                                                    !value.is_empty() && value.len() <= 32
                                                })
                                        }
                                        ReasoningControl::BudgetTokens { min, max } => {
                                            min.is_none_or(|min| min >= -1)
                                                && max.is_none_or(|max| {
                                                    max > 0
                                                        && min
                                                            .is_none_or(|min| min <= i64::from(max))
                                                })
                                        }
                                    })
                                    .collect()
                            } else {
                                Vec::new()
                            };
                            let efforts = controls.iter().find_map(|control| match control {
                                ReasoningControl::Effort { values } => Some(values.clone()),
                                _ => None,
                            });
                            let can_disable = controls
                                .iter()
                                .any(|control| matches!(control, ReasoningControl::Toggle))
                                .then_some(true);
                            let supports_dynamic_budget = controls
                                .iter()
                                .any(|control| {
                                    matches!(
                                        control,
                                        ReasoningControl::BudgetTokens { min: Some(-1), .. }
                                    )
                                })
                                .then_some(true);
                            ReasoningOptions {
                                supported: Some(supported),
                                controls,
                                supported_efforts: efforts,
                                can_disable,
                                supports_dynamic_budget,
                                ..Default::default()
                            }
                        }),
                    updated_at: Some(timestamp),
                    ..Default::default()
                };
                // Reuse the field merge to mark every public fact with its source.
                let mut marked = ModelDetails::default();
                marked.supplement(&info);
                details.insert(model.clone(), marked);
            }
            if !details.is_empty() {
                providers.insert(id.clone(), details);
            }
        }
        if providers.is_empty() {
            return Err("No supported providers in catalog".into());
        }
        Ok(Self {
            version: 1,
            updated_at: timestamp,
            checked_at: timestamp,
            etag,
            providers,
        })
    }

    fn lookup(&self, profile: &ApiProfile, model: &str) -> Option<ModelDetails> {
        let id = provider_id(profile)?;
        let model = model.trim();
        let model = if profile.provider == "gemini" {
            model.strip_prefix("models/").unwrap_or(model)
        } else {
            model
        };
        let mut details = self.providers.get(id)?.get(model)?.clone();
        details.stale = now().saturating_sub(self.checked_at) > 7 * 24 * 3600;
        Some(details)
    }
}

type DirectCatalog = (ApiProfile, Instant, BTreeMap<String, ModelDetails>);

struct Store {
    snapshot: RwLock<Snapshot>,
    direct: RwLock<BTreeMap<String, DirectCatalog>>,
    runtime: Mutex<Option<(PathBuf, watch::Receiver<bool>, tokio::runtime::Handle)>>,
    refresh: Mutex<RefreshState>,
}

#[derive(Default)]
struct RefreshState {
    running: bool,
    retry_at: Option<Instant>,
}

fn shared() -> &'static Arc<Store> {
    STORE.get_or_init(|| {
        let value: Value =
            serde_json::from_str(include_str!("../../assets/models-dev/catalog.json"))
                .expect("Invalid bundled models.dev snapshot");
        let timestamp = value.get("updated_at").and_then(Value::as_i64).unwrap_or(0);
        let snapshot =
            Snapshot::parse(&value, timestamp, None).expect("Empty bundled models.dev snapshot");
        Arc::new(Store {
            snapshot: RwLock::new(snapshot),
            direct: RwLock::new(BTreeMap::new()),
            runtime: Mutex::new(None),
            refresh: Mutex::new(RefreshState::default()),
        })
    })
}

pub fn initialize(config_path: &Path, profiles: &[ApiProfile], shutdown: watch::Receiver<bool>) {
    let store = shared();
    let cache_path = config_path.with_file_name("models-dev-cache.json");
    let mut current = store.snapshot.write().expect("catalog lock");
    if let Some(cache) = load_cache(&cache_path, current.updated_at) {
        *current = cache;
    }
    drop(current);
    *store.runtime.lock().expect("catalog runtime lock") =
        Some((cache_path, shutdown, tokio::runtime::Handle::current()));
    if profiles
        .iter()
        .any(|profile| provider_id(profile).is_some())
    {
        store.schedule_refresh();
    }
}

pub fn model_details(profile: &ApiProfile, model: &str) -> Option<ModelDetails> {
    provider_id(profile)?;
    let model = model.trim();
    let model = if profile.provider == "gemini" {
        model.strip_prefix("models/").unwrap_or(model)
    } else {
        model
    };
    let store = shared();
    let facts = store
        .snapshot
        .read()
        .expect("catalog lock")
        .lookup(profile, model);
    let direct = store.direct.read().expect("provider facts lock");
    let current = direct
        .get(&profile.id)
        .filter(|(saved, updated, _)| {
            saved == profile && updated.elapsed() < Duration::from_secs(300)
        })
        .and_then(|(_, _, models)| models.get(model))
        .cloned();
    match (current, facts) {
        (Some(mut current), Some(facts)) => {
            current.supplement(&facts);
            Some(current)
        }
        (Some(current), None) => Some(current),
        (None, facts) => facts,
    }
}

pub fn apply_request(body: &mut Value, profile: &ApiProfile, request: &super::LlmRequest<'_>) {
    if let Some(details) = model_details(profile, request.model) {
        details.apply_request(body, &profile.provider, request.thinking_enabled);
    }
}

pub fn supplement(profile: &ApiProfile, catalog: &mut ModelCatalog) {
    if provider_id(profile).is_none() {
        return;
    }
    let store = shared();
    if !catalog.model_details.is_empty() {
        store.direct.write().expect("provider facts lock").insert(
            profile.id.clone(),
            (
                profile.clone(),
                Instant::now(),
                catalog.model_details.clone(),
            ),
        );
    }
    store.schedule_refresh();
    let snapshot = store.snapshot.read().expect("catalog lock");
    for model in &catalog.models {
        if let Some(facts) = snapshot.lookup(profile, model) {
            catalog
                .model_details
                .entry(model.clone())
                .or_default()
                .supplement(&facts);
        }
    }
}

impl Store {
    fn schedule_refresh(self: &Arc<Self>) {
        self.schedule_refresh_from(URL);
    }

    fn schedule_refresh_from(self: &Arc<Self>, url: &str) {
        let Some((path, mut shutdown, runtime)) =
            self.runtime.lock().expect("catalog runtime lock").clone()
        else {
            return;
        };
        if *shutdown.borrow() {
            return;
        }
        let mut state = self.refresh.lock().expect("catalog refresh lock");
        if state.running || state.retry_at.is_some_and(|time| time > Instant::now()) {
            return;
        }
        if now().saturating_sub(self.snapshot.read().expect("catalog lock").checked_at) < 24 * 3600
        {
            return;
        }
        state.running = true;
        let store = Arc::clone(self);
        let url = url.to_owned();
        runtime.spawn(async move {
            let etag = store.snapshot.read().expect("catalog lock").etag.clone();
            let result = tokio::select! {
                result = fetch(&url, etag.as_deref()) => Some(result),
                _ = shutdown.changed() => None,
            };
            let success = matches!(result, Some(Ok(_)));
            if let Some(result) = result {
                match result {
                    Ok(update) => {
                        let snapshot = {
                            let mut current = store.snapshot.write().expect("catalog lock");
                            match update {
                                Some(next) => *current = next,
                                None => current.checked_at = now(),
                            }
                            current.clone()
                        };
                        if let Err(error) = save_cache(&path, &snapshot) {
                            tracing::debug!(%error, "Model catalog cache could not be saved");
                        }
                        tracing::debug!("Public model catalog updated");
                    }
                    Err(error) => {
                        tracing::debug!(%error, "Public model catalog update unavailable")
                    }
                }
            }
            let mut state = store.refresh.lock().expect("catalog refresh lock");
            state.running = false;
            state.retry_at =
                Some(Instant::now() + Duration::from_secs(if success { 24 * 3600 } else { 3600 }));
        });
    }
}

async fn fetch(url: &str, etag: Option<&str>) -> Result<Option<Snapshot>, String> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = http.get(url);
    if let Some(etag) = etag {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("Catalog HTTP status {}", response.status()));
    }
    let response = response
        .error_for_status()
        .map_err(|error| error.to_string())?;
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if response
        .content_length()
        .is_some_and(|size| size > MAX_BYTES as u64)
    {
        return Err("Catalog response too large".into());
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        if bytes.len() + chunk.len() > MAX_BYTES {
            return Err("Catalog response too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    Snapshot::parse(&value, now(), etag).map(Some)
}

fn load_cache(path: &Path, minimum_timestamp: i64) -> Option<Snapshot> {
    if std::fs::metadata(path).ok()?.len() > MAX_BYTES as u64 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let cache: Snapshot = serde_json::from_slice(&bytes).ok()?;
    (cache.version == 1
        && !cache.providers.is_empty()
        && cache
            .providers
            .keys()
            .all(|id| PROVIDERS.contains(&id.as_str()))
        && cache.updated_at >= minimum_timestamp
        && cache.updated_at <= cache.checked_at
        && cache.checked_at <= now() + 300)
        .then_some(cache)
}

fn save_cache(path: &Path, snapshot: &Snapshot) -> Result<(), String> {
    let bytes = serde_json::to_vec(snapshot).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    std::fs::rename(temporary, path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile(provider: &str, region: Option<&str>) -> ApiProfile {
        ApiProfile {
            provider: provider.into(),
            region: region.map(str::to_owned),
            ..Default::default()
        }
    }

    fn fixture() -> Value {
        json!({
            "openai": {"models": {"shared": {"modalities": {"input": ["text", "image"], "output": ["text"]}, "reasoning": false, "limit": {"output": 1024}, "provider": {"api": "https://wrong.invalid"}}}},
            "google": {"models": {"shared": {"modalities": {"input": ["text"], "output": ["text"]}}}},
            "alibaba": {"models": {"shared": {"limit": {"output": 2048}}}},
            "alibaba-cn": {"models": {"shared": {"limit": {"output": 4096}, "reasoning": true, "reasoning_options": [{"type": "toggle"}]}}},
            "alibaba-token-plan-cn": {"models": {"shared": {"limit": {"output": 8192}, "reasoning": false}}}
        })
    }

    #[test]
    fn matching_is_exact_and_regions_do_not_share_facts() {
        let snapshot = Snapshot::parse(&fixture(), now(), None).unwrap();
        assert_eq!(
            snapshot
                .lookup(&profile("openai", None), "shared")
                .unwrap()
                .input_modalities,
            ["text", "image"]
        );
        assert_eq!(
            snapshot
                .lookup(&profile("gemini", None), "models/shared")
                .unwrap()
                .input_modalities,
            ["text"]
        );
        assert_eq!(
            snapshot
                .lookup(&profile("alibaba_cloud", Some("singapore")), "shared")
                .unwrap()
                .max_output_tokens,
            Some(2048)
        );
        assert_eq!(
            snapshot
                .lookup(&profile("alibaba_cloud", Some("china_beijing")), "shared")
                .unwrap()
                .max_output_tokens,
            Some(4096)
        );
        for model in ["Shared", "shared:free", "org/shared"] {
            assert!(snapshot.lookup(&profile("openai", None), model).is_none());
        }
        assert!(snapshot
            .lookup(&profile("openai_compatible", None), "shared")
            .is_none());
        assert!(provider_id(&profile("alibaba_cloud", None)).is_none());
        let token_plan = snapshot
            .lookup(&profile("alibaba_token_plan", None), "shared")
            .unwrap();
        assert_eq!(token_plan.max_output_tokens, Some(8192));
        assert_eq!(token_plan.reasoning.unwrap().supported, Some(false));
        let mut body = json!({"max_tokens": 5000, "enable_thinking": false});
        snapshot
            .lookup(&profile("alibaba_cloud", Some("china_beijing")), "shared")
            .unwrap()
            .apply_request(&mut body, "alibaba_cloud", true);
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["enable_thinking"], true);
    }

    #[test]
    fn missing_and_invalid_fields_stay_unknown_and_remote_settings_are_discarded() {
        let value = json!({"openai": {"models": {
            "unknown": {"reasoning": null, "temperature": "false", "modalities": {"input": ["text", false]}, "limit": {"output": 0}},
            "controlled": {"reasoning": true, "reasoning_options": [{"type": "toggle"}, {"type": "future"}, {"type": "effort", "values": []}, {"type": "budget_tokens", "min": 100, "max": 10}]}
        }}});
        let snapshot = Snapshot::parse(&value, now(), None).unwrap();
        let unknown = snapshot
            .lookup(&profile("openai", None), "unknown")
            .unwrap();
        assert!(unknown.reasoning.is_none());
        assert!(unknown.temperature.is_none());
        assert!(unknown.input_modalities.is_empty());
        assert!(unknown.max_output_tokens.is_none());
        assert_eq!(
            snapshot
                .lookup(&profile("openai", None), "controlled")
                .unwrap()
                .reasoning
                .unwrap()
                .controls
                .len(),
            1
        );
        let snapshot = Snapshot::parse(&fixture(), now(), None).unwrap();
        assert!(!serde_json::to_string(&snapshot)
            .unwrap()
            .contains("wrong.invalid"));
    }

    #[test]
    fn provider_values_win_without_losing_missing_facts() {
        let facts = Snapshot::parse(&fixture(), now(), None)
            .unwrap()
            .lookup(&profile("openai", None), "shared")
            .unwrap();
        let mut direct = ModelDetails::from_provider(
            &json!({"architecture": {"input_modalities": ["text"]}, "top_provider": {"max_completion_tokens": 256}}),
        );
        direct.supplement(&facts);
        assert_eq!(direct.input_modalities, ["text"]);
        assert_eq!(direct.output_modalities, ["text"]);
        assert_eq!(direct.max_output_tokens, Some(256));
        assert_eq!(direct.field_sources["input_modalities"], "provider_api");
        assert_eq!(direct.field_sources["output_modalities"], "models_dev");
    }

    #[test]
    fn damaged_older_and_future_cache_files_do_not_replace_the_bundle() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.json");
        assert!(load_cache(&path, now()).is_none());
        std::fs::write(&path, "broken").unwrap();
        assert!(load_cache(&path, now()).is_none());
        let mut snapshot = Snapshot::parse(&fixture(), now(), None).unwrap();
        save_cache(&path, &snapshot).unwrap();
        assert!(load_cache(&path, snapshot.updated_at).is_some());
        assert!(load_cache(&path, snapshot.updated_at + 1).is_none());
        snapshot.version = 2;
        save_cache(&path, &snapshot).unwrap();
        assert!(load_cache(&path, snapshot.updated_at).is_none());
        snapshot.version = 1;
        snapshot.checked_at = now() + 3600;
        save_cache(&path, &snapshot).unwrap();
        assert!(load_cache(&path, snapshot.updated_at).is_none());
    }

    #[test]
    fn cache_roundtrip_and_stale_facts_preserve_offline_capabilities() {
        let snapshot =
            Snapshot::parse(&fixture(), now() - 8 * 24 * 3600, Some("etag".into())).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.json");
        save_cache(&path, &snapshot).unwrap();
        save_cache(&path, &snapshot).unwrap();
        let loaded: Snapshot = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let details = loaded.lookup(&profile("openai", None), "shared").unwrap();
        assert!(details.stale);
        assert_eq!(details.max_output_tokens, Some(1024));
        assert_eq!(loaded.etag.as_deref(), Some("etag"));
    }

    #[tokio::test]
    async fn refresh_is_shared_nonblocking_and_cancelled_on_shutdown() {
        use axum::{routing::get, Router};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let count = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Notify::new());
        let app = Router::new().route(
            "/",
            get({
                let count = Arc::clone(&count);
                let started = Arc::clone(&started);
                move || {
                    let count = Arc::clone(&count);
                    let started = Arc::clone(&started);
                    async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        started.notify_one();
                        std::future::pending::<String>().await
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache.json");
        let (shutdown, receiver) = watch::channel(false);
        let store = Arc::new(Store {
            snapshot: RwLock::new(
                Snapshot::parse(&fixture(), now() - 2 * 24 * 3600, None).unwrap(),
            ),
            direct: RwLock::new(BTreeMap::new()),
            runtime: Mutex::new(Some((
                path.clone(),
                receiver,
                tokio::runtime::Handle::current(),
            ))),
            refresh: Mutex::new(RefreshState::default()),
        });
        let sync_store = Arc::clone(&store);
        std::thread::spawn(move || {
            sync_store.schedule_refresh_from(&url);
            sync_store.schedule_refresh_from(&url);
        })
        .join()
        .unwrap();
        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        // Reads continue while the HTTP response is blocked.
        assert_eq!(
            store
                .snapshot
                .read()
                .unwrap()
                .lookup(&profile("openai", None), "shared")
                .unwrap()
                .max_output_tokens,
            Some(1024)
        );
        shutdown.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while store.refresh.lock().unwrap().running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!path.exists());
        assert_eq!(count.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn conditional_fetch_accepts_304_and_rejects_invalid_updates() {
        use axum::{routing::get, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route(
                "/unchanged",
                get(|headers: axum::http::HeaderMap| async move {
                    assert_eq!(headers["if-none-match"], "tag");
                    assert!(!headers.contains_key("authorization"));
                    axum::http::StatusCode::NOT_MODIFIED
                }),
            )
            .route("/invalid", get(|| async { "not json" }));
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        assert!(fetch(&format!("{url}/unchanged"), Some("tag"))
            .await
            .unwrap()
            .is_none());
        assert!(fetch(&format!("{url}/invalid"), None).await.is_err());
        server.abort();
    }
}
