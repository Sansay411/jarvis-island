//! «Спроси про область»: screen area selection picker, pending image
//! attachment for the next chat or voice turn, and cursor-circle detection
//! while JARVIS is listening.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

use crate::error::{AppError, AppResult};
use crate::platform::display::{self, MonitorInfo, Rect};
use crate::platform::{self, capture, winmgr};
use crate::state::AppState;

pub const PICKER_LABEL: &str = "areapick";
pub const EVT_PENDING: &str = "area:pending";
pub const EVT_CIRCLED: &str = "area:circled";

const THUMB_MAX_SIDE: u32 = 180;
const CIRCLE_POLL_MS: u64 = 16;
const CIRCLE_WINDOW: Duration = Duration::from_millis(2400);
const CIRCLE_COOLDOWN: Duration = Duration::from_millis(1600);
const CIRCLE_MARGIN_LOGICAL: f64 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AreaSource {
    Picker,
    Window,
    Circle,
}

/// Full captured screen region kept in memory until the next user message
/// (typed or spoken) consumes it or the user dismisses the chip.
#[derive(Debug, Clone)]
pub struct PendingArea {
    pub id: String,
    pub data_url: String,
    pub thumb_data_url: String,
    pub width: u32,
    pub height: u32,
    pub screen_rect: Rect,
    pub source: AreaSource,
    pub window_title: Option<String>,
}

/// Lightweight preview sent to the island UI (full high-res data URL stays in
/// Rust so IPC stays fast and voice turns consume the exact same capture).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAreaPreview {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub screen_rect: Rect,
    pub thumb_data_url: String,
    pub source: AreaSource,
    pub window_title: Option<String>,
}

impl PendingArea {
    pub fn preview(&self) -> PendingAreaPreview {
        PendingAreaPreview {
            id: self.id.clone(),
            width: self.width,
            height: self.height,
            screen_rect: self.screen_rect,
            thumb_data_url: self.thumb_data_url.clone(),
            source: self.source,
            window_title: self.window_title.clone(),
        }
    }
}

/// Emitted as `area:circled` when the user circles an area while talking.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircledEvent {
    pub rect: Rect,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickerWindowRect {
    pub hwnd: i64,
    pub title: String,
    pub process: String,
    pub rect: Rect,
    pub local_x: f64,
    pub local_y: f64,
    pub local_width: f64,
    pub local_height: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickerState {
    pub monitor_bounds: Rect,
    pub scale: f64,
    pub logical_width: f64,
    pub logical_height: f64,
    pub windows: Vec<PickerWindowRect>,
}

struct PickerSession {
    monitor: MonitorInfo,
    windows: Vec<PickerWindowRect>,
}

struct WatcherGate {
    listening: bool,
}

pub struct AreaManager {
    app: AppHandle,
    pending: Mutex<Option<PendingArea>>,
    picker: Mutex<Option<PickerSession>>,
    watcher_gate: Mutex<WatcherGate>,
    watcher_cv: Condvar,
}

impl AreaManager {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(AreaManager {
            app,
            pending: Mutex::new(None),
            picker: Mutex::new(None),
            watcher_gate: Mutex::new(WatcherGate { listening: false }),
            watcher_cv: Condvar::new(),
        })
    }

    pub fn pending_preview(&self) -> Option<PendingAreaPreview> {
        self.pending.lock().as_ref().map(PendingArea::preview)
    }

    pub fn has_pending(&self) -> bool {
        self.pending.lock().is_some()
    }

    pub fn set_pending(&self, area: PendingArea) {
        let preview = area.preview();
        *self.pending.lock() = Some(area);
        let _ = self.app.emit(EVT_PENDING, Some(preview));
    }

    pub fn clear_pending(&self) {
        if self.pending.lock().take().is_some() {
            let _ = self.app.emit(EVT_PENDING, Option::<PendingAreaPreview>::None);
        }
    }

    /// Consume the pending area for a turn (if any) and notify the UI.
    pub fn take_pending(&self) -> Option<PendingArea> {
        let area = self.pending.lock().take();
        if area.is_some() {
            let _ = self.app.emit(EVT_PENDING, Option::<PendingAreaPreview>::None);
        }
        area
    }

    pub fn picker_state(&self) -> Option<PickerState> {
        let guard = self.picker.lock();
        let s = guard.as_ref()?;
        let scale = s.monitor.scale.max(0.5);
        Some(PickerState {
            monitor_bounds: s.monitor.bounds,
            scale,
            logical_width: s.monitor.bounds.width as f64 / scale,
            logical_height: s.monitor.bounds.height as f64 / scale,
            windows: s.windows.clone(),
        })
    }

    /// Called by `VoiceManager` whenever voice state changes. Only active
    /// listening/hearing wakes the cursor watcher thread.
    pub fn set_voice_listening(&self, listening: bool) {
        let mut g = self.watcher_gate.lock();
        if g.listening != listening {
            g.listening = listening;
            if listening {
                self.watcher_cv.notify_one();
            }
        }
    }

    /// Spawn the background cursor watcher (~60 Hz while voice is listening,
    /// sleeping on a condvar at 0% CPU otherwise).
    pub fn spawn_circle_watcher(self: &Arc<Self>) {
        let me = Arc::clone(self);
        std::thread::Builder::new()
            .name("area-circle-watcher".into())
            .spawn(move || me.circle_watch_loop())
            .expect("spawn area circle watcher");
    }

    fn circle_watch_loop(&self) {
        let mut trail: VecDeque<(f64, f64, Instant)> = VecDeque::with_capacity(180);
        let mut last_detected: Option<Instant> = None;

        loop {
            {
                let mut g = self.watcher_gate.lock();
                while !g.listening {
                    trail.clear();
                    self.watcher_cv.wait(&mut g);
                }
            }

            let settings = self.app.state::<AppState>().store.settings();
            if !settings.voice.circle_to_ask || !settings.privacy.allow_screen {
                trail.clear();
                std::thread::sleep(Duration::from_millis(80));
                continue;
            }

            if let Some((x, y)) = display::cursor_position() {
                let now = Instant::now();
                trail.push_back((x as f64, y as f64, now));
                while trail.front().is_some_and(|(_, _, t)| now.duration_since(*t) > CIRCLE_WINDOW) {
                    trail.pop_front();
                }
                while trail.len() > 180 {
                    trail.pop_front();
                }

                let cooled = last_detected.is_none_or(|t| now.duration_since(t) >= CIRCLE_COOLDOWN);
                if cooled && trail.len() >= 8 {
                    let pts: Vec<(f64, f64)> = trail.iter().map(|&(px, py, _)| (px, py)).collect();
                    if let Some(raw_rect) = detect_circle(&pts) {
                        trail.clear();
                        last_detected = Some(now);
                        self.handle_circled_rect(raw_rect);
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(CIRCLE_POLL_MS));
        }
    }

    fn handle_circled_rect(&self, raw_rect: Rect) {
        let cx = raw_rect.x + raw_rect.width / 2;
        let cy = raw_rect.y + raw_rect.height / 2;
        let Some(mon) = display::monitor_for_point(cx, cy).or_else(display::primary_monitor) else {
            return;
        };
        let margin = (CIRCLE_MARGIN_LOGICAL * mon.scale.max(1.0)).round() as i32;
        let Some(rect) = expand_rect(raw_rect, margin, mon.bounds) else {
            return;
        };
        let island_hwnd = self.app.state::<AppState>().island.hwnd();
        let captured = capture_and_encode(rect, &[island_hwnd], AreaSource::Circle, None);
        match captured {
            Ok(area) => {
                tracing::info!(
                    x = area.screen_rect.x,
                    y = area.screen_rect.y,
                    w = area.width,
                    h = area.height,
                    "cursor circle detected and captured"
                );
                let ev = CircledEvent {
                    rect: area.screen_rect,
                    width: area.width,
                    height: area.height,
                };
                // A brief ring on the real screen confirms what was taken.
                let ring = serde_json::json!({
                    "kind": "ring", "id": "area-circled", "x": ev.rect.x, "y": ev.rect.y,
                    "w": ev.rect.width, "h": ev.rect.height, "label": "Область прикреплена", "seconds": 2.5,
                });
                if let Err(e) = crate::overlay::draw(&self.app, vec![ring], false) {
                    tracing::debug!("circled ring not drawn: {e}");
                }
                self.set_pending(area);
                let _ = self.app.emit(EVT_CIRCLED, ev);
            }
            Err(e) => {
                tracing::warn!("circled area capture failed: {e}");
            }
        }
    }
}

/// Open (or focus) the fullscreen area picker over the monitor under the cursor.
pub fn open_picker(app: &AppHandle) -> AppResult<()> {
    let settings = app.state::<AppState>().store.settings();
    if !settings.privacy.allow_screen {
        return Err(AppError::PermissionDenied(
            "Снимки экрана выключены в настройках приватности".into(),
        ));
    }

    if let Some(existing) = app.get_webview_window(PICKER_LABEL) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }

    let monitor = display::monitor_for_cursor()
        .or_else(display::primary_monitor)
        .ok_or_else(|| AppError::platform("Не удалось определить монитор под курсором"))?;
    let scale = monitor.scale.max(0.5);

    let windows: Vec<PickerWindowRect> = winmgr::list_visible_windows_with_frames()
        .into_iter()
        .filter_map(|w| {
            let clipped = clamp_rect(w.rect, monitor.bounds)?;
            if clipped.width < 24 || clipped.height < 24 {
                return None;
            }
            let (lx, ly, lw, lh) = physical_to_local_logical(clipped, monitor.bounds, scale)?;
            Some(PickerWindowRect {
                hwnd: w.hwnd,
                title: w.title,
                process: w.process,
                rect: clipped,
                local_x: lx,
                local_y: ly,
                local_width: lw,
                local_height: lh,
            })
        })
        .collect();

    {
        let area = app.state::<Arc<AreaManager>>();
        *area.picker.lock() = Some(PickerSession { monitor: monitor.clone(), windows });
    }

    let window = WebviewWindowBuilder::new(app, PICKER_LABEL, WebviewUrl::App("index.html".into()))
        .title("JARVIS — выбор области")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(true)
        .visible(false)
        .build()?;

    style_picker_window(&window);

    let b = monitor.bounds;
    // Move to the target monitor first so per-monitor DPI applies, then size
    // in exact physical pixels and re-anchor origin to cover the whole display.
    let _ = window.set_position(PhysicalPosition::new(b.x, b.y));
    let _ = window.set_size(PhysicalSize::new(b.width.max(1) as u32, b.height.max(1) as u32));
    let _ = window.set_position(PhysicalPosition::new(b.x, b.y));
    window.show()?;
    let _ = window.set_focus();
    Ok(())
}

/// Remove Windows 11 rounded corners/borders and permanently exclude the
/// picker overlay from screen capture.
fn style_picker_window(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
        };
        let raw = hwnd.0 as isize;
        capture::set_excluded_from_capture(raw, true);
        unsafe {
            let h = HWND(hwnd.0);
            let pref = DWMWCP_DONOTROUND;
            let _ = DwmSetWindowAttribute(
                h,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const _ as *const _,
                std::mem::size_of_val(&pref) as u32,
            );
            let none: u32 = 0xFFFF_FFFE;
            let _ = DwmSetWindowAttribute(
                h,
                DWMWA_BORDER_COLOR,
                &none as *const _ as *const _,
                std::mem::size_of_val(&none) as u32,
            );
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// Close and hide the picker window, returning its HWND (if any) so capture can
/// also exclude it as a second layer of safety.
fn close_picker_window(app: &AppHandle) -> Option<isize> {
    let w = app.get_webview_window(PICKER_LABEL)?;
    #[cfg(windows)]
    let hwnd = w.hwnd().ok().map(|h| h.0 as isize);
    #[cfg(not(windows))]
    let hwnd = None;
    let _ = w.hide();
    let _ = w.destroy();
    hwnd
}

pub fn cancel_picker(app: &AppHandle) {
    let _ = close_picker_window(app);
    app.state::<Arc<AreaManager>>().picker.lock().take();
}

/// Confirm a dragged logical rect (in monitor-local CSS pixels).
pub async fn confirm_rect(app: AppHandle, x1: f64, y1: f64, x2: f64, y2: f64) -> AppResult<PendingAreaPreview> {
    let session = app
        .state::<Arc<AreaManager>>()
        .picker
        .lock()
        .take()
        .ok_or_else(|| AppError::invalid("Выбор области уже завершён"))?;
    let picker_hwnd = close_picker_window(&app);
    // Let DWM finish compositing a frame without the picker overlay.
    tokio::time::sleep(Duration::from_millis(45)).await;

    let rect = logical_to_physical_rect(x1, y1, x2, y2, session.monitor.bounds, session.monitor.scale)
        .ok_or_else(|| AppError::invalid("Выбранная область слишком маленькая"))?;

    finish_picker_capture(&app, rect, picker_hwnd, AreaSource::Picker, None).await
}

/// Confirm a single click on the picker: pick the whole window under `(local_x, local_y)`.
pub async fn confirm_click(app: AppHandle, local_x: f64, local_y: f64, hwnd_hint: Option<i64>) -> AppResult<PendingAreaPreview> {
    let session = app
        .state::<Arc<AreaManager>>()
        .picker
        .lock()
        .take()
        .ok_or_else(|| AppError::invalid("Выбор области уже завершён"))?;
    let picker_hwnd = close_picker_window(&app);
    tokio::time::sleep(Duration::from_millis(45)).await;

    let scale = session.monitor.scale.max(0.5);
    let phys_x = session.monitor.bounds.x + (local_x * scale).round() as i32;
    let phys_y = session.monitor.bounds.y + (local_y * scale).round() as i32;

    let chosen = hwnd_hint
        .and_then(|h| session.windows.iter().find(|w| w.hwnd == h).cloned())
        .or_else(|| {
            session.windows.iter().find(|w| {
                phys_x >= w.rect.x
                    && phys_y >= w.rect.y
                    && phys_x < w.rect.x + w.rect.width
                    && phys_y < w.rect.y + w.rect.height
            }).cloned()
        });

    let (rect, title) = match chosen {
        Some(w) => (w.rect, Some(w.title)),
        None => {
            // Fall back to a live platform hit-test, then to the monitor bounds.
            let live = platform::blocking(move || Ok(winmgr::window_at_point(phys_x, phys_y))).await?;
            match live {
                Some(w) => {
                    let r = clamp_rect(w.rect, session.monitor.bounds).unwrap_or(w.rect);
                    (r, Some(w.title))
                }
                None => (session.monitor.bounds, None),
            }
        }
    };

    finish_picker_capture(&app, rect, picker_hwnd, AreaSource::Window, title).await
}

async fn finish_picker_capture(
    app: &AppHandle,
    rect: Rect,
    picker_hwnd: Option<isize>,
    source: AreaSource,
    window_title: Option<String>,
) -> AppResult<PendingAreaPreview> {
    let settings = app.state::<AppState>().store.settings();
    if !settings.privacy.allow_screen {
        return Err(AppError::PermissionDenied(
            "Снимки экрана выключены в настройках приватности".into(),
        ));
    }

    let island_hwnd = app.state::<AppState>().island.hwnd();
    let mut exclude = vec![island_hwnd];
    if let Some(h) = picker_hwnd {
        exclude.push(h);
    }

    let title_for_task = window_title.clone();
    let area = platform::blocking(move || capture_and_encode(rect, &exclude, source, title_for_task)).await?;
    let preview = area.preview();

    let manager = app.state::<Arc<AreaManager>>();
    manager.set_pending(area);

    // Reveal the island, open chat with the area chip, and if voice is enabled
    // and ready, start listening so the user can speak immediately.
    app.state::<AppState>().island.set_user_hidden(false);
    crate::shortcuts::emit_command(app, "area-picked");

    if settings.voice.enabled {
        let voice = app.state::<Arc<crate::voice::VoiceManager>>();
        if let Err(e) = voice.start_listening_if_ready() {
            tracing::debug!("voice auto-listen after area pick skipped: {e}");
        }
    }

    Ok(preview)
}

fn capture_and_encode(
    rect: Rect,
    exclude_hwnds: &[isize],
    source: AreaSource,
    window_title: Option<String>,
) -> AppResult<PendingArea> {
    let frame = capture::capture_region(rect, exclude_hwnds)?;
    let thumb_data_url = capture::encode_thumbnail(&frame, THUMB_MAX_SIDE)?;
    let encoded = capture::encode(frame, capture::MAX_SIDE)?;
    Ok(PendingArea {
        id: uuid::Uuid::new_v4().to_string(),
        data_url: encoded.data_url,
        thumb_data_url,
        width: rect.width.max(0) as u32,
        height: rect.height.max(0) as u32,
        screen_rect: rect,
        source,
        window_title,
    })
}

// ---------- pure geometry & DPI helpers ----------

/// Normalize any two corner coordinates into a non-negative `Rect`.
pub fn normalize_rect(x1: i32, y1: i32, x2: i32, y2: i32) -> Rect {
    let left = x1.min(x2);
    let top = y1.min(y2);
    let right = x1.max(x2);
    let bottom = y1.max(y2);
    Rect {
        x: left,
        y: top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    }
}

/// Intersect `rect` with `bounds` (handles negative multi-monitor coordinates).
/// Returns `None` when the intersection has zero or negative area.
pub fn clamp_rect(rect: Rect, bounds: Rect) -> Option<Rect> {
    if rect.width <= 0 || rect.height <= 0 || bounds.width <= 0 || bounds.height <= 0 {
        return None;
    }
    let left = (rect.x as i64).max(bounds.x as i64);
    let top = (rect.y as i64).max(bounds.y as i64);
    let right = (rect.x as i64 + rect.width as i64).min(bounds.x as i64 + bounds.width as i64);
    let bottom = (rect.y as i64 + rect.height as i64).min(bounds.y as i64 + bounds.height as i64);
    let width = right - left;
    let height = bottom - top;
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(Rect {
        x: left as i32,
        y: top as i32,
        width: width as i32,
        height: height as i32,
    })
}

/// Expand `rect` by `margin` pixels on every side and clamp to `bounds`.
pub fn expand_rect(rect: Rect, margin: i32, bounds: Rect) -> Option<Rect> {
    let m = margin.max(0) as i64;
    let expanded = Rect {
        x: (rect.x as i64 - m).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        y: (rect.y as i64 - m).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        width: (rect.width as i64 + m * 2).clamp(0, i32::MAX as i64) as i32,
        height: (rect.height as i64 + m * 2).clamp(0, i32::MAX as i64) as i32,
    };
    clamp_rect(expanded, bounds)
}

/// Convert a drag selection `(x1, y1) → (x2, y2)` in monitor-local logical
/// CSS pixels into a normalized physical-pixel `Rect` clamped to `monitor_bounds`.
pub fn logical_to_physical_rect(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    monitor_bounds: Rect,
    scale: f64,
) -> Option<Rect> {
    if !x1.is_finite() || !y1.is_finite() || !x2.is_finite() || !y2.is_finite() || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let lx1 = x1.min(x2);
    let ly1 = y1.min(y2);
    let lx2 = x1.max(x2);
    let ly2 = y1.max(y2);

    let px1 = monitor_bounds.x + (lx1 * scale).round() as i32;
    let py1 = monitor_bounds.y + (ly1 * scale).round() as i32;
    let px2 = monitor_bounds.x + (lx2 * scale).round() as i32;
    let py2 = monitor_bounds.y + (ly2 * scale).round() as i32;

    let norm = normalize_rect(px1, py1, px2, py2);
    let clamped = clamp_rect(norm, monitor_bounds)?;
    if clamped.width < 2 || clamped.height < 2 {
        return None;
    }
    Some(clamped)
}

/// Convert a physical-pixel rect into monitor-local logical CSS coordinates
/// `(x, y, width, height)` after clamping to `monitor_bounds`.
pub fn physical_to_local_logical(rect: Rect, monitor_bounds: Rect, scale: f64) -> Option<(f64, f64, f64, f64)> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let clipped = clamp_rect(rect, monitor_bounds)?;
    Some((
        (clipped.x - monitor_bounds.x) as f64 / scale,
        (clipped.y - monitor_bounds.y) as f64 / scale,
        clipped.width as f64 / scale,
        clipped.height as f64 / scale,
    ))
}

// ---------- pure circle detection ----------

const MIN_BBOX_SIDE: f64 = 40.0;
const MAX_ASPECT_RATIO: f64 = 6.5;
const MIN_TURNING_RAD: f64 = 300.0 * std::f64::consts::PI / 180.0;
const MAX_TURNING_RAD: f64 = 540.0 * std::f64::consts::PI / 180.0;
const MIN_MONOTONICITY: f64 = 0.72;
const MIN_MEAN_NORM_R: f64 = 0.65;
const MAX_INNER_FRACTION: f64 = 0.20;
const MIN_STEP_PX: f64 = 3.0;

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Detect whether a recent cursor trail ends with a hand-drawn closed loop
/// (circle, ellipse, or hand-drawn rectangle) around an area on screen.
///
/// Pure function: takes physical-pixel `(x, y)` points in chronological order
/// and returns the loop's bounding `Rect` if a valid loop is detected.
pub fn detect_circle(points: &[(f64, f64)]) -> Option<Rect> {
    if points.len() < 5 {
        return None;
    }

    // Dedupe tiny stationary jitter so micro-movements cannot inflate turning.
    let mut pts: Vec<(f64, f64)> = Vec::with_capacity(points.len());
    for &(x, y) in points {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        if pts.last().is_none_or(|&prev| dist(prev, (x, y)) >= MIN_STEP_PX) {
            pts.push((x, y));
        }
    }
    if let Some(&(lx, ly)) = points.last() {
        if lx.is_finite() && ly.is_finite() && pts.last().is_some_and(|&p| dist(p, (lx, ly)) > 0.0) {
            pts.push((lx, ly));
        }
    }

    let n = pts.len();
    if n < 5 {
        return None;
    }

    let mut best: Option<(f64, Rect)> = None;
    let two_pi = std::f64::consts::TAU;

    // Scan suffixes ending at the latest point so a straight approach line
    // before the circle is ignored automatically.
    for start in 0..=(n - 5) {
        let sub = &pts[start..];
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut path_len = 0.0;

        for (i, &(x, y)) in sub.iter().enumerate() {
            if x < min_x { min_x = x; }
            if x > max_x { max_x = x; }
            if y < min_y { min_y = y; }
            if y > max_y { max_y = y; }
            if i > 0 {
                path_len += dist(sub[i - 1], (x, y));
            }
        }

        let w = max_x - min_x;
        let h = max_y - min_y;
        if w < MIN_BBOX_SIDE || h < MIN_BBOX_SIDE {
            continue;
        }
        let aspect = w.max(h) / w.min(h);
        if aspect > MAX_ASPECT_RATIO {
            continue;
        }

        let diag = w.hypot(h);
        let gap = dist(sub[0], sub[sub.len() - 1]);
        if gap > diag * 0.35 || gap > w.min(h) * 0.85 {
            continue;
        }

        let semi_perim = w + h;
        if path_len < 1.25 * semi_perim || path_len > 4.2 * semi_perim {
            continue;
        }

        // Normalize into [-1, 1] x [-1, 1] around the bbox centre so ellipses
        // and wide rectangles measure turning angle cleanly.
        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        let rx = w * 0.5;
        let ry = h * 0.5;

        let mut signed_turn = 0.0;
        let mut abs_turn = 0.0;
        let mut sum_r = 0.0;
        let mut inner_count = 0usize;
        let mut quadrants = [false; 4];

        let mut prev_n: Option<(f64, f64)> = None;
        for &(x, y) in sub {
            let nx = (x - cx) / rx;
            let ny = (y - cy) / ry;
            let r = nx.hypot(ny);
            sum_r += r;
            if r < 0.35 {
                inner_count += 1;
            }
            let q = ((nx >= 0.0) as usize) | (((ny >= 0.0) as usize) << 1);
            quadrants[q] = true;

            if let Some((px, py)) = prev_n {
                let pr = px.hypot(py);
                if pr > 1e-4 && r > 1e-4 {
                    let cross = px * ny - py * nx;
                    let dot = px * nx + py * ny;
                    let dt = cross.atan2(dot);
                    signed_turn += dt;
                    abs_turn += dt.abs();
                }
            }
            prev_n = Some((nx, ny));
        }

        let net_turn = signed_turn.abs();
        if !(MIN_TURNING_RAD..=MAX_TURNING_RAD).contains(&net_turn) {
            continue;
        }
        if abs_turn <= 1e-6 || (net_turn / abs_turn) < MIN_MONOTONICITY {
            continue;
        }
        let mean_r = sum_r / sub.len() as f64;
        let inner_frac = inner_count as f64 / sub.len() as f64;
        if mean_r < MIN_MEAN_NORM_R || inner_frac > MAX_INNER_FRACTION {
            continue;
        }
        let visited_quadrants = quadrants.iter().filter(|&&v| v).count();
        if visited_quadrants < 3 {
            continue;
        }

        let score = (gap / diag) + (net_turn - two_pi).abs() * 0.25;
        let rect = Rect {
            x: min_x.floor() as i32,
            y: min_y.floor() as i32,
            width: (max_x.ceil() - min_x.floor()) as i32,
            height: (max_y.ceil() - min_y.floor()) as i32,
        };
        if best.is_none_or(|(best_score, _)| score < best_score) {
            best = Some((score, rect));
        }
    }

    best.map(|(_, rect)| rect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn sample_ellipse(cx: f64, cy: f64, rx: f64, ry: f64, n: usize, sweep: f64, ccw: bool) -> Vec<(f64, f64)> {
        let dir = if ccw { -1.0 } else { 1.0 };
        (0..=n)
            .map(|i| {
                let a = dir * sweep * (i as f64 / n as f64);
                (cx + rx * a.cos(), cy + ry * a.sin())
            })
            .collect()
    }

    #[test]
    fn normalizes_and_clamps_including_negative_monitors() {
        assert_eq!(normalize_rect(300, 400, 100, 50), Rect { x: 100, y: 50, width: 200, height: 350 });
        assert_eq!(normalize_rect(-50, -20, -250, -120), Rect { x: -250, y: -120, width: 200, height: 100 });

        let left_monitor = Rect { x: -1920, y: -120, width: 1920, height: 1080 };
        assert_eq!(
            clamp_rect(Rect { x: -2000, y: -200, width: 300, height: 300 }, left_monitor),
            Some(Rect { x: -1920, y: -120, width: 220, height: 220 })
        );
        // Outside monitor entirely -> None.
        assert_eq!(clamp_rect(Rect { x: 100, y: 100, width: 200, height: 200 }, left_monitor), None);
        // Zero-width -> None.
        assert_eq!(clamp_rect(Rect { x: -500, y: 0, width: 0, height: 100 }, left_monitor), None);
    }

    #[test]
    fn converts_logical_and_physical_rects_across_dpi_and_monitors() {
        // Secondary monitor to the left at 150% DPI.
        let mon = Rect { x: -2880, y: 0, width: 2880, height: 1620 };
        let scale = 1.5;
        // Dragged from bottom-right to top-left in logical CSS pixels.
        let phys = logical_to_physical_rect(500.0, 400.0, 100.0, 200.0, mon, scale).unwrap();
        assert_eq!(phys, Rect { x: -2880 + 150, y: 300, width: 600, height: 300 });

        // Round-trip back to local logical coordinates.
        let (lx, ly, lw, lh) = physical_to_local_logical(phys, mon, scale).unwrap();
        assert!((lx - 100.0).abs() < 1e-6);
        assert!((ly - 200.0).abs() < 1e-6);
        assert!((lw - 400.0).abs() < 1e-6);
        assert!((lh - 200.0).abs() < 1e-6);

        // Selection extending past monitor edge is clamped.
        let clamped = logical_to_physical_rect(-40.0, 10.0, 2000.0, 1200.0, mon, scale).unwrap();
        assert_eq!(clamped, Rect { x: -2880, y: 15, width: 2880, height: 1605 });

        // Fractional 125% DPI on negative Y monitor.
        let top_mon = Rect { x: 0, y: -1080, width: 1920, height: 1080 };
        let r125 = logical_to_physical_rect(80.0, 40.0, 240.0, 120.0, top_mon, 1.25).unwrap();
        assert_eq!(r125, Rect { x: 100, y: -1030, width: 200, height: 100 });

        // Tiny drag (< 2px) rejected.
        assert!(logical_to_physical_rect(10.0, 10.0, 10.5, 10.5, mon, scale).is_none());
    }

    #[test]
    fn expands_and_clamps_circle_bbox() {
        let mon = Rect { x: -1920, y: 0, width: 1920, height: 1080 };
        let raw = Rect { x: -1915, y: 10, width: 100, height: 80 };
        assert_eq!(
            expand_rect(raw, 20, mon),
            Some(Rect { x: -1920, y: 0, width: 125, height: 110 })
        );
    }

    #[test]
    fn detects_clockwise_and_counterclockwise_circles() {
        let cw = sample_ellipse(500.0, 400.0, 80.0, 80.0, 40, TAU, false);
        let r = detect_circle(&cw).expect("clockwise circle");
        assert!((r.x - 420).abs() <= 2);
        assert!((r.y - 320).abs() <= 2);
        assert!((r.width - 160).abs() <= 3);
        assert!((r.height - 160).abs() <= 3);

        let ccw = sample_ellipse(-900.0, -300.0, 60.0, 60.0, 36, TAU, true);
        let r = detect_circle(&ccw).expect("counter-clockwise circle on negative monitor");
        assert!((r.x - (-960)).abs() <= 2);
        assert!((r.y - (-360)).abs() <= 2);
        assert!((r.width - 120).abs() <= 3);

        // Slightly open (~325°) hand circle still closes cleanly.
        let open = sample_ellipse(300.0, 300.0, 75.0, 75.0, 32, 325.0_f64.to_radians(), false);
        assert!(detect_circle(&open).is_some());
    }

    #[test]
    fn detects_ellipses_and_hand_drawn_rectangles() {
        // Wide ellipse around a UI button or text line.
        let ell = sample_ellipse(600.0, 250.0, 180.0, 45.0, 44, TAU, false);
        let r = detect_circle(&ell).expect("ellipse");
        assert!((r.width - 360).abs() <= 4);
        assert!((r.height - 90).abs() <= 4);

        // Hand-drawn rectangle with slight wobble along the 4 edges.
        let mut rect_pts = Vec::new();
        for i in 0..10 {
            rect_pts.push((200.0 + i as f64 * 22.0, 100.0 + ((i % 2) as f64) * 3.0));
        }
        for i in 0..8 {
            rect_pts.push((420.0 - ((i % 2) as f64) * 3.0, 100.0 + i as f64 * 18.0));
        }
        for i in 0..10 {
            rect_pts.push((420.0 - i as f64 * 22.0, 244.0 + ((i % 2) as f64) * 3.0));
        }
        for i in 0..=8 {
            rect_pts.push((200.0 + ((i % 2) as f64) * 3.0, 244.0 - i as f64 * 17.5));
        }
        let r = detect_circle(&rect_pts).expect("hand-drawn rectangle");
        assert!(r.width >= 210 && r.height >= 135);
    }

    #[test]
    fn detects_circle_after_straight_approach_trail() {
        let mut pts = Vec::new();
        // Straight cursor move from (50, 50) to (480, 400).
        for i in 0..20 {
            let t = i as f64 / 20.0;
            pts.push((50.0 + 430.0 * t, 50.0 + 350.0 * t));
        }
        // Followed by circling around (400, 400) with radius 80.
        pts.extend(sample_ellipse(400.0, 400.0, 80.0, 80.0, 36, TAU, false));
        let r = detect_circle(&pts).expect("circle after approach");
        // Bbox should tightly match the circle, not include (50, 50).
        assert!(r.x >= 315 && r.y >= 315, "unexpected bbox including approach: {r:?}");
        assert!(r.width <= 170 && r.height <= 170, "unexpected size: {r:?}");
    }

    #[test]
    fn rejects_lines_zigzags_half_arcs_and_tiny_jitter() {
        // Horizontal line.
        let horiz: Vec<(f64, f64)> = (0..30).map(|i| (100.0 + i as f64 * 10.0, 200.0)).collect();
        assert!(detect_circle(&horiz).is_none());

        // Diagonal line and back-and-forth scribble along a diagonal.
        let mut diag: Vec<(f64, f64)> = (0..20).map(|i| (100.0 + i as f64 * 10.0, 100.0 + i as f64 * 8.0)).collect();
        diag.extend((0..20).rev().map(|i| (100.0 + i as f64 * 10.0, 104.0 + i as f64 * 8.0)));
        assert!(detect_circle(&diag).is_none());

        // Zig-zag across a 240x120 box returning near start.
        let mut zigzag = vec![
            (100.0, 100.0), (160.0, 220.0), (220.0, 100.0), (280.0, 220.0), (340.0, 100.0),
            (280.0, 210.0), (220.0, 110.0), (160.0, 210.0), (105.0, 105.0),
        ];
        // Subdivide segments so it has plenty of points like a 60 Hz mouse trail.
        let mut dense_zz = Vec::new();
        for w in zigzag.windows(2) {
            for k in 0..4 {
                let t = k as f64 / 4.0;
                dense_zz.push((w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t));
            }
        }
        zigzag.push((100.0, 100.0));
        assert!(detect_circle(&dense_zz).is_none());

        // Half-circle (180° U-turn).
        let u_turn = sample_ellipse(400.0, 400.0, 80.0, 80.0, 25, std::f64::consts::PI, false);
        assert!(detect_circle(&u_turn).is_none());

        // Tiny jitter (14x14 circle — below 40x40 minimum).
        let tiny = sample_ellipse(500.0, 500.0, 7.0, 7.0, 30, TAU, false);
        assert!(detect_circle(&tiny).is_none());

        // Empty / too few points.
        assert!(detect_circle(&[]).is_none());
        assert!(detect_circle(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)]).is_none());
    }
}
