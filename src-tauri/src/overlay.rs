//! Full-monitor transparent click-through overlay window.
//!
//! Renders visual annotations (rings, arrows, captions, interactive cards) on
//! top of the user's real screen without stealing keyboard focus and without
//! appearing in screen captures (`WDA_EXCLUDEFROMCAPTURE`).
//!
//! Elements marked with `data-overlay-interactive` in the overlay DOM report
//! their physical screen rectangles via IPC; a lightweight ~30 Hz tracker
//! thread wakes only while interactive rectangles exist and toggles
//! `set_ignore_cursor_events` as the cursor enters/leaves them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::error::{AppError, AppResult};
use crate::platform::display::{self, MonitorInfo};

pub const LABEL: &str = "overlay";
pub const EVT_DRAW: &str = "overlay:draw";
pub const EVT_CLEAR: &str = "overlay:clear";
pub const EVT_PULSE: &str = "overlay:pulse";

/// Physical geometry and DPI scale of the monitor currently covered by the overlay.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayMonitor {
    pub origin_x: i32,
    pub origin_y: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f64,
}

impl Default for OverlayMonitor {
    fn default() -> Self {
        OverlayMonitor {
            origin_x: 0,
            origin_y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
        }
    }
}

impl From<&MonitorInfo> for OverlayMonitor {
    fn from(m: &MonitorInfo) -> Self {
        OverlayMonitor {
            origin_x: m.bounds.x,
            origin_y: m.bounds.y,
            width: m.bounds.width.max(1),
            height: m.bounds.height.max(1),
            scale: if m.scale > 0.0 && m.scale.is_finite() { m.scale } else { 1.0 },
        }
    }
}

/// Interactive hit-test rectangle in physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HitRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl HitRect {
    pub fn contains(&self, px: i32, py: i32, margin: i32) -> bool {
        if self.width <= 0 || self.height <= 0 {
            return false;
        }
        px >= self.x - margin
            && px < self.x + self.width + margin
            && py >= self.y - margin
            && py < self.y + self.height + margin
    }
}

/// Snapshot returned to the overlay frontend once it finishes mounting.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySnapshot {
    pub items: Vec<Value>,
    pub monitor: OverlayMonitor,
}

/// Pure hit-test: true when `(px, py)` falls inside any interactive rect (+ margin).
pub fn point_in_any_rect(rects: &[HitRect], px: i32, py: i32, margin: i32) -> bool {
    rects.iter().any(|r| r.contains(px, py, margin))
}

/// Pure pointer-capture state machine for interactive overlay regions.
/// Keeps the overlay interactive during a drag that started inside a rect.
#[derive(Debug, Clone, Copy, Default)]
pub struct HitTrackerState {
    pub interactive: bool,
    pub was_down: bool,
    pub press_inside: bool,
}

impl HitTrackerState {
    /// Compute whether cursor events should be received (`true`) or ignored (`false`).
    pub fn step(&mut self, rects: &[HitRect], px: i32, py: i32, button_down: bool, scale: f64) -> bool {
        let margin = if self.interactive { (4.0 * scale.max(1.0)).round() as i32 } else { 0 };
        let inside = point_in_any_rect(rects, px, py, margin);
        if button_down && !self.was_down {
            self.press_inside = inside;
        }
        if !button_down {
            self.press_inside = false;
        }
        self.was_down = button_down;
        let want = inside || (button_down && self.press_inside);
        self.interactive = want;
        want
    }
}

/// Convert a physical screen point to overlay CSS pixels for `mon`.
pub fn physical_to_css_point(px: f64, py: f64, mon: OverlayMonitor) -> (f64, f64) {
    let s = if mon.scale > 0.0 && mon.scale.is_finite() { mon.scale } else { 1.0 };
    ((px - mon.origin_x as f64) / s, (py - mon.origin_y as f64) / s)
}

/// Convert an overlay CSS rectangle to physical screen pixels for `mon`.
pub fn css_to_physical_rect(x: f64, y: f64, width: f64, height: f64, mon: OverlayMonitor) -> HitRect {
    let s = if mon.scale > 0.0 && mon.scale.is_finite() { mon.scale } else { 1.0 };
    HitRect {
        x: mon.origin_x + (x * s).round() as i32,
        y: mon.origin_y + (y * s).round() as i32,
        width: (width * s).round().max(0.0) as i32,
        height: (height * s).round().max(0.0) as i32,
    }
}

/// Extract a representative physical screen point from the first positioned item.
pub fn target_point_from_items(items: &[Value]) -> Option<(i32, i32)> {
    for item in items {
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
        match kind {
            "ring" => {
                let x = item.get("x").and_then(Value::as_f64)?;
                let y = item.get("y").and_then(Value::as_f64)?;
                let w = item.get("w").and_then(Value::as_f64).unwrap_or(0.0);
                let h = item.get("h").and_then(Value::as_f64).unwrap_or(0.0);
                return Some(((x + w / 2.0).round() as i32, (y + h / 2.0).round() as i32));
            }
            "arrow" => {
                if let Some(to) = item.get("to") {
                    if let (Some(x), Some(y)) = (to.get("x").and_then(Value::as_f64), to.get("y").and_then(Value::as_f64)) {
                        return Some((x.round() as i32, y.round() as i32));
                    }
                }
            }
            "caption" => {
                if let (Some(x), Some(y)) = (item.get("x").and_then(Value::as_f64), item.get("y").and_then(Value::as_f64)) {
                    return Some((x.round() as i32, y.round() as i32));
                }
            }
            "card" => {
                if let Some(a) = item.get("anchor") {
                    if let (Some(x), Some(y)) = (a.get("x").and_then(Value::as_f64), a.get("y").and_then(Value::as_f64)) {
                        let w = a.get("w").and_then(Value::as_f64).unwrap_or(0.0);
                        let h = a.get("h").and_then(Value::as_f64).unwrap_or(0.0);
                        return Some(((x + w / 2.0).round() as i32, (y + h / 2.0).round() as i32));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Pick the monitor covering `target` (or nearest/primary if outside bounds).
pub fn pick_monitor(monitors: &[MonitorInfo], target: Option<(i32, i32)>) -> Option<OverlayMonitor> {
    if monitors.is_empty() {
        return None;
    }
    if let Some((px, py)) = target {
        if let Some(m) = monitors.iter().find(|m| {
            px >= m.bounds.x
                && px < m.bounds.x + m.bounds.width
                && py >= m.bounds.y
                && py < m.bounds.y + m.bounds.height
        }) {
            return Some(OverlayMonitor::from(m));
        }
        // Nearest monitor by squared distance to its clamped point.
        let nearest = monitors.iter().min_by_key(|m| {
            let cx = px.clamp(m.bounds.x, m.bounds.x + m.bounds.width);
            let cy = py.clamp(m.bounds.y, m.bounds.y + m.bounds.height);
            let dx = (px - cx) as i64;
            let dy = (py - cy) as i64;
            dx * dx + dy * dy
        });
        if let Some(m) = nearest {
            return Some(OverlayMonitor::from(m));
        }
    }
    monitors
        .iter()
        .find(|m| m.primary)
        .or_else(|| monitors.first())
        .map(OverlayMonitor::from)
}

struct OverlayInner {
    items: Vec<Value>,
    monitor: OverlayMonitor,
    visible: bool,
}

struct TrackerShared {
    rects: Vec<HitRect>,
    scale: f64,
    visible: bool,
    app: Option<AppHandle>,
}

struct OverlayState {
    inner: Mutex<OverlayInner>,
    tracker_lock: StdMutex<TrackerShared>,
    tracker_cv: Condvar,
    tracker_started: AtomicBool,
    guide_active: AtomicBool,
    guide_cancelled: AtomicBool,
}

static STATE: OnceLock<Arc<OverlayState>> = OnceLock::new();

fn state() -> &'static Arc<OverlayState> {
    STATE.get_or_init(|| {
        Arc::new(OverlayState {
            inner: Mutex::new(OverlayInner {
                items: Vec::new(),
                monitor: OverlayMonitor::default(),
                visible: false,
            }),
            tracker_lock: StdMutex::new(TrackerShared {
                rects: Vec::new(),
                scale: 1.0,
                visible: false,
                app: None,
            }),
            tracker_cv: Condvar::new(),
            tracker_started: AtomicBool::new(false),
            guide_active: AtomicBool::new(false),
            guide_cancelled: AtomicBool::new(false),
        })
    })
}

fn update_tracker(app: Option<&AppHandle>, visible: bool, scale: f64, rects: Option<Vec<HitRect>>) {
    let st = state();
    {
        let mut g = st.tracker_lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(a) = app {
            g.app = Some(a.clone());
        }
        g.visible = visible;
        g.scale = if scale > 0.0 && scale.is_finite() { scale } else { 1.0 };
        if let Some(r) = rects {
            g.rects = r;
        } else if !visible {
            g.rects.clear();
        }
    }
    st.tracker_cv.notify_one();
    if visible {
        spawn_tracker_if_needed();
    }
}

fn spawn_tracker_if_needed() {
    let st = state();
    if st.tracker_started.swap(true, Ordering::SeqCst) {
        return;
    }
    let st_arc = Arc::clone(st);
    let _ = std::thread::Builder::new()
        .name("overlay-tracker".into())
        .spawn(move || tracker_loop(st_arc));
}

#[cfg(windows)]
fn poll_cursor() -> Option<(i32, i32, bool)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut pt = POINT::default();
    if unsafe { GetCursorPos(&mut pt) }.is_err() {
        return None;
    }
    let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
    Some((pt.x, pt.y, down))
}

#[cfg(not(windows))]
fn poll_cursor() -> Option<(i32, i32, bool)> {
    None
}

fn tracker_loop(st: Arc<OverlayState>) {
    let mut machine = HitTrackerState::default();
    let mut applied_interactive = false;

    loop {
        let (app, rects, scale) = {
            let mut g = st.tracker_lock.lock().unwrap_or_else(|e| e.into_inner());
            while !g.visible || g.rects.is_empty() {
                if applied_interactive {
                    if let Some(win) = g.app.as_ref().and_then(|a| a.get_webview_window(LABEL)) {
                        let _ = win.set_ignore_cursor_events(true);
                    }
                    applied_interactive = false;
                    machine = HitTrackerState::default();
                }
                g = st.tracker_cv.wait(g).unwrap_or_else(|e| e.into_inner());
            }
            (g.app.clone(), g.rects.clone(), g.scale)
        };

        if let Some((px, py, down)) = poll_cursor() {
            let want = machine.step(&rects, px, py, down, scale);
            if want != applied_interactive {
                if let Some(win) = app.as_ref().and_then(|a| a.get_webview_window(LABEL)) {
                    if win.set_ignore_cursor_events(!want).is_ok() {
                        applied_interactive = want;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(33));
    }
}

#[cfg(windows)]
fn style_overlay_window(win: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    };
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};

    if let Ok(raw) = win.hwnd() {
        let hwnd = HWND(raw.0 as *mut _);
        unsafe {
            let pref = DWMWCP_DONOTROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const _ as *const _,
                std::mem::size_of_val(&pref) as u32,
            );
            let none: u32 = 0xFFFF_FFFE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &none as *const _ as *const _,
                std::mem::size_of_val(&none) as u32,
            );
            // Permanently exclude the overlay from screen capture so the model
            // never sees our rings, arrows or cards in its screenshots.
            let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        }
    }
}

#[cfg(not(windows))]
fn style_overlay_window(_win: &WebviewWindow) {}

#[cfg(windows)]
fn show_no_activate(win: &WebviewWindow) -> AppResult<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        SW_SHOWNOACTIVATE,
    };

    if let Ok(raw) = win.hwnd() {
        let hwnd = HWND(raw.0 as *mut _);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
        }
        return Ok(());
    }
    win.show()?;
    Ok(())
}

#[cfg(not(windows))]
fn show_no_activate(win: &WebviewWindow) -> AppResult<()> {
    win.show()?;
    Ok(())
}

fn resolve_monitor_for_items(items: &[Value]) -> OverlayMonitor {
    let target = target_point_from_items(items);
    let monitors = display::list_monitors();
    if let Some(m) = pick_monitor(&monitors, target) {
        return m;
    }
    display::primary_monitor()
        .as_ref()
        .map(OverlayMonitor::from)
        .unwrap_or_default()
}

fn ensure_window(app: &AppHandle, mon: OverlayMonitor) -> AppResult<WebviewWindow> {
    let win = if let Some(existing) = app.get_webview_window(LABEL) {
        existing
    } else {
        let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
            .title("JARVIS Overlay")
            .decorations(false)
            .transparent(true)
            .background_color(tauri::webview::Color(0, 0, 0, 0))
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .shadow(false)
            .focused(false)
            .focusable(false)
            .visible(false)
            .build()?;
        let _ = built.set_ignore_cursor_events(true);
        built
    };

    // Place onto target monitor first so per-monitor DPI takes effect, then
    // size to full monitor physical bounds and re-pin origin.
    let _ = win.set_position(PhysicalPosition::new(mon.origin_x, mon.origin_y));
    let _ = win.set_size(PhysicalSize::new(mon.width.max(1) as u32, mon.height.max(1) as u32));
    let _ = win.set_position(PhysicalPosition::new(mon.origin_x, mon.origin_y));
    style_overlay_window(&win);
    show_no_activate(&win)?;
    Ok(win)
}

/// Draw overlay items (tagged by `kind`: `ring`, `arrow`, `caption`, `card`)
/// using physical screen pixel coordinates.
///
/// Creates and positions the transparent click-through overlay window on the
/// monitor containing the target, shows it without stealing focus, and emits
/// `overlay:draw` `{ items, replace }` to the overlay webview.
pub fn draw(app: &tauri::AppHandle, items: Vec<serde_json::Value>, replace: bool) -> crate::error::AppResult<()> {
    if replace && items.is_empty() {
        return clear(app);
    }

    let mon = resolve_monitor_for_items(&items);
    let merged = {
        let mut inner = state().inner.lock();
        if replace {
            inner.items = items.clone();
        } else {
            for item in &items {
                if let Some(id) = item.get("id").and_then(Value::as_str) {
                    if let Some(slot) = inner.items.iter_mut().find(|old| old.get("id").and_then(Value::as_str) == Some(id)) {
                        *slot = item.clone();
                        continue;
                    }
                }
                inner.items.push(item.clone());
            }
        }
        inner.monitor = mon;
        inner.visible = !inner.items.is_empty();
        inner.items.clone()
    };

    if merged.is_empty() {
        return clear(app);
    }

    let _win = ensure_window(app, mon)?;
    update_tracker(Some(app), true, mon.scale, None);
    app.emit_to(
        LABEL,
        EVT_DRAW,
        json!({
            "items": items,
            "replace": replace,
            "monitor": mon,
        }),
    )
    .map_err(|e| AppError::internal(format!("Не удалось отправить overlay:draw: {e}")))?;
    Ok(())
}

/// Clear all overlay items, reset click-through, and hide the overlay window.
pub fn clear(app: &tauri::AppHandle) -> crate::error::AppResult<()> {
    {
        let mut inner = state().inner.lock();
        inner.items.clear();
        inner.visible = false;
    }
    state().guide_active.store(false, Ordering::SeqCst);
    update_tracker(Some(app), false, 1.0, Some(Vec::new()));
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = app.emit_to(LABEL, EVT_CLEAR, json!({}));
        let _ = win.set_ignore_cursor_events(true);
        let _ = win.hide();
    }
    Ok(())
}

/// Send a subtle visual pulse hint to the active ring (used when the user
/// clicks outside the highlighted control during a `guide_step`).
pub fn pulse(app: &tauri::AppHandle) {
    if app.get_webview_window(LABEL).is_some() {
        let _ = app.emit_to(LABEL, EVT_PULSE, json!({}));
    }
}

/// Track whether a guided teach step is currently active so turn completion
/// cleans up guide overlays without prematurely dismissing timed `screen_point` rings.
pub fn set_guide_active(active: bool) {
    state().guide_active.store(active, Ordering::SeqCst);
}

pub fn is_guide_active() -> bool {
    state().guide_active.load(Ordering::SeqCst)
}

pub fn reset_guide_cancel() {
    state().guide_cancelled.store(false, Ordering::SeqCst);
}

pub fn cancel_guide() {
    state().guide_cancelled.store(true, Ordering::SeqCst);
}

pub fn is_guide_cancelled() -> bool {
    state().guide_cancelled.load(Ordering::SeqCst)
}

// ---------- Tauri IPC commands ----------

/// Called by the overlay frontend once mounted so any items drawn while the
/// webview was booting are rendered immediately.
#[tauri::command]
pub fn overlay_ready() -> OverlaySnapshot {
    let inner = state().inner.lock();
    OverlaySnapshot {
        items: inner.items.clone(),
        monitor: inner.monitor,
    }
}

/// Frontend reports physical screen rectangles of all `[data-overlay-interactive]`
/// elements currently rendered in the overlay DOM.
#[tauri::command]
pub fn overlay_set_interactive_rects(app: AppHandle, rects: Vec<HitRect>) {
    let (visible, scale) = {
        let inner = state().inner.lock();
        (inner.visible, inner.monitor.scale)
    };
    if rects.is_empty() {
        if let Some(win) = app.get_webview_window(LABEL) {
            let _ = win.set_ignore_cursor_events(true);
        }
    }
    update_tracker(Some(&app), visible, scale, Some(rects));
}

/// Frontend notifies Rust when a user closes a card or all timed items finish
/// auto-hiding; hides the overlay window once nothing remains on screen.
#[tauri::command]
pub fn overlay_dismiss_item(app: AppHandle, id: Option<String>, empty: bool) {
    if empty {
        let _ = clear(&app);
        return;
    }
    if let Some(target_id) = id {
        let now_empty = {
            let mut inner = state().inner.lock();
            inner.items.retain(|it| it.get("id").and_then(Value::as_str) != Some(target_id.as_str()));
            inner.items.is_empty()
        };
        if now_empty {
            let _ = clear(&app);
        }
    }
}

/// Frontend close button on a waiting guide caption cancels the active guide step.
#[tauri::command]
pub fn overlay_cancel_guide(app: AppHandle) {
    cancel_guide();
    let _ = clear(&app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::display::Rect;

    fn mon(id: isize, x: i32, y: i32, width: i32, height: i32, scale: f64, primary: bool) -> MonitorInfo {
        MonitorInfo {
            id,
            name: format!("DISP{id}"),
            primary,
            bounds: Rect { x, y, width, height },
            work_area: Rect { x, y, width, height: height - 48 },
            scale,
        }
    }

    #[test]
    fn physical_css_coordinate_conversion_handles_multi_monitor_and_dpi() {
        // Left secondary monitor at 125% DPI with negative X origin.
        let m = OverlayMonitor {
            origin_x: -1920,
            origin_y: 120,
            width: 1920,
            height: 1080,
            scale: 1.25,
        };
        let (cx, cy) = physical_to_css_point(-1420.0, 370.0, m);
        assert!((cx - 400.0).abs() < 1e-6);
        assert!((cy - 200.0).abs() < 1e-6);

        let phys = css_to_physical_rect(cx, cy, 120.0, 40.0, m);
        assert_eq!(phys, HitRect { x: -1420, y: 370, width: 150, height: 50 });

        // Primary 4K monitor at 200% DPI.
        let m2 = OverlayMonitor {
            origin_x: 0,
            origin_y: 0,
            width: 3840,
            height: 2160,
            scale: 2.0,
        };
        let (cx2, cy2) = physical_to_css_point(1000.0, 500.0, m2);
        assert_eq!((cx2, cy2), (500.0, 250.0));
        assert_eq!(
            css_to_physical_rect(10.0, 20.0, 100.0, 40.0, m2),
            HitRect { x: 20, y: 40, width: 200, height: 80 }
        );
    }

    #[test]
    fn pick_monitor_selects_display_containing_target_or_primary() {
        let displays = vec![
            mon(1, 0, 0, 2560, 1440, 1.5, true),
            mon(2, -1920, 0, 1920, 1080, 1.0, false),
            mon(3, 2560, 0, 3840, 2160, 2.0, false),
        ];

        let left = pick_monitor(&displays, Some((-500, 300))).unwrap();
        assert_eq!((left.origin_x, left.scale), (-1920, 1.0));

        let right = pick_monitor(&displays, Some((3000, 900))).unwrap();
        assert_eq!((right.origin_x, right.scale), (2560, 2.0));

        let fallback = pick_monitor(&displays, None).unwrap();
        assert_eq!((fallback.origin_x, fallback.scale), (0, 1.5));
    }

    #[test]
    fn target_point_extracts_from_ring_arrow_caption_or_card() {
        assert_eq!(
            target_point_from_items(&[json!({"kind": "ring", "x": 100, "y": 200, "w": 80, "h": 40})]),
            Some((140, 220))
        );
        assert_eq!(
            target_point_from_items(&[json!({"kind": "arrow", "to": {"x": 310, "y": 420}})]),
            Some((310, 420))
        );
        assert_eq!(
            target_point_from_items(&[json!({"kind": "card", "id": "c1", "anchor": {"x": 50, "y": 60, "w": 100, "h": 20}, "spec": {}})]),
            Some((100, 70))
        );
        assert_eq!(
            target_point_from_items(&[json!({"kind": "caption", "text": "Привет"})]),
            None
        );
    }

    #[test]
    fn hit_tracker_handles_hysteresis_and_drag_capture() {
        let rects = vec![HitRect { x: 100, y: 100, width: 200, height: 120 }];
        let mut t = HitTrackerState::default();

        // Outside: click-through.
        assert!(!t.step(&rects, 50, 50, false, 1.0));
        // Inside: interactive.
        assert!(t.step(&rects, 150, 150, false, 1.0));
        // 3px outside edge while already interactive: hysteresis keeps it interactive.
        assert!(t.step(&rects, 97, 150, false, 1.0));
        // Far outside: returns to click-through.
        assert!(!t.step(&rects, 80, 150, false, 1.0));

        // Drag starting inside stays interactive even when cursor moves far outside.
        assert!(t.step(&rects, 150, 150, true, 1.5));
        assert!(t.step(&rects, 600, 600, true, 1.5));
        // Releasing outside ends drag capture.
        assert!(!t.step(&rects, 600, 600, false, 1.5));

        // Drag starting outside never activates when passing over, until pointer enters.
        assert!(!t.step(&rects, 10, 10, true, 1.0));
    }
}
