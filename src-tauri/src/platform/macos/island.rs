pub use super::data_island::*;
#[path = "notch.rs"]
pub mod notch;
use crate::error::AppResult;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplicationActivationOptions, NSMainMenuWindowLevel, NSRunningApplication, NSWindow,
    NSWindowCollectionBehavior, NSWorkspace,
};
use objc2_foundation::NSPoint;
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
    Arc,
};
use std::time::Instant;
use tauri::{AppHandle, Emitter, LogicalSize, WebviewWindow};
pub const LABEL: &str = "island";
pub const CANVAS_W: f64 = 760.0;
pub const CANVAS_H: f64 = 640.0;
pub const EVT_POINTER: &str = "island:pointer";
pub const EVT_PRESENCE: &str = "island:presence";
pub const EVT_BLUR: &str = "island:blur";
pub const EVT_CONTEXT: &str = "island:context";
pub struct Island {
    pub tracker_ticks: AtomicU64,
    window: WebviewWindow,
    app: AppHandle,
    hidden: AtomicBool,
    fullscreen_hidden: AtomicBool,
    covered: AtomicBool,
    shape: Mutex<ShapeRect>,
    pointer_suppressed: AtomicBool,
    pointer_inside: AtomicBool,
    previous_app: AtomicI32,
    screen: Mutex<Option<notch::ScreenGeometry>>,
}
impl Island {
    pub fn new(app: AppHandle, window: WebviewWindow) -> AppResult<Arc<Self>> {
        app.set_activation_policy(tauri::ActivationPolicy::Accessory)?;
        window.set_visible_on_all_workspaces(true)?;
        window.set_ignore_cursor_events(true)?;
        let island = Arc::new(Self {
            tracker_ticks: AtomicU64::new(0),
            window,
            app,
            hidden: AtomicBool::new(false),
            fullscreen_hidden: AtomicBool::new(false),
            covered: AtomicBool::new(false),
            shape: Mutex::new(ShapeRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            }),
            pointer_suppressed: AtomicBool::new(false),
            pointer_inside: AtomicBool::new(false),
            previous_app: AtomicI32::new(0),
            screen: Mutex::new(None),
        });
        island.place()?;
        Ok(island)
    }
    pub fn hwnd(&self) -> isize {
        0
    }
    pub fn place(&self) -> AppResult<()> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Ok(());
        };
        if let Some(screen) = notch::read(mtm) {
            let mut current = self.screen.lock();
            if current.as_ref() == Some(&screen) {
                return Ok(());
            }
            self.window.set_size(LogicalSize::new(CANVAS_W, CANVAS_H))?;
            // AppKit screen coordinates are logical points with a bottom-left origin.
            let window = unsafe { &*(self.window.ns_window()? as *const NSWindow) };
            window.setFrameTopLeftPoint(NSPoint::new(
                screen.center_x - CANVAS_W / 2.0,
                screen.frame.origin.y + screen.frame.size.height,
            ));
            window.setLevel(NSMainMenuWindowLevel + 2);
            window.setHasShadow(false);
            window.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::Stationary
                    | NSWindowCollectionBehavior::FullScreenAuxiliary
                    | NSWindowCollectionBehavior::IgnoresCycle,
            );
            *current = Some(screen);
            let _ = self
                .app
                .emit_to(LABEL, "island:notch-geometry", screen.notch);
        }
        Ok(())
    }
    pub fn set_shape(&self, shape: ShapeRect) {
        *self.shape.lock() = shape;
    }
    pub fn is_visible(&self) -> bool {
        !self.hidden.load(Ordering::Relaxed) && !self.fullscreen_hidden.load(Ordering::Relaxed)
    }
    fn apply_visibility(&self, reason: &'static str) {
        let visible = self.is_visible();
        let result = if visible {
            self.window.show()
        } else {
            self.window.hide()
        };
        if let Err(e) = result {
            tracing::warn!("island visibility: {e}");
        }
        let _ = self
            .app
            .emit_to(LABEL, EVT_PRESENCE, PresenceEvent { visible, reason });
    }
    pub fn monitor_and_center(&self) -> (isize, i32, i32) {
        let pos = self.window.outer_position().unwrap_or_default();
        let scale = self.window.scale_factor().unwrap_or(1.0);
        (
            0,
            pos.x + (CANVAS_W * scale / 2.0) as i32,
            pos.y + (34.0 * scale) as i32,
        )
    }
    pub fn set_top_covered(&self, covered: bool) {
        self.covered.store(covered, Ordering::Relaxed);
        let _ = self.app.emit_to(
            LABEL,
            EVT_CONTEXT,
            serde_json::json!({"topCovered": covered}),
        );
    }
    pub fn top_covered(&self) -> bool {
        self.covered.load(Ordering::Relaxed)
    }
    pub fn set_fullscreen_hidden(&self, hidden: bool) {
        self.fullscreen_hidden.store(hidden, Ordering::Relaxed);
        self.apply_visibility("fullscreen");
    }
    pub fn set_user_hidden(&self, hidden: bool) {
        self.hidden.store(hidden, Ordering::Relaxed);
        self.apply_visibility("user");
    }
    pub fn toggle_user_hidden(&self) {
        self.set_user_hidden(!self.hidden.load(Ordering::Relaxed));
    }
    pub fn begin_input(&self) -> AppResult<()> {
        if self.previous_app.load(Ordering::Relaxed) == 0 {
            if let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() {
                let pid = app.processIdentifier();
                if pid != std::process::id() as i32 {
                    self.previous_app.store(pid, Ordering::Relaxed);
                }
            }
        }
        self.window.set_focusable(true)?;
        self.window.set_focus()?;
        Ok(())
    }
    pub fn end_input(&self, restore: bool) -> AppResult<()> {
        let was_focused = self.window.is_focused()?;
        self.window.set_focusable(false)?;
        let pid = self.previous_app.swap(0, Ordering::Relaxed);
        if restore && was_focused && pid != 0 {
            if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
                if !app.activateWithOptions(NSApplicationActivationOptions::empty()) {
                    tracing::warn!(pid, "could not restore focus to previous application");
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
        let _ = self.app.emit_to(LABEL, EVT_BLUR, ());
    }
    pub fn spawn_tracker(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(32));
            loop {
                interval.tick().await;
                let Some(island) = weak.upgrade() else { break };
                let refresh = island.tracker_ticks.fetch_add(1, Ordering::Relaxed) % 32 == 0;
                let weak = Arc::downgrade(&island);
                let _ = island.app.run_on_main_thread(move || {
                    if let Some(island) = weak.upgrade() {
                        if refresh {
                            if let Err(e) = island.place() {
                                tracing::warn!("island screen refresh: {e}");
                            }
                        }
                        let Ok(raw) = island.window.ns_window() else {
                            return;
                        };
                        let window = unsafe { &*(raw as *const NSWindow) };
                        let point = window.mouseLocationOutsideOfEventStream();
                        let inside = island.is_visible()
                            && !island.pointer_suppressed.load(Ordering::Relaxed)
                            && contains_cocoa(*island.shape.lock(), point.x, point.y);
                        window.setIgnoresMouseEvents(!inside);
                        if inside != island.pointer_inside.swap(inside, Ordering::Relaxed) {
                            let _ = island.app.emit_to(
                                LABEL,
                                EVT_POINTER,
                                PointerEvent {
                                    inside,
                                    drag_approach: false,
                                },
                            );
                        }
                    }
                });
            }
        });
    }
    pub fn tracker_rate(&self, since: Instant, ticks_then: u64) -> f64 {
        self.tracker_ticks
            .load(Ordering::Relaxed)
            .saturating_sub(ticks_then) as f64
            / since.elapsed().as_secs_f64().max(0.001)
    }
}

fn contains(shape: ShapeRect, x: f64, y: f64) -> bool {
    x >= shape.x && y >= shape.y && x < shape.x + shape.width && y < shape.y + shape.height
}

fn contains_cocoa(shape: ShapeRect, x: f64, y: f64) -> bool {
    contains(shape, x, CANVAS_H - y)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_hit_test_handles_inside_outside_and_empty_shapes() {
        let shape = ShapeRect {
            x: 10.0,
            y: 0.0,
            width: 174.0,
            height: 36.0,
        };
        assert!(contains(shape, 50.0, 20.0));
        assert!(!contains(shape, 184.0, 20.0));
        assert!(!contains(shape, 50.0, -1.0));
        assert!(contains_cocoa(shape, 50.0, CANVAS_H));
        assert!(contains_cocoa(shape, 50.0, CANVAS_H - 20.0));
        assert!(!contains_cocoa(shape, 50.0, CANVAS_H - 36.0));
        assert!(!contains_cocoa(shape, 50.0, CANVAS_H + 1.0));
        assert!(!contains(
            ShapeRect {
                width: 0.0,
                ..shape
            },
            10.0,
            0.0
        ));
    }
}
