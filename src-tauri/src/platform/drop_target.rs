//! Files dropped on the island, taken by our own OLE drop target.
//!
//! A drop over the webview lands on `Chrome_WidgetWin_1`, a window of the
//! WebView2 browser process with its own drop target that rejects external
//! files (Tauri's target on our windows is only reached when the race at
//! start-up goes its way). So while a file drag is near, an invisible window
//! of ours covers the island and takes the drop.

use std::cell::Cell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use windows::core::{implement, w};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINTL, RECT, WPARAM};
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, CF_HDROP, DROPEFFECT, DROPEFFECT_COPY,
    DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowRect, RegisterClassW, SetLayeredWindowAttributes, SetWindowPos, ShowWindow, HWND_TOPMOST,
    LWA_ALPHA, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

pub const EVT_FILE_DROP: &str = "island:file-drop";

#[derive(Serialize, Clone)]
pub struct FileDropEvent {
    /// enter | over | leave | drop
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub paths: Vec<String>,
}

#[implement(IDropTarget)]
struct DropTarget {
    app: AppHandle,
    label: &'static str,
    /// The data under the cursor holds files.
    valid: Cell<bool>,
}

impl DropTarget {
    fn emit(&self, kind: &'static str, paths: Vec<String>) {
        tracing::debug!(kind, files = paths.len(), "island file drop");
        let _ = self.app.emit_to(self.label, EVT_FILE_DROP, FileDropEvent { kind, paths });
    }
}

fn file_paths(data: Option<&IDataObject>) -> Option<Vec<String>> {
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let mut medium = unsafe { data?.GetData(&format) }.ok()?;
    let hdrop = HDROP(unsafe { medium.u.hGlobal.0 });
    let count = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
    let paths = (0..count)
        .map(|i| {
            let len = unsafe { DragQueryFileW(hdrop, i, None) } as usize;
            let mut buf = vec![0u16; len + 1];
            unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) };
            OsString::from_wide(&buf[..len]).to_string_lossy().into_owned()
        })
        .collect();
    unsafe { ReleaseStgMedium(&mut medium) };
    Some(paths)
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: windows::core::Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let paths = file_paths(data.as_ref());
        self.valid.set(paths.is_some());
        unsafe { *effect = if paths.is_some() { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        if let Some(paths) = paths {
            self.emit("enter", paths);
        }
        Ok(())
    }

    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        unsafe { *effect = if self.valid.get() { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        if self.valid.replace(false) {
            self.emit("leave", vec![]);
        }
        Ok(())
    }

    fn Drop(
        &self,
        data: windows::core::Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.valid.set(false);
        match file_paths(data.as_ref()) {
            Some(paths) => {
                unsafe { *effect = DROPEFFECT_COPY };
                self.emit("drop", paths);
            }
            None => unsafe { *effect = DROPEFFECT_NONE },
        }
        Ok(())
    }
}

thread_local! {
    static CATCHER: Cell<Option<isize>> = const { Cell::new(None) };
}

unsafe extern "system" fn catcher_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, w, l)
}

/// The catcher window, created on first use. UI thread only.
fn catcher(app: &AppHandle, label: &'static str) -> Option<HWND> {
    if let Some(h) = CATCHER.with(Cell::get) {
        return Some(HWND(h as _));
    }
    unsafe {
        let class = w!("JarvisDropCatcher");
        let instance = GetModuleHandleW(None).ok()?;
        let wc = WNDCLASSW { lpfnWndProc: Some(catcher_proc), hInstance: instance.into(), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!(""),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        // Alpha 1 of 255: invisible, yet Windows still hit-tests it.
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 1, LWA_ALPHA);
        let target: IDropTarget = DropTarget { app: app.clone(), label, valid: Cell::new(false) }.into();
        if let Err(e) = RegisterDragDrop(hwnd, &target) {
            tracing::warn!("drop catcher registration failed: {e}");
        }
        CATCHER.with(|c| c.set(Some(hwnd.0 as isize)));
        Some(hwnd)
    }
}

/// Cover the island's window with the catcher while a file drag is near:
/// the webview's own windows belong to the WebView2 browser process and
/// take the drop for themselves, rejecting it. `None` hides it.
/// UI thread only.
pub fn cover(app: &AppHandle, label: &'static str, island: HWND, show: bool) {
    let Some(hwnd) = catcher(app, label) else { return };
    unsafe {
        if show {
            let mut r = RECT::default();
            if GetWindowRect(island, &mut r).is_err() {
                return;
            }
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        } else {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
    tracing::debug!(show, "island drop catcher");
}
