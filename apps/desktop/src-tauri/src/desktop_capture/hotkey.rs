use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::{AppHandle, Manager as _};
use windows_sys::Win32::{
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT},
        WindowsAndMessaging::{
            GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, PM_REMOVE, WM_APP,
            WM_HOTKEY, WM_QUIT,
        },
    },
};

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Ocr,
    Vision(u64),
}

impl Action {
    fn error(self, code: &str) -> String {
        let prefix = match self {
            Self::Ocr => "desktop_ocr",
            Self::Vision(_) => "desktop_vision",
        };
        format!("{prefix}.{code}")
    }

    fn report(self, app: &AppHandle, error: Option<String>) {
        match self {
            Self::Ocr => app
                .state::<crate::desktop_ocr::Manager>()
                .shortcut_error(error),
            Self::Vision(lifecycle) => app
                .state::<crate::desktop_vision::Manager>()
                .shortcut_error_for(lifecycle, error),
        }
    }

    fn trigger(self, app: &AppHandle) {
        match self {
            Self::Ocr => {
                let _ = app.state::<crate::desktop_ocr::Manager>().scan();
            }
            Self::Vision(lifecycle) => {
                let _ = app
                    .state::<crate::desktop_vision::Manager>()
                    .capture_for(lifecycle);
            }
        }
    }
}

enum Command {
    Update(Option<String>, Option<String>),
    Pause(bool, Sender<Result<(), String>>),
}

#[derive(Default)]
struct Binding {
    shortcut: Option<String>,
    preflight_error: Option<String>,
    paused: bool,
}

impl Binding {
    fn registration(&self) -> Option<Result<Option<super::shortcut::Shortcut>, String>> {
        (!self.paused).then(|| {
            self.shortcut
                .as_deref()
                .map(super::shortcut::Shortcut::parse)
                .transpose()
        })
    }

    fn apply(&self, app: &AppHandle, action: Action, registered: &mut bool) {
        let Some(binding) = self.registration() else {
            return;
        };
        if *registered {
            unsafe {
                UnregisterHotKey(std::ptr::null_mut(), 1);
            }
        }
        *registered = false;
        let error = match binding {
            Ok(None) => None,
            Err(_) => Some(action.error("invalid_shortcut")),
            Ok(Some(shortcut)) => {
                let (modifiers, key) = shortcut.windows_registration();
                *registered = unsafe {
                    RegisterHotKey(std::ptr::null_mut(), 1, modifiers | MOD_NOREPEAT, key) != 0
                };
                if !*registered {
                    tracing::warn!(?action, error = %std::io::Error::last_os_error(), "RegisterHotKey failed");
                }
                (!*registered).then(|| action.error("shortcut_conflict"))
            }
        };
        let error = error.or_else(|| self.preflight_error.clone());
        if let Some(error) = &error {
            tracing::warn!(?action, %error, "Desktop screenshot shortcut registration failed");
        } else if *registered {
            tracing::info!(?action, "Desktop screenshot shortcut registered");
        }
        action.report(app, error);
    }
}

pub struct Hotkey {
    sender: Sender<Command>,
    thread_id: u32,
    worker: Option<JoinHandle<()>>,
}

impl Hotkey {
    pub fn new(app: AppHandle, action: Action) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel::<Command>();
        let paused = app.state::<super::ShortcutRecording>().is_active();
        let (ready, thread_id) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vrcs-desktop-hotkey".into())
            .spawn(move || {
                let mut message: MSG = unsafe { std::mem::zeroed() };
                unsafe {
                    PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
                }
                if ready.send(unsafe { GetCurrentThreadId() }).is_err() {
                    return;
                }
                let mut registered = false;
                let mut binding = Binding {
                    paused,
                    ..Binding::default()
                };
                while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
                    if message.message == WM_APP {
                        for command in receiver.try_iter() {
                            match command {
                                Command::Update(shortcut, preflight_error) => {
                                    binding.shortcut = shortcut;
                                    binding.preflight_error = preflight_error;
                                    binding.apply(&app, action, &mut registered);
                                }
                                Command::Pause(paused, acknowledged) => {
                                    let result = if paused && registered {
                                        if unsafe { UnregisterHotKey(std::ptr::null_mut(), 1) } == 0
                                        {
                                            Err(std::io::Error::last_os_error().to_string())
                                        } else {
                                            registered = false;
                                            // A queued press from the previous registration must not
                                            // trigger a capture after the recorder has finished.
                                            let mut pending: MSG = unsafe { std::mem::zeroed() };
                                            while unsafe {
                                                PeekMessageW(
                                                    &mut pending,
                                                    std::ptr::null_mut(),
                                                    WM_HOTKEY,
                                                    WM_HOTKEY,
                                                    PM_REMOVE,
                                                )
                                            } != 0
                                            {}
                                            Ok(())
                                        }
                                    } else {
                                        Ok(())
                                    };
                                    if result.is_ok() && binding.paused != paused {
                                        binding.paused = paused;
                                        binding.apply(&app, action, &mut registered);
                                    }
                                    let _ = acknowledged.send(result);
                                }
                            }
                        }
                    } else if message.message == WM_HOTKEY
                        && registered
                        && !app.state::<super::ShortcutRecording>().is_active()
                    {
                        let app = app.clone();
                        let dispatcher = app.clone();
                        let _ = dispatcher.run_on_main_thread(move || {
                            if !app.state::<super::ShortcutRecording>().is_active() {
                                action.trigger(&app);
                            }
                        });
                    }
                }
                if registered {
                    unsafe {
                        UnregisterHotKey(std::ptr::null_mut(), 1);
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        let thread_id = thread_id.recv().map_err(|error| error.to_string())?;
        Ok(Self {
            sender,
            thread_id,
            worker: Some(worker),
        })
    }

    pub fn update(&self, shortcut: Option<String>) -> Result<(), String> {
        self.update_with_error(shortcut, None)
    }

    pub fn update_with_error(
        &self,
        shortcut: Option<String>,
        error: Option<String>,
    ) -> Result<(), String> {
        self.send(Command::Update(shortcut, error))
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), String> {
        let (acknowledged, receiver) = mpsc::channel();
        self.send(Command::Pause(paused, acknowledged))?;
        receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| format!("Desktop shortcut suspension was not acknowledged: {error}"))?
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.sender
            .send(command)
            .map_err(|error| error.to_string())?;
        if unsafe { PostThreadMessageW(self.thread_id, WM_APP, 0, 0) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
}

impl Drop for Hotkey {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paused_worker_keeps_the_latest_binding_without_registering() {
        let mut binding = Binding {
            shortcut: Some("Ctrl+Alt+O".into()),
            paused: true,
            ..Binding::default()
        };
        assert!(binding.registration().is_none());
        binding.shortcut = Some("Shift+Ctrl+V".into());
        assert!(binding.registration().is_none());
        binding.paused = false;
        assert_eq!(binding.registration().unwrap().unwrap(), Some((6, 86)));
    }

    #[test]
    fn resume_keeps_the_latest_disabled_binding_and_preflight_error() {
        let mut binding = Binding {
            shortcut: Some("Ctrl+Alt+V".into()),
            paused: true,
            ..Binding::default()
        };
        binding.shortcut = None;
        binding.preflight_error = Some("desktop_vision.shortcut_conflict".into());
        assert!(binding.registration().is_none());
        binding.paused = false;
        assert_eq!(binding.registration().unwrap().unwrap(), None);
        assert_eq!(
            binding.preflight_error.as_deref(),
            Some("desktop_vision.shortcut_conflict")
        );
        binding.preflight_error = None;
        assert_eq!(binding.registration().unwrap().unwrap(), None);
    }

    #[test]
    fn resume_validates_the_latest_shortcut() {
        let mut binding = Binding {
            shortcut: Some("Ctrl+Alt+O".into()),
            paused: true,
            ..Binding::default()
        };
        binding.shortcut = Some("Ctrl+Space".into());
        assert!(binding.registration().is_none());
        binding.paused = false;
        assert!(binding.registration().unwrap().is_err());
    }
}
