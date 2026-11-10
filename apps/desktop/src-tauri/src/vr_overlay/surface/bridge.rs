use super::SurfaceHub;
use crate::vr_overlay::surface_protocol::{
    BridgeError, DocumentSession, HostId, Operation, PageReply, PageRequest,
};
#[cfg(test)]
use crate::vr_overlay::surface_protocol::{decode_request, SurfaceId};


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
            Operation::Ready {} => PageReply::State(Box::new(entry.page_ready())),
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_overlay::surface_protocol::SurfaceView;
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

}
