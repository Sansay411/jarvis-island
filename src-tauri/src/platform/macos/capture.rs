pub use super::data_capture::*;
use crate::error::AppError;
use crate::error::AppResult;
use crate::platform::display::Rect;
use crate::platform::HWND;
pub const MAX_SIDE: u32 = 1600;
pub fn capture_rect(_rect: Rect, _island_hwnd: Option<isize>) -> AppResult<Frame> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn window_frame(_hwnd: HWND) -> AppResult<Rect> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn capture_window(_hwnd: HWND) -> AppResult<Frame> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
pub fn encode(_frame: Frame, _max_side: u32) -> AppResult<EncodedCapture> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
