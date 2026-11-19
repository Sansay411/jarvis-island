//! Dragging shelf files out of the island into Explorer or any window.
//!
//! The shell builds the data object (`CF_HDROP` plus shell IDLists, so every
//! drop target that takes files from Explorer takes them from us) and
//! `SHDoDragDrop` runs the modal drag loop. It must run on the STA UI thread
//! that owns the island window, so the command hops to the main thread.

use std::path::Path;

use crate::error::{AppError, AppResult};

/// Paths a drag may carry: at least one, all existing.
pub fn validate_drag_paths(paths: &[String]) -> AppResult<()> {
    if paths.is_empty() {
        return Err(AppError::invalid("Нет файлов для перетаскивания"));
    }
    if let Some(p) = paths.iter().find(|p| !Path::new(p.as_str()).exists()) {
        return Err(AppError::not_found(format!("Файл не найден: {p}")));
    }
    Ok(())
}

/// Runs the drag loop; `true` when a target accepted the files.
#[cfg(windows)]
pub fn drag_files_out(hwnd: windows::Win32::Foundation::HWND, paths: &[String]) -> AppResult<bool> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Com::{CoTaskMemFree, IDataObject};
    use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_NONE};
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        BHID_DataObject, IShellItemArray, SHCreateShellItemArrayFromIDLists, SHDoDragDrop, SHParseDisplayName,
    };

    validate_drag_paths(paths)?;
    let mut pidls: Vec<*mut ITEMIDLIST> = Vec::with_capacity(paths.len());
    let result = (|| -> AppResult<bool> {
        for p in paths {
            let wide = HSTRING::from(p.as_str());
            let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            unsafe { SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut pidl, 0, None) }
                .map_err(|e| AppError::internal(format!("Путь не разобран: {e}")))?;
            pidls.push(pidl);
        }
        let consts: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| *p as *const ITEMIDLIST).collect();
        let array: IShellItemArray = unsafe { SHCreateShellItemArrayFromIDLists(&consts) }
            .map_err(|e| AppError::internal(format!("Нет набора файлов: {e}")))?;
        let data: IDataObject = unsafe { array.BindToHandler(None, &BHID_DataObject) }
            .map_err(|e| AppError::internal(format!("Нет объекта данных: {e}")))?;
        let effect = unsafe { SHDoDragDrop(Some(hwnd), &data, None, DROPEFFECT_COPY) }
            .map_err(|e| AppError::internal(format!("Перетаскивание прервано: {e}")))?;
        Ok(effect != DROPEFFECT_NONE)
    })();
    for p in pidls {
        unsafe { CoTaskMemFree(Some(p as *const _)) };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_out_rejects_missing_paths() {
        assert!(validate_drag_paths(&[]).is_err());
        assert!(validate_drag_paths(&["C:/nope/definitely/x.txt".to_string()]).is_err());
        let p = std::env::temp_dir().join(format!("jarvis-dragout-{}.txt", std::process::id()));
        std::fs::write(&p, b"x").unwrap();
        assert!(validate_drag_paths(&[p.to_string_lossy().to_string()]).is_ok());
        let _ = std::fs::remove_file(p);
    }
}
