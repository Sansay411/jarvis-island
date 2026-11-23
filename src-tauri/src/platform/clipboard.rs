//! Clipboard text and file lists via Win32.

use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardOwner, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

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

pub use super::data_clipboard::ClipboardContent;

pub fn read() -> AppResult<ClipboardContent> {
    let _guard = Open::acquire()?;
    unsafe { Ok(ClipboardContent { text: text_open(usize::MAX), files: files_open() }) }
}

/// The text on the clipboard, which must be open. `max_bytes` skips larger
/// ones without copying them.
unsafe fn text_open(max_bytes: usize) -> Option<String> {
    if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_err() {
        return None;
    }
    let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
    let hg = HGLOBAL(h.0);
    if GlobalSize(hg) > max_bytes {
        return None;
    }
    let p = GlobalLock(hg) as *const u16;
    if p.is_null() {
        return None;
    }
    let mut len = 0;
    while *p.add(len) != 0 {
        len += 1;
    }
    let text = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
    let _ = GlobalUnlock(hg);
    Some(text)
}

/// The copied files, which must be open.
unsafe fn files_open() -> Vec<String> {
    let mut files = Vec::new();
    if IsClipboardFormatAvailable(CF_HDROP.0 as u32).is_ok() {
        if let Ok(h) = GetClipboardData(CF_HDROP.0 as u32) {
            let drop = HDROP(h.0);
            let count = DragQueryFileW(drop, u32::MAX, None);
            for i in 0..count {
                let mut buf = [0u16; 1024];
                let n = DragQueryFileW(drop, i, Some(&mut buf));
                files.push(String::from_utf16_lossy(&buf[..n as usize]));
            }
        }
    }
    files
}

/// Largest text or picture the history even looks at.
const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 200 * 1024 * 1024;

pub use super::data_clipboard::Snapshot;

fn registered(name: PCWSTR) -> u32 {
    unsafe { RegisterClipboardFormatW(name) }
}

unsafe fn available(format: u32) -> bool {
    format != 0 && IsClipboardFormatAvailable(format).is_ok()
}

/// The bytes behind a clipboard handle, if there are not too many.
unsafe fn bytes_of(handle: HANDLE, limit: usize) -> Option<Vec<u8>> {
    let hg = HGLOBAL(handle.0);
    let size = GlobalSize(hg);
    if size == 0 || size > limit {
        return None;
    }
    let p = GlobalLock(hg) as *const u8;
    if p.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(p, size).to_vec();
    let _ = GlobalUnlock(hg);
    Some(bytes)
}

/// One look at the clipboard for the history: files, else text, else a
/// picture. Copies the owner marked private are not read at all — the two
/// registered formats are those documented for the Windows clipboard history
/// (`ExcludeClipboardContentFromMonitorProcessing`, and
/// `CanIncludeInClipboardHistory` holding a DWORD 0).
pub fn snapshot() -> AppResult<Snapshot> {
    let _guard = Open::acquire()?;
    let mut s = Snapshot { sequence: unsafe { GetClipboardSequenceNumber() }, ..Default::default() };
    unsafe {
        if let Ok(owner) = GetClipboardOwner() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(owner, Some(&mut pid));
            s.owner_pid = (pid != 0).then_some(pid);
        }
        if available(registered(w!("ExcludeClipboardContentFromMonitorProcessing"))) {
            s.private = true;
            return Ok(s);
        }
        let history = registered(w!("CanIncludeInClipboardHistory"));
        if available(history) {
            let flag = GetClipboardData(history).ok().and_then(|h| bytes_of(h, 64));
            if flag.is_some_and(|b| b.len() >= 4 && b[..4] == [0, 0, 0, 0]) {
                s.private = true;
                return Ok(s);
            }
        }
        s.files = files_open();
        if !s.files.is_empty() {
            return Ok(s);
        }
        s.text = text_open(MAX_TEXT_BYTES).filter(|t| !t.trim().is_empty());
        if s.text.is_some() {
            return Ok(s);
        }
        let png = registered(w!("PNG"));
        if available(png) {
            s.png = GetClipboardData(png).ok().and_then(|h| bytes_of(h, MAX_IMAGE_BYTES));
        }
        if s.png.is_none() {
            // Windows converts between DIB formats, so a bitmap from any
            // program is available as DIBV5 (with alpha when it has some).
            for format in [CF_DIBV5.0 as u32, CF_DIB.0 as u32] {
                if available(format) {
                    s.dib = GetClipboardData(format).ok().and_then(|h| bytes_of(h, MAX_IMAGE_BYTES));
                    if s.dib.is_some() {
                        break;
                    }
                }
            }
        }
    }
    Ok(s)
}

/// Put a picture back: raw PNG for programs that read it, and a 24-bit DIB
/// that every program understands.
pub fn write_image(png: &[u8], dib: &[u8]) -> AppResult<()> {
    let png_format = registered(w!("PNG"));
    let _guard = Open::acquire()?;
    unsafe {
        EmptyClipboard()?;
        if png_format != 0 {
            set_bytes(png_format, png)?;
        }
        set_bytes(CF_DIB.0 as u32, dib)?;
    }
    Ok(())
}

unsafe fn set_bytes(format: u32, bytes: &[u8]) -> AppResult<()> {
    let hg = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
    let p = GlobalLock(hg) as *mut u8;
    if p.is_null() {
        return Err(AppError::platform("Не удалось выделить память для буфера обмена"));
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
    let _ = GlobalUnlock(hg);
    // On success the system owns the memory.
    SetClipboardData(format, Some(HANDLE(hg.0)))?;
    Ok(())
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
