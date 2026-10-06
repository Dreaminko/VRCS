use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use tauri::{AppHandle, Manager as _};
use windows_sys::Win32::{
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT},
        WindowsAndMessaging::{
            GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, WM_APP, WM_HOTKEY,
            WM_QUIT,
        },
    },
};

pub struct Hotkey {
    sender: Sender<Option<String>>,
    thread_id: u32,
    worker: Option<JoinHandle<()>>,
}

impl Hotkey {
    pub fn new(app: AppHandle) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel::<Option<String>>();
        let (ready, thread_id) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("vrcs-ocr-hotkey".into())
            .spawn(move || {
                let mut message: MSG = unsafe { std::mem::zeroed() };
                unsafe {
                    PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
                }
                if ready.send(unsafe { GetCurrentThreadId() }).is_err() {
                    return;
                }
                let mut registered = false;
                while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
                    if message.message == WM_APP {
                        for shortcut in receiver.try_iter() {
                            if registered {
                                unsafe {
                                    UnregisterHotKey(std::ptr::null_mut(), 1);
                                }
                            }
                            registered = false;
                            let error =
                                match shortcut.as_deref().map(super::shortcut::parse_shortcut) {
                                    None => None,
                                    Some(Err(_)) => Some("desktop_ocr.invalid_shortcut".into()),
                                    Some(Ok((modifiers, key))) => {
                                        registered = unsafe {
                                            RegisterHotKey(
                                                std::ptr::null_mut(),
                                                1,
                                                modifiers | MOD_NOREPEAT,
                                                key,
                                            ) != 0
                                        };
                                        (!registered)
                                            .then(|| "desktop_ocr.shortcut_conflict".into())
                                    }
                                };
                            app.state::<super::Manager>().shortcut_error(error);
                        }
                    } else if message.message == WM_HOTKEY && registered {
                        let app = app.clone();
                        let dispatcher = app.clone();
                        let _ = dispatcher.run_on_main_thread(move || {
                            if let Err(error) = app.state::<super::Manager>().scan() {
                                tracing::debug!(%error, "Desktop OCR shortcut was ignored");
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
        self.sender
            .send(shortcut)
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
