use serde::{Deserialize, Serialize};

use super::{
    SurfaceActivity, SurfaceHub, SurfaceId, SurfaceKey, SurfaceLayout, SurfacePointer,
    SurfaceScroll, SurfaceState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(super) struct HostId {
    pub surface: SurfaceId,
    pub host_epoch: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DocumentSession {
    pub host: HostId,
    pub navigation_id: u64,
    pub nonce: String,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BridgeError {
    InvalidRequest,
    TooLarge,
    InvalidSource,
    StaleSession,
    InvalidLayout,
}

impl BridgeError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::TooLarge => "request_too_large",
            Self::InvalidSource => "invalid_source",
            Self::StaleSession => "stale_session",
            Self::InvalidLayout => "invalid_layout",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "op",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum Operation {
    Ready {},
    Layout {
        layout: SurfaceLayout,
        paint_serial: u32,
    },
    Paint {
        key: SurfaceKey,
        paint_serial: u32,
    },
    Snapshot {
        key: Option<SurfaceKey>,
        activity_request: u32,
        scroll_top: Option<f64>,
    },
    Fault {
        stage: FaultStage,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum FaultStage {
    Startup,
    Render,
    Unhandled,
}

impl FaultStage {
    fn code(&self) -> &'static str {
        match self {
            Self::Startup => "page_startup_failed",
            Self::Render => "page_render_failed",
            Self::Unhandled => "page_unhandled_error",
        }
    }
}

#[derive(Debug)]
pub(super) enum PageRequest {
    Connect,
    Call {
        nonce: String,
        request_id: u64,
        operation: Operation,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum PageReply {
    State(Option<SurfaceState>),
    Accepted(bool),
}

#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "payload", rename_all = "snake_case")]
pub(super) enum SurfaceEvent {
    View(SurfaceState),
    Activity(SurfaceActivity),
    Pointer(SurfacePointer),
    Scroll(SurfaceScroll),
    Repaint,
}

pub(super) fn decode_request(json: &str) -> Result<PageRequest, BridgeError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Envelope {
        version: u8,
        op: String,
        nonce: Option<String>,
        request_id: Option<u64>,
        payload: Option<serde_json::Value>,
    }
    if json.len() > 262144 {
        return Err(BridgeError::TooLarge);
    }
    let message: Envelope = serde_json::from_str(json).map_err(|_| BridgeError::InvalidRequest)?;
    if message.version != 1 {
        return Err(BridgeError::InvalidRequest);
    }
    if message.op == "connect" {
        return if message.nonce.is_none()
            && message.request_id.is_none()
            && message.payload.is_none()
        {
            Ok(PageRequest::Connect)
        } else {
            Err(BridgeError::InvalidRequest)
        };
    }
    let nonce = message
        .nonce
        .filter(|nonce| !nonce.is_empty() && nonce.len() <= 128)
        .ok_or(BridgeError::InvalidRequest)?;
    let request_id = message
        .request_id
        .filter(|id| (1..=9007199254740991).contains(id))
        .ok_or(BridgeError::InvalidRequest)?;
    let operation =
        serde_json::from_value(serde_json::json!({ "op": message.op, "payload": message.payload }))
            .map_err(|_| BridgeError::InvalidRequest)?;
    Ok(PageRequest::Call {
        nonce,
        request_id,
        operation,
    })
}

impl DocumentSession {
    pub fn authorize(&self, source: &str, request: &PageRequest) -> Result<(), BridgeError> {
        if source != self.source {
            return Err(BridgeError::InvalidSource);
        }
        if let PageRequest::Call { nonce, .. } = request {
            if nonce != &self.nonce {
                return Err(BridgeError::StaleSession);
            }
        }
        Ok(())
    }
}

impl SurfaceHub {
    pub(super) fn navigation_started(&self, host: HostId, navigation_id: u64) -> bool {
        let mut entries = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let entry = &mut entries[host.surface.index()];
        if entry.host_epoch != host.host_epoch
            || navigation_id <= entry.navigation_id
            || matches!(
                entry.recovery.phase,
                super::recovery::Phase::Failed | super::recovery::Phase::Closing
            )
        {
            return false;
        }
        entry.reload();
        entry.navigation_id = navigation_id;
        true
    }

    pub(super) fn document_loaded(
        &self,
        host: HostId,
        navigation_id: u64,
        source: String,
    ) -> Option<DocumentSession> {
        let mut entries = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let entry = &mut entries[host.surface.index()];
        if entry.host_epoch != host.host_epoch
            || navigation_id == 0
            || entry.navigation_id != navigation_id
            || entry.error.is_some()
        {
            return None;
        }
        Some(
            entry
                .document
                .get_or_insert_with(|| DocumentSession {
                    host,
                    navigation_id,
                    source,
                    nonce: uuid::Uuid::new_v4().to_string(),
                })
                .clone(),
        )
    }

    pub(super) fn handle_page_request(
        &self,
        session: &DocumentSession,
        request: PageRequest,
    ) -> Result<PageReply, BridgeError> {
        session.authorize(&session.source, &request)?;
        let mut entries = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let entry = &mut entries[session.host.surface.index()];
        if entry.host_epoch != session.host.host_epoch
            || entry.document.as_ref() != Some(session)
            || entry.error.is_some()
        {
            return Err(BridgeError::StaleSession);
        }
        let PageRequest::Call { operation, .. } = request else {
            return Err(BridgeError::InvalidRequest);
        };
        Ok(match operation {
            Operation::Ready {} => PageReply::State(entry.page_ready()),
            Operation::Layout {
                layout,
                paint_serial,
            } => PageReply::Accepted(
                entry
                    .accept_layout_paint(session.host.surface, layout, paint_serial)
                    .map_err(|_| BridgeError::InvalidLayout)?,
            ),
            Operation::Paint { key, paint_serial } => {
                PageReply::Accepted(entry.accept_paint(key, paint_serial))
            }
            Operation::Snapshot {
                key,
                activity_request,
                scroll_top,
            } => PageReply::Accepted(entry.accept_snapshot(key, activity_request, scroll_top)),
            Operation::Fault { stage } => {
                entry.error = Some(stage.code().into());
                entry.layout = None;
                entry.displayed_layout = None;
                entry.displayed_scroll_key = None;
                PageReply::Accepted(true)
            }
        })
    }
}

pub(super) struct ResourcePolicy {
    origin: tauri::Url,
    embedded: bool,
    assets: std::collections::HashSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ResourceRoute {
    Network,
    Asset(String),
    Reject(u16),
}

impl ResourcePolicy {
    pub fn new(
        dev_url: Option<&tauri::Url>,
        assets: impl Iterator<Item = String>,
    ) -> Result<Self, String> {
        let mut origin = dev_url
            .cloned()
            .unwrap_or_else(|| tauri::Url::parse("https://vrcs-surface.invalid/").unwrap());
        if dev_url.is_some()
            && (!matches!(origin.scheme(), "http" | "https")
                || !matches!(origin.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
                || !origin.username().is_empty()
                || origin.password().is_some())
        {
            return Err("Surface dev URL must use a loopback HTTP origin".into());
        }
        origin.set_path("/");
        origin.set_query(None);
        origin.set_fragment(None);
        Ok(Self {
            origin,
            embedded: dev_url.is_none(),
            assets: assets
                .map(|path| path.trim_start_matches('/').to_owned())
                .collect(),
        })
    }

    pub fn entry_url(&self, host: HostId) -> String {
        format!(
            "{}vr-surface.html?surface={}&host_epoch={}",
            self.origin,
            host.surface.name(),
            host.host_epoch
        )
    }

    pub fn route(&self, method: &str, uri: &str) -> ResourceRoute {
        if !matches!(method, "GET" | "HEAD") {
            return ResourceRoute::Reject(405);
        }
        let Ok(url) = tauri::Url::parse(uri) else {
            return ResourceRoute::Reject(403);
        };
        if url.origin() != self.origin.origin()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return ResourceRoute::Reject(403);
        }
        if !self.embedded {
            return ResourceRoute::Network;
        }
        // Reject ambiguous raw paths before URL normalization can remove dot segments.
        let raw_path = uri
            .split_once("://")
            .and_then(|(_, tail)| tail.find('/').map(|at| &tail[at..]))
            .unwrap_or("/")
            .split(['?', '#'])
            .next()
            .unwrap_or("/");
        if raw_path.contains(['%', '\\'])
            || raw_path.split('/').any(|part| part == "." || part == "..")
        {
            return ResourceRoute::Reject(403);
        }
        let path = url.path().trim_start_matches('/');
        if path.starts_with("tests/") || path.ends_with(".html") && path != "vr-surface.html" {
            return ResourceRoute::Reject(403);
        }
        if self.assets.contains(path) {
            ResourceRoute::Asset(path.into())
        } else {
            ResourceRoute::Reject(404)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_overlay::surface::SurfaceView;
    use serde_json::{json, Value};
    use std::time::Instant;

    const SOURCE: &str = "https://vrcs-surface.invalid/vr-surface.html?surface=reader&host_epoch=1";

    fn fixture() -> (SurfaceHub, HostId) {
        let hub = SurfaceHub::default();
        let host = HostId {
            surface: SurfaceId::Reader,
            host_epoch: 1,
        };
        {
            let mut entries = hub.0.lock().unwrap();
            let entry = &mut entries[host.surface.index()];
            entry.creating = true;
            entry.set_active(true, Instant::now());
            entry.publish(SurfaceView::OcrSelection { dark: true }, None, None);
        }
        (hub, host)
    }

    fn call(session: &DocumentSession, op: &str, payload: Value) -> PageRequest {
        decode_request(&json!({ "version": 1, "nonce": session.nonce, "request_id": 1, "op": op, "payload": payload }).to_string()).unwrap()
    }

    #[test]
    fn ready_is_idempotent_per_document() {
        let (hub, host) = fixture();
        assert!(hub.navigation_started(host, 10));
        let session = hub.document_loaded(host, 10, SOURCE.into()).unwrap();
        let ready = || {
            serde_json::to_value(
                hub.handle_page_request(&session, call(&session, "ready", json!({})))
                    .unwrap(),
            )
            .unwrap()
        };
        let first = ready();
        assert_eq!(first["key"]["generation"], 2);
        assert_eq!(ready(), first);
        assert_eq!(hub.generation(host.surface), 2);
        assert!(hub.navigation_started(host, 11));
        assert!(hub.document_loaded(host, 10, SOURCE.into()).is_none());
        assert!(!hub.navigation_started(host, 10));
        assert_eq!(hub.generation(host.surface), 3);
        let next = hub.document_loaded(host, 11, SOURCE.into()).unwrap();
        assert_ne!(session.nonce, next.nonce);
        assert_eq!(hub.document_loaded(host, 11, SOURCE.into()).unwrap(), next);
    }

    #[test]
    fn stale_document_nonce_cannot_mutate_entry() {
        let (hub, host) = fixture();
        assert!(hub.navigation_started(host, 1));
        let old = hub.document_loaded(host, 1, SOURCE.into()).unwrap();
        assert!(hub.navigation_started(host, 2));
        let session = hub.document_loaded(host, 2, SOURCE.into()).unwrap();
        let old_request = call(&old, "ready", json!({}));
        assert_eq!(
            session.authorize(SOURCE, &old_request),
            Err(BridgeError::StaleSession)
        );
        assert!(matches!(
            hub.handle_page_request(&old, old_request),
            Err(BridgeError::StaleSession)
        ));
        let key = hub.0.lock().unwrap()[host.surface.index()]
            .state
            .as_ref()
            .unwrap()
            .key;
        for (op, payload) in [
            ("ready", json!({})),
            (
                "layout",
                json!({ "layout": { "key": key, "regions": [], "page_count": 1 }, "paint_serial": 1 }),
            ),
            ("paint", json!({ "key": key, "paint_serial": 1 })),
            (
                "snapshot",
                json!({ "key": key, "activity_request": 1, "scroll_top": 12 }),
            ),
        ] {
            assert!(matches!(
                hub.handle_page_request(&session, call(&old, op, payload)),
                Err(BridgeError::StaleSession)
            ));
        }
        let entries = hub.0.lock().unwrap();
        let entry = &entries[host.surface.index()];
        assert_eq!(entry.generation, 3);
        assert!(!entry.ready);
        assert!(entry.layout.is_none());
        assert!(entry.paint.is_none());
        assert!(entry.saved_scroll.is_none());
    }

    #[test]
    fn foreign_source_and_surface_are_rejected() {
        let (hub, host) = fixture();
        assert!(hub.navigation_started(host, 1));
        let session = hub.document_loaded(host, 1, SOURCE.into()).unwrap();
        for source in [
            "https://example.com/vr-surface.html",
            "https://vrcs-surface.invalid:8000/vr-surface.html",
            "https://vrcs-surface.invalid/index.html",
            "https://vrcs-surface.invalid/vr-surface.html?surface=menu&host_epoch=1",
        ] {
            assert_eq!(
                session.authorize(source, &call(&session, "ready", json!({}))),
                Err(BridgeError::InvalidSource)
            );
        }
        assert!(session
            .authorize(SOURCE, &call(&session, "ready", json!({})))
            .is_ok());
        let mut foreign = session.clone();
        foreign.host.surface = SurfaceId::Menu;
        assert!(matches!(
            hub.handle_page_request(&foreign, call(&foreign, "ready", json!({}))),
            Err(BridgeError::StaleSession)
        ));
        foreign = session.clone();
        foreign.host.host_epoch = 2;
        assert!(!hub.navigation_started(foreign.host, 2));
        assert!(matches!(
            hub.handle_page_request(&foreign, call(&foreign, "ready", json!({}))),
            Err(BridgeError::StaleSession)
        ));
        assert!(!hub.0.lock().unwrap()[host.surface.index()].ready);
    }

    #[test]
    fn oversized_or_unknown_requests_are_rejected() {
        assert!(matches!(
            decode_request(r#"{"version":1,"op":"connect"}"#),
            Ok(PageRequest::Connect)
        ));
        let good =
            json!({ "version": 1, "nonce": "test", "request_id": 1, "op": "ready", "payload": {} });
        assert!(decode_request(&good.to_string()).is_ok());
        let invalid = [
            ("version", json!(2)),
            ("request_id", json!(0)),
            ("request_id", json!(-1)),
            ("request_id", json!(9007199254740992_u64)),
            ("op", json!("invoke")),
            ("nonce", json!("")),
            ("surface", json!("menu")),
            ("payload", json!({ "command": "read_file" })),
        ];
        for (key, value) in invalid {
            let mut request = good.clone();
            request[key] = value;
            assert!(
                decode_request(&request.to_string()).is_err(),
                "accepted {key}"
            );
        }
        assert!(decode_request(r#"{"version":1,"op":"connect","nonce":"injected"}"#).is_err());
        assert!(matches!(
            decode_request(&" ".repeat(262145)),
            Err(BridgeError::TooLarge)
        ));
    }

    #[test]
    fn dev_resources_use_the_exact_configured_loopback_origin() {
        let url = tauri::Url::parse("http://127.0.0.1:15341/").unwrap();
        let policy = ResourcePolicy::new(Some(&url), std::iter::empty()).unwrap();
        assert_eq!(
            policy.entry_url(HostId {
                surface: SurfaceId::Reader,
                host_epoch: 7
            }),
            "http://127.0.0.1:15341/vr-surface.html?surface=reader&host_epoch=7"
        );
        assert_eq!(
            policy.route("GET", "http://127.0.0.1:15341/@vite/client"),
            ResourceRoute::Network
        );
        for uri in [
            "http://127.0.0.1:1420/src/main.tsx",
            "http://localhost:15341/src/main.tsx",
            "https://127.0.0.1:15341/src/main.tsx",
            "http://127.0.0.1:15341@evil.invalid/a",
        ] {
            assert_eq!(policy.route("GET", uri), ResourceRoute::Reject(403));
        }
        assert!(ResourcePolicy::new(
            Some(&tauri::Url::parse("https://example.com/").unwrap()),
            std::iter::empty()
        )
        .is_err());
    }

    #[test]
    fn embedded_resources_require_an_exact_asset_and_never_use_spa_fallback() {
        let policy = ResourcePolicy::new(
            None,
            [
                "vr-surface.html",
                "index.html",
                "assets/surface.js",
                "assets/surface.css",
                "tests/fixture.js",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        let route =
            |method, path| policy.route(method, &format!("https://vrcs-surface.invalid{path}"));
        assert_eq!(
            route("GET", "/vr-surface.html?surface=reader&host_epoch=1"),
            ResourceRoute::Asset("vr-surface.html".into())
        );
        assert_eq!(
            route("HEAD", "/assets/surface.css"),
            ResourceRoute::Asset("assets/surface.css".into())
        );
        assert_eq!(
            route("GET", "/assets/missing.js"),
            ResourceRoute::Reject(404)
        );
        assert_eq!(
            route("POST", "/assets/surface.js"),
            ResourceRoute::Reject(405)
        );
        for path in [
            "/index.html",
            "/tests/fixture.js",
            "/assets/../vr-surface.html",
            "/assets/%2e%2e/vr-surface.html",
            "/%2561ssets/surface.js",
            "/assets\\surface.js",
        ] {
            assert_eq!(
                route("GET", path),
                ResourceRoute::Reject(403),
                "accepted {path}"
            );
        }
        assert_eq!(
            policy.route("GET", "file:///assets/surface.js"),
            ResourceRoute::Reject(403)
        );
        assert_eq!(
            policy.route("GET", "https://example.com/assets/surface.js"),
            ResourceRoute::Reject(403)
        );
    }
}
