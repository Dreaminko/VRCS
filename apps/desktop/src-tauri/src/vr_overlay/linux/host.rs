//! Real GTK3/WebKitGTK 4.1 offscreen experiment, not a production VR capability.
use std::{cell::{Cell, RefCell}, path::{Path, PathBuf}, rc::Rc, time::{Duration, Instant}};
use gtk::prelude::*;
use webkit2gtk::{NavigationPolicyDecisionExt, PermissionRequestExt, PolicyDecisionExt, SecurityManagerExt,
    SettingsExt, URIRequestExt, URISchemeRequestExt, URISchemeResponseExt, UserContentManagerExt, WebContextExt, WebResourceExt, WebViewExt};
use serde_json::{json, Value};
use super::{openvr::{Input, Overlay}, protocol::{self, Session}, visibility::Fade};

const WIDTH: i32 = 1024;
const HEIGHT: i32 = 1024;
const SOURCE: &str = "vrcs-surface://localhost/vr-surface.html?surface=reader";
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self'; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

pub struct Options {
    pub assets: PathBuf,
    pub output: PathBuf,
    pub seconds: u64,
    pub submit_vr: bool,
}

struct State {
    session: Session,
    fade: Fade,
    target: bool,
    active: bool,
    activity_request: u32,
    pinned: bool,
    pointer: Option<[f64; 2]>,
    pressed: Option<(protocol::Key, u32, u32)>,
    runtime: Option<Overlay>,
    runtime_retry: Instant,
    started: Instant,
    next_view: Instant,
    snapshots: u64,
    accepted: u64,
    rejected: u64,
    hidden_frames: u64,
    runtime_submissions: u64,
    snapshot_us: u128,
    max_snapshot_us: u128,
    last_pixels: Option<Vec<u8>>,
    hidden_pixels: Option<Vec<u8>>,
    hidden_updates: u64,
    clicks: u64,
    stage: u8,
    error: Option<String>,
}

fn view(state: &State) -> Value {
    let text = format!("动态中文字幕 · 日本語の字幕 · 한국어 자막\nRevision {} · {}\n{}", state.session.key.revision,
        if state.pinned { "Pinned / 已固定" } else { "Unpinned / 未固定" },
        "透明度、换行与滚动 / 透明な背景と折り返し / 투명 배경과 줄바꿈\n".repeat(18));
    json!({
        "key": state.session.key, "hovered": null, "pressed": null, "pointer": state.pointer,
        "activity": {"active": state.active, "request": state.activity_request},
        "view": {"kind":"reader", "dark":true, "font_size":32, "opacity":0.75,
            "labels": {"source":"原文 / Source", "answer":"翻译 / Translation", "ocrTitle":"WebKitGTK CJK prototype",
                "complete":"动态画面 / Live", "block":"Text", "previousBlock":"Previous", "nextBlock":"Next",
                "pin":"固定 / Pin", "unpin":"取消固定 / Unpin", "close":"隐藏 / Hide"},
            "reader": {"mode":"ocr", "source":text, "translation":"日本語 · 한국어 · 中文\n真实 React surface；没有第二套 UI。",
                "translation_label":"answer", "title":"ocrTitle", "status":"complete", "detail":"Linux offscreen WebKitGTK",
                "block_index":0,"block_count":2,"page":0,"pinned":state.pinned,"source_only":false,"language_count":1}
        }
    })
}

fn post(web: &webkit2gtk::WebView, message: Value) {
    web.evaluate_javascript(&format!("window.__vrcsSurfaceReceive({message})"), None, None, gio::Cancellable::NONE, |result| {
        if let Err(error) = result { eprintln!("surface message delivery failed: {error}"); }
    });
}
fn event(web: &webkit2gtk::WebView, state: &State, name: &str, payload: Value) {
    post(web, json!({"version":1,"nonce":state.session.nonce,"op":"event","event":name,"payload":payload}));
}
fn publish(web: &webkit2gtk::WebView, state: &mut State) {
    state.session.advance();
    state.pressed = None;
    event(web, state, "view", view(state));
}

fn resource(root: &Path, uri: &str) -> Result<(Vec<u8>, &'static str), String> {
    let path = uri.strip_prefix("vrcs-surface://localhost/").ok_or("invalid surface resource origin")?;
    let path = path.split('?').next().unwrap_or_default();
    if path != "vr-surface.html" && !path.starts_with("assets/") && !path.starts_with("logos/") {
        return Err("resource outside surface asset allowlist".into());
    }
    if path.split('/').any(|part| part.is_empty() || part == "." || part == "..")
        || path.contains(['%', '\\', '#']) { return Err("invalid surface resource path".into()); }
    let file = root.join(path).canonicalize().map_err(|e| e.to_string())?;
    if !file.starts_with(root) || std::fs::metadata(&file).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
        return Err("resource outside root or too large".into());
    }
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let mime = match Path::new(path).extension().and_then(|s| s.to_str()) {
        Some("html") => "text/html", Some("js") => "text/javascript", Some("css") => "text/css",
        Some("svg") => "image/svg+xml", Some("png") => "image/png", Some("woff2") => "font/woff2",
        Some("woff") => "font/woff", Some("ttf") => "font/ttf", _ => return Err("unsupported resource type".into()),
    };
    if path == "vr-surface.html" {
        let html = String::from_utf8(bytes).map_err(|e| e.to_string())?;
        if !html.contains("<head>") || html.contains("/src/vr-surface/main.tsx") {
            return Err("--assets requires built frontend dist, not Vite source".into());
        }
        return Ok((html.replacen("<head>", &format!("<head><meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">"), 1).into_bytes(), mime));
    }
    Ok((bytes, mime))
}

pub fn rgba(image: &cairo::ImageSurface) -> Result<Vec<u8>, String> {
    if ![cairo::Format::ARgb32, cairo::Format::Rgb24].contains(&image.format()) { return Err("unsupported snapshot format".into()); }
    let width = image.width() as usize;
    let height = image.height() as usize;
    let stride = image.stride() as usize;
    let mut pixels = vec![0; width * height * 4];
    image.with_data(|data| {
        for y in 0..height {
            for x in 0..width {
                let offset = y * stride + x * 4;
                let word = u32::from_ne_bytes(data[offset..offset + 4].try_into().unwrap());
                let alpha = if image.format() == cairo::Format::Rgb24 { 255 } else { (word >> 24) as u8 };
                let dest = &mut pixels[(y * width + x) * 4..][..4];
                for (channel, shift) in [16, 8, 0].iter().enumerate() {
                    let premultiplied = (word >> shift) & 255;
                    dest[channel] = if alpha == 0 { 0 } else { ((premultiplied * 255 + u32::from(alpha) / 2) / u32::from(alpha)).min(255) as u8 };
                }
                dest[3] = alpha;
            }
        }
    }).map_err(|e| e.to_string())?;
    Ok(pixels)
}

fn input(web: &webkit2gtk::WebView, state: &mut State, input: Input) {
    let allowed = state.session.current_frame() && state.fade.input_allowed(state.target) && state.active;
    match input {
        Input::Quit => { state.runtime = None; state.session.submitted = None; state.fade.reset(); state.pressed = None; }
        Input::Move(point) if allowed && point.iter().all(|p| p.is_finite() && (0.0..=1.0).contains(p)) => {
            state.pointer = Some(point);
            event(web, state, "pointer", json!({"key":state.session.key,"point":point}));
        }
        Input::Down(point) if allowed => {
            state.pressed = state.session.layout.as_ref().and_then(|(layout, _)| layout.hit(point)).map(|id| (state.session.key, state.session.paint, id));
        }
        Input::Up(point) if allowed => {
            let hit = state.session.layout.as_ref().and_then(|(layout, _)| layout.hit(point));
            if let Some((key, paint, id)) = state.pressed.take() {
                if key == state.session.key && paint == state.session.paint && Some(id) == hit {
                    state.clicks += 1;
                    match id { 6 => state.pinned = !state.pinned, 7 => state.target = false, _ => {} }
                    publish(web, state);
                    println!("{}", json!({"event":"click_accepted","id":id,"key":key}));
                }
            }
        }
        _ => state.pressed = None,
    }
}

pub fn run(options: Options) -> Result<(), String> {
    gtk::init().map_err(|e| format!("GTK display initialization failed: {e}"))?;
    let root = options.assets.canonicalize().map_err(|e| e.to_string())?;
    resource(&root, SOURCE)?;
    std::fs::create_dir_all(&options.output).map_err(|e| e.to_string())?;
    let context = webkit2gtk::WebContext::new_ephemeral();
    context.register_uri_scheme("vrcs-surface", move |request| {
        if request.http_method().as_deref() != Some("GET") {
            eprintln!("{}", json!({"event":"resource_method_rejected","uri":request.uri().map(|uri| uri.to_string()),"method":request.http_method().map(|method| method.to_string())}));
            request.finish_error(&mut glib::Error::new(gio::IOErrorEnum::PermissionDenied, "surface resources require GET"));
            return;
        }
        let result = request.uri().ok_or_else(|| "missing resource URI".to_owned())
            .and_then(|uri| resource(&root, &uri));
        match result {
            Ok((bytes, mime)) => {
                println!("{}", json!({"event":"resource_served","uri":request.uri().map(|uri| uri.to_string()),"bytes":bytes.len(),"mime":mime}));
                let length = bytes.len() as i64;
                let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes));
                let headers = soup::MessageHeaders::new(soup::MessageHeadersType::Response);
                // WebKit checks CORS for module/preload assets on custom schemes
                // even when the document and resource share this exact origin.
                headers.append("Access-Control-Allow-Origin", "vrcs-surface://localhost");
                headers.append("Content-Type", mime);
                headers.append("Content-Security-Policy", CSP);
                headers.append("X-Content-Type-Options", "nosniff");
                headers.append("Cache-Control", "no-store");
                let response = webkit2gtk::URISchemeResponse::new(&stream, length);
                response.set_content_type(mime);
                response.set_status(200, Some("OK"));
                response.set_http_headers(headers);
                request.finish_with_response(&response);
            }
            Err(error) => {
                eprintln!("{}", json!({"event":"resource_rejected","uri":request.uri().map(|uri| uri.to_string()),"error":error}));
                request.finish_error(&mut glib::Error::new(gio::IOErrorEnum::PermissionDenied, &error));
            }
        }
    });
    if let Some(security) = context.security_manager() {
        security.register_uri_scheme_as_secure("vrcs-surface");
        security.register_uri_scheme_as_local("vrcs-surface");
        security.register_uri_scheme_as_cors_enabled("vrcs-surface");
    }
    let manager = webkit2gtk::UserContentManager::new();
    manager.add_script(&webkit2gtk::UserScript::new(include_str!("port.js"), webkit2gtk::UserContentInjectedFrames::TopFrame,
        webkit2gtk::UserScriptInjectionTime::Start, &[], &[]));
    if !manager.register_script_message_handler("vrcsSurface") { return Err("script message registration failed".into()); }
    let web = webkit2gtk::WebView::builder().web_context(&context).user_content_manager(&manager).build();
    web.set_background_color(&gtk::gdk::RGBA::new(0., 0., 0., 0.));
    web.set_size_request(WIDTH, HEIGHT + 16);
    if let Some(settings) = WebViewExt::settings(&web) {
        // GtkOffscreenWindow has no compositor GL context on Wayland. This
        // experiment measures the intended CPU snapshot/raw upload route.
        settings.set_hardware_acceleration_policy(webkit2gtk::HardwareAccelerationPolicy::Never);
        settings.set_enable_developer_extras(false);
        settings.set_enable_html5_database(false);
        settings.set_enable_html5_local_storage(false);
        settings.set_enable_page_cache(false);
        settings.set_allow_file_access_from_file_urls(false);
        settings.set_allow_universal_access_from_file_urls(false);
        settings.set_enable_write_console_messages_to_stdout(true);
    }
    web.connect_permission_request(|_, request| { request.deny(); true });
    web.connect_load_failed(|_, stage, uri, error| {
        eprintln!("{}", json!({"event":"load_failed","stage":format!("{stage:?}"),"uri":uri,"error":error.to_string()}));
        false
    });
    web.connect_resource_load_started(|_, resource, request| {
        println!("{}", json!({"event":"resource_requested","uri":request.uri().map(|uri| uri.to_string())}));
        resource.connect_failed(|resource, error| {
            eprintln!("{}", json!({"event":"resource_failed","uri":resource.uri().map(|uri| uri.to_string()),"error":error.to_string()}));
        });
    });
    web.connect_create(|_, _| None);
    web.connect_context_menu(|_, _, _, _| true);
    web.connect_decide_policy(|_, decision, kind| {
        if matches!(kind, webkit2gtk::PolicyDecisionType::NavigationAction | webkit2gtk::PolicyDecisionType::NewWindowAction) {
            let allowed = decision.downcast_ref::<webkit2gtk::NavigationPolicyDecision>()
                .and_then(|nav| nav.navigation_action()).and_then(|action| action.request()).and_then(|req| req.uri()).is_some_and(|uri| uri == SOURCE);
            if !allowed { decision.ignore(); return true; }
        }
        false
    });
    // GtkOffscreenWindow maps only an offscreen backing drawable. No compositor
    // toplevel, no visible desktop window, and no call to present/grab_focus.
    let window = gtk::OffscreenWindow::new();
    window.set_accept_focus(false);
    window.set_focus_on_map(false);
    window.set_default_size(WIDTH, HEIGHT + 16);
    window.add(&web);
    window.show_all();
    let started = Instant::now();
    let state = Rc::new(RefCell::new(State {
        session: Session::new(1), fade: Fade::default(), target: true, active: true, activity_request: 1,
        pinned: false, pointer: None, pressed: None, runtime: None, runtime_retry: started,
        started, next_view: started + Duration::from_secs(1), snapshots: 0, accepted: 0, rejected: 0,
        hidden_frames: 0, runtime_submissions: 0, snapshot_us: 0, max_snapshot_us: 0,
        last_pixels: None, hidden_pixels: None, hidden_updates: 0, clicks: 0, stage: 0, error: None,
    }));
    let weak = web.downgrade();
    let messages = state.clone();
    manager.connect_script_message_received(Some("vrcsSurface"), move |_, result| {
        let Some(web) = weak.upgrade() else { return; };
        let Some(js) = result.js_value() else { return; };
        let bytes = js.to_string();
        if bytes.len() > 263000 || web.uri().as_deref() != Some(SOURCE) { return; }
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Message { source: String, message: Value }
        let Ok(message) = serde_json::from_str::<Message>(&bytes) else { return; };
        if message.source != SOURCE { return; }
        let mut state = messages.borrow_mut();
        let envelope = match state.session.decode(&message.message.to_string()) {
            Ok(envelope) => envelope,
            Err(error) => { eprintln!("{}", json!({"event":"bridge_rejected","error":error})); return; }
        };
        if ["connect", "ready", "fault"].contains(&envelope.op.as_str()) {
            println!("{}", json!({"event":"bridge_request","op":envelope.op,"epoch":state.session.epoch}));
        }
        if envelope.op == "connect" {
            state.session.connected = true;
            post(&web, json!({"version":1,"op":"welcome","nonce":state.session.nonce,"host":{"surface":"reader","host_epoch":state.session.epoch}}));
        } else if state.session.connected {
            let current = view(&state);
            let response = state.session.accept(&envelope, current);
            post(&web, match response {
                Ok(value) => json!({"version":1,"op":"reply","nonce":state.session.nonce,"request_id":envelope.request_id,"ok":true,"value":value}),
                Err(error) => json!({"version":1,"op":"reply","nonce":state.session.nonce,"request_id":envelope.request_id,"ok":false,"error":error}),
            });
        }
    });
    let navigation = state.clone();
    web.connect_load_changed(move |web, event| {
        println!("{}", json!({"event":"load_changed","stage":format!("{event:?}")}));
        if event == webkit2gtk::LoadEvent::Started {
            let mut state = navigation.borrow_mut();
            let epoch = state.session.epoch + 1;
            state.session = Session::new(epoch);
            state.fade.reset(); state.pressed = None;
        }
        if event == webkit2gtk::LoadEvent::Finished {
            web.evaluate_javascript("JSON.stringify({href:location.href,origin:location.origin,ready:document.readyState,port:!!window.__vrcsSurfacePort,receive:!!window.__vrcsSurfaceReceive,children:document.getElementById('root')?.childElementCount,errors:window.__vrcsSurfaceDiagnostics})", None, None, gio::Cancellable::NONE, |result| {
                match result { Ok(value) => println!("page_diagnostic: {value}"), Err(error) => eprintln!("page_diagnostic_failed: {error}") }
            });
        }
    });
    let terminated = state.clone();
    web.connect_web_process_terminated(move |_, reason| {
        let mut state = terminated.borrow_mut();
        state.session.submitted = None; state.fade.reset(); state.pressed = None;
        state.error = Some(format!("WebKit process terminated: {reason:?}"));
    });
    let main_loop = glib::MainLoop::new(None, false);
    for signal in [2, 15] {
        let shutdown = main_loop.clone();
        glib::unix_signal_add_local(signal, move || { shutdown.quit(); glib::ControlFlow::Break });
    }
    let busy = Rc::new(Cell::new(false));
    let cancellation = gio::Cancellable::new();
    let capture_cancel = cancellation.clone();
    let tick_state = state.clone();
    let tick_web = web.clone();
    let tick_loop = main_loop.clone();
    let timer = glib::timeout_add_local(Duration::from_millis(33), move || {
        let mut state = tick_state.borrow_mut();
        let now = Instant::now();
        if state.started.elapsed() >= Duration::from_secs(options.seconds) || state.error.is_some() {
            tick_loop.quit(); return glib::ControlFlow::Continue;
        }
        if options.submit_vr && state.runtime.is_none() && now >= state.runtime_retry {
            match Overlay::connect() {
                Ok(runtime) => { state.runtime = Some(runtime); state.session.submitted = None; state.fade.reset(); }
                Err(error) => eprintln!("{error}"),
            }
            state.runtime_retry = now + Duration::from_secs(2);
        }
        if let Some(runtime) = state.runtime.as_ref() {
            match runtime.events() {
                Ok(events) => for next in events { input(&tick_web, &mut state, next); },
                Err(error) => { eprintln!("{error}"); state.runtime = None; state.session.submitted = None; state.fade.reset(); }
            }
        }
        let frame_ready = if state.fade.active() { state.session.submitted.is_some() } else { state.session.current_frame() };
        let target = state.target;
        let fade = state.fade.advance(target, frame_ready, now);
        if let Some(runtime) = state.runtime.as_ref() {
            if let Err(error) = runtime.visibility(fade.factor) {
                eprintln!("{error}"); state.runtime = None; state.session.submitted = None; state.fade.reset();
            }
        }
        let seconds = state.started.elapsed().as_secs();
        // Leave the offscreen webview mapped throughout. The hidden interval is
        // overlay-hidden, not a visible helper window moved outside the screen.
        if seconds >= 4 && state.stage == 0 { state.target = false; state.stage = 1; }
        if seconds >= 5 && state.stage == 1 {
            state.hidden_pixels = state.last_pixels.clone(); state.stage = 2;
            publish(&tick_web, &mut state);
        }
        if seconds >= 7 && state.stage == 2 {
            state.active = false; state.activity_request += 1;
            event(&tick_web, &state, "activity", json!({"active":false,"request":state.activity_request}));
            state.stage = 3;
        }
        if seconds >= 9 && state.stage == 3 {
            state.active = true; state.activity_request += 1; state.target = true;
            event(&tick_web, &state, "activity", json!({"active":true,"request":state.activity_request}));
            publish(&tick_web, &mut state); state.stage = 4;
        }
        if seconds >= 12 && state.stage == 4 {
            state.session.submitted = None; state.fade.reset(); state.pressed = None;
            state.stage = 5;
            drop(state);
            tick_web.reload();
            return glib::ControlFlow::Continue;
        }
        if state.session.ready && state.active && (state.target || state.stage == 2) && now >= state.next_view {
            publish(&tick_web, &mut state); state.next_view = now + Duration::from_secs(1);
        }
        // Exercise real normalized hit-map input on the reader's pin button.
        if state.clicks == 0 && state.session.current_frame() && state.fade.input_allowed(state.target) {
            let point = state.session.layout.as_ref().and_then(|(layout, _)| layout.regions.iter().find(|region| region.id == 6))
                .map(|region| [(region.bounds[0] + region.bounds[2]) / 2., (region.bounds[1] + region.bounds[3]) / 2.]);
            if let Some(point) = point {
                input(&tick_web, &mut state, Input::Down(point)); input(&tick_web, &mut state, Input::Up(point));
            }
        }
        if seconds == 3 && state.session.current_frame() {
            event(&tick_web, &state, "scroll", json!({"key":state.session.key,"delta":0.5}));
        }
        if busy.get() || !state.active || !state.session.ready || state.session.paint == 0 || state.session.current_frame()
            || (!state.target && state.fade.active()) {
            return glib::ControlFlow::Continue;
        }
        let requested_key = state.session.key;
        let requested_paint = state.session.paint;
        let epoch = state.session.epoch;
        drop(state);
        busy.set(true);
        let completed = busy.clone();
        let frames = tick_state.clone();
        let output = options.output.clone();
        let began = Instant::now();
        tick_web.snapshot(webkit2gtk::SnapshotRegion::Visible, webkit2gtk::SnapshotOptions::TRANSPARENT_BACKGROUND,
            Some(&capture_cancel), move |result| {
            completed.set(false);
            let mut state = frames.borrow_mut();
            let elapsed = began.elapsed().as_micros();
            state.snapshots += 1; state.snapshot_us += elapsed; state.max_snapshot_us = state.max_snapshot_us.max(elapsed);
            let accepted = (|| -> Result<(), String> {
                let surface = result.map_err(|e| e.to_string())?;
                let image = cairo::ImageSurface::try_from(surface).map_err(|_| "snapshot is not a Cairo image surface")?;
                let scale = image.width() / WIDTH;
                if scale < 1 || image.width() != WIDTH * scale || image.height() != (HEIGHT + 16) * scale {
                    return Err(format!("unexpected snapshot dimensions {}x{}", image.width(), image.height()));
                }
                let pixels = rgba(&image)?;
                let Some((key, paint)) = protocol::marker(&pixels, image.width() as usize, HEIGHT as usize, scale as usize) else {
                    if state.rejected == 0 {
                        image.write_to_png(&mut std::fs::File::create(output.join("rejected.png")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                    }
                    return Err("snapshot marker not painted".into());
                };
                if !state.active || (!state.target && state.fade.active()) || epoch != state.session.epoch
                    || key != requested_key || key != state.session.key || paint != state.session.paint
                    || !protocol::at_least(paint, requested_paint)
                    || !state.session.layout.as_ref().is_some_and(|(layout, serial)| layout.key == key && protocol::at_least(paint, *serial)) {
                    return Err("stale snapshot/layout receipt".into());
                }
                let crop = pixels[..image.width() as usize * (HEIGHT * scale) as usize * 4].to_vec();
                if options.submit_vr {
                    let runtime = state.runtime.as_ref().ok_or("VR runtime unavailable; snapshot is not a VR submission")?;
                    runtime.submit(&crop, image.width() as u32, (HEIGHT * scale) as u32)?;
                    state.runtime_submissions += 1;
                }
                state.session.submitted = Some((key, paint));
                state.accepted += 1;
                if !state.target && state.stage == 2 {
                    state.hidden_frames += 1;
                    if state.hidden_pixels.as_ref().is_some_and(|previous| previous != &crop) { state.hidden_updates += 1; }
                }
                if state.accepted == 1 || (state.hidden_frames == 1 && !state.target) {
                    let path = output.join(if state.accepted == 1 { "first.png" } else { "hidden.png" });
                    image.write_to_png(&mut std::fs::File::create(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                    println!("{}", json!({"event":"snapshot_accepted","key":key,"paint":paint,"width":image.width(),"height":image.height(),"snapshot_us":elapsed,"transparent_corner":crop[3] == 0,"vr_submitted":options.submit_vr}));
                }
                state.last_pixels = Some(crop);
                Ok(())
            })();
            if let Err(error) = accepted {
                state.rejected += 1;
                if state.rejected <= 5 { eprintln!("snapshot rejected: {error}"); }
            }
        });
        glib::ControlFlow::Continue
    });
    web.load_uri(SOURCE);
    main_loop.run();
    timer.remove();
    cancellation.cancel();
    manager.unregister_script_message_handler("vrcsSurface");
    web.stop_loading();
    window.close();
    let mut state = state.borrow_mut();
    state.runtime = None;
    println!("{}", json!({"event":"summary","offscreen":true,"visible_window":false,"elapsed_ms":state.started.elapsed().as_millis(),
        "connected":state.session.connected,"page_ready":state.session.ready,"paint_serial":state.session.paint,"layout_ready":state.session.layout.is_some(),
        "snapshots":state.snapshots,"accepted":state.accepted,"rejected":state.rejected,"hidden_frames":state.hidden_frames,
        "hidden_updates":state.hidden_updates,"clicks":state.clicks,"mean_snapshot_us":state.snapshot_us.checked_div(u128::from(state.snapshots)),
        "max_snapshot_us":state.max_snapshot_us,"runtime_submissions":state.runtime_submissions,"host_epoch":state.session.epoch,
        "process_status":std::fs::read_to_string("/proc/self/status").ok(),"error":state.error}));
    if let Some(error) = state.error.take() { return Err(error); }
    if state.accepted == 0 { return Err("No matching WebKit frame; hidden snapshot route is unverified".into()); }
    if options.seconds >= 14 && (state.hidden_updates == 0 || state.clicks == 0 || state.session.epoch < 3) {
        return Err("Hidden updates, input or document recovery did not complete; inspect prototype JSON/PNG".into());
    }
    Ok(())
}
