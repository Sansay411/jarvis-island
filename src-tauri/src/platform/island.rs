//! The Dynamic Island overlay window.
//!
//! The window is a fixed transparent canvas (large enough for the biggest
//! island state plus its shadow) pinned to the top centre of the primary
//! display. It is click-through by default (`WS_EX_TRANSPARENT`); a small
//! tracker thread watches the cursor and makes the window interactive only
//! while the pointer is over the island's *current* shape, which the frontend
//! reports whenever the shape changes. This keeps transparent pixels from
//! blocking the apps underneath without per-frame IPC.
//!
//! Keyboard focus: the window is non-activating (`WS_EX_NOACTIVATE`) so
//! clicks on media buttons never steal focus. When the chat workspace needs
//! typing, [`Island::begin_input`] makes it focusable, remembers the previous
//! foreground window and [`Island::end_input`] hands focus back.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, LogicalSize, PhysicalPosition, WebviewWindow};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetGUIThreadInfo, IsWindow, SetForegroundWindow,
    GUITHREADINFO, GUI_INMOVESIZE,
};

use super::display;
use crate::error::AppResult;

pub const LABEL: &str = "island";
/// Logical size of the transparent canvas window.
pub const CANVAS_W: f64 = 640.0;
pub const CANVAS_H: f64 = 600.0;

pub const EVT_POINTER: &str = "island:pointer";
pub const EVT_PRESENCE: &str = "island:presence";
pub const EVT_BLUR: &str = "island:blur";

/// Island shape in logical pixels relative to the canvas window's client area.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct ShapeRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct ScreenRect {
    l: i32,
    t: i32,
    r: i32,
    b: i32,
}

impl ScreenRect {
    fn contains(&self, p: POINT, margin: i32) -> bool {
        p.x >= self.l - margin && p.x < self.r + margin && p.y >= self.t - margin && p.y < self.b + margin
    }

    /// Chebyshev distance from the rect edge (0 when inside).
    fn distance(&self, p: POINT) -> i32 {
        let dx = (self.l - p.x).max(p.x - self.r).max(0);
        let dy = (self.t - p.y).max(p.y - self.b).max(0);
        dx.max(dy)
    }

    /// Zone in which a press-and-drag counts as "approaching" the island
    /// (file drags from Explorer toward the top of the screen).
    fn approach_zone(&self, scale: f64) -> ScreenRect {
        let side = (140.0 * scale) as i32;
        let below = (90.0 * scale) as i32;
        ScreenRect { l: self.l - side, t: self.t, r: self.r + side, b: self.b + below }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Placement {
    /// Canvas window origin in physical screen pixels.
    x: i32,
    y: i32,
    scale: f64,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PointerEvent {
    pub inside: bool,
    pub drag_approach: bool,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct PresenceEvent {
    pub visible: bool,
    pub reason: &'static str,
}

pub struct Island {
    window: WebviewWindow,
    app: AppHandle,
    hwnd: isize,
    shape: Mutex<ShapeRect>,
    placement: Mutex<Placement>,
    screen_rect: Mutex<ScreenRect>,
    /// Hidden because a fullscreen app owns the island's display.
    hidden_for_fullscreen: AtomicBool,
    /// Hidden by the user (tray / shortcut).
    hidden_by_user: AtomicBool,
    input_mode: AtomicBool,
    /// While the agent drives the mouse, the island stays click-through so a
    /// synthetic click near the top centre reaches the app underneath.
    pointer_suppressed: AtomicBool,
    prev_foreground: AtomicIsize,
    /// Counters for diagnostics: tracker iterations and wall time spent.
    pub tracker_ticks: std::sync::atomic::AtomicU64,
}

impl Island {
    pub fn new(app: AppHandle, window: WebviewWindow) -> AppResult<Arc<Self>> {
        let hwnd = window.hwnd()?.0 as isize;
        let island = Arc::new(Island {
            window,
            app,
            hwnd,
            shape: Mutex::new(ShapeRect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 }),
            placement: Mutex::new(Placement::default()),
            screen_rect: Mutex::new(ScreenRect::default()),
            hidden_for_fullscreen: AtomicBool::new(false),
            hidden_by_user: AtomicBool::new(false),
            input_mode: AtomicBool::new(false),
            pointer_suppressed: AtomicBool::new(false),
            prev_foreground: AtomicIsize::new(0),
            tracker_ticks: std::sync::atomic::AtomicU64::new(0),
        });
        island.style_window();
        island.window.set_ignore_cursor_events(true)?;
        island.place()?;
        Ok(island)
    }

    pub fn hwnd(&self) -> isize {
        self.hwnd
    }

    fn hwnd_handle(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    /// Remove the Windows 11 rounded corners and accent border DWM would draw
    /// around the transparent canvas.
    fn style_window(&self) {
        let hwnd = self.hwnd_handle();
        unsafe {
            let pref = DWMWCP_DONOTROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const _ as *const _,
                std::mem::size_of_val(&pref) as u32,
            );
            // DWMWA_COLOR_NONE
            let none: u32 = 0xFFFF_FFFE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &none as *const _ as *const _,
                std::mem::size_of_val(&none) as u32,
            );
        }
    }

    /// Pin the canvas to the top centre of the primary display's work area.
    /// Called at start-up and whenever displays, DPI or the work area change.
    pub fn place(&self) -> AppResult<()> {
        let Some(mon) = display::primary_monitor() else {
            tracing::warn!("no primary monitor found; island not placed");
            return Ok(());
        };
        let scale = mon.scale;
        let w_phys = (CANVAS_W * scale).round() as i32;
        let x = mon.work_area.x + (mon.work_area.width - w_phys) / 2;
        let y = mon.work_area.y;
        // Move onto the target monitor first so a DPI change is applied
        // before sizing, then size in logical units and centre precisely.
        self.window.set_position(PhysicalPosition::new(x, y))?;
        self.window.set_size(LogicalSize::new(CANVAS_W, CANVAS_H))?;
        self.window.set_position(PhysicalPosition::new(x, y))?;
        *self.placement.lock() = Placement { x, y, scale };
        self.recompute_screen_rect();
        tracing::info!(x, y, scale, "island placed on {}", mon.name);
        Ok(())
    }

    fn recompute_screen_rect(&self) {
        let p = *self.placement.lock();
        let s = *self.shape.lock();
        let r = ScreenRect {
            l: p.x + (s.x * p.scale).round() as i32,
            t: p.y + (s.y * p.scale).round() as i32,
            r: p.x + ((s.x + s.width) * p.scale).round() as i32,
            b: p.y + ((s.y + s.height) * p.scale).round() as i32,
        };
        *self.screen_rect.lock() = r;
    }

    /// Frontend reports the island's interactive shape (target geometry of the
    /// current state, so the pointer never "falls out" mid-animation).
    pub fn set_shape(&self, shape: ShapeRect) {
        *self.shape.lock() = shape;
        self.recompute_screen_rect();
        tracing::debug!(?shape, rect = ?*self.screen_rect.lock(), "island shape");
    }

    pub fn is_visible(&self) -> bool {
        !self.hidden_for_fullscreen.load(Ordering::Relaxed) && !self.hidden_by_user.load(Ordering::Relaxed)
    }

    fn apply_visibility(&self, reason: &'static str) {
        let visible = self.is_visible();
        let res = if visible { self.window.show() } else { self.window.hide() };
        if let Err(e) = res {
            tracing::warn!("island visibility change failed: {e}");
        }
        let _ = self.app.emit_to(LABEL, EVT_PRESENCE, PresenceEvent { visible, reason });
    }

    pub fn set_fullscreen_hidden(&self, hidden: bool) {
        if self.hidden_for_fullscreen.swap(hidden, Ordering::Relaxed) != hidden {
            self.apply_visibility("fullscreen");
        }
    }

    pub fn set_user_hidden(&self, hidden: bool) {
        if self.hidden_by_user.swap(hidden, Ordering::Relaxed) != hidden {
            self.apply_visibility("user");
        }
    }

    pub fn toggle_user_hidden(&self) {
        let hidden = !self.hidden_by_user.load(Ordering::Relaxed);
        self.set_user_hidden(hidden);
    }

    /// Make the island focusable and bring it to the foreground for typing.
    pub fn begin_input(&self) -> AppResult<()> {
        unsafe {
            let fg = GetForegroundWindow();
            if !fg.is_invalid() && fg.0 as isize != self.hwnd {
                self.prev_foreground.store(fg.0 as isize, Ordering::Relaxed);
            }
        }
        self.input_mode.store(true, Ordering::Relaxed);
        self.window.set_focusable(true)?;
        self.window.set_focus()?;
        Ok(())
    }

    /// Return to the non-activating overlay. With `restore`, hand keyboard
    /// focus back to the window that had it before the workspace opened —
    /// but only if we still own the foreground (the user may have clicked
    /// somewhere else on purpose).
    pub fn end_input(&self, restore: bool) -> AppResult<()> {
        if !self.input_mode.swap(false, Ordering::Relaxed) {
            return Ok(());
        }
        self.window.set_focusable(false)?;
        let prev = self.prev_foreground.swap(0, Ordering::Relaxed);
        if restore && prev != 0 {
            unsafe {
                let prev = HWND(prev as *mut _);
                if IsWindow(Some(prev)).as_bool() && GetForegroundWindow().0 as isize == self.hwnd {
                    let _ = SetForegroundWindow(prev);
                }
            }
        }
        Ok(())
    }

    pub fn suppress_pointer(&self, on: bool) {
        self.pointer_suppressed.store(on, Ordering::Relaxed);
        if on {
            let _ = self.window.set_ignore_cursor_events(true);
        }
    }

    pub fn on_focus_lost(&self) {
        if self.input_mode.load(Ordering::Relaxed) {
            // Focus already moved elsewhere by the user: don't steal it back.
            let _ = self.end_input(false);
            let _ = self.app.emit_to(LABEL, EVT_BLUR, ());
        }
    }

    /// Spawn the pointer tracker. Polling `GetCursorPos` is a single cheap
    /// syscall; the interval adapts to the pointer's distance from the island
    /// (16 ms nearby, 100 ms far away, 250 ms while hidden), which keeps the
    /// idle cost negligible while hover stays responsive. A low-level mouse
    /// hook would be event-driven but sits in the input path of every mouse
    /// move system-wide, so it was rejected.
    pub fn spawn_tracker(self: &Arc<Self>) {
        let island = Arc::clone(self);
        std::thread::Builder::new()
            .name("island-tracker".into())
            .spawn(move || island.track_loop())
            .expect("spawn island tracker");
    }

    fn track_loop(&self) {
        let mut interactive = false;
        let mut last = PointerEvent { inside: false, drag_approach: false };
        let mut was_down = false;
        let mut press_inside = false;
        loop {
            self.tracker_ticks.fetch_add(1, Ordering::Relaxed);
            if !self.is_visible() || self.pointer_suppressed.load(Ordering::Relaxed) {
                if interactive {
                    let _ = self.window.set_ignore_cursor_events(true);
                    interactive = false;
                }
                std::thread::sleep(Duration::from_millis(if self.is_visible() { 30 } else { 250 }));
                continue;
            }

            let mut pt = POINT::default();
            if unsafe { GetCursorPos(&mut pt) }.is_err() {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            let rect = *self.screen_rect.lock();
            let scale = self.placement.lock().scale.max(1.0);
            let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;

            // Hysteresis: once inside, allow a few pixels of slack so the
            // pointer resting on the edge doesn't flicker the hover state.
            let margin = if last.inside { (4.0 * scale) as i32 } else { 0 };
            let inside = rect.contains(pt, margin);

            if down && !was_down {
                press_inside = inside;
            }
            if !down {
                press_inside = false;
            }

            // A press that started outside and is now dragging near the
            // island may be a file drag. Window move/resize loops are
            // excluded so dragging a window to the top edge doesn't react.
            let drag_approach = down
                && !press_inside
                && !inside
                && rect.approach_zone(scale).contains(pt, 0)
                && !foreground_in_move_size();

            // Keep capturing while a press that began on the island is held
            // (slider drags may leave the shape).
            let want = inside || drag_approach || (down && press_inside);
            if want != interactive {
                if self.window.set_ignore_cursor_events(!want).is_ok() {
                    interactive = want;
                }
            }

            let ev = PointerEvent { inside, drag_approach };
            if ev != last {
                tracing::debug!(inside, drag_approach, x = pt.x, y = pt.y, ?rect, "island pointer");
                let _ = self.app.emit_to(LABEL, EVT_POINTER, ev);
                last = ev;
            }
            was_down = down;

            let d = rect.distance(pt);
            let wait = if inside || down || d < (120.0 * scale) as i32 {
                16
            } else if d < (400.0 * scale) as i32 {
                40
            } else {
                100
            };
            std::thread::sleep(Duration::from_millis(wait));
        }
    }

    /// Diagnostics: how many tracker iterations ran since `since`.
    pub fn tracker_rate(&self, since: Instant, ticks_then: u64) -> f64 {
        let ticks = self.tracker_ticks.load(Ordering::Relaxed) - ticks_then;
        ticks as f64 / since.elapsed().as_secs_f64().max(0.001)
    }
}

/// True while the foreground thread is in a window move/size modal loop.
fn foreground_in_move_size() -> bool {
    unsafe {
        let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        GetGUIThreadInfo(0, &mut info).is_ok() && (info.flags & GUI_INMOVESIZE).0 != 0
    }
}
