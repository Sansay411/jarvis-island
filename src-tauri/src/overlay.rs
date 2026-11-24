//! Full-monitor transparent click-through overlay window.
//!
//! Draws rings, arrows, captions and cards on top of the user's real screen
//! without taking focus and without showing up in screen captures
//! (`WDA_EXCLUDEFROMCAPTURE`), so the model's screenshots stay clean.
//!
//! Item coordinates are physical screen pixels (the space of UIA bounding
//! rectangles, `GetCursorPos` and `screen_capture`'s `screen_rect`). The
//! window covers exactly one monitor; the frontend maps items to CSS pixels
//! with that monitor's origin and scale.
//!
//! Elements marked with `data-overlay-interactive` report their physical
//! rectangles over IPC; a ~30 Hz tracker thread runs only while such rects
//! exist and turns click-through off while the cursor is over one of them.
//!
//! Windows: tao re-applies `ShowWindow(SW_SHOW)` / `SW_HIDE` from its own
//! visibility flag whenever a style flag changes (e.g. ignore-cursor-events),
//! and `SW_SHOW` on a hidden window activates it. So the window is created
//! visible without activation, tao's flag is never touched again, and every
//! show/hide/click-through change runs in order on the main thread through
//! raw `SetWindowPos`/`ShowWindow` calls with `SWP_NOACTIVATE`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::error::{AppError, AppResult};
use crate::platform::display::{self, MonitorInfo};

pub const LABEL: &str = "overlay";
pub const EVT_DRAW: &str = "overlay:draw";
pub const EVT_CLEAR: &str = "overlay:clear";
pub const EVT_PULSE: &str = "overlay:pulse";

/// Lifetime of rings, arrows and captions without `seconds`.
pub const DEFAULT_ITEM_SECONDS: f64 = 8.0;

/// Physical geometry and DPI scale of the monitor covered by the overlay.
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
        OverlayMonitor { origin_x: 0, origin_y: 0, width: 1920, height: 1080, scale: 1.0 }
    }
}

impl From<&MonitorInfo> for OverlayMonitor {
    fn from(m: &MonitorInfo) -> Self {
        OverlayMonitor {
            origin_x: m.bounds.x,
            origin_y: m.bounds.y,
            width: m.bounds.width.max(1),
            height: m.bounds.height.max(1),
            scale: safe_scale(m.scale),
        }
    }
}

fn safe_scale(s: f64) -> f64 {
    if s > 0.0 && s.is_finite() {
        s
    } else {
        1.0
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

/// True when `(px, py)` falls inside any interactive rect (+ margin).
pub fn point_in_any_rect(rects: &[HitRect], px: i32, py: i32, margin: i32) -> bool {
    rects.iter().any(|r| r.contains(px, py, margin))
}

/// Pointer state machine for the interactive overlay regions: interactive
/// while the cursor is over a rect (with a few pixels of hysteresis once
/// inside) and for the whole drag that started inside one.
#[derive(Debug, Clone, Copy, Default)]
pub struct HitTrackerState {
    pub interactive: bool,
    pub was_down: bool,
    pub press_inside: bool,
}

impl HitTrackerState {
    /// Whether the overlay should receive the mouse (`true`) or let it through.
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

/// Physical screen point → overlay CSS pixels on `mon`.
pub fn physical_to_css_point(px: f64, py: f64, mon: OverlayMonitor) -> (f64, f64) {
    let s = safe_scale(mon.scale);
    ((px - mon.origin_x as f64) / s, (py - mon.origin_y as f64) / s)
}

/// Overlay CSS rectangle on `mon` → physical screen pixels.
pub fn css_to_physical_rect(x: f64, y: f64, width: f64, height: f64, mon: OverlayMonitor) -> HitRect {
    let s = safe_scale(mon.scale);
    HitRect {
        x: mon.origin_x + (x * s).round() as i32,
        y: mon.origin_y + (y * s).round() as i32,
        width: (width * s).round().max(0.0) as i32,
        height: (height * s).round().max(0.0) as i32,
    }
}

/// A representative physical screen point of the first positioned item.
pub fn target_point_from_items(items: &[Value]) -> Option<(i32, i32)> {
    let num = |v: &Value, k: &str| v.get(k).and_then(Value::as_f64);
    let center = |v: &Value| -> Option<(i32, i32)> {
        let (x, y) = (num(v, "x")?, num(v, "y")?);
        let (w, h) = (num(v, "w").unwrap_or(0.0), num(v, "h").unwrap_or(0.0));
        Some(((x + w / 2.0).round() as i32, (y + h / 2.0).round() as i32))
    };
    items.iter().find_map(|item| match item.get("kind").and_then(Value::as_str).unwrap_or("") {
        "ring" | "caption" => center(item),
        "arrow" => item.get("to").and_then(center),
        "card" => item.get("anchor").and_then(center),
        _ => None,
    })
}

/// How long an item stays on screen; `None` = until replaced or closed.
/// Cards and `persistent` items (a waiting teach step) never expire.
pub fn item_lifetime(item: &Value) -> Option<Duration> {
    if item.get("kind").and_then(Value::as_str) == Some("card") {
        return None;
    }
    if item.get("persistent").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let secs = item.get("seconds").and_then(Value::as_f64).unwrap_or(DEFAULT_ITEM_SECONDS);
    (secs > 0.0 && secs.is_finite()).then(|| Duration::from_secs_f64(secs.min(3600.0)))
}

fn monitor_contains(m: &MonitorInfo, px: i32, py: i32) -> bool {
    px >= m.bounds.x && px < m.bounds.x + m.bounds.width && py >= m.bounds.y && py < m.bounds.y + m.bounds.height
}

/// The monitor holding `target` (or the nearest one), else the primary.
pub fn pick_monitor(monitors: &[MonitorInfo], target: Option<(i32, i32)>) -> Option<OverlayMonitor> {
    if monitors.is_empty() {
        return None;
    }
    if let Some((px, py)) = target {
        if let Some(m) = monitors.iter().find(|m| monitor_contains(m, px, py)) {
            return Some(OverlayMonitor::from(m));
        }
        let nearest = monitors.iter().min_by_key(|m| {
            let cx = px.clamp(m.bounds.x, m.bounds.x + m.bounds.width - 1);
            let cy = py.clamp(m.bounds.y, m.bounds.y + m.bounds.height - 1);
            let (dx, dy) = ((px - cx) as i64, (py - cy) as i64);
            dx * dx + dy * dy
        });
        if let Some(m) = nearest {
            return Some(OverlayMonitor::from(m));
        }
    }
    monitors.iter().find(|m| m.primary).or_else(|| monitors.first()).map(OverlayMonitor::from)
}

/// Monitor for a draw: the one holding the items; for unpositioned items
/// (a bare caption, a card without anchor) the monitor already shown, then
/// the one under the cursor, then the primary.
pub fn choose_monitor(
    monitors: &[MonitorInfo],
    target: Option<(i32, i32)>,
    current: Option<OverlayMonitor>,
    cursor: Option<(i32, i32)>,
) -> OverlayMonitor {
    if target.is_some() {
        if let Some(m) = pick_monitor(monitors, target) {
            return m;
        }
    }
    if let Some(c) = current {
        return c;
    }
    pick_monitor(monitors, cursor).unwrap_or_default()
}

struct StoredItem {
    value: Value,
    expires: Option<Instant>,
}

impl StoredItem {
    fn new(value: Value, now: Instant) -> Self {
        let expires = item_lifetime(&value).map(|d| now + d);
        StoredItem { value, expires }
    }

    fn id(&self) -> Option<&str> {
        self.value.get("id").and_then(Value::as_str)
    }

    fn alive(&self, at: Instant) -> bool {
        self.expires.is_none_or(|e| e > at)
    }
}

struct OverlayInner {
    items: Vec<StoredItem>,
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
    /// The window is on screen (set on the main thread by show/hide).
    shown: AtomicBool,
    guide_active: AtomicBool,
    guide_cancelled: AtomicBool,
}

static STATE: OnceLock<Arc<OverlayState>> = OnceLock::new();

fn state() -> &'static Arc<OverlayState> {
    STATE.get_or_init(|| {
        Arc::new(OverlayState {
            inner: Mutex::new(OverlayInner { items: Vec::new(), monitor: OverlayMonitor::default(), visible: false }),
            tracker_lock: StdMutex::new(TrackerShared { rects: Vec::new(), scale: 1.0, visible: false, app: None }),
            tracker_cv: Condvar::new(),
            tracker_started: AtomicBool::new(false),
            shown: AtomicBool::new(false),
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
        g.scale = safe_scale(scale);
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
    let _ = std::thread::Builder::new().name("overlay-tracker".into()).spawn(move || tracker_loop(st_arc));
}

/// Cursor position (physical pixels) and whether the left button is down.
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
                    if let Some(a) = g.app.as_ref() {
                        set_click_through(a, true);
                    }
                    applied_interactive = false;
                    machine = HitTrackerState::default();
                }
                g = st.tracker_cv.wait(g).unwrap_or_else(|e| e.into_inner());
            }
            (g.app.clone(), g.rects.clone(), g.scale)
        };

        if let (Some(app), Some((px, py, down))) = (app, poll_cursor()) {
            let want = machine.step(&rects, px, py, down, scale);
            if want != applied_interactive {
                set_click_through(&app, !want);
                applied_interactive = want;
            }
        }
        std::thread::sleep(Duration::from_millis(33));
    }
}

/// Toggle click-through on the main thread, only while the window is shown
/// (a hidden window is always left click-through, see the module docs).
fn set_click_through(app: &AppHandle, ignore: bool) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if !state().shown.load(Ordering::SeqCst) {
            return;
        }
        if let Some(win) = handle.get_webview_window(LABEL) {
            let _ = win.set_ignore_cursor_events(ignore);
        }
    });
}

#[cfg(windows)]
fn hwnd_of(win: &WebviewWindow) -> Option<windows::Win32::Foundation::HWND> {
    win.hwnd().ok().map(|raw| windows::Win32::Foundation::HWND(raw.0 as *mut _))
}

/// One-time window styling: square corners, no border, excluded from capture.
#[cfg(windows)]
fn style_overlay_window(win: &WebviewWindow) {
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    };
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};

    let Some(hwnd) = hwnd_of(win) else { return };
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
        // The model's screenshots must never contain our own drawings.
        if SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).is_err() {
            tracing::warn!("overlay: capture exclusion is not available on this Windows build");
        }
    }
}

#[cfg(not(windows))]
fn style_overlay_window(_win: &WebviewWindow) {}

/// Cover `mon` exactly and show without activation. Runs on the main thread.
#[cfg(windows)]
fn place_and_show(win: &WebviewWindow, mon: OverlayMonitor) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_SHOWWINDOW,
    };

    let Some(hwnd) = hwnd_of(win) else { return };
    // Moving onto a monitor with another DPI makes the window resize itself
    // to the suggested rect (WM_DPICHANGED); the second call pins the exact
    // physical bounds once the DPI is settled.
    for _ in 0..2 {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                mon.origin_x,
                mon.origin_y,
                mon.width.max(1),
                mon.height.max(1),
                SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
            );
        }
    }
}

#[cfg(not(windows))]
fn place_and_show(win: &WebviewWindow, mon: OverlayMonitor) {
    use tauri::{PhysicalPosition, PhysicalSize};
    let _ = win.set_position(PhysicalPosition::new(mon.origin_x, mon.origin_y));
    let _ = win.set_size(PhysicalSize::new(mon.width.max(1) as u32, mon.height.max(1) as u32));
    let _ = win.set_position(PhysicalPosition::new(mon.origin_x, mon.origin_y));
    let _ = win.show();
}

#[cfg(windows)]
fn hide_window(win: &WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    if let Some(hwnd) = hwnd_of(win) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

#[cfg(not(windows))]
fn hide_window(win: &WebviewWindow) {
    let _ = win.hide();
}

fn ensure_window(app: &AppHandle) -> AppResult<()> {
    if app.get_webview_window(LABEL).is_some() {
        return Ok(());
    }
    // Created visible on Windows (shown without activation thanks to
    // `focused(false)`) so tao's own visibility flag never re-shows it.
    let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("JARVIS")
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
        .visible(cfg!(windows))
        .build()?;
    let _ = win.set_ignore_cursor_events(true);
    style_overlay_window(&win);
    Ok(())
}

fn show_on(app: &AppHandle, mon: OverlayMonitor) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = handle.get_webview_window(LABEL) {
            place_and_show(&win, mon);
            state().shown.store(true, Ordering::SeqCst);
        }
    });
}

fn hide(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = handle.get_webview_window(LABEL) {
            // Click-through first, while still visible (see the module docs).
            let _ = win.set_ignore_cursor_events(true);
            hide_window(&win);
        }
        state().shown.store(false, Ordering::SeqCst);
    });
}

/// Draw overlay items (tagged by `kind`: `ring`, `arrow`, `caption`, `card`)
/// given in physical screen pixels.
///
/// Creates the overlay window if needed, moves it onto the monitor holding
/// the items, shows it without taking focus and emits `overlay:draw`
/// `{ items, replace, monitor }`. Without `replace`, items with the same `id`
/// are updated in place and the rest are added. Must not be called from a
/// synchronous command on the main thread (window creation would block).
pub fn draw(app: &tauri::AppHandle, items: Vec<serde_json::Value>, replace: bool) -> crate::error::AppResult<()> {
    if replace && items.is_empty() {
        return clear(app);
    }
    let target = target_point_from_items(&items);
    let monitors = display::list_monitors();
    let cursor = poll_cursor().map(|(x, y, _)| (x, y));
    let now = Instant::now();

    let (mon, payload_items, payload_replace) = {
        let mut inner = state().inner.lock();
        inner.items.retain(|it| it.alive(now));
        let current = (inner.visible && !inner.items.is_empty()).then_some(inner.monitor);
        let mon = choose_monitor(&monitors, target, if replace { None } else { current }, cursor);
        if replace {
            inner.items = items.iter().cloned().map(|v| StoredItem::new(v, now)).collect();
        } else {
            for item in &items {
                let fresh = StoredItem::new(item.clone(), now);
                match fresh.id().and_then(|id| inner.items.iter().position(|old| old.id() == Some(id))) {
                    Some(i) => inner.items[i] = fresh,
                    None => inner.items.push(fresh),
                }
            }
        }
        inner.monitor = mon;
        inner.visible = !inner.items.is_empty();
        // Moved to another monitor: everything is re-sent for the new origin.
        let moved = !replace && current.is_some_and(|c| c != mon);
        if moved {
            (mon, inner.items.iter().map(|it| it.value.clone()).collect::<Vec<_>>(), true)
        } else {
            (mon, items, replace)
        }
    };

    ensure_window(app)?;
    show_on(app, mon);
    update_tracker(Some(app), true, mon.scale, None);
    app.emit_to(LABEL, EVT_DRAW, json!({ "items": payload_items, "replace": payload_replace, "monitor": mon }))
        .map_err(|e| AppError::internal(format!("Не удалось показать подсказку на экране: {e}")))?;
    Ok(())
}

/// Remove everything from the overlay and hide its window.
pub fn clear(app: &tauri::AppHandle) -> crate::error::AppResult<()> {
    let had_window = app.get_webview_window(LABEL).is_some();
    {
        let mut inner = state().inner.lock();
        inner.items.clear();
        inner.visible = false;
    }
    state().guide_active.store(false, Ordering::SeqCst);
    update_tracker(Some(app), false, 1.0, Some(Vec::new()));
    if had_window {
        let _ = app.emit_to(LABEL, EVT_CLEAR, json!({}));
        hide(app);
    }
    Ok(())
}

/// A short ripple on the rings (a teach-step click landed elsewhere).
pub fn pulse(app: &tauri::AppHandle) {
    if app.get_webview_window(LABEL).is_some() {
        let _ = app.emit_to(LABEL, EVT_PULSE, json!({}));
    }
}

/// True when `(px, py)` is over an interactive overlay element (a card, the
/// caption's close button): such clicks belong to the overlay itself.
pub fn interactive_hit(px: i32, py: i32) -> bool {
    let g = state().tracker_lock.lock().unwrap_or_else(|e| e.into_inner());
    let margin = (4.0 * g.scale.max(1.0)).round() as i32;
    g.visible && point_in_any_rect(&g.rects, px, py, margin)
}

/// A teach step is on screen: the turn's end clears the overlay, while
/// timed `screen_point` rings may outlive it.
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

// ---------- IPC ----------

/// The overlay webview finished mounting: items drawn while it was booting.
#[tauri::command]
pub fn overlay_ready() -> OverlaySnapshot {
    let now = Instant::now();
    let mut inner = state().inner.lock();
    inner.items.retain(|it| it.alive(now));
    OverlaySnapshot { items: inner.items.iter().map(|it| it.value.clone()).collect(), monitor: inner.monitor }
}

/// Physical screen rectangles of the `[data-overlay-interactive]` elements.
#[tauri::command]
pub fn overlay_set_interactive_rects(app: AppHandle, rects: Vec<HitRect>) {
    let (visible, scale) = {
        let inner = state().inner.lock();
        (inner.visible, inner.monitor.scale)
    };
    update_tracker(Some(&app), visible, scale, Some(rects));
}

/// A card was closed (`id`) or every timed item ran out (`empty`); the
/// window hides once nothing is left. `empty` is checked against our own
/// expiry times so a draw that raced the frontend's timer is not lost.
#[tauri::command]
pub fn overlay_dismiss_item(app: AppHandle, id: Option<String>, empty: bool) {
    // The frontend starts its timers on receipt, a little after ours.
    let at = Instant::now() + Duration::from_millis(150);
    let now_empty = {
        let mut inner = state().inner.lock();
        if let Some(target) = id.as_deref() {
            inner.items.retain(|it| it.id() != Some(target));
        }
        if empty {
            inner.items.retain(|it| it.alive(at));
        }
        inner.items.is_empty()
    };
    if now_empty {
        let _ = clear(&app);
    }
}

/// The close button on a waiting teach step: stop the guide.
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
        // Left secondary monitor at 125% DPI with a negative origin.
        let m = OverlayMonitor { origin_x: -1920, origin_y: 120, width: 1920, height: 1080, scale: 1.25 };
        let (cx, cy) = physical_to_css_point(-1420.0, 370.0, m);
        assert!((cx - 400.0).abs() < 1e-6);
        assert!((cy - 200.0).abs() < 1e-6);
        assert_eq!(css_to_physical_rect(cx, cy, 120.0, 40.0, m), HitRect { x: -1420, y: 370, width: 150, height: 50 });

        // 4K at 200%.
        let m2 = OverlayMonitor { origin_x: 0, origin_y: 0, width: 3840, height: 2160, scale: 2.0 };
        assert_eq!(physical_to_css_point(1000.0, 500.0, m2), (500.0, 250.0));
        assert_eq!(css_to_physical_rect(10.0, 20.0, 100.0, 40.0, m2), HitRect { x: 20, y: 40, width: 200, height: 80 });

        // A broken scale never divides by zero.
        let m3 = OverlayMonitor { scale: 0.0, ..m2 };
        assert_eq!(physical_to_css_point(10.0, 20.0, m3), (10.0, 20.0));
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
        // The shared edge belongs to the right-hand monitor.
        assert_eq!(pick_monitor(&displays, Some((2560, 10))).unwrap().origin_x, 2560);
        // Off every screen: the nearest one.
        assert_eq!(pick_monitor(&displays, Some((-5000, 200))).unwrap().origin_x, -1920);
        assert_eq!(pick_monitor(&displays, None).unwrap().scale, 1.5);
        assert!(pick_monitor(&[], Some((0, 0))).is_none());
    }

    #[test]
    fn choose_monitor_keeps_unpositioned_items_where_the_user_looks() {
        let displays = vec![mon(1, 0, 0, 1920, 1080, 1.0, true), mon(2, 1920, 0, 2560, 1440, 1.25, false)];
        let second = OverlayMonitor::from(&displays[1]);
        // Positioned items pick their own monitor.
        assert_eq!(choose_monitor(&displays, Some((100, 100)), Some(second), None).origin_x, 0);
        // A bare caption stays on the monitor already shown...
        assert_eq!(choose_monitor(&displays, None, Some(second), Some((10, 10))), second);
        // ...or goes where the cursor is.
        assert_eq!(choose_monitor(&displays, None, None, Some((2000, 500))), second);
        assert_eq!(choose_monitor(&displays, None, None, None).origin_x, 0);
        assert_eq!(choose_monitor(&[], None, None, None), OverlayMonitor::default());
    }

    #[test]
    fn target_point_extracts_from_ring_arrow_caption_or_card() {
        assert_eq!(target_point_from_items(&[json!({"kind": "ring", "x": 100, "y": 200, "w": 80, "h": 40})]), Some((140, 220)));
        assert_eq!(target_point_from_items(&[json!({"kind": "arrow", "to": {"x": 310, "y": 420}})]), Some((310, 420)));
        assert_eq!(
            target_point_from_items(&[json!({"kind": "card", "id": "c1", "anchor": {"x": 50, "y": 60, "w": 100, "h": 20}, "spec": {}})]),
            Some((100, 70))
        );
        assert_eq!(target_point_from_items(&[json!({"kind": "caption", "text": "Привет"})]), None);
        // The first positioned item wins, even after an unpositioned one.
        assert_eq!(
            target_point_from_items(&[json!({"kind": "caption", "text": "a"}), json!({"kind": "ring", "x": 0, "y": 0, "w": 10, "h": 10})]),
            Some((5, 5))
        );
    }

    #[test]
    fn item_lifetime_defaults_and_persistence() {
        assert_eq!(item_lifetime(&json!({"kind": "ring"})), Some(Duration::from_secs(8)));
        assert_eq!(item_lifetime(&json!({"kind": "arrow", "seconds": 2.5})), Some(Duration::from_millis(2500)));
        assert_eq!(item_lifetime(&json!({"kind": "ring", "persistent": true})), None);
        assert_eq!(item_lifetime(&json!({"kind": "caption", "seconds": 0})), None);
        assert_eq!(item_lifetime(&json!({"kind": "card", "id": "c", "seconds": 3})), None);
    }

    #[test]
    fn hit_tracker_handles_hysteresis_and_drag_capture() {
        let rects = vec![HitRect { x: 100, y: 100, width: 200, height: 120 }];
        let mut t = HitTrackerState::default();
        assert!(!t.step(&rects, 50, 50, false, 1.0));
        assert!(t.step(&rects, 150, 150, false, 1.0));
        // 3 px outside while already interactive: hysteresis keeps it.
        assert!(t.step(&rects, 97, 150, false, 1.0));
        assert!(!t.step(&rects, 80, 150, false, 1.0));
        // A drag that starts inside stays interactive anywhere.
        assert!(t.step(&rects, 150, 150, true, 1.5));
        assert!(t.step(&rects, 600, 600, true, 1.5));
        assert!(!t.step(&rects, 600, 600, false, 1.5));
        // A press that started outside never grabs the mouse.
        assert!(!t.step(&rects, 10, 10, true, 1.0));
        assert!(t.step(&rects, 150, 150, true, 1.0));
        assert!(!t.step(&rects, 400, 400, true, 1.0));
        assert!(!t.step(&[], 150, 150, false, 1.0));
    }
}
