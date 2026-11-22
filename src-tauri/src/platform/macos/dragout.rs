use crate::error::AppError;
use crate::error::AppResult;
use crate::platform::HWND;
pub fn validate_drag_paths(_paths: &[String]) -> AppResult<()> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn drag_files_out(_hwnd: HWND, _paths: &[String]) -> AppResult<bool> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
