pub use super::data_display::*;
use crate::platform::HWND;
pub fn primary_monitor() -> Option<MonitorInfo> {
    None
}
pub fn monitor_for_window(_hwnd: HWND) -> Option<MonitorInfo> {
    None
}
pub fn monitor_for_point(_x: i32, _y: i32) -> Option<MonitorInfo> {
    None
}
pub fn list_monitors() -> Vec<MonitorInfo> {
    Vec::new()
}
pub fn foreground_shape(
    _own_hwnd: isize,
    _island_monitor: isize,
    _island_center_x: i32,
    _island_bottom_y: i32,
) -> ForegroundShape {
    ForegroundShape::default()
}
