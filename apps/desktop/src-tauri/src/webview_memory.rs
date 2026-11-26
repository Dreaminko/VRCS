#[cfg(windows)]
use webview2_com::{
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2, ICoreWebView2_3, ICoreWebView2_19,
    },
    TrySuspendCompletedHandler,
};
#[cfg(windows)]
use windows::core::Interface;

#[cfg(windows)]
pub fn set_webview_memory_low(webview: &ICoreWebView2, low: bool) {
    let level = if low {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    };
    if let Ok(wv19) = webview.cast::<ICoreWebView2_19>() {
        let _ = unsafe { wv19.SetMemoryUsageTargetLevel(level) };
    }
}

#[cfg(windows)]
pub fn suspend_webview(webview: &ICoreWebView2) {
    set_webview_memory_low(webview, true);
    if let Ok(wv3) = webview.cast::<ICoreWebView2_3>() {
        let handler = TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(())));
        let _ = unsafe { wv3.TrySuspend(&handler) };
    }
}

#[cfg(windows)]
pub fn resume_webview(webview: &ICoreWebView2) {
    if let Ok(wv3) = webview.cast::<ICoreWebView2_3>() {
        let _ = unsafe { wv3.Resume() };
    }
    set_webview_memory_low(webview, false);
}

pub fn set_window_memory_low(window: &tauri::WebviewWindow, low: bool) {
    #[cfg(windows)]
    let _ = window.with_webview(move |wv| unsafe {
        if let Ok(core) = wv.controller().CoreWebView2() {
            set_webview_memory_low(&core, low);
        }
    });
    #[cfg(not(windows))]
    let _ = (window, low);
}

pub fn suspend_window(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    let _ = window.with_webview(|wv| unsafe {
        if let Ok(core) = wv.controller().CoreWebView2() {
            suspend_webview(&core);
        }
    });
    #[cfg(not(windows))]
    let _ = window;
}

pub fn resume_window(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    let _ = window.with_webview(|wv| unsafe {
        if let Ok(core) = wv.controller().CoreWebView2() {
            resume_webview(&core);
        }
    });
    #[cfg(not(windows))]
    let _ = window;
}
