//! Native Windows platform layer. Everything here talks to Win32 / COM / WinRT
//! and knows nothing about the UI or the agent. Blocking calls must be run via
//! [`blocking`] so COM is initialised on the worker thread and the async
//! runtime is never stalled.

pub mod apps;
pub mod audio;
pub mod brightness;
pub mod capture;
pub mod clipboard;
pub mod com;
pub mod display;
pub mod input;
pub mod island;
pub mod media;
pub mod process;
pub mod supervisor;
pub mod uia;
pub mod watcher;
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
        com::ensure_mta();
        f()
    })
    .await
    .map_err(|e| AppError::internal(format!("Фоновая задача завершилась аварийно: {e}")))?
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
