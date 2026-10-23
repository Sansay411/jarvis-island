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

/// Put files on the clipboard as Explorer does (CF_HDROP, «copy»): Ctrl+V
/// pastes them into Telegram, mail, chats or a folder.
pub fn write_files(paths: &[String]) -> AppResult<()> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    use windows::Win32::UI::Shell::DROPFILES;
    if paths.is_empty() {
        return Err(AppError::invalid("Нет файлов"));
    }
    let mut list: Vec<u16> = Vec::new();
    for p in paths {
        let abs = std::path::absolute(p).map(|a| a.to_string_lossy().replace('/', "\\")).unwrap_or_else(|_| p.clone());
        list.extend(abs.encode_utf16());
        list.push(0);
    }
    list.push(0);
    let header = std::mem::size_of::<DROPFILES>();
    let _guard = Open::acquire()?;
    unsafe {
        EmptyClipboard()?;
        let hg = GlobalAlloc(GMEM_MOVEABLE, header + list.len() * 2)?;
        let p = GlobalLock(hg) as *mut u8;
        if p.is_null() {
            return Err(AppError::platform("Не удалось выделить память для буфера обмена"));
        }
        let df = DROPFILES { pFiles: header as u32, pt: POINT::default(), fNC: BOOL(0), fWide: BOOL(1) };
        std::ptr::write_unaligned(p as *mut DROPFILES, df);
        std::ptr::copy_nonoverlapping(list.as_ptr() as *const u8, p.add(header), list.len() * 2);
        let _ = GlobalUnlock(hg);
        SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(hg.0)))?;
        // «Preferred DropEffect» = copy: the target must not move the original.
        let fmt = RegisterClipboardFormatW(windows::core::w!("Preferred DropEffect"));
        if fmt != 0 {
            if let Ok(he) = GlobalAlloc(GMEM_MOVEABLE, 4) {
                let pe = GlobalLock(he) as *mut u32;
                if !pe.is_null() {
                    *pe = 1; // DROPEFFECT_COPY
                    let _ = GlobalUnlock(he);
                    let _ = SetClipboardData(fmt, Some(HANDLE(he.0)));
                }
            }
        }
    }
    Ok(())
}

/// Put back what was on the clipboard before (text or files).
pub fn restore(prev: &ClipboardContent) -> AppResult<()> {
    if !prev.files.is_empty() {
        write_files(&prev.files)
    } else if let Some(t) = &prev.text {
        write_text(t)
    } else {
        let _guard = Open::acquire()?;
        unsafe { EmptyClipboard()? };
        Ok(())
    }
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
