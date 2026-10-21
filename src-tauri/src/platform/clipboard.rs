//! Clipboard text and file lists via Win32.

use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

use crate::error::{AppError, AppResult};

struct Open;

impl Open {
    fn acquire() -> AppResult<Open> {
        // Another process may briefly hold the clipboard.
        for _ in 0..10 {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(Open);
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        Err(AppError::platform("Буфер обмена занят другим приложением"))
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardContent {
    pub text: Option<String>,
    pub files: Vec<String>,
}

pub fn read() -> AppResult<ClipboardContent> {
    let _guard = Open::acquire()?;
    let mut out = ClipboardContent { text: None, files: Vec::new() };
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_ok() {
            if let Ok(h) = GetClipboardData(CF_UNICODETEXT.0 as u32) {
                let hg = HGLOBAL(h.0);
                let p = GlobalLock(hg) as *const u16;
                if !p.is_null() {
                    let mut len = 0;
                    while *p.add(len) != 0 {
                        len += 1;
                    }
                    out.text = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)));
                    let _ = GlobalUnlock(hg);
                }
            }
        }
        if IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok() {
            if let Ok(h) = GetClipboardData(CF_HDROP.0 as u32) {
                let drop = HDROP(h.0);
                let count = DragQueryFileW(drop, u32::MAX, None);
                for i in 0..count {
                    let mut buf = [0u16; 1024];
                    let n = DragQueryFileW(drop, i, Some(&mut buf));
                    out.files.push(String::from_utf16_lossy(&buf[..n as usize]));
                }
            }
        }
    }
    Ok(out)
}

pub fn write_text(text: &str) -> AppResult<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let _guard = Open::acquire()?;
    unsafe {
        EmptyClipboard()?;
        let bytes = wide.len() * 2;
        let hg = GlobalAlloc(GMEM_MOVEABLE, bytes)?;
        let p = GlobalLock(hg) as *mut u16;
        if p.is_null() {
            return Err(AppError::platform("Не удалось выделить память для буфера обмена"));
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
        let _ = GlobalUnlock(hg);
        // On success the system owns the memory.
        SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hg.0)))?;
    }
    Ok(())
}
