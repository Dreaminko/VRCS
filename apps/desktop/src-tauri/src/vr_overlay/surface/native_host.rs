use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use webview2_com::{
    CoTaskMemPWSTR, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2CompositionControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, DownloadStartingEventHandler,
    Microsoft::Web::WebView2::Win32::*, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler,
    PermissionRequestedEventHandler, ProcessFailedEventHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler,
};
use windows::{
    core::{w, Interface, BOOL, PCWSTR, PWSTR},
    Graphics::Capture::GraphicsCaptureItem,
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        System::{
            Threading::GetCurrentThreadId,
            WinRT::{
                CreateDispatcherQueueController, DispatcherQueueOptions, RoInitialize,
                RoUninitialize, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, RO_INIT_SINGLETHREADED,
            },
        },
        UI::{Shell::SHCreateMemStream, WindowsAndMessaging::*},
    },
    UI::Composition::{Compositor, ContainerVisual},
};

use super::bridge::{
    DocumentSession, HostId, Operation, PageRequest, ResourcePolicy, ResourceRoute, SurfaceEvent,
};
use super::{SurfaceActivity, SurfaceHub};

const WAKE: u32 = WM_APP + 41;
const BROWSER_ARGS: &str = "--disable-features=CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows --disable-renderer-backgrounding";
const SURFACE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self'; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

#[derive(Clone)]
pub(super) struct CaptureEndpoint {
    pub host: HostId,
    pub item: GraphicsCaptureItem,
    #[cfg(all(debug_assertions, feature = "vr-preview"))]
    pub parent: isize,
}

struct QueuedEvent {
    session: DocumentSession,
    event: SurfaceEvent,
}

struct Shared {
    control: Mutex<ControlMailbox>,
    events: mpsc::SyncSender<QueuedEvent>,
    thread_id: AtomicU32,
    notified: AtomicBool,
    error: Mutex<Option<String>>,
    #[cfg(all(debug_assertions, feature = "vr-preview"))]
    counts: Mutex<(usize, usize)>,
}

impl Shared {
    fn wake(&self) {
        let thread = self.thread_id.load(Ordering::Acquire);
        if thread != 0 && !self.notified.swap(true, Ordering::AcqRel) {
            if unsafe { PostThreadMessageW(thread, WAKE, WPARAM(0), LPARAM(0)) }.is_err() {
                self.notified.store(false, Ordering::Release);
            }
        }
    }

    fn current(&self, id: HostId) -> Option<SurfaceActivity> {
        self.control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .current(id)
    }
}

struct HostThread {
    join: JoinHandle<()>,
    done: mpsc::Receiver<Result<(), String>>,
}

pub(crate) struct NativeHostManager {
    app: AppHandle,
    shared: Arc<Shared>,
    events: Mutex<Option<mpsc::Receiver<QueuedEvent>>>,
    thread: Mutex<Option<HostThread>>,
}

impl NativeHostManager {
    pub fn start(app: AppHandle) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel(256);
        Ok(Self {
            app,
            shared: Arc::new(Shared {
                control: Mutex::new(ControlMailbox::default()),
                events: tx,
                thread_id: AtomicU32::new(0),
                notified: AtomicBool::new(false),
                error: Mutex::new(None),
                #[cfg(all(debug_assertions, feature = "vr-preview"))]
                counts: Mutex::new((0, 0)),
            }),
            events: Mutex::new(Some(rx)),
            thread: Mutex::new(None),
        })
    }

    pub(super) fn ensure(&self, id: HostId) -> Result<(), String> {
        let activity = {
            let hub = self.app.state::<SurfaceHub>();
            let entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
            let entry = &entries[id.surface.index()];
            if entry.host_epoch != id.host_epoch || !entry.active {
                return Ok(());
            }
            entry.activity()
        };
        if let Some(error) = self
            .shared
            .error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Err(error);
        }
        if !self
            .shared
            .control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ensure(DesiredHost { id, activity })
        {
            return Err("Native surface host is closed or stale".into());
        }
        let mut thread = self.thread.lock().unwrap_or_else(|e| e.into_inner());
        if thread.is_none() {
            let events = self
                .events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .ok_or("Native surface thread has stopped")?;
            let shared = self.shared.clone();
            let app = self.app.clone();
            let (done, receiver) = mpsc::sync_channel(1);
            let join = std::thread::Builder::new()
                .name("vr-surface-sta".into())
                .spawn(move || {
                    let result = run(app, shared.clone(), events);
                    if let Err(error) = &result {
                        *shared.error.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(error.clone());
                    }
                    shared.thread_id.store(0, Ordering::Release);
                    let _ = done.send(result);
                })
                .map_err(|e| e.to_string())?;
            *thread = Some(HostThread {
                join,
                done: receiver,
            });
        }
        drop(thread);
        self.shared.wake();
        Ok(())
    }

    pub(super) fn send(&self, id: HostId, event: SurfaceEvent) -> Result<(), String> {
        if self.shared.current(id).is_none() {
            return Err("Stale surface host".into());
        }
        let session = self
            .app
            .state::<SurfaceHub>()
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())[id.surface.index()]
        .document
        .clone();
        let Some(session) = session.filter(|session| session.host == id) else {
            return Ok(());
        };
        self.shared
            .events
            .try_send(QueuedEvent { session, event })
            .map_err(|_| "Surface event queue is full or closed")?;
        self.shared.wake();
        Ok(())
    }

    pub(super) fn set_activity(&self, id: HostId, activity: SurfaceActivity) -> Result<(), String> {
        self.shared
            .control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .activity(id, activity);
        self.shared.wake();
        Ok(())
    }

    pub(super) fn close(&self, id: HostId) -> Result<(), String> {
        self.shared
            .control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .close(id);
        self.shared.wake();
        Ok(())
    }

    pub fn begin_shutdown(&self) {
        self.shared
            .control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .closing = true;
        for entry in self
            .app
            .state::<SurfaceHub>()
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter_mut()
        {
            entry.recovery.phase = super::recovery::Phase::Closing;
            entry.document = None;
        }
        self.shared.wake();
    }

    #[cfg(all(debug_assertions, feature = "vr-preview"))]
    pub(super) fn reload(&self, id: HostId) {
        self.shared
            .control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .reload[id.surface.index()] = Some(id);
        self.shared.wake();
    }

    #[cfg(all(debug_assertions, feature = "vr-preview"))]
    pub(super) fn counts(&self) -> (usize, usize) {
        *self.shared.counts.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[cfg(all(debug_assertions, feature = "vr-preview"))]
    pub(super) fn crash_browser(&self) {
        self.shared
            .control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .crash_browser = true;
        self.shared.wake();
    }

    pub fn finish_shutdown(&self, timeout: Duration) -> Result<(), String> {
        self.shared
            .control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .shutdown();
        self.shared.wake();
        let mut thread = self.thread.lock().unwrap_or_else(|e| e.into_inner());
        let Some(running) = thread.as_ref() else {
            return Ok(());
        };
        let result = running
            .done
            .recv_timeout(timeout)
            .map_err(|_| "Native surface shutdown did not drain in time")?;
        let running = thread.take().unwrap();
        running
            .join
            .join()
            .map_err(|_| "Native surface thread panicked")?;
        result
    }
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

struct ParentWindow(HWND);
impl Drop for ParentWindow {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

struct NativeHost {
    id: HostId,
    composition: ICoreWebView2CompositionController,
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
    root: ContainerVisual,
    _content: ContainerVisual,
    _item: GraphicsCaptureItem,
    _parent: ParentWindow,
    tokens: [i64; 8],
    session: Rc<RefCell<Option<DocumentSession>>>,
    snapshot: Rc<Cell<Option<u32>>>,
    activity: Option<SurfaceActivity>,
    sleep: Option<(u32, Instant)>,
}

impl Drop for NativeHost {
    fn drop(&mut self) {
        unsafe {
            let _ = self.webview.remove_NavigationStarting(self.tokens[0]);
            let _ = self.webview.remove_NavigationCompleted(self.tokens[1]);
            let _ = self.webview.remove_WebMessageReceived(self.tokens[2]);
            let _ = self.webview.remove_WebResourceRequested(self.tokens[3]);
            let _ = self.webview.remove_NewWindowRequested(self.tokens[4]);
            let _ = self.webview.remove_PermissionRequested(self.tokens[5]);
            let _ = self.webview.remove_ProcessFailed(self.tokens[6]);
            if let Ok(webview) = self.webview.cast::<ICoreWebView2_4>() {
                let _ = webview.remove_DownloadStarting(self.tokens[7]);
            }
            let _ = self.composition.SetRootVisualTarget(None);
            let _ = self.controller.Close();
        }
    }
}

struct Sta {
    app: AppHandle,
    shared: Arc<Shared>,
    policy: Rc<ResourcePolicy>,
    compositor: Compositor,
    environment: Option<ICoreWebView2Environment>,
    hosts: [Option<NativeHost>; 10],
    creating: [Option<HostId>; 10],
    inflight: usize,
    environment_pending: bool,
    browser_failed: Rc<Cell<bool>>,
    failure: Option<String>,
}

fn failure(stage: &str, error: windows::core::Error) -> String {
    format!("{stage}: HRESULT {:#010x}", error.code().0)
}

fn read_string(
    read: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>,
) -> windows::core::Result<String> {
    let mut value = PWSTR::null();
    let result = read(&mut value);
    let value = CoTaskMemPWSTR::from(value);
    result?;
    Ok(value.to_string())
}

fn post(webview: &ICoreWebView2, message: serde_json::Value) -> windows::core::Result<()> {
    let json = CoTaskMemPWSTR::from(message.to_string().as_str());
    unsafe { webview.PostWebMessageAsJson(*json.as_ref().as_pcwstr()) }
}

fn post_event(
    webview: &ICoreWebView2,
    session: &DocumentSession,
    event: SurfaceEvent,
) -> windows::core::Result<()> {
    let mut message = serde_json::to_value(event).expect("surface events are serializable");
    message["version"] = 1.into();
    message["op"] = "event".into();
    message["nonce"] = session.nonce.clone().into();
    post(webview, message)
}

fn host_error(app: &AppHandle, id: HostId, error: String) {
    let hub = app.state::<SurfaceHub>();
    let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
    let entry = &mut entries[id.surface.index()];
    if entry.host_epoch == id.host_epoch {
        if entry.error.as_ref() == Some(&error) {
            return;
        }
        entry.error = Some(error.clone());
        entry.layout = None;
        entry.displayed_layout = None;
        entry.displayed_scroll_key = None;
        tracing::warn!(
            surface = id.surface.name(),
            host_epoch = id.host_epoch,
            "{error}"
        );
    }
}

fn run(
    app: AppHandle,
    shared: Arc<Shared>,
    events: mpsc::Receiver<QueuedEvent>,
) -> Result<(), String> {
    super::windows_wgc::ensure_capture_supported()?;
    unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }
        .map_err(|e| failure("Initialize surface STA", e))?;
    let _apartment = Apartment;
    let dispatcher = unsafe {
        CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        })
    }
    .map_err(|e| failure("Create surface dispatcher", e))?;
    let policy = ResourcePolicy::new(
        if tauri::is_dev() {
            app.config().build.dev_url.as_ref()
        } else {
            None
        },
        app.asset_resolver()
            .iter()
            .map(|(path, _)| path.to_string()),
    )?;
    let profile = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("vr-surfaces");
    let compositor = Compositor::new().map_err(|e| failure("Create surface compositor", e))?;
    let state = Rc::new(RefCell::new(Sta {
        app,
        shared: shared.clone(),
        policy: Rc::new(policy),
        compositor,
        environment: None,
        hosts: std::array::from_fn(|_| None),
        creating: [None; 10],
        inflight: 0,
        environment_pending: false,
        browser_failed: Rc::new(Cell::new(false)),
        failure: None,
    }));
    let mut msg = MSG::default();
    unsafe {
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
    }
    shared
        .thread_id
        .store(unsafe { GetCurrentThreadId() }, Ordering::Release);
    start_environment(&state, &profile);
    loop {
        shared.notified.store(false, Ordering::Release);
        let retry_environment = {
            let state = state.borrow();
            let control = state
                .shared
                .control
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let desired = control.hosts;
            let closing = control.closing;
            drop(control);
            let hub = state.app.state::<SurfaceHub>();
            let entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
            !closing
                && state.failure.is_some()
                && state.inflight == 0
                && state.hosts.iter().all(Option::is_none)
                && desired.iter().flatten().any(|host| {
                    let entry = &entries[host.id.surface.index()];
                    host.activity.active
                        && entry.host_epoch == host.id.host_epoch
                        && entry.error.is_none()
                })
        };
        if retry_environment {
            {
                let mut state = state.borrow_mut();
                state.failure = None;
                state.environment = None;
            }
            start_environment(&state, &profile);
        }
        let timeout = process(&state, &events);
        #[cfg(all(debug_assertions, feature = "vr-preview"))]
        {
            let state = state.borrow();
            *shared.counts.lock().unwrap_or_else(|e| e.into_inner()) = (
                state.hosts.iter().filter(|host| host.is_some()).count(),
                state.inflight,
            );
        }
        let closing = shared
            .control
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .closing;
        if !closing
            && state.borrow().browser_failed.get()
            && state.borrow().inflight == 0
            && state.borrow().hosts.iter().all(Option::is_none)
        {
            {
                let mut state = state.borrow_mut();
                state.environment = None;
                state.browser_failed = Rc::new(Cell::new(false));
            }
            start_environment(&state, &profile);
            continue;
        }
        if closing
            && state.borrow().inflight == 0
            && state.borrow().hosts.iter().all(Option::is_none)
        {
            break;
        }
        unsafe {
            let _ = MsgWaitForMultipleObjectsEx(None, timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    shared
                        .control
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .shutdown();
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    let error = state.borrow_mut().failure.take();
    state.borrow_mut().environment = None;
    state
        .borrow()
        .compositor
        .Close()
        .map_err(|e| failure("Close surface compositor", e))?;
    drop(state);
    let shutdown = dispatcher
        .ShutdownQueueAsync()
        .map_err(|e| failure("Shutdown surface dispatcher", e))?;
    let done = Arc::new(AtomicBool::new(false));
    let completed = done.clone();
    let wake = shared.clone();
    shutdown
        .when(move |_| {
            completed.store(true, Ordering::Release);
            wake.wake();
        })
        .map_err(|e| failure("Await dispatcher shutdown", e))?;
    while !done.load(Ordering::Acquire) {
        shared.notified.store(false, Ordering::Release);
        unsafe {
            if GetMessageW(&mut msg, None, 0, 0).0 <= 0 {
                return Err("Surface dispatcher stopped before shutdown".into());
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    shutdown
        .GetResults()
        .map_err(|e| failure("Drain surface dispatcher", e))?;
    error.map_or(Ok(()), Err)
}

fn start_environment(state: &Rc<RefCell<Sta>>, profile: &std::path::Path) {
    if state.borrow().environment_pending {
        return;
    }
    let weak = Rc::downgrade(state);
    let options = CoreWebView2EnvironmentOptions::default();
    unsafe {
        options.set_additional_browser_arguments(BROWSER_ARGS.into());
    }
    let options: ICoreWebView2EnvironmentOptions = options.into();
    let path = CoTaskMemPWSTR::from(profile.to_string_lossy().as_ref());
    state.borrow_mut().inflight += 1;
    state.borrow_mut().environment_pending = true;
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |status, environment| {
            if let Some(state) = weak.upgrade() {
                let mut state = state.borrow_mut();
                state.inflight -= 1;
                state.environment_pending = false;
                if state
                    .shared
                    .control
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .closing
                {
                    state.shared.wake();
                    return Ok(());
                }
                match status.and_then(|_| {
                    environment.ok_or_else(|| {
                        windows::core::Error::from(windows::Win32::Foundation::E_POINTER)
                    })
                }) {
                    Ok(environment) => state.environment = Some(environment),
                    Err(error) => {
                        environment_error(&mut state, failure("Create WebView2 environment", error))
                    }
                }
                state.shared.wake();
            }
            Ok(())
        },
    ));
    if let Err(error) = unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            *path.as_ref().as_pcwstr(),
            &options,
            &handler,
        )
    } {
        let mut state = state.borrow_mut();
        state.inflight -= 1;
        state.environment_pending = false;
        environment_error(&mut state, failure("Request WebView2 environment", error));
    }
}

fn environment_error(state: &mut Sta, error: String) {
    let hosts = state
        .shared
        .control
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .hosts;
    for host in hosts.into_iter().flatten() {
        host_error(&state.app, host.id, error.clone());
    }
    state.failure = Some(error);
}

fn process(state: &Rc<RefCell<Sta>>, events: &mpsc::Receiver<QueuedEvent>) -> u32 {
    if state.borrow().browser_failed.get() {
        let state = state.borrow();
        for host in state.hosts.iter().flatten() {
            host_error(&state.app, host.id, "surface_browser_process_failed".into());
            let hub = state.app.state::<SurfaceHub>();
            let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
            let entry = &mut entries[host.id.surface.index()];
            // Hidden entries have already suspended capture on the worker.
            let closing = state
                .shared
                .control
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .closing;
            if !closing && !entry.active && entry.host_epoch == host.id.host_epoch {
                entry.invalidate_host();
                state
                    .shared
                    .control
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .close(host.id);
            }
        }
    }
    let desired = state
        .borrow()
        .shared
        .control
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .hosts;
    let mut timeout = u32::MAX;
    #[cfg(all(debug_assertions, feature = "vr-preview"))]
    {
        let crash = std::mem::take(
            &mut state
                .borrow()
                .shared
                .control
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .crash_browser,
        );
        if crash {
            // Only the isolated capture-smoke environment may terminate its own browser.
            let state = state.borrow();
            if state.app.config().identifier == "app.vrcs.vr-surface-smoke" {
                if let Some(host) = state.hosts.iter().flatten().next() {
                    let mut pid = 0;
                    if unsafe { host.webview.BrowserProcessId(&mut pid) }.is_ok() {
                        use windows::Win32::{
                            Foundation::CloseHandle,
                            System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE},
                        };
                        if let Ok(process) = unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) } {
                            let _ = unsafe { TerminateProcess(process, 1) };
                            let _ = unsafe { CloseHandle(process) };
                        }
                    }
                }
            }
        }
        let reloads = std::mem::take(
            &mut state
                .borrow()
                .shared
                .control
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .reload,
        );
        for id in reloads.into_iter().flatten() {
            let state = state.borrow();
            if let Some(host) = &state.hosts[id.surface.index()] {
                if host.id == id {
                    if let Err(error) = unsafe { host.webview.Reload() } {
                        host_error(&state.app, id, failure("Reload surface", error));
                    }
                }
            }
        }
    }
    for (index, desired) in desired.into_iter().enumerate() {
        let remove = state.borrow().hosts[index]
            .as_ref()
            .is_some_and(|host| desired.is_none_or(|desired| desired.id != host.id));
        if remove {
            state.borrow_mut().hosts[index] = None;
        }
        if let Some(desired) =
            desired.filter(|desired| state.borrow().shared.current(desired.id).is_some())
        {
            if let Some(error) = state.borrow().failure.clone() {
                host_error(&state.borrow().app, desired.id, error);
                continue;
            }
            let create = {
                let s = state.borrow();
                let hub = s.app.state::<SurfaceHub>();
                let entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                s.hosts[index].is_none()
                    && s.creating[index].is_none()
                    && s.environment.is_some()
                    && !s.browser_failed.get()
                    && desired.activity.active
                    && entries[index].host_epoch == desired.id.host_epoch
                    && entries[index].error.is_none()
            };
            if create {
                start_host(state, desired.id);
            }
            let mut state = state.borrow_mut();
            let app = state.app.clone();
            if let Some(host) = &mut state.hosts[index] {
                let waking = desired.activity.active
                    && host.activity.is_none_or(|previous| {
                        !previous.active || previous.request != desired.activity.request
                    });
                if let Err(error) = host.update_activity(desired.activity) {
                    host_error(&app, host.id, failure("Set surface activity", error));
                }
                if let Some((_, deadline)) = host.sleep {
                    timeout = timeout.min(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .as_millis()
                            .min(u32::MAX as u128) as u32,
                    );
                }
                let hub = app.state::<SurfaceHub>();
                let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                let entry = &mut entries[index];
                if entry.host_epoch == host.id.host_epoch
                    && entry.activity_request == desired.activity.request
                {
                    entry.host_awake = desired.activity.active;
                }
                let view = (waking && entry.ready)
                    .then(|| entry.state.clone())
                    .flatten();
                drop(entries);
                if let (Some(view), Some(session)) = (view, host.session.borrow().as_ref()) {
                    if let Err(error) = post_event(&host.webview, session, SurfaceEvent::View(view))
                    {
                        host_error(&app, host.id, failure("Wake surface view", error));
                    }
                }
            }
        }
    }
    for _ in 0..256 {
        let Ok(event) = events.try_recv() else {
            break;
        };
        let state = state.borrow();
        if state.shared.current(event.session.host).is_none() {
            continue;
        }
        if let Some(host) = &state.hosts[event.session.host.surface.index()] {
            if host.session.borrow().as_ref() == Some(&event.session) {
                if let Err(error) = post_event(&host.webview, &event.session, event.event) {
                    host_error(&state.app, host.id, failure("Send surface event", error));
                }
            }
        }
    }
    timeout
}

fn start_host(state: &Rc<RefCell<Sta>>, id: HostId) {
    let result = (|| -> windows::core::Result<()> {
        let (width, height) = id.surface.size();
        let parent = ParentWindow(unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                w!("VRCS Composition Surface"),
                WS_POPUP,
                0,
                0,
                width as i32,
                (height + super::windows_wgc::MARKER_HEIGHT) as i32,
                None,
                None,
                None,
                None,
            )?
        });
        let environment: ICoreWebView2Environment3 =
            state.borrow().environment.as_ref().unwrap().cast()?;
        let weak = Rc::downgrade(state);
        let hwnd = parent.0;
        let handler = CreateCoreWebView2CompositionControllerCompletedHandler::create(Box::new(
            move |status, composition| {
                if let Some(state) = weak.upgrade() {
                    let mut state = state.borrow_mut();
                    state.inflight -= 1;
                    state.creating[id.surface.index()] = None;
                    let current = !state.browser_failed.get()
                        && state
                            .shared
                            .current(id)
                            .is_some_and(|activity| activity.active);
                    if !current {
                        if let Some(composition) = composition {
                            if let Ok(controller) = composition.cast::<ICoreWebView2Controller>() {
                                let _ = unsafe { controller.Close() };
                            }
                        }
                        if state.browser_failed.get() {
                            host_error(&state.app, id, "surface_browser_process_failed".into());
                        }
                    } else {
                        let result = status
                            .and_then(|_| {
                                composition.ok_or_else(|| {
                                    windows::core::Error::from(
                                        windows::Win32::Foundation::E_POINTER,
                                    )
                                })
                            })
                            .and_then(|composition| {
                                NativeHost::create(&state, id, parent, composition)
                            });
                        match result {
                            Ok(host) => {
                                let hub = state.app.state::<SurfaceHub>();
                                let mut entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                                let entry = &mut entries[id.surface.index()];
                                // Recheck both authorities after asynchronous controller creation.
                                if entry.host_epoch == id.host_epoch
                                    && entry.active
                                    && state
                                        .shared
                                        .current(id)
                                        .is_some_and(|activity| activity.active)
                                {
                                    entry.endpoint = Some(CaptureEndpoint {
                                        host: id,
                                        item: host._item.clone(),
                                        #[cfg(all(debug_assertions, feature = "vr-preview"))]
                                        parent: host._parent.0 .0 as isize,
                                    });
                                    entry.creating = false;
                                    entry.host_awake = true;
                                    drop(entries);
                                    state.hosts[id.surface.index()] = Some(host);
                                }
                            }
                            Err(error) => host_error(
                                &state.app,
                                id,
                                failure("Create composition surface", error),
                            ),
                        }
                    }
                    state.shared.wake();
                }
                Ok(())
            },
        ));
        state.borrow_mut().inflight += 1;
        state.borrow_mut().creating[id.surface.index()] = Some(id);
        if let Err(error) =
            unsafe { environment.CreateCoreWebView2CompositionController(hwnd, &handler) }
        {
            state.borrow_mut().inflight -= 1;
            state.borrow_mut().creating[id.surface.index()] = None;
            return Err(error);
        }
        Ok(())
    })();
    if let Err(error) = result {
        host_error(
            &state.borrow().app,
            id,
            failure("Request composition surface", error),
        );
    }
}

impl NativeHost {
    fn create(
        state: &Sta,
        id: HostId,
        parent: ParentWindow,
        composition: ICoreWebView2CompositionController,
    ) -> windows::core::Result<Self> {
        let controller: ICoreWebView2Controller = composition.cast()?;
        let result = (|| {
            let (width, height) = id.surface.size();
            let height = height + super::windows_wgc::MARKER_HEIGHT;
            let root = state.compositor.CreateContainerVisual()?;
            let content = state.compositor.CreateContainerVisual()?;
            let mut size = root.Size()?;
            size.X = width as f32;
            size.Y = height as f32;
            root.SetSize(size)?;
            content.SetSize(size)?;
            root.SetClip(&state.compositor.CreateInsetClip()?)?;
            root.Children()?.InsertAtTop(&content)?;
            unsafe {
                composition.SetRootVisualTarget(&content)?;
                controller
                    .cast::<ICoreWebView2Controller2>()?
                    .SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                        A: 0,
                        R: 0,
                        G: 0,
                        B: 0,
                    })?;
                let scaled: ICoreWebView2Controller3 = controller.cast()?;
                scaled.SetShouldDetectMonitorScaleChanges(false)?;
                scaled.SetRasterizationScale(1.0)?;
                scaled.SetBoundsMode(COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS)?;
                controller.SetZoomFactor(1.0)?;
                controller.SetBounds(RECT {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                })?;
                controller.SetIsVisible(true)?;
            }
            let item = GraphicsCaptureItem::CreateFromVisual(&root)?;
            let webview = unsafe { controller.CoreWebView2()? };
            let session = Rc::new(RefCell::new(None));
            let snapshot = Rc::new(Cell::new(None));
            Ok(Self {
                id,
                composition: composition.clone(),
                controller: controller.clone(),
                webview,
                root,
                _content: content,
                _item: item,
                _parent: parent,
                tokens: [0; 8],
                session,
                snapshot,
                activity: None,
                sleep: None,
            })
        })();
        let mut host = match result {
            Ok(host) => host,
            Err(error) => {
                let _ = unsafe { controller.Close() };
                return Err(error);
            }
        };
        host.install_handlers(state)?;
        let url = CoTaskMemPWSTR::from(state.policy.entry_url(id).as_str());
        unsafe {
            host.webview.Navigate(*url.as_ref().as_pcwstr())?;
        }
        Ok(host)
    }

    fn update_activity(&mut self, activity: SurfaceActivity) -> windows::core::Result<()> {
        if self.activity.is_none_or(|previous| {
            previous.request != activity.request || previous.active != activity.active
        }) {
            self.activity = Some(activity);
            if activity.active {
                self.sleep = None;
                self.root.SetIsVisible(true)?;
                unsafe {
                    self.controller.SetIsVisible(true)?;
                }
            } else {
                self.snapshot.set(None);
                self.sleep = Some((
                    activity.request,
                    Instant::now() + Duration::from_millis(500),
                ));
            }
            if let Some(session) = self.session.borrow().as_ref() {
                post_event(&self.webview, session, SurfaceEvent::Activity(activity))?;
            }
        }
        if let Some((request, deadline)) = self.sleep {
            if self.snapshot.get() == Some(request) || Instant::now() >= deadline {
                if self.snapshot.get() != Some(request) {
                    tracing::warn!(surface = self.id.surface.name(), "snapshot_missing");
                }
                unsafe {
                    self.controller.SetIsVisible(false)?;
                }
                self.root.SetIsVisible(false)?;
                self.sleep = None;
            }
        }
        Ok(())
    }

    fn install_handlers(&mut self, state: &Sta) -> windows::core::Result<()> {
        let id = self.id;
        let source = state.policy.entry_url(id);
        let connecting = Rc::new(Cell::new(false));
        unsafe {
            let settings = self.webview.Settings()?;
            settings.SetAreDefaultContextMenusEnabled(false)?;
            settings.SetAreDefaultScriptDialogsEnabled(false)?;
            settings.SetIsStatusBarEnabled(false)?;
            settings.SetAreHostObjectsAllowed(false)?;
            settings.SetAreDevToolsEnabled(tauri::is_dev())?;
            settings.SetIsWebMessageEnabled(true)?;
            settings
                .cast::<ICoreWebView2Settings3>()?
                .SetAreBrowserAcceleratorKeysEnabled(false)?;
            self.webview.cast::<ICoreWebView2_8>()?.SetIsMuted(true)?;

            let app = state.app.clone();
            let shared = state.shared.clone();
            let session = self.session.clone();
            let expected = source.clone();
            let intent = connecting.clone();
            self.webview.add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    let uri = read_string(|out| args.Uri(out))?;
                    if uri != expected || shared.current(id).is_none() {
                        args.SetCancel(true)?;
                        return Ok(());
                    }
                    session.borrow_mut().take();
                    intent.set(false);
                    let mut navigation = 0;
                    args.NavigationId(&mut navigation)?;
                    if !app.state::<SurfaceHub>().navigation_started(id, navigation) {
                        args.SetCancel(true)?;
                    }
                    Ok(())
                })),
                &mut self.tokens[0],
            )?;

            let app = state.app.clone();
            let shared = state.shared.clone();
            let session = self.session.clone();
            let expected = source.clone();
            let intent = connecting.clone();
            self.webview.add_NavigationCompleted(&NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()); };
                if shared.current(id).is_none() { return Ok(()); }
                let mut navigation = 0;
                args.NavigationId(&mut navigation)?;
                {
                    let hub = app.state::<SurfaceHub>();
                    let entries = hub.0.lock().unwrap_or_else(|e| e.into_inner());
                    let entry = &entries[id.surface.index()];
                    // Rejected external and superseded navigations must not fault this document.
                    if entry.host_epoch != id.host_epoch || entry.navigation_id != navigation { return Ok(()); }
                }
                let mut success = BOOL(0);
                args.IsSuccess(&mut success)?;
                if !success.as_bool() { host_error(&app, id, "surface_navigation_failed".into()); return Ok(()); }
                if let Some(document) = app.state::<SurfaceHub>().document_loaded(id, navigation, expected.clone()) {
                    *session.borrow_mut() = Some(document.clone());
                    if intent.get() { post(&sender, serde_json::json!({ "version": 1, "op": "welcome", "host": document.host, "nonce": document.nonce }))?; }
                }
                Ok(())
            })), &mut self.tokens[1])?;

            let app = state.app.clone();
            let shared = state.shared.clone();
            let session = self.session.clone();
            let expected = source;
            let snapshot = self.snapshot.clone();
            self.webview.add_WebMessageReceived(&WebMessageReceivedEventHandler::create(Box::new(move |sender, args| {
                let (Some(sender), Some(args)) = (sender, args) else { return Ok(()); };
                if shared.current(id).is_none() { return Ok(()); }
                let source = read_string(|out| args.Source(out))?;
                if source != expected { return Ok(()); }
                let message = read_string(|out| args.WebMessageAsJson(out))?;
                let Ok(request) = super::bridge::decode_request(&message) else { return Ok(()); };
                if matches!(request, PageRequest::Connect) {
                    connecting.set(true);
                    if let Some(document) = session.borrow().as_ref() { post(&sender, serde_json::json!({ "version": 1, "op": "welcome", "host": document.host, "nonce": document.nonce }))?; }
                    return Ok(());
                }
                let Some(document) = session.borrow().clone() else { return Ok(()); };
                if document.authorize(&source, &request).is_err() { return Ok(()); }
                let PageRequest::Call { request_id, ref operation, .. } = request else { return Ok(()); };
                let sleeping = if let Operation::Snapshot { activity_request, .. } = operation { Some(*activity_request) } else { None };
                let result = app.state::<SurfaceHub>().handle_page_request(&document, request);
                if matches!(&result, Ok(super::bridge::PageReply::Accepted(true))) {
                    if let Some(request) = sleeping { snapshot.set(Some(request)); shared.wake(); }
                }
                let reply = match result {
                    Ok(value) => serde_json::json!({ "version": 1, "op": "reply", "nonce": document.nonce, "request_id": request_id, "ok": true, "value": value }),
                    Err(error) => serde_json::json!({ "version": 1, "op": "reply", "nonce": document.nonce, "request_id": request_id, "ok": false, "error": error.code() }),
                };
                post(&sender, reply)
            })), &mut self.tokens[2])?;

            let app = state.app.clone();
            let environment = state.environment.as_ref().unwrap().clone();
            let policy = state.policy.clone();
            self.webview
                .AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;
            self.webview.add_WebResourceRequested(&WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()); };
                let request = args.Request()?;
                let uri = read_string(|out| request.Uri(out))?;
                let method = read_string(|out| request.Method(out))?;
                let (status, bytes, mime, csp) = match policy.route(&method, &uri) {
                    ResourceRoute::Network => return Ok(()),
                    ResourceRoute::Reject(status) => (status, Vec::new(), "text/plain".into(), None),
                    ResourceRoute::Asset(path) => match app.asset_resolver().get_for_scheme(path, true) {
                        Some(asset) => (200, asset.bytes, asset.mime_type, asset.csp_header),
                        None => (404, Vec::new(), "text/plain".into(), None),
                    },
                };
                let mut headers = format!("Content-Type: {mime}\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: {SURFACE_CSP}\r\nCache-Control: no-store\r\n");
                if let Some(csp) = csp { headers.push_str(&format!("Content-Security-Policy: {csp}\r\n")); }
                let headers = CoTaskMemPWSTR::from(headers.as_str());
                let stream = SHCreateMemStream(Some(if method == "HEAD" { &[] } else { &bytes }));
                let response = environment.CreateWebResourceResponse(stream.as_ref(), status as i32, w!("Surface"), *headers.as_ref().as_pcwstr())?;
                args.SetResponse(&response)
            })), &mut self.tokens[3])?;

            self.webview.add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        args.SetHandled(true)?;
                    }
                    Ok(())
                })),
                &mut self.tokens[4],
            )?;
            self.webview.add_PermissionRequested(
                &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                    }
                    Ok(())
                })),
                &mut self.tokens[5],
            )?;
            let app = state.app.clone();
            let browser_failed = state.browser_failed.clone();
            let shared = state.shared.clone();
            self.webview.add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
                        args.ProcessFailedKind(&mut kind)?;
                        if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                            browser_failed.set(true);
                        }
                    }
                    host_error(&app, id, "surface_browser_process_failed".into());
                    shared.wake();
                    Ok(())
                })),
                &mut self.tokens[6],
            )?;
            self.webview
                .cast::<ICoreWebView2_4>()?
                .add_DownloadStarting(
                    &DownloadStartingEventHandler::create(Box::new(|_, args| {
                        if let Some(args) = args {
                            args.SetCancel(true)?;
                            args.SetHandled(true)?;
                        }
                        Ok(())
                    })),
                    &mut self.tokens[7],
                )?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct DesiredHost {
    id: HostId,
    activity: SurfaceActivity,
}

#[derive(Default)]
struct ControlMailbox {
    hosts: [Option<DesiredHost>; 10],
    closing: bool,
    #[cfg(all(debug_assertions, feature = "vr-preview"))]
    reload: [Option<HostId>; 10],
    #[cfg(all(debug_assertions, feature = "vr-preview"))]
    crash_browser: bool,
}

impl ControlMailbox {
    fn ensure(&mut self, host: DesiredHost) -> bool {
        if self.closing {
            return false;
        }
        let slot = &mut self.hosts[host.id.surface.index()];
        if let Some(previous) = slot {
            if previous.id != host.id
                && !super::serial_at_least(host.id.host_epoch, previous.id.host_epoch)
            {
                return false;
            }
            if previous.id == host.id
                && !super::serial_at_least(host.activity.request, previous.activity.request)
            {
                return false;
            }
        }
        *slot = Some(host);
        true
    }

    fn activity(&mut self, id: HostId, activity: SurfaceActivity) -> bool {
        if self.current(id).is_none() {
            return false;
        }
        self.ensure(DesiredHost { id, activity })
    }

    fn close(&mut self, id: HostId) {
        if self.current(id).is_some() {
            self.hosts[id.surface.index()] = None;
        }
    }

    fn current(&self, id: HostId) -> Option<SurfaceActivity> {
        if self.closing {
            return None;
        }
        self.hosts[id.surface.index()]
            .filter(|host| host.id == id)
            .map(|host| host.activity)
    }

    fn shutdown(&mut self) {
        self.closing = true;
        self.hosts.fill(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr_overlay::surface::SurfaceId;

    #[test]
    fn full_event_queue_cannot_block_hide_close_or_shutdown() {
        let mut control = ControlMailbox::default();
        let id = HostId {
            surface: SurfaceId::Reader,
            host_epoch: 1,
        };
        assert!(control.ensure(DesiredHost {
            id,
            activity: SurfaceActivity {
                active: true,
                request: 1
            }
        }));
        let (tx, _rx) = std::sync::mpsc::sync_channel(256);
        for _ in 0..256 {
            tx.try_send(SurfaceEvent::Repaint).unwrap();
        }
        assert!(tx.try_send(SurfaceEvent::Repaint).is_err());
        assert!(control.activity(
            id,
            SurfaceActivity {
                active: false,
                request: 2
            }
        ));
        assert!(
            !control.current(id).unwrap().active,
            "a late create cannot publish while hidden"
        );
        assert!(!control.activity(
            id,
            SurfaceActivity {
                active: true,
                request: 1
            }
        ));
        control.close(id);
        assert!(
            control.current(id).is_none(),
            "a late create cannot publish after close"
        );
        let next = HostId {
            host_epoch: 2,
            ..id
        };
        assert!(control.ensure(DesiredHost {
            id: next,
            activity: SurfaceActivity {
                active: true,
                request: 3
            }
        }));
        control.close(id);
        assert!(
            control.current(next).unwrap().active,
            "an old close must not close its replacement"
        );
        control.shutdown();
        assert!(control.current(next).is_none());
        assert!(!control.ensure(DesiredHost {
            id: next,
            activity: SurfaceActivity {
                active: true,
                request: 4
            }
        }));
    }

    #[test]
    fn begin_shutdown_keeps_hosts_until_worker_has_stopped() {
        let id = HostId {
            surface: SurfaceId::Menu,
            host_epoch: 1,
        };
        let mut control = ControlMailbox::default();
        control.ensure(DesiredHost {
            id,
            activity: SurfaceActivity {
                active: true,
                request: 1,
            },
        });
        control.closing = true;
        assert!(
            control.current(id).is_none(),
            "late callbacks lose authority immediately"
        );
        assert!(
            control.hosts[0].is_some(),
            "worker can still own the capture item"
        );
        control.shutdown();
        assert!(control.hosts.iter().all(Option::is_none));
    }
}
