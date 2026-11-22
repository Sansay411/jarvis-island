pub use super::data_drop_target::*;
use crate::platform::HWND;
use tauri::AppHandle;
pub const EVT_FILE_DROP: &str = "island:file-drop";
pub fn cover(_app: &AppHandle, _label: &'static str, _island: HWND, _show: bool) {}
