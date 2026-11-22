pub use super::data_island::*;
#[path = "notch.rs"]
pub mod notch;
use crate::error::{AppError, AppResult};
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSStatusWindowLevel,
    NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
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
static INPUT_FOCUSABLE: AtomicBool = AtomicBool::new(false);
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
    last_external_app: AtomicI32,
    screen: Mutex<Option<notch::ScreenGeometry>>,
}
impl Island {
    pub fn new(app: AppHandle, window: WebviewWindow) -> AppResult<Arc<Self>> {
        app.set_activation_policy(tauri::ActivationPolicy::Accessory)?;
        unsafe {
            install_panel(window.ns_window()? as *mut AnyObject)?;
        }
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
            last_external_app: AtomicI32::new(
                NSWorkspace::sharedWorkspace()
                    .frontmostApplication()
                    .map(|app| app.processIdentifier())
                    .filter(|pid| *pid != std::process::id() as i32)
                    .unwrap_or(0),
            ),
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
        let window = unsafe { &*(self.window.ns_window()? as *const NSWindow) };
        let mut style = window.styleMask();
        style.set(
            NSWindowStyleMask::NonactivatingPanel,
            !INPUT_FOCUSABLE.load(Ordering::Relaxed),
        );
        window.setStyleMask(style);
        window.setLevel(NSStatusWindowLevel + 2);
        window.setHasShadow(false);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        if let Some(screen) = notch::read(mtm) {
            let mut current = self.screen.lock();
            if current.as_ref() == Some(&screen) {
                return Ok(());
            }
            self.window.set_size(LogicalSize::new(CANVAS_W, CANVAS_H))?;
            // AppKit screen coordinates are logical points with a bottom-left origin.
            window.setFrameTopLeftPoint(NSPoint::new(
                screen.center_x - CANVAS_W / 2.0,
                screen.frame.origin.y + screen.frame.size.height,
            ));
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
            let front = NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .map(|app| app.processIdentifier());
            self.previous_app.store(
                previous_app_pid(front, self.last_external_app.load(Ordering::Relaxed)),
                Ordering::Relaxed,
            );
        }
        INPUT_FOCUSABLE.store(true, Ordering::Relaxed);
        let window = unsafe { &*(self.window.ns_window()? as *const NSWindow) };
        window.setStyleMask(window.styleMask() & !NSWindowStyleMask::NonactivatingPanel);
        self.window.set_focus()?;
        Ok(())
    }
    pub fn end_input(&self, restore: bool) -> AppResult<()> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| AppError::platform("Фокус окна требует главный поток"))?;
        let window = unsafe { &*(self.window.ns_window()? as *const NSWindow) };
        window.makeFirstResponder(None);
        window.resignKeyWindow();
        INPUT_FOCUSABLE.store(false, Ordering::Relaxed);
        window.setStyleMask(window.styleMask() | NSWindowStyleMask::NonactivatingPanel);
        let pid = self.previous_app.swap(0, Ordering::Relaxed);
        let front = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|app| app.processIdentifier())
            .unwrap_or(0);
        if should_restore_input(restore, pid, front) {
            if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
                let application = NSApplication::sharedApplication(mtm);
                let can_yield: bool = unsafe {
                    objc2::msg_send![&application, respondsToSelector: objc2::sel!(yieldActivationToApplication:)]
                };
                if can_yield {
                    application.yieldActivationToApplication(&app);
                }
                application.deactivate();
                #[allow(deprecated)]
                let activated = app.activateWithOptions(
                    NSApplicationActivationOptions::ActivateIgnoringOtherApps
                        | NSApplicationActivationOptions::ActivateAllWindows,
                );
                if !activated {
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
                        if let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() {
                            let pid = app.processIdentifier();
                            if pid != std::process::id() as i32 {
                                island.last_external_app.store(pid, Ordering::Relaxed);
                            }
                        }
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

fn should_restore_input(restore: bool, previous: i32, front: i32) -> bool {
    restore && previous != 0 && (front == previous || front == std::process::id() as i32)
}

unsafe fn install_panel(window: *mut AnyObject) -> AppResult<()> {
    let original = unsafe { &*window }.class();
    let panel =
        AnyClass::get(c"NSPanel").ok_or_else(|| AppError::platform("NSPanel недоступен"))?;
    let tao =
        AnyClass::get(c"TaoWindow").ok_or_else(|| AppError::platform("Нет класса TaoWindow"))?;
    let is_tao = std::iter::successors(Some(original), |class| class.superclass())
        .any(|class| std::ptr::eq(class, tao));
    if !is_tao || panel.instance_size() > original.instance_size() {
        return Err(AppError::platform(format!(
            "Несовместимая структура окна {:?}: {} / NSPanel {}",
            original.name(),
            original.instance_size(),
            panel.instance_size()
        )));
    }
    let class = AnyClass::get(c"JarvisIslandPanel").unwrap_or_else(|| {
        let mut builder =
            ClassBuilder::new(c"JarvisIslandPanel", panel).expect("unique panel class");
        unsafe {
            builder.add_method(
                objc2::sel!(canBecomeKeyWindow),
                panel_can_become_key as extern "C" fn(_, _) -> _,
            );
        }
        let class = builder.register();
        for method in tao.instance_methods().iter() {
            if method.name() != objc2::sel!(sendEvent:) {
                continue;
            }
            unsafe {
                objc2::ffi::class_addMethod(
                    class as *const _ as *mut _,
                    method.name(),
                    method.implementation(),
                    objc2::ffi::method_getTypeEncoding(*method),
                );
            }
        }
        class
    });
    // NSPanel fits in TaoWindow's allocation; keep NSWindow storage and Tao's event dispatch.
    unsafe {
        objc2::ffi::object_setClass(window, class);
    }
    let _: () = unsafe { objc2::msg_send![window, setHidesOnDeactivate: false] };
    Ok(())
}

extern "C" fn panel_can_become_key(_: &AnyObject, _: Sel) -> Bool {
    Bool::new(INPUT_FOCUSABLE.load(Ordering::Relaxed))
}

fn contains(shape: ShapeRect, x: f64, y: f64) -> bool {
    x >= shape.x && y >= shape.y && x < shape.x + shape.width && y < shape.y + shape.height
}

fn previous_app_pid(front: Option<i32>, last_external: i32) -> i32 {
    front
        .filter(|pid| *pid > 0 && *pid != std::process::id() as i32)
        .unwrap_or(last_external)
}

fn contains_cocoa(shape: ShapeRect, x: f64, y: f64) -> bool {
    contains(shape, x, CANVAS_H - y)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn previous_app_survives_activation_before_input_begins() {
        assert_eq!(previous_app_pid(Some(42), 43), 42);
        assert_eq!(previous_app_pid(Some(std::process::id() as i32), 42), 42);
        assert_eq!(previous_app_pid(None, 42), 42);
        assert_eq!(previous_app_pid(None, 0), 0);
        assert_eq!(previous_app_pid(Some(0), 42), 42);
    }
    #[test]
    fn input_restore_respects_the_frontmost_application() {
        let own = std::process::id() as i32;
        assert!(should_restore_input(true, 42, own));
        assert!(should_restore_input(true, 42, 42));
        assert!(!should_restore_input(false, 42, own));
        assert!(!should_restore_input(true, 0, own));
        assert!(!should_restore_input(true, 42, 43));
    }
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
