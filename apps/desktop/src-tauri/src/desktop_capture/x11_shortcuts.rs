//! Native X11 global shortcuts via runtime-loaded `libxcb.so.1`.
//!
//! Uses XCB instead of Xlib so failed key grabs return `BadAccess` per request
//! without touching process-global Xlib error handlers shared with GTK/GDK.

use std::ffi::{c_int, c_uint, c_void};
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::OnceLock;
use std::thread::JoinHandle;
use std::time::Duration;

use tauri::{AppHandle, Manager as _};

const XCB_KEY_PRESS: u8 = 2;
const XCB_MAPPING_NOTIFY: u8 = 34;
const XCB_GRAB_MODE_ASYNC: u8 = 1;
const TRACKED_MOD_MASK: u16 = 0x01 | 0x04 | 0x08; // Shift | Control | Mod1 (Alt)
const LOCK_MASKS: [u16; 4] = [0, 0x02, 0x10, 0x02 | 0x10]; // none, Lock, Mod2, Lock|Mod2

#[repr(C)]
struct XcbConnection {
    _private: [u8; 0],
}

#[repr(C)]
struct XcbSetup {
    status: u8,
    pad0: u8,
    protocol_major_version: u16,
    protocol_minor_version: u16,
    length: u16,
    release_number: u32,
    resource_id_base: u32,
    resource_id_mask: u32,
    motion_buffer_size: u32,
    vendor_len: u16,
    maximum_request_length: u16,
    roots_len: u8,
    pixmap_formats_len: u8,
    image_byte_order: u8,
    bitmap_format_bit_order: u8,
    bitmap_format_scanline_unit: u8,
    bitmap_format_scanline_pad: u8,
    min_keycode: u8,
    max_keycode: u8,
    pad1: [u8; 4],
}

#[repr(C)]
struct XcbScreen {
    root: u32,
}

#[repr(C)]
struct XcbScreenIterator {
    data: *mut XcbScreen,
    rem: c_int,
    index: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XcbCookie {
    sequence: c_uint,
}

#[repr(C)]
struct XcbGetKeyboardMappingReply {
    response_type: u8,
    keysyms_per_keycode: u8,
    sequence: u16,
    length: u32,
    pad0: [u8; 24],
}

#[repr(C)]
struct XcbGenericEvent {
    response_type: u8,
    pad0: u8,
    sequence: u16,
    pad: [u32; 7],
    full_sequence: u32,
}

#[repr(C)]
struct XcbKeyPressEvent {
    response_type: u8,
    detail: u8,
    sequence: u16,
    time: u32,
    root: u32,
    event: u32,
    child: u32,
    root_x: i16,
    root_y: i16,
    event_x: i16,
    event_y: i16,
    state: u16,
    same_screen: u8,
    pad0: u8,
}

type ConnectFn = unsafe extern "C" fn(*const libc::c_char, *mut c_int) -> *mut XcbConnection;
type ConnectionHasErrorFn = unsafe extern "C" fn(*mut XcbConnection) -> c_int;
type DisconnectFn = unsafe extern "C" fn(*mut XcbConnection);
type GetFileDescriptorFn = unsafe extern "C" fn(*mut XcbConnection) -> c_int;
type GetSetupFn = unsafe extern "C" fn(*mut XcbConnection) -> *const XcbSetup;
type SetupRootsIteratorFn = unsafe extern "C" fn(*const XcbSetup) -> XcbScreenIterator;
type GetKeyboardMappingFn = unsafe extern "C" fn(*mut XcbConnection, u8, u8) -> XcbCookie;
type GetKeyboardMappingReplyFn = unsafe extern "C" fn(
    *mut XcbConnection,
    XcbCookie,
    *mut *mut c_void,
) -> *mut XcbGetKeyboardMappingReply;
type GetKeyboardMappingKeysymsFn =
    unsafe extern "C" fn(*const XcbGetKeyboardMappingReply) -> *mut u32;
type GetKeyboardMappingKeysymsLenFn =
    unsafe extern "C" fn(*const XcbGetKeyboardMappingReply) -> c_int;
type GrabKeyCheckedFn =
    unsafe extern "C" fn(*mut XcbConnection, u8, u32, u16, u8, u8, u8) -> XcbCookie;
type UngrabKeyFn = unsafe extern "C" fn(*mut XcbConnection, u8, u32, u16) -> XcbCookie;
type RequestCheckFn = unsafe extern "C" fn(*mut XcbConnection, XcbCookie) -> *mut c_void;
type FlushFn = unsafe extern "C" fn(*mut XcbConnection) -> c_int;
type PollForEventFn = unsafe extern "C" fn(*mut XcbConnection) -> *mut XcbGenericEvent;

struct XcbApi {
    _handle: usize,
    connect: ConnectFn,
    connection_has_error: ConnectionHasErrorFn,
    disconnect: DisconnectFn,
    get_file_descriptor: GetFileDescriptorFn,
    get_setup: GetSetupFn,
    setup_roots_iterator: SetupRootsIteratorFn,
    get_keyboard_mapping: GetKeyboardMappingFn,
    get_keyboard_mapping_reply: GetKeyboardMappingReplyFn,
    get_keyboard_mapping_keysyms: GetKeyboardMappingKeysymsFn,
    get_keyboard_mapping_keysyms_len: GetKeyboardMappingKeysymsLenFn,
    grab_key_checked: GrabKeyCheckedFn,
    ungrab_key: UngrabKeyFn,
    request_check: RequestCheckFn,
    flush: FlushFn,
    poll_for_event: PollForEventFn,
}

unsafe fn sym<T: Copy>(handle: *mut c_void, name: &std::ffi::CStr) -> Option<T> {
    let ptr = libc::dlsym(handle, name.as_ptr());
    (!ptr.is_null()).then(|| std::mem::transmute_copy(&ptr))
}

impl XcbApi {
    fn load() -> Option<&'static Self> {
        static API: OnceLock<Option<XcbApi>> = OnceLock::new();
        API.get_or_init(|| unsafe {
            let handle = libc::dlopen(c"libxcb.so.1".as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL);
            if handle.is_null() {
                return None;
            }
            let api = (|| -> Option<Self> {
                Some(Self {
                    _handle: handle as usize,
                    connect: sym(handle, c"xcb_connect")?,
                    connection_has_error: sym(handle, c"xcb_connection_has_error")?,
                    disconnect: sym(handle, c"xcb_disconnect")?,
                    get_file_descriptor: sym(handle, c"xcb_get_file_descriptor")?,
                    get_setup: sym(handle, c"xcb_get_setup")?,
                    setup_roots_iterator: sym(handle, c"xcb_setup_roots_iterator")?,
                    get_keyboard_mapping: sym(handle, c"xcb_get_keyboard_mapping")?,
                    get_keyboard_mapping_reply: sym(handle, c"xcb_get_keyboard_mapping_reply")?,
                    get_keyboard_mapping_keysyms: sym(handle, c"xcb_get_keyboard_mapping_keysyms")?,
                    get_keyboard_mapping_keysyms_len: sym(
                        handle,
                        c"xcb_get_keyboard_mapping_keysyms_length",
                    )?,
                    grab_key_checked: sym(handle, c"xcb_grab_key_checked")?,
                    ungrab_key: sym(handle, c"xcb_ungrab_key")?,
                    request_check: sym(handle, c"xcb_request_check")?,
                    flush: sym(handle, c"xcb_flush")?,
                    poll_for_event: sym(handle, c"xcb_poll_for_event")?,
                })
            })();
            if api.is_none() {
                libc::dlclose(handle);
            }
            api
        })
        .as_ref()
    }
}

struct KeyboardMap {
    min_keycode: u8,
    per: usize,
    syms: Vec<u32>,
}

impl KeyboardMap {
    unsafe fn query(
        api: &XcbApi,
        conn: *mut XcbConnection,
        min_keycode: u8,
        max_keycode: u8,
    ) -> Option<Self> {
        let count = max_keycode.checked_sub(min_keycode)?.saturating_add(1);
        let cookie = (api.get_keyboard_mapping)(conn, min_keycode, count);
        let reply = (api.get_keyboard_mapping_reply)(conn, cookie, ptr::null_mut());
        if reply.is_null() {
            return None;
        }
        let per = usize::from((*reply).keysyms_per_keycode);
        let len = (api.get_keyboard_mapping_keysyms_len)(reply);
        let syms_ptr = (api.get_keyboard_mapping_keysyms)(reply);
        let syms = if per > 0 && len > 0 && !syms_ptr.is_null() {
            std::slice::from_raw_parts(syms_ptr, len as usize).to_vec()
        } else {
            Vec::new()
        };
        libc::free(reply.cast());
        (per > 0 && !syms.is_empty()).then_some(Self {
            min_keycode,
            per,
            syms,
        })
    }

    fn keycode_for(&self, keysym: u32) -> Option<u8> {
        let rows = self.syms.len() / self.per;
        for row_idx in 0..rows {
            if self.syms[row_idx * self.per] == keysym {
                return self.min_keycode.checked_add(u8::try_from(row_idx).ok()?);
            }
        }
        let upper = if (b'a' as u32..=b'z' as u32).contains(&keysym) {
            Some(keysym - 32)
        } else {
            None
        };
        for row_idx in 0..rows {
            let row = &self.syms[row_idx * self.per..(row_idx + 1) * self.per];
            if row
                .iter()
                .take(4)
                .any(|&sym| sym == keysym || upper == Some(sym))
            {
                return self.min_keycode.checked_add(u8::try_from(row_idx).ok()?);
            }
        }
        None
    }
}

#[derive(Clone, Copy)]
enum Action {
    Ocr,
    Vision,
}

#[derive(Clone, Copy)]
struct Grab {
    action: Action,
    modifiers: u16,
    keycode: u8,
}

enum Command {
    Update {
        ocr: Option<String>,
        vision: Option<String>,
        reply: SyncSender<Result<Vec<String>, String>>,
    },
    Pause(bool),
    Shutdown,
}

struct X11Worker {
    api: &'static XcbApi,
    conn: *mut XcbConnection,
    root: u32,
    min_keycode: u8,
    max_keycode: u8,
    keymap: KeyboardMap,
    desired: Vec<(Action, super::shortcut::Shortcut, String)>,
    grabbed: Vec<Grab>,
    paused: bool,
}

impl X11Worker {
    unsafe fn ungrab_all(&mut self) {
        for grab in self.grabbed.drain(..) {
            for mask in LOCK_MASKS {
                (self.api.ungrab_key)(self.conn, grab.keycode, self.root, grab.modifiers | mask);
            }
        }
        (self.api.flush)(self.conn);
    }

    unsafe fn apply_grabs(&mut self) -> Result<Vec<String>, String> {
        self.ungrab_all();
        if self.paused || self.desired.is_empty() {
            return Ok(self.desired.iter().map(|(_, _, raw)| raw.clone()).collect());
        }
        let mut labels = Vec::with_capacity(self.desired.len());
        let mut applied: Vec<Grab> = Vec::with_capacity(self.desired.len());
        let ungrab = |grabbed: &[Grab]| {
            for grab in grabbed {
                for mask in LOCK_MASKS {
                    (self.api.ungrab_key)(
                        self.conn,
                        grab.keycode,
                        self.root,
                        grab.modifiers | mask,
                    );
                }
            }
            (self.api.flush)(self.conn);
        };
        for (action, shortcut, raw) in &self.desired {
            let (modifiers, keysym) = shortcut.x11_binding();
            let Some(keycode) = self.keymap.keycode_for(keysym) else {
                ungrab(&applied);
                return Err("capture.shortcuts_unavailable".into());
            };
            for mask in LOCK_MASKS {
                let cookie = (self.api.grab_key_checked)(
                    self.conn,
                    0,
                    self.root,
                    modifiers | mask,
                    keycode,
                    XCB_GRAB_MODE_ASYNC,
                    XCB_GRAB_MODE_ASYNC,
                );
                let error = (self.api.request_check)(self.conn, cookie);
                if !error.is_null() {
                    libc::free(error);
                    for rollback_mask in LOCK_MASKS {
                        (self.api.ungrab_key)(
                            self.conn,
                            keycode,
                            self.root,
                            modifiers | rollback_mask,
                        );
                    }
                    ungrab(&applied);
                    return Err("desktop_vision.shortcut_conflict".into());
                }
            }
            applied.push(Grab {
                action: *action,
                modifiers,
                keycode,
            });
            labels.push(raw.clone());
        }
        self.grabbed = applied;
        (self.api.flush)(self.conn);
        Ok(labels)
    }

    unsafe fn drain_events(&mut self, app: &AppHandle) {
        let mut mapping_changed = false;
        loop {
            let event = (self.api.poll_for_event)(self.conn);
            if event.is_null() {
                break;
            }
            let event_type = (*event).response_type & 0x7f;
            if event_type == XCB_KEY_PRESS
                && !self.paused
                && !app.state::<super::ShortcutRecording>().is_active()
            {
                let kp = &*(event as *const XcbKeyPressEvent);
                let state = kp.state & TRACKED_MOD_MASK;
                if let Some(grab) = self
                    .grabbed
                    .iter()
                    .find(|grab| grab.keycode == kp.detail && grab.modifiers == state)
                {
                    let action = grab.action;
                    let dispatch_app = app.clone();
                    let run_app = app.clone();
                    let _ = dispatch_app.run_on_main_thread(move || {
                        if run_app.state::<super::ShortcutRecording>().is_active() {
                            return;
                        }
                        match action {
                            Action::Ocr => {
                                let _ = run_app.state::<crate::desktop_ocr::Manager>().scan();
                            }
                            Action::Vision => {
                                let _ = run_app.state::<crate::desktop_vision::Manager>().capture();
                            }
                        }
                    });
                }
            } else if event_type == XCB_MAPPING_NOTIFY {
                mapping_changed = true;
            }
            libc::free(event.cast());
        }
        if mapping_changed {
            if let Some(keymap) =
                KeyboardMap::query(self.api, self.conn, self.min_keycode, self.max_keycode)
            {
                self.keymap = keymap;
                let _ = self.apply_grabs();
            }
        }
    }

    unsafe fn update(
        &mut self,
        ocr: Option<String>,
        vision: Option<String>,
    ) -> Result<Vec<String>, String> {
        let mut desired = Vec::new();
        let ocr_parsed = match ocr.as_deref() {
            Some(raw) => Some((
                super::shortcut::Shortcut::parse(raw)
                    .map_err(|_| "desktop_ocr.invalid_shortcut")?,
                raw.to_owned(),
            )),
            None => None,
        };
        let vision_parsed = match vision.as_deref() {
            Some(raw) => Some((
                super::shortcut::Shortcut::parse(raw)
                    .map_err(|_| "desktop_vision.invalid_shortcut")?,
                raw.to_owned(),
            )),
            None => None,
        };
        if let (Some((ocr_sc, _)), Some((vision_sc, _))) = (&ocr_parsed, &vision_parsed) {
            if ocr_sc == vision_sc {
                return Err("desktop_vision.shortcut_conflict".into());
            }
        }
        if let Some((shortcut, raw)) = ocr_parsed {
            desired.push((Action::Ocr, shortcut, raw));
        }
        if let Some((shortcut, raw)) = vision_parsed {
            desired.push((Action::Vision, shortcut, raw));
        }
        self.desired = desired;
        self.apply_grabs()
    }
}

pub fn available() -> bool {
    let Some(api) = XcbApi::load() else {
        return false;
    };
    unsafe {
        let conn = (api.connect)(ptr::null(), ptr::null_mut());
        if conn.is_null() {
            return false;
        }
        let ok = (api.connection_has_error)(conn) == 0;
        (api.disconnect)(conn);
        ok
    }
}

pub struct X11Session {
    sender: Sender<Command>,
    wake_write: c_int,
    worker: Option<JoinHandle<()>>,
}

impl X11Session {
    pub fn start(app: AppHandle) -> Result<Self, String> {
        let api = XcbApi::load().ok_or_else(|| "capture.shortcuts_unavailable".to_string())?;
        let mut fds = [-1; 2];
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
            return Err("capture.shortcuts_unavailable".into());
        }
        let (wake_read, wake_write) = (fds[0], fds[1]);
        let (sender, receiver) = mpsc::channel::<Command>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        let worker = std::thread::Builder::new()
            .name("vrcs-x11-shortcuts".into())
            .spawn(move || unsafe {
                run_worker(api, app, receiver, wake_read, ready_tx);
                libc::close(wake_read);
            })
            .map_err(|_| {
                unsafe {
                    libc::close(wake_read);
                    libc::close(wake_write);
                }
                "capture.shortcuts_unavailable".to_string()
            })?;

        match ready_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Self {
                sender,
                wake_write,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                unsafe { libc::close(wake_write) };
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = sender.send(Command::Shutdown);
                let _ = unsafe { libc::write(wake_write, [1u8].as_ptr().cast(), 1) };
                unsafe { libc::close(wake_write) };
                let _ = worker.join();
                Err("capture.shortcuts_unavailable".into())
            }
        }
    }

    pub fn update(
        &self,
        ocr: Option<String>,
        vision: Option<String>,
    ) -> Result<Vec<String>, String> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.send(Command::Update {
            ocr,
            vision,
            reply: reply_tx,
        })?;
        reply_rx
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "capture.shortcuts_unavailable".to_string())?
    }

    pub fn set_paused(&self, paused: bool) {
        let _ = self.send(Command::Pause(paused));
    }

    fn send(&self, cmd: Command) -> Result<(), String> {
        self.sender
            .send(cmd)
            .map_err(|_| "capture.shortcuts_unavailable".to_string())?;
        let byte = 1u8;
        let _ = unsafe { libc::write(self.wake_write, (&byte as *const u8).cast(), 1) };
        Ok(())
    }
}

impl Drop for X11Session {
    fn drop(&mut self) {
        let _ = self.send(Command::Shutdown);
        unsafe { libc::close(self.wake_write) };
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

unsafe fn run_worker(
    api: &'static XcbApi,
    app: AppHandle,
    receiver: Receiver<Command>,
    wake_read: c_int,
    ready: SyncSender<Result<(), String>>,
) {
    let conn = (api.connect)(ptr::null(), ptr::null_mut());
    if conn.is_null() {
        let _ = ready.send(Err("capture.shortcuts_unavailable".into()));
        return;
    }
    if (api.connection_has_error)(conn) != 0 {
        (api.disconnect)(conn);
        let _ = ready.send(Err("capture.shortcuts_unavailable".into()));
        return;
    }

    let setup = (api.get_setup)(conn);
    let roots = if setup.is_null() {
        XcbScreenIterator {
            data: ptr::null_mut(),
            rem: 0,
            index: 0,
        }
    } else {
        (api.setup_roots_iterator)(setup)
    };
    if setup.is_null() || roots.rem <= 0 || roots.data.is_null() {
        (api.disconnect)(conn);
        let _ = ready.send(Err("capture.shortcuts_unavailable".into()));
        return;
    }
    let root = (*roots.data).root;
    let min_keycode = (*setup).min_keycode;
    let max_keycode = (*setup).max_keycode;
    let Some(keymap) = KeyboardMap::query(api, conn, min_keycode, max_keycode) else {
        (api.disconnect)(conn);
        let _ = ready.send(Err("capture.shortcuts_unavailable".into()));
        return;
    };
    let xcb_fd = (api.get_file_descriptor)(conn);
    if xcb_fd < 0 {
        (api.disconnect)(conn);
        let _ = ready.send(Err("capture.shortcuts_unavailable".into()));
        return;
    }

    let mut worker = X11Worker {
        api,
        conn,
        root,
        min_keycode,
        max_keycode,
        keymap,
        desired: Vec::new(),
        grabbed: Vec::new(),
        paused: app.state::<super::ShortcutRecording>().is_active(),
    };
    let _ = ready.send(Ok(()));

    let mut fds = [
        libc::pollfd {
            fd: wake_read,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: xcb_fd,
            events: libc::POLLIN,
            revents: 0,
        },
    ];

    loop {
        if (api.connection_has_error)(conn) != 0 {
            break;
        }
        fds[0].revents = 0;
        fds[1].revents = 0;
        let ret = libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1);
        if ret < 0 {
            if *libc::__errno_location() == libc::EINTR {
                continue;
            }
            break;
        }

        if (fds[0].revents & libc::POLLIN) != 0 {
            let mut buf = [0u8; 64];
            while libc::read(wake_read, buf.as_mut_ptr().cast(), buf.len()) > 0 {}
            let mut stop = false;
            for cmd in receiver.try_iter() {
                match cmd {
                    Command::Update { ocr, vision, reply } => {
                        let _ = reply.send(worker.update(ocr, vision));
                    }
                    Command::Pause(paused) => {
                        if worker.paused != paused {
                            worker.paused = paused;
                            if paused {
                                worker.ungrab_all();
                            } else {
                                let _ = worker.apply_grabs();
                            }
                            worker.drain_events(&app);
                        }
                    }
                    Command::Shutdown => {
                        stop = true;
                        break;
                    }
                }
            }
            if stop {
                break;
            }
        }
        if (fds[1].revents & (libc::POLLIN | libc::POLLERR | libc::POLLHUP)) != 0 {
            worker.drain_events(&app);
        }
        if (fds[0].revents & libc::POLLHUP) != 0 {
            break;
        }
    }

    worker.ungrab_all();
    (api.disconnect)(conn);
}
