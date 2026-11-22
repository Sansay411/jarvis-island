//! Native Windows platform layer. Everything here talks to Win32 / COM / WinRT
//! and knows nothing about the UI or the agent. Blocking calls must be run via
//! [`blocking`] so COM is initialised on the worker thread and the async
//! runtime is never stalled.

#[cfg(windows)]
pub mod appicon;
#[cfg(target_os = "macos")]
#[path = "macos/appicon.rs"]
pub mod appicon;
#[cfg(windows)]
pub mod apps;
#[cfg(target_os = "macos")]
#[path = "macos/apps.rs"]
pub mod apps;
#[cfg(windows)]
pub mod audio;
#[cfg(target_os = "macos")]
#[path = "macos/audio.rs"]
pub mod audio;
#[cfg(windows)]
pub mod brightness;
#[cfg(target_os = "macos")]
#[path = "macos/brightness.rs"]
pub mod brightness;
#[cfg(windows)]
pub mod browser;
#[cfg(target_os = "macos")]
#[path = "macos/browser.rs"]
pub mod browser;
#[cfg(windows)]
pub mod capture;
#[cfg(target_os = "macos")]
#[path = "macos/capture.rs"]
pub mod capture;
#[cfg(windows)]
pub mod clipboard;
#[cfg(target_os = "macos")]
#[path = "macos/clipboard.rs"]
pub mod clipboard;
#[cfg(windows)]
pub mod clipwatch;
#[cfg(target_os = "macos")]
#[path = "macos/clipwatch.rs"]
pub mod clipwatch;
#[cfg(windows)]
pub mod com;
#[cfg(target_os = "macos")]
#[path = "macos/com.rs"]
pub mod com;
#[cfg(windows)]
pub mod display;
#[cfg(target_os = "macos")]
#[path = "macos/display.rs"]
pub mod display;
#[cfg(windows)]
pub mod dragout;
#[cfg(target_os = "macos")]
#[path = "macos/dragout.rs"]
pub mod dragout;
#[cfg(windows)]
pub mod drop_target;
#[cfg(target_os = "macos")]
#[path = "macos/drop_target.rs"]
pub mod drop_target;
#[cfg(windows)]
pub mod input;
#[cfg(target_os = "macos")]
#[path = "macos/input.rs"]
pub mod input;
#[cfg(windows)]
pub mod island;
#[cfg(target_os = "macos")]
#[path = "macos/island.rs"]
pub mod island;
#[cfg(windows)]
pub mod media;
#[cfg(target_os = "macos")]
#[path = "macos/media.rs"]
pub mod media;
#[cfg(windows)]
pub mod process;
#[cfg(target_os = "macos")]
#[path = "macos/process.rs"]
pub mod process;
#[cfg(windows)]
pub mod supervisor;
#[cfg(target_os = "macos")]
#[path = "macos/supervisor.rs"]
pub mod supervisor;
#[cfg(windows)]
pub mod uia;
#[cfg(target_os = "macos")]
#[path = "macos/uia.rs"]
pub mod uia;
#[cfg(windows)]
pub mod watcher;
#[cfg(target_os = "macos")]
#[path = "macos/watcher.rs"]
pub mod watcher;
#[cfg(windows)]
pub mod winmgr;
#[cfg(target_os = "macos")]
#[path = "macos/winmgr.rs"]
pub mod winmgr;

use crate::error::{AppError, AppResult};

/// Run a blocking platform call on Tokio's blocking pool with COM (MTA)
/// initialised on that thread.
pub async fn blocking<T, F>(f: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        #[cfg(windows)]
        com::ensure_mta();
        f()
    })
    .await
    .map_err(|e| AppError::internal(format!("Фоновая задача завершилась аварийно: {e}")))?
}

/// Run `f` in the background scheduling mode (low CPU, I/O and memory
/// priority) on the current thread: warm-up work nobody is waiting for —
/// loading speech models, indexing apps — must not make the desktop stutter.
#[cfg(windows)]
pub fn in_background<T>(f: impl FnOnce() -> T) -> T {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN, THREAD_MODE_BACKGROUND_END};
    let lowered = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) }.is_ok();
    let out = f();
    if lowered {
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_END) };
    }
    out
}

/// Encode a Rust string as a NUL-terminated UTF-16 buffer for Win32 calls.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Decode a UTF-16 buffer up to the first NUL.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

mod data_apps;

#[cfg(target_os = "macos")]
#[path = "macos/native.rs"]
mod native;

mod data_audio;

mod data_brightness;

mod data_browser;

mod data_capture;

mod data_clipboard;

mod data_display;

mod data_drop_target;

mod data_input;

mod data_island;

mod data_media;

mod data_supervisor;

mod data_uia;

mod data_winmgr;

#[cfg(target_os = "macos")]
pub fn in_background<T>(f: impl FnOnce() -> T) -> T { f() }

#[cfg(windows)]
pub use windows::Win32::Foundation::HWND;
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HWND(pub isize);

#[cfg(all(test, target_os = "macos"))]
#[path = "macos/tests.rs"]
mod tests;
