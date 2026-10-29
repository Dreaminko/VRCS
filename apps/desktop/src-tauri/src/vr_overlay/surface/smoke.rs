use super::*;
use native_host::CaptureEndpoint;
use tauri::{Emitter, WebviewUrl, WebviewWindowBuilder};

/// Exercises hidden Composition hosts and shared GPU textures without initializing OpenVR.
#[cfg(all(debug_assertions, feature = "vr-preview"))]
pub fn smoke(app: AppHandle, views: Vec<(SurfaceId, SurfaceView)>) {
    fn check_player_display_bridge(app: &AppHandle) -> Result<(), String> {
        use tauri::Listener;
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let listener = app.once("vr-player-display-smoke", move |event| {
            let result = serde_json::from_str::<Option<String>>(event.payload())
                .map_err(|error| error.to_string())
                .and_then(|error| error.map_or(Ok(()), Err));
            let _ = sender.send(result);
        });
        let host_app = app.clone();
        app.run_on_main_thread(move || {
            // The real main capability is exercised without loading App or starting Core.
            let result = WebviewWindowBuilder::new(
                &host_app,
                "main",
                WebviewUrl::App("tests/vr-player-display.smoke.html".into()),
            )
            .transparent(true)
            .visible(false)
            .focused(false)
            .skip_taskbar(true)
            .build();
            if let Err(error) = result {
                let _ = host_app.emit("vr-player-display-smoke", Some(error.to_string()));
            }
        })
        .map_err(|error| error.to_string())?;
        let result = receiver
            .recv_timeout(Duration::from_secs(20))
            .map_err(|error| format!("Main-window player display bridge timed out: {error}"));
        app.unlisten(listener);
        result?
    }

    fn wait_for_layout(
        app: &AppHandle,
        surface: SurfaceId,
        view: &SurfaceView,
        previous: Option<SurfaceKey>,
    ) -> Result<(CaptureEndpoint, SurfaceKey), String> {
        let hub = app.state::<SurfaceHub>();
        let started = Instant::now();
        loop {
            hub.set_active(app, surface, true)?;
            hub.publish(app, surface, view.clone(), None, None)?;
            {
                let entries = hub.0.lock().map_err(|e| e.to_string())?;
                let entry = &entries[surface.index()];
                if let Some(error) = &entry.error {
                    return Err(format!("{} layout: {error}", surface.name()));
                }
                if entry.ready {
                    if let (Some(endpoint), Some(layout)) = (&entry.endpoint, &entry.layout) {
                        if entry
                            .state
                            .as_ref()
                            .is_some_and(|state| state.key == layout.key)
                            && previous.is_none_or(|key| key.generation != layout.key.generation)
                        {
                            if unsafe {
                                windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(
                                    endpoint.parent as _,
                                )
                            } != 0
                            {
                                return Err(format!("{} parent is visible", surface.name()));
                            }
                            use windows_sys::Win32::UI::WindowsAndMessaging::{
                                GetWindowLongPtrW, GWL_EXSTYLE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
                            };
                            let style =
                                unsafe { GetWindowLongPtrW(endpoint.parent as _, GWL_EXSTYLE) }
                                    as u32;
                            if style & (WS_EX_APPWINDOW | WS_EX_TOOLWINDOW) != WS_EX_TOOLWINDOW {
                                return Err(format!(
                                    "{} host lost its tool window style",
                                    surface.name()
                                ));
                            }
                            return Ok((endpoint.clone(), layout.key));
                        }
                    }
                }
            }
            if started.elapsed() >= Duration::from_secs(20) {
                let entries = hub.0.lock().map_err(|e| e.to_string())?;
                let entry = &entries[surface.index()];
                return Err(format!(
                    "{} host did not report a new layout (epoch={}, generation={}, ready={}, awake={}, endpoint={}, layout={}, previous={previous:?})",
                    surface.name(), entry.host_epoch, entry.generation, entry.ready, entry.host_awake, entry.endpoint.is_some(), entry.layout.is_some()
                ));
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn wait_for_frame(
        app: &AppHandle,
        capture: &mut windows_wgc::Capture,
        surface: SurfaceId,
        key: SurfaceKey,
    ) -> Result<windows_wgc::Captured, String> {
        let started = Instant::now();
        loop {
            let requested = {
                let hub = app.state::<SurfaceHub>();
                let entries = hub.0.lock().map_err(|e| e.to_string())?;
                let entry = &entries[surface.index()];
                entry.needs_paint().then_some(entry.paint).flatten()
            };
            if let Some(frame) = capture.poll()? {
                let hub = app.state::<SurfaceHub>();
                let entries = hub.0.lock().map_err(|e| e.to_string())?;
                if (frame.generation, frame.revision) == (key.generation, key.revision)
                    && entries[surface.index()].accepts_frame(
                        key,
                        frame.paint_serial,
                        frame.requested_serial,
                    )
                {
                    return Ok(frame);
                }
            }
            capture.start(requested)?;
            if started.elapsed() >= Duration::from_secs(10) {
                return Err(format!(
                    "{} capture did not contain its marker",
                    surface.name()
                ));
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    std::thread::spawn(move || {
        let result = (|| -> Result<Vec<serde_json::Value>, String> {
            let directory = std::path::Path::new("target/vr-surface-smoke");
            std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
            let hub = app.state::<SurfaceHub>();
            let mut results = Vec::new();
            let lifecycle_view = views
                .iter()
                .find(|(surface, _)| *surface == SurfaceId::Menu)
                .ok_or("Menu smoke view is missing")?
                .1
                .clone();
            let reader = views
                .iter()
                .find(|(surface, _)| *surface == SurfaceId::Reader)
                .ok_or("Reader smoke view is missing")?
                .1
                .clone();
            results.push(native_reader(&app, reader)?);
            check_player_display_bridge(&app)?;
            // Keep sibling hosts alive when a recovery check destroys one window.
            for (surface, view) in &views {
                wait_for_layout(&app, *surface, view, None)?;
                // A live animated sibling without a capture consumer would correctly overload.
                hub.set_active(&app, *surface, false)?;
            }
            for (surface, view) in views {
                let case = match &view {
                    SurfaceView::Menu { ui, .. }
                        if ui.page != super::super::wrist_menu::Page::Home =>
                    {
                        "menu-expanded"
                    }
                    _ => surface.name(),
                };
                let (endpoint, mut key) = wait_for_layout(&app, surface, &view, None)?;
                let (width, height) = surface.size();
                let counters = hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                    .metrics
                    .capture
                    .clone();
                let mut capture = windows_wgc::Capture::with_counters(
                    &endpoint.item,
                    0,
                    width,
                    height,
                    counters.clone(),
                )?;
                let first_frame = wait_for_frame(&app, &mut capture, surface, key)?;
                let first = capture.snapshot(first_frame)?;
                let transparent = first
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| pixel[3] == 0)
                    .count();
                let visible = first
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| pixel[3] > 0)
                    .count();
                let translucent = first
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| pixel[3] > 0 && pixel[3] < 255)
                    .count();
                if (surface != SurfaceId::DashboardThumbnail
                    && (transparent == 0 || translucent == 0))
                    || visible < 100
                {
                    return Err(format!(
                        "{} capture lost transparency or content",
                        surface.name()
                    ));
                }
                std::fs::write(directory.join(format!("{case}.bgra")), &first)
                    .map_err(|e| e.to_string())?;
                capture.submitted(first_frame)?;
                hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                    .commit_paint(key, first_frame.paint_serial);
                // Ignore late font receipts before measuring the settled static interval.
                let settle_until = Instant::now() + Duration::from_millis(350);
                while Instant::now() < settle_until {
                    if let Some(frame) = capture.poll()? {
                        let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
                        if entries[surface.index()].accepts_frame(
                            key,
                            frame.paint_serial,
                            frame.requested_serial,
                        ) {
                            entries[surface.index()].commit_paint(key, frame.paint_serial);
                            // Protect first_frame for the existing buffer-preservation check below.
                        }
                    }
                    let requested = {
                        let entries = hub.0.lock().map_err(|e| e.to_string())?;
                        let entry = &entries[surface.index()];
                        entry.needs_paint().then_some(entry.paint).flatten()
                    };
                    capture.start(requested)?;
                    std::thread::sleep(Duration::from_millis(15));
                }
                let static_surface = surface != SurfaceId::OcrProgress;
                let copies_before_idle = counters.copies.load(std::sync::atomic::Ordering::Relaxed);
                if static_surface {
                    let until = Instant::now() + Duration::from_secs(2);
                    while Instant::now() < until {
                        let requested = {
                            let entries = hub.0.lock().map_err(|e| e.to_string())?;
                            let entry = &entries[surface.index()];
                            entry.needs_paint().then_some(entry.paint).flatten()
                        };
                        if capture.poll()?.is_some() || capture.start(requested)? {
                            return Err("Static surface requested another copy".into());
                        }
                        std::thread::sleep(Duration::from_millis(15));
                    }
                    if counters.copies.load(std::sync::atomic::Ordering::Relaxed)
                        != copies_before_idle
                    {
                        return Err(format!("{} made idle texture copies", surface.name()));
                    }
                }
                let hovered = hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                    .layout
                    .as_ref()
                    .and_then(|layout| layout.regions.last())
                    .map(|region| region.id);
                let mut previous = first.clone();
                for hover in hovered
                    .into_iter()
                    .flat_map(|id| [Some(id), None, Some(id)])
                {
                    hub.publish(&app, surface, view.clone(), hover, None)?;
                    let changed_at = std::time::Instant::now();
                    let mut last_frame = None;
                    let paint_before_hover =
                        hub.0.lock().map_err(|e| e.to_string())?[surface.index()].paint;
                    let copies_before_hover =
                        counters.copies.load(std::sync::atomic::Ordering::Relaxed);
                    let frames_before_hover = counters
                        .source_frames
                        .load(std::sync::atomic::Ordering::Relaxed);
                    loop {
                        let requested =
                            hub.0.lock().map_err(|e| e.to_string())?[surface.index()].paint;
                        if let Some(frame) = capture.poll()? {
                            last_frame = Some(frame);
                            if capture.snapshot(first_frame)? != first {
                                return Err(format!(
                                    "{} capture overwrote its submitted texture",
                                    surface.name()
                                ));
                            }
                            let pixels = capture.snapshot(frame)?;
                            if (frame.generation, frame.revision) == (key.generation, key.revision)
                                && pixels != previous
                            {
                                previous = pixels;
                                break;
                            }
                        }
                        capture.start(requested)?;
                        if changed_at.elapsed().as_secs() >= 5 {
                            let entries = hub.0.lock().map_err(|e| e.to_string())?;
                            let entry = &entries[surface.index()];
                            return Err(format!(
                                "{} hover {hover:?} did not update its captured texture: paint={paint_before_hover:?}->{:?}, feedback={:?}, last_frame={last_frame:?}, copies={}, frames={}",
                                surface.name(), entry.paint, entry.state.as_ref().and_then(|state| state.hovered),
                                counters.copies.load(std::sync::atomic::Ordering::Relaxed) - copies_before_hover,
                                counters.source_frames.load(std::sync::atomic::Ordering::Relaxed) - frames_before_hover
                            ));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                }
                if hovered.is_none() && surface != SurfaceId::DashboardThumbnail {
                    let mut updated = view.clone();
                    match &mut updated {
                        SurfaceView::Headset { messages, .. } => {
                            *messages = vec![WristMessage {
                                original: None,
                                text: "Updated caption / 字幕内容已更新".into(),
                                side: super::super::presentation::MessageSide::Left,
                                speaker: None,
                            }];
                        }
                        SurfaceView::OcrProgress {
                            progress: Some(progress),
                            ..
                        } => {
                            progress.status = "Updated OCR progress".into();
                            progress.translation_fraction = Some((2, 3));
                        }
                        SurfaceView::OcrResult { scene, .. } => {
                            for block in &mut scene.blocks {
                                block.text = "Updated OCR translation / 已更新".into();
                            }
                        }
                        SurfaceView::OcrSelection { dark } => {
                            *dark = !*dark;
                        }
                        _ => return Err("Smoke surface has no content to update".into()),
                    }
                    hub.publish(&app, surface, updated, None, None)?;
                    key = hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                        .state
                        .as_ref()
                        .ok_or("Smoke surface has no state")?
                        .key;
                    let frame = wait_for_frame(&app, &mut capture, surface, key)?;
                    if capture.snapshot(frame)? == first {
                        return Err(format!("{} content did not update", surface.name()));
                    }
                    if capture.snapshot(first_frame)? != first {
                        return Err(format!(
                            "{} capture overwrote its submitted texture",
                            surface.name()
                        ));
                    }
                }
                // The same source shutdown and host request used by the worker must be reversible.
                capture.suspend();
                hub.set_active(&app, surface, false)?;
                if capture.active() {
                    return Err("Hidden source remained active".into());
                }
                let hidden_copies = counters.copies.load(std::sync::atomic::Ordering::Relaxed);
                let until = Instant::now() + Duration::from_secs(2);
                while Instant::now() < until {
                    if capture.poll()?.is_some() || capture.start(Some(1))? {
                        return Err("Hidden capture produced a frame".into());
                    }
                    std::thread::sleep(Duration::from_millis(15));
                }
                if counters.copies.load(std::sync::atomic::Ordering::Relaxed) != hidden_copies {
                    return Err("Hidden capture copied pixels".into());
                }
                let (_, warm_key) = wait_for_layout(&app, surface, &view, None)?;
                if warm_key == key {
                    return Err("Wake reused an old input key".into());
                }
                key = warm_key;
                capture.resume()?;
                let warm_frame = wait_for_frame(&app, &mut capture, surface, key)?;
                capture.submitted(warm_frame)?;
                hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                    .commit_paint(key, warm_frame.paint_serial);
                let held = capture.snapshot(warm_frame)?;
                capture.restart_source(&endpoint.item)?;
                hub.send(&app, surface, SurfaceEvent::Repaint)?;
                let recovered = wait_for_frame(&app, &mut capture, surface, key)?;
                if capture.snapshot(warm_frame)? != held {
                    return Err("Source restart overwrote the submitted texture".into());
                }
                capture.submitted(recovered)?;
                hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                    .commit_paint(key, recovered.paint_serial);
                for recreate in [false, true] {
                    drop(capture);
                    let generations = hub
                        .0
                        .lock()
                        .map_err(|e| e.to_string())?
                        .each_ref()
                        .map(|entry| entry.generation);
                    if recreate {
                        let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
                        entries[surface.index()].invalidate_host();
                        if entries[surface.index()].active_layout().is_some() {
                            return Err("Invalidated host retained its input layout".into());
                        }
                    } else {
                        let host_epoch =
                            hub.0.lock().map_err(|e| e.to_string())?[surface.index()].host_epoch;
                        app.state::<NativeHostManager>().reload(HostId {
                            surface,
                            host_epoch,
                        });
                    }
                    let (endpoint, recovered) = wait_for_layout(&app, surface, &view, Some(key))?;
                    capture = windows_wgc::Capture::with_counters(
                        &endpoint.item,
                        0,
                        width,
                        height,
                        counters.clone(),
                    )?;
                    let frame = wait_for_frame(&app, &mut capture, surface, recovered)?;
                    let pixels = capture.snapshot(frame)?;
                    if surface != SurfaceId::DashboardThumbnail
                        && !pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] == 0)
                        || pixels
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .filter(|pixel| pixel[3] > 0)
                            .count()
                            < 100
                    {
                        return Err(format!("{} recovery lost its content", surface.name()));
                    }
                    capture.submitted(frame)?;
                    let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
                    if entries.iter().enumerate().any(|(index, entry)| {
                        index != surface.index() && entry.generation != generations[index]
                    }) {
                        return Err("Surface recovery changed another host".into());
                    }
                    if !entries[surface.index()].commit_paint(recovered, frame.paint_serial) {
                        return Err("Recovered frame did not match the current layout".into());
                    }
                    key = recovered;
                }
                let mut pipelined_ticks = 0;
                let mut hold_submissions = 0;
                if surface == SurfaceId::Menu && metrics::enabled() {
                    let mut renderer = SurfaceRenderer::new(surface);
                    renderer.capture = Some(Rc::new(RefCell::new(capture)));
                    for step in 0..60 {
                        let started = Instant::now();
                        let mut holding = view.clone();
                        if let SurfaceView::Menu { hold_progress, .. } = &mut holding {
                            *hold_progress = Some(step as f32 / 59.0);
                        }
                        hub.publish(&app, surface, holding, Some(0), Some(0))?;
                        let copies = counters.copies.load(std::sync::atomic::Ordering::Relaxed);
                        let mut submitted = false;
                        renderer.capture_frame(&app, &hub, |_| {
                            submitted = true;
                            Ok(())
                        })?;
                        hold_submissions += u32::from(submitted);
                        if submitted
                            && counters.copies.load(std::sync::atomic::Ordering::Relaxed) > copies
                        {
                            pipelined_ticks += 1;
                        }
                        std::thread::sleep(
                            Duration::from_millis(16).saturating_sub(started.elapsed()),
                        );
                    }
                    if pipelined_ticks == 0 {
                        return Err(format!(
                            "Menu capture never submitted and started a copy in the same tick ({hold_submissions} hold frames)"
                        ));
                    }
                    capture =
                        Rc::try_unwrap(renderer.capture.take().expect("smoke capture exists"))
                            .map_err(|_| "Smoke capture still has an owner")?
                            .into_inner();
                }
                capture.suspend();
                hub.set_active(&app, surface, false)?;
                results.push(serde_json::json!({"surface":surface,"case":case,"width":width,"height":height,"key":key,"desktop_hidden":true,"transparent_pixels":transparent,"translucent_pixels":translucent,"visible_pixels":visible,"hover_updated":hovered.map(|_| true),"content_updated":(surface != SurfaceId::DashboardThumbnail).then_some(hovered.is_none()),"submitted_texture_preserved":true,"source_restarted":true,"reload_recovered":true,"host_recreated":true,"idle_no_copies":static_surface.then_some(true),"hidden_source_closed":true,"hidden_no_copies":true,"warm_recovered":true,"pipelined_ticks":pipelined_ticks,"hold_submissions":hold_submissions}));
            }
            results.push(lifecycle(&app, lifecycle_view)?);
            if app.get_webview_window("main").is_none() {
                return Err("Default desktop WebView was closed during capture smoke".into());
            }
            let hosts = app.state::<NativeHostManager>();
            hosts.begin_shutdown();
            hosts.finish_shutdown(Duration::from_secs(5))?;
            if hosts.counts() != (0, 0) {
                return Err("Native shutdown left live hosts or creates".into());
            }
            results.push(
                serde_json::json!({"case":"shutdown", "hosts":0, "inflight":0, "drained":true}),
            );
            Ok(results)
        })();
        let report = match &result {
            Ok(surfaces) => {
                serde_json::json!({"ok":true,"desktop_webview_coexisted":true,"player_display_bridge":true,"surfaces":surfaces})
            }
            Err(error) => serde_json::json!({"ok":false,"error":error}),
        };
        eprintln!("VR surface capture smoke: {report}");
        let _ = std::fs::write(
            "target/vr-surface-smoke/result.json",
            serde_json::to_vec_pretty(&report).unwrap_or_default(),
        );
        app.exit(if result.is_ok() { 0 } else { 1 });
    });
}

fn lifecycle(app: &AppHandle, view: SurfaceView) -> Result<serde_json::Value, String> {
    let surface = SurfaceId::Menu;
    let hub = app.state::<SurfaceHub>();
    let hosts = app.state::<NativeHostManager>();
    let mut renderer = SurfaceRenderer::new(surface);
    let pump = |renderer: &mut SurfaceRenderer| -> Result<SurfaceKey, String> {
        let started = Instant::now();
        loop {
            renderer.request(app)?;
            hub.publish(app, surface, view.clone(), None, None)?;
            renderer.advance_recovery(&hub, Instant::now());
            renderer.apply_recovery(app, || Ok(()), Instant::now())?;
            let endpoint = {
                let entries = hub.0.lock().map_err(|e| e.to_string())?;
                let entry = &entries[surface.index()];
                if entry.recovery.phase == recovery::Phase::Failed {
                    return Err(entry.error.clone().unwrap_or_default());
                }
                entry.capture_generation().and_then(|generation| {
                    entry
                        .endpoint
                        .clone()
                        .map(|endpoint| (endpoint, generation))
                })
            };
            if let Some((endpoint, generation)) = endpoint {
                if unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(
                        endpoint.parent as _,
                    )
                } != 0
                {
                    return Err("Lifecycle parent became visible".into());
                }
                if renderer.capture.is_none() {
                    let (width, height) = surface.size();
                    let counters = hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                        .metrics
                        .capture
                        .clone();
                    renderer.capture =
                        Some(Rc::new(RefCell::new(windows_wgc::Capture::with_counters(
                            &endpoint.item,
                            0,
                            width,
                            height,
                            counters,
                        )?)));
                    renderer.identity = Some((endpoint.host.host_epoch, 0, generation));
                }
                renderer.capture_frame(app, &hub, |_| Ok(()))?;
                let entries = hub.0.lock().map_err(|e| e.to_string())?;
                let entry = &entries[surface.index()];
                if entry.active_layout().is_some() && renderer.frame_visible(entry, Instant::now())
                {
                    return Ok(entry.state.as_ref().unwrap().key);
                }
            }
            if started.elapsed() >= Duration::from_secs(25) {
                return Err("Lifecycle did not recover a usable frame".into());
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    };
    pump(&mut renderer)?;
    for _ in 0..50 {
        renderer.finish_tick(app, None, OverlayKind::WristMenu);
        if renderer
            .capture
            .as_ref()
            .is_some_and(|capture| capture.borrow().active())
            || hub.layout(surface).is_some()
        {
            return Err("Hidden lifecycle retained capture or input".into());
        }
        pump(&mut renderer)?;
    }
    let before = renderer.identity;
    renderer.capture_fault = Some(windows_wgc::CaptureError::new(
        recovery::Fault::Session,
        "smoke_session_fault",
    ));
    pump(&mut renderer)?;
    if renderer.identity != before {
        return Err("Session recovery replaced the host or device".into());
    }
    renderer.finish_tick(app, None, OverlayKind::WristMenu);
    let hidden = Instant::now();
    while hidden.elapsed() < Duration::from_secs(60) {
        renderer.finish_tick(app, None, OverlayKind::WristMenu);
        std::thread::sleep(Duration::from_millis(100));
    }
    renderer.finish_tick(app, None, OverlayKind::WristMenu);
    if renderer.capture.is_some()
        || hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
            .endpoint
            .is_some()
    {
        return Err("Cold lifecycle retained its capture or host".into());
    }
    let before_crash = pump(&mut renderer)?;
    hosts.crash_browser();
    let failed = Instant::now();
    while hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
        .error
        .is_none()
    {
        if failed.elapsed() >= Duration::from_secs(5) {
            return Err("Isolated browser exit was not observed".into());
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    let after_crash = pump(&mut renderer)?;
    if before_crash.generation == after_crash.generation {
        return Err("Browser failure reused an old document".into());
    }
    renderer.finish_tick(app, None, OverlayKind::WristMenu);
    renderer.reset();
    Ok(
        serde_json::json!({"case":"lifecycle", "hide_wake_cycles":50, "cold_wait_seconds":hidden.elapsed().as_secs(), "source_recovered":true, "browser_crash_recovered":true}),
    )
}

fn native_reader(app: &AppHandle, mut view: SurfaceView) -> Result<serde_json::Value, String> {
    use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::IsWindowVisible};
    let surface = SurfaceId::Reader;
    let hub = app.state::<SurfaceHub>();
    let hosts = app.state::<NativeHostManager>();
    if let SurfaceView::Reader {
        reader, opacity, ..
    } = &mut view
    {
        reader.source = "Stable source text.\n".repeat(100);
        reader.translation = "Reader answer line.\n".repeat(120);
        reader.source_only = false;
        *opacity = 0.82;
    }
    let id = {
        let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
        let entry = &mut entries[surface.index()];
        entry.set_active(true, Instant::now());
        entry.publish(view.clone(), None, None);
        entry.creating = true;
        bridge::HostId {
            surface,
            host_epoch: entry.host_epoch,
        }
    };
    hosts.ensure(id)?;
    let result = (|| {
        let started = Instant::now();
        let endpoint = loop {
            let entries = hub.0.lock().map_err(|e| e.to_string())?;
            let entry = &entries[surface.index()];
            if let Some(error) = &entry.error {
                return Err(error.clone());
            }
            if entry.ready && entry.layout.is_some() {
                if let Some(endpoint) = &entry.endpoint {
                    break endpoint.clone();
                }
            }
            if started.elapsed() >= Duration::from_secs(20) {
                return Err("Native reader did not report a layout".into());
            }
            drop(entries);
            std::thread::sleep(Duration::from_millis(15));
        };
        let hidden = || {
            if unsafe { IsWindowVisible(HWND(endpoint.parent as *mut _)) }.as_bool() {
                Err("Native reader parent became visible".to_owned())
            } else {
                Ok(())
            }
        };
        hidden()?;
        let counters = hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
            .metrics
            .capture
            .clone();
        let (width, height) = surface.size();
        let mut capture =
            windows_wgc::Capture::with_counters(&endpoint.item, 0, width, height, counters)?;
        let mut frame = || -> Result<Vec<u8>, String> {
            let started = Instant::now();
            loop {
                hidden()?;
                let requested = {
                    let entries = hub.0.lock().map_err(|e| e.to_string())?;
                    let entry = &entries[surface.index()];
                    entry.needs_paint().then_some(entry.paint).flatten()
                };
                if let Some(frame) = capture.poll()? {
                    let key = SurfaceKey {
                        generation: frame.generation,
                        revision: frame.revision,
                    };
                    if hub.0.lock().map_err(|e| e.to_string())?[surface.index()].accepts_frame(
                        key,
                        frame.paint_serial,
                        frame.requested_serial,
                    ) {
                        let pixels = capture.snapshot(frame)?;
                        capture.submitted(frame)?;
                        hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                            .commit_paint(key, frame.paint_serial);
                        return Ok(pixels);
                    }
                }
                capture.start(requested)?;
                if started.elapsed() >= Duration::from_secs(3) {
                    return Err("Native reader did not produce a matching marker".into());
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        };
        let first = frame()?;
        let transparent = first
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] == 0)
            .count();
        let translucent = first
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[3] > 0 && pixel[3] < 255)
            .count();
        if transparent == 0 || translucent < 100 {
            return Err("Native reader lost transparent or translucent pixels".into());
        }
        std::fs::write("target/vr-surface-smoke/native-reader.bgra", &first)
            .map_err(|e| e.to_string())?;
        let start = Instant::now();
        let mut changes = 0;
        let mut scroll_changes = 0;
        let mut previous = first;
        while start.elapsed() < Duration::from_secs(30) {
            let state = {
                let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
                let entry = &mut entries[surface.index()];
                let mut next = view.clone();
                if let SurfaceView::Reader { reader, .. } = &mut next {
                    reader.detail = format!("Update {changes}");
                }
                entry.publish(next, None, None);
                entry.state.clone().unwrap()
            };
            hosts.send(id, bridge::SurfaceEvent::View(state))?;
            let pixels = frame()?;
            if pixels != previous {
                changes += 1;
            }
            let key = hub.0.lock().map_err(|e| e.to_string())?[surface.index()]
                .scroll_key()
                .ok_or("Native reader scroll layout is unavailable")?;
            hosts.send(
                id,
                bridge::SurfaceEvent::Scroll(SurfaceScroll {
                    key,
                    delta: if scroll_changes % 2 == 0 { 0.25 } else { -0.25 },
                }),
            )?;
            previous = frame()?;
            if previous != pixels {
                scroll_changes += 1;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if changes < 2 || scroll_changes < 2 {
            return Err("Native reader did not update its content and scroll pixels".into());
        }
        drop(frame);
        capture.suspend();
        drop(capture);
        Ok(
            serde_json::json!({ "surface": "reader", "host": "composition", "parent_hidden": true, "duration_seconds": start.elapsed().as_secs_f32(), "content_changes": changes, "scroll_changes": scroll_changes, "transparent_pixels": transparent, "translucent_pixels": translucent }),
        )
    })();
    {
        let mut entries = hub.0.lock().map_err(|e| e.to_string())?;
        entries[surface.index()].set_active(false, Instant::now());
        entries[surface.index()].invalidate_host();
    }
    hosts.close(id)?;
    result
}
