//! Production Linux surface host.
//!
//! GTK/WebKit objects never leave the main thread. The VR worker and the Tauri
//! commands communicate with it through a bounded command queue.
use super::super::surface_protocol::{
    decode_request, DocumentSession, HostId, PageRequest, ResourcePolicy, ResourceRoute,
    SurfaceActivity, SurfaceEvent,
};
use super::endpoint::CaptureEndpoint;
use super::SurfaceHub;
use crate::vr_overlay::frame_receipt::decode_marker_pixels;
use crate::vr_overlay::surface_protocol::SurfaceId;
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use webkit2gtk::{
    NavigationPolicyDecisionExt, PermissionRequestExt, PolicyDecisionExt, SecurityManagerExt,
    SettingsExt, URIRequestExt, URISchemeRequestExt, URISchemeResponseExt, UserContentManagerExt,
    WebContextExt, WebViewExt,
};

pub(crate) const MARKER_HEIGHT: u32 = 16;

const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob:; font-src 'self'; connect-src 'none'; frame-src 'none'; \
object-src 'none'; base-uri 'none'; form-action 'none'";
const PORT_JS: &str = include_str!("linux_port.js");

pub(crate) struct Snapshot {
    pub(crate) pixels: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) generation: u32,
    pub(crate) revision: u32,
    pub(crate) paint: u32,
}

enum Command {
    Ensure(HostId),
    Send(DocumentSession, SurfaceEvent),
    Activity(HostId, SurfaceActivity),
    Close(HostId),
    Snapshot(SnapshotRequest),
    Shutdown,
}

struct SnapshotRequest {
    id: HostId,
    bounds: [u32; 4],
    reply: SyncSender<Result<Snapshot, String>>,
}

/// Worker-side handle to one surface host. Snapshot requests are fulfilled on
/// the Tauri main thread and delivered through a bounded channel.
pub(crate) struct HostSlot {
    app: AppHandle,
    queue: Arc<Mutex<VecDeque<Command>>>,
    id: HostId,
}

impl HostSlot {
    pub(crate) fn request_snapshot(
        &self,
        bounds: [u32; 4],
    ) -> Result<Receiver<Result<Snapshot, String>>, String> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.push(Command::Snapshot(SnapshotRequest {
            id: self.id,
            bounds,
            reply,
        }))?;
        Ok(receiver)
    }

    fn push(&self, command: Command) -> Result<(), String> {
        self.queue
            .lock()
            .map_err(|_| "Surface host queue poisoned".to_string())?
            .push_back(command);
        let dispatch = self.app.clone();
        let queue = self.queue.clone();
        let run = dispatch.clone();
        dispatch
            .run_on_main_thread(move || pump(run, queue))
            .map_err(|error| format!("Surface host dispatch failed: {error}"))
    }
}

struct HostState {
    session: Option<DocumentSession>,
    connecting: bool,
    navigation: u64,
    activity: Option<SurfaceActivity>,
}

struct Host {
    web: webkit2gtk::WebView,
    _window: gtk::OffscreenWindow,
    state: Rc<RefCell<HostState>>,
}

struct Dispatcher {
    app: AppHandle,
    queue: Arc<Mutex<VecDeque<Command>>>,
    policy: Rc<ResourcePolicy>,
    context: webkit2gtk::WebContext,
    hosts: std::collections::HashMap<(SurfaceId, u32), Host>,
    closing: bool,
}

thread_local! {
    static DISPATCHER: RefCell<Option<Dispatcher>> = const { RefCell::new(None) };
}

fn pump(app: AppHandle, queue: Arc<Mutex<VecDeque<Command>>>) {
    DISPATCHER.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            match Dispatcher::new(app, queue.clone()) {
                Ok(dispatcher) => *slot = Some(dispatcher),
                Err(error) => {
                    tracing::error!(%error, "Linux surface host unavailable");
                    return;
                }
            }
        }
        let Some(dispatcher) = slot.as_mut() else {
            return;
        };
        loop {
            let command = queue
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .pop_front();
            let Some(command) = command else {
                break;
            };
            dispatcher.handle(command);
        }
    });
}

impl Dispatcher {
    fn new(app: AppHandle, queue: Arc<Mutex<VecDeque<Command>>>) -> Result<Self, String> {
        let assets = app.asset_resolver();
        let dev = if tauri::is_dev() {
            app.config().build.dev_url.as_ref()
        } else {
            None
        };
        let policy = ResourcePolicy::new(dev, assets.iter().map(|(path, _)| path.to_string()))?;
        // Production WebKit only serves the embedded asset scheme; development
        // keeps loading the Vite loopback origin.
        let policy = if dev.is_some() {
            policy
        } else {
            policy.with_embedded_origin("vrcs-surface://localhost/")?
        };
        let policy = Rc::new(policy);
        let context = webkit2gtk::WebContext::new_ephemeral();
        let handler_app = app.clone();
        let handler_policy = policy.clone();
        context.register_uri_scheme("vrcs-surface", move |request| {
            serve(&handler_app, &handler_policy, request);
        });
        if let Some(security) = context.security_manager() {
            security.register_uri_scheme_as_secure("vrcs-surface");
            security.register_uri_scheme_as_local("vrcs-surface");
            security.register_uri_scheme_as_cors_enabled("vrcs-surface");
        }
        Ok(Self {
            app,
            queue,
            policy,
            context,
            hosts: std::collections::HashMap::new(),
            closing: false,
        })
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Ensure(id) => self.ensure(id),
            Command::Send(session, event) => {
                if let Some(host) = self
                    .hosts
                    .get(&(session.host.surface, session.host.host_epoch))
                {
                    post_event(&host.web, &session, event);
                }
            }
            Command::Activity(id, activity) => {
                if let Some(host) = self.hosts.get_mut(&(id.surface, id.host_epoch)) {
                    let mut state = host.state.borrow_mut();
                    if state.activity.is_none_or(|previous| previous != activity) {
                        state.activity = Some(activity);
                        if let Some(session) = state.session.clone() {
                            post_event(&host.web, &session, SurfaceEvent::Activity(activity));
                        }
                    }
                }
            }
            Command::Close(id) => {
                self.hosts.remove(&(id.surface, id.host_epoch));
            }
            Command::Snapshot(request) => self.snapshot(request),
            Command::Shutdown => {
                self.closing = true;
                self.hosts.clear();
            }
        }
    }

    fn ensure(&mut self, id: HostId) {
        if self.closing || self.hosts.contains_key(&(id.surface, id.host_epoch)) {
            return;
        }
        {
            let hub = self.app.state::<SurfaceHub>();
            let mut entries = hub.0.lock().unwrap_or_else(|error| error.into_inner());
            let entry = &mut entries[id.surface.index()];
            if entry.host_epoch != id.host_epoch || !entry.active {
                if entry.host_epoch == id.host_epoch {
                    entry.creating = false;
                }
                return;
            }
        }
        match self.create(id) {
            Ok((host, slot)) => {
                self.hosts.insert((id.surface, id.host_epoch), host);
                let hub = self.app.state::<SurfaceHub>();
                let mut entries = hub.0.lock().unwrap_or_else(|error| error.into_inner());
                let entry = &mut entries[id.surface.index()];
                if entry.host_epoch == id.host_epoch && entry.active {
                    entry.endpoint = Some(CaptureEndpoint {
                        host: id,
                        source: slot,
                    });
                    entry.creating = false;
                    entry.host_awake = true;
                } else {
                    self.hosts.remove(&(id.surface, id.host_epoch));
                }
            }
            Err(error) => host_error(&self.app, id, error),
        }
    }

    fn create(&self, id: HostId) -> Result<(Host, Arc<HostSlot>), String> {
        let (width, height) = id.surface.size();
        let source = self.policy.entry_url(id);
        let manager = webkit2gtk::UserContentManager::new();
        manager.add_script(&webkit2gtk::UserScript::new(
            PORT_JS,
            webkit2gtk::UserContentInjectedFrames::TopFrame,
            webkit2gtk::UserScriptInjectionTime::Start,
            &[],
            &[],
        ));
        if !manager.register_script_message_handler("vrcsSurface") {
            return Err("Surface message handler registration failed".into());
        }
        let web = webkit2gtk::WebView::builder()
            .web_context(&self.context)
            .user_content_manager(&manager)
            .build();
        web.set_background_color(&gtk::gdk::RGBA::new(0., 0., 0., 0.));
        web.set_size_request(width as i32, (height + MARKER_HEIGHT) as i32);
        if let Some(settings) = WebViewExt::settings(&web) {
            // GtkOffscreenWindow has no compositor GL surface; snapshots are
            // the transport until a GPU capture path is verified on hardware.
            settings
                .set_hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::Never);
            settings.set_enable_developer_extras(false);
            settings.set_enable_html5_database(false);
            settings.set_enable_html5_local_storage(false);
            settings.set_enable_page_cache(false);
            settings.set_allow_file_access_from_file_urls(false);
            settings.set_allow_universal_access_from_file_urls(false);
        }
        let window = gtk::OffscreenWindow::new();
        window.set_accept_focus(false);
        window.set_focus_on_map(false);
        window.set_default_size(width as i32, (height + MARKER_HEIGHT) as i32);
        window.add(&web);
        window.show_all();
        web.connect_permission_request(|_, request| {
            request.deny();
            true
        });
        web.connect_create(|_, _| None);

        let state = Rc::new(RefCell::new(HostState {
            session: None,
            connecting: false,
            navigation: 0,
            activity: None,
        }));
        let nav_state = state.clone();
        let nav_app = self.app.clone();
        let nav_source = source.clone();
        web.connect_load_changed(move |web, event| {
            let mut state = nav_state.borrow_mut();
            match event {
                webkit2gtk::LoadEvent::Started => {
                    state.session = None;
                    state.connecting = false;
                    state.navigation = state.navigation.wrapping_add(1).max(1);
                    if !nav_app
                        .state::<SurfaceHub>()
                        .navigation_started(id, state.navigation)
                    {
                        state.navigation = 0;
                    }
                }
                webkit2gtk::LoadEvent::Finished => {
                    if state.navigation == 0 {
                        return;
                    }
                    if let Some(document) = nav_app.state::<SurfaceHub>().document_loaded(
                        id,
                        state.navigation,
                        nav_source.clone(),
                    ) {
                        state.session = Some(document.clone());
                        if state.connecting {
                            post(
                                web,
                                serde_json::json!({
                                    "version": 1,
                                    "op": "welcome",
                                    "host": document.host,
                                    "nonce": document.nonce,
                                }),
                            );
                        }
                    }
                }
                _ => {}
            }
        });

        let fail_app = self.app.clone();
        web.connect_load_failed(move |_, _, _, error| {
            host_error(&fail_app, id, format!("surface_navigation_failed: {error}"));
            false
        });
        let term_app = self.app.clone();
        web.connect_web_process_terminated(move |_, reason| {
            host_error(
                &term_app,
                id,
                format!("surface_browser_process_failed: {reason:?}"),
            );
        });

        let msg_app = self.app.clone();
        let msg_source = source.clone();
        let msg_web = web.downgrade();
        let msg_state = state.clone();
        manager.connect_script_message_received(Some("vrcsSurface"), move |_, result| {
            let Some(web) = msg_web.upgrade() else {
                return;
            };
            let Some(js) = result.js_value() else {
                return;
            };
            let bytes = js.to_string();
            if bytes.len() > 263_000 {
                return;
            }
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Wrapper {
                source: String,
                message: serde_json::Value,
            }
            let Ok(wrapper) = serde_json::from_str::<Wrapper>(&bytes) else {
                return;
            };
            // The wrapper source is page-reported; the controlled document and
            // navigation checks are the authorization boundary.
            if wrapper.source != msg_source || web.uri().as_deref() != Some(msg_source.as_str()) {
                return;
            }
            let Ok(request) = decode_request(&wrapper.message.to_string()) else {
                return;
            };
            let mut state = msg_state.borrow_mut();
            if matches!(request, PageRequest::Connect) {
                state.connecting = true;
                if let Some(document) = state.session.as_ref() {
                    post(
                        &web,
                        serde_json::json!({
                            "version": 1,
                            "op": "welcome",
                            "host": document.host,
                            "nonce": document.nonce,
                        }),
                    );
                }
                return;
            }
            let Some(document) = state.session.clone() else {
                return;
            };
            if document.authorize(&wrapper.source, &request).is_err() {
                return;
            }
            let request_id = match &request {
                PageRequest::Call { request_id, .. } => *request_id,
                PageRequest::Connect => return,
            };
            let reply = match msg_app
                .state::<SurfaceHub>()
                .handle_page_request(&document, request)
            {
                Ok(value) => serde_json::json!({
                    "version": 1,
                    "op": "reply",
                    "nonce": document.nonce,
                    "request_id": request_id,
                    "ok": true,
                    "value": value,
                }),
                Err(error) => serde_json::json!({
                    "version": 1,
                    "op": "reply",
                    "nonce": document.nonce,
                    "request_id": request_id,
                    "ok": false,
                    "error": error.code(),
                }),
            };
            post(&web, reply);
        });

        let policy_source = source.clone();
        web.connect_decide_policy(move |_, decision, kind| {
            if matches!(
                kind,
                webkit2gtk::PolicyDecisionType::NavigationAction
                    | webkit2gtk::PolicyDecisionType::NewWindowAction
            ) {
                let allowed = decision
                    .downcast_ref::<webkit2gtk::NavigationPolicyDecision>()
                    .and_then(|nav| nav.navigation_action())
                    .and_then(|action| action.request())
                    .and_then(|request| request.uri())
                    .is_some_and(|uri| uri == policy_source);
                if !allowed {
                    decision.ignore();
                }
                return true;
            }
            false
        });

        web.load_uri(&source);
        let slot = Arc::new(HostSlot {
            app: self.app.clone(),
            queue: self.queue.clone(),
            id,
        });
        Ok((
            Host {
                web,
                _window: window,
                state,
            },
            slot,
        ))
    }

    fn snapshot(&mut self, request: SnapshotRequest) {
        let key = (request.id.surface, request.id.host_epoch);
        let Some(host) = self.hosts.get(&key) else {
            let _ = request
                .reply
                .send(Err("Surface host is not running".into()));
            return;
        };
        let [left, top, right, bottom] = request.bounds;
        let (logical_width, logical_height) = request.id.surface.size();
        if left >= right || top >= bottom || right > logical_width || bottom > logical_height {
            let _ = request.reply.send(Err("Invalid surface crop".into()));
            return;
        }
        let cancellation = gio::Cancellable::new();
        let reply = request.reply;
        host.web.snapshot(
            webkit2gtk::SnapshotRegion::Visible,
            webkit2gtk::SnapshotOptions::TRANSPARENT_BACKGROUND,
            Some(&cancellation),
            move |result| {
                let snapshot = (|| {
                    let surface = result.map_err(|error| error.to_string())?;
                    let image = cairo::ImageSurface::try_from(surface)
                        .map_err(|_| "snapshot is not a Cairo image surface".to_string())?;
                    let scale = image.width() as f32 / logical_width as f32;
                    let expected_height = (logical_height + MARKER_HEIGHT) as f32 * scale;
                    if scale < 1.0 || (image.height() as f32 - expected_height).abs() > 2.0 {
                        return Err(format!(
                            "unexpected snapshot dimensions {}x{}",
                            image.width(),
                            image.height()
                        ));
                    }
                    let rgba = rgba(&image)?;
                    let width = image.width() as usize;
                    let (generation, revision, paint) =
                        marker(&rgba, width, logical_height as usize, scale)
                            .ok_or_else(|| "snapshot marker not painted".to_string())?;
                    let px = |value: usize| ((value as f32) * scale).round() as usize;
                    let (xs, ys) = (px(left as usize), px(top as usize));
                    let (xe, ye) = (px(right as usize), px(bottom as usize));
                    if xe <= xs || ye <= ys || xe > width || ye > image.height() as usize {
                        return Err("snapshot crop is outside the surface".into());
                    }
                    let mut pixels = Vec::with_capacity((xe - xs) * (ye - ys) * 4);
                    for row in ys..ye {
                        let start = (row * width + xs) * 4;
                        pixels.extend_from_slice(&rgba[start..start + (xe - xs) * 4]);
                    }
                    Ok(Snapshot {
                        pixels,
                        width: (xe - xs) as u32,
                        height: (ye - ys) as u32,
                        generation,
                        revision,
                        paint,
                    })
                })();
                let _ = reply.send(snapshot);
            },
        );
    }
}

fn post(web: &webkit2gtk::WebView, message: serde_json::Value) {
    let message = message
        .to_string()
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    web.evaluate_javascript(
        &format!("window.__vrcsSurfaceReceive({message})"),
        None,
        None,
        gio::Cancellable::NONE,
        |result| {
            if let Err(error) = result {
                tracing::warn!(%error, "surface message delivery failed");
            }
        },
    );
}

fn post_event(web: &webkit2gtk::WebView, session: &DocumentSession, event: SurfaceEvent) {
    let mut message = serde_json::to_value(event).expect("surface events are serializable");
    message["version"] = 1.into();
    message["op"] = "event".into();
    message["nonce"] = session.nonce.clone().into();
    post(web, message);
}

fn host_error(app: &AppHandle, id: HostId, error: String) {
    let hub = app.state::<SurfaceHub>();
    let mut entries = hub.0.lock().unwrap_or_else(|error| error.into_inner());
    let entry = &mut entries[id.surface.index()];
    if entry.host_epoch != id.host_epoch || entry.error.as_ref() == Some(&error) {
        return;
    }
    entry.error = Some(error.clone());
    entry.creating = false;
    entry.layout = None;
    entry.displayed_layout = None;
    entry.displayed_scroll_key = None;
    tracing::warn!(
        surface = id.surface.name(),
        host_epoch = id.host_epoch,
        "{error}"
    );
}

fn serve(app: &AppHandle, policy: &ResourcePolicy, request: &webkit2gtk::URISchemeRequest) {
    let method = request
        .http_method()
        .map(|method| method.to_string())
        .unwrap_or_default();
    let Some(uri) = request.uri().map(|uri| uri.to_string()) else {
        fail(request, "missing surface URI");
        return;
    };
    let path = match policy.route(&method, &uri) {
        ResourceRoute::Asset(path) => path,
        ResourceRoute::Network => {
            fail(
                request,
                "network surface resources are not served by the scheme",
            );
            return;
        }
        ResourceRoute::Reject(_) => {
            fail(request, "surface resource rejected");
            return;
        }
    };
    let Some(asset) = app.asset_resolver().get_for_scheme(path.clone(), true) else {
        fail(request, "surface resource not found");
        return;
    };
    let mut bytes = asset.bytes;
    if path == "vr-surface.html" {
        let Ok(html) = String::from_utf8(bytes) else {
            fail(request, "surface document is not UTF-8");
            return;
        };
        bytes = html
            .replacen(
                "<head>",
                &format!("<head><meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">"),
                1,
            )
            .into_bytes();
    }
    let length = bytes.len() as i64;
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
    let headers = soup::MessageHeaders::new(soup::MessageHeadersType::Response);
    headers.append("Access-Control-Allow-Origin", "vrcs-surface://localhost");
    headers.append("Content-Type", &asset.mime_type);
    headers.append("X-Content-Type-Options", "nosniff");
    headers.append(
        "Cache-Control",
        if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-store"
        },
    );
    if let Some(csp) = asset.csp_header {
        headers.append("Content-Security-Policy", &csp);
    }
    let response = webkit2gtk::URISchemeResponse::new(&stream, length);
    response.set_content_type(&asset.mime_type);
    response.set_status(200u32, Some("OK"));
    response.set_http_headers(headers);
    request.finish_with_response(&response);
}

fn fail(request: &webkit2gtk::URISchemeRequest, message: &str) {
    request.finish_error(&mut glib::Error::new(gio::IOErrorEnum::NotFound, message));
}

fn marker(
    pixels: &[u8],
    width: usize,
    logical_height: usize,
    scale: f32,
) -> Option<(u32, u32, u32)> {
    decode_marker_pixels(|bit| {
        let x = (((bit % 64) * 4 + 2) as f32 * scale).round() as usize;
        let y = ((logical_height + 4 + (bit / 64) * 8) as f32 * scale).round() as usize;
        let offset = (y * width + x) * 4;
        pixels.get(offset..offset + 4)?.try_into().ok()
    })
}

fn rgba(image: &cairo::ImageSurface) -> Result<Vec<u8>, String> {
    if ![cairo::Format::ARgb32, cairo::Format::Rgb24].contains(&image.format()) {
        return Err("unsupported snapshot format".into());
    }
    let width = image.width() as usize;
    let height = image.height() as usize;
    let stride = image.stride() as usize;
    let mut pixels = vec![0; width * height * 4];
    image
        .with_data(|data| {
            for y in 0..height {
                for x in 0..width {
                    let offset = y * stride + x * 4;
                    let word = u32::from_ne_bytes(data[offset..offset + 4].try_into().unwrap());
                    let alpha = if image.format() == cairo::Format::Rgb24 {
                        255
                    } else {
                        (word >> 24) as u8
                    };
                    let dest = &mut pixels[(y * width + x) * 4..][..4];
                    for (channel, shift) in [16, 8, 0].iter().enumerate() {
                        let premultiplied = (word >> shift) & 255;
                        dest[channel] = if alpha == 0 {
                            0
                        } else {
                            ((premultiplied * 255 + u32::from(alpha) / 2) / u32::from(alpha))
                                .min(255) as u8
                        };
                    }
                    dest[3] = alpha;
                }
            }
        })
        .map_err(|error| error.to_string())?;
    Ok(pixels)
}

pub(crate) struct NativeHostManager {
    app: AppHandle,
    queue: Arc<Mutex<VecDeque<Command>>>,
}

impl NativeHostManager {
    pub fn start(app: AppHandle) -> Result<Self, String> {
        Ok(Self {
            app,
            queue: Arc::new(Mutex::new(VecDeque::new())),
        })
    }

    pub(super) fn ensure(&self, id: HostId) -> Result<(), String> {
        {
            let hub = self.app.state::<SurfaceHub>();
            let entries = hub.0.lock().unwrap_or_else(|error| error.into_inner());
            let entry = &entries[id.surface.index()];
            if entry.host_epoch != id.host_epoch || !entry.active {
                return Ok(());
            }
        }
        self.push(Command::Ensure(id))
    }

    pub(super) fn send(&self, id: HostId, event: SurfaceEvent) -> Result<(), String> {
        let session = self
            .app
            .state::<SurfaceHub>()
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())[id.surface.index()]
        .document
        .clone();
        let Some(session) = session.filter(|session| session.host == id) else {
            return Ok(());
        };
        self.push(Command::Send(session, event))
    }

    pub(super) fn set_activity(&self, id: HostId, activity: SurfaceActivity) -> Result<(), String> {
        self.push(Command::Activity(id, activity))
    }

    pub(super) fn close(&self, id: HostId) -> Result<(), String> {
        self.push(Command::Close(id))
    }

    pub fn begin_shutdown(&self) {
        let _ = self.push(Command::Shutdown);
    }

    pub fn finish_shutdown(&self, _timeout: std::time::Duration) -> Result<(), String> {
        Ok(())
    }

    fn push(&self, command: Command) -> Result<(), String> {
        self.queue
            .lock()
            .map_err(|_| "Surface host queue poisoned".to_string())?
            .push_back(command);
        let dispatch = self.app.clone();
        let queue = self.queue.clone();
        let run = dispatch.clone();
        dispatch
            .run_on_main_thread(move || pump(run, queue))
            .map_err(|error| format!("Surface host dispatch failed: {error}"))
    }
}

impl Drop for NativeHostManager {
    fn drop(&mut self) {
        let _ = self.push(Command::Shutdown);
    }
}
