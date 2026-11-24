//! On-screen overlay tools: `screen_point`, `screen_clear`, and step-by-step
//! interactive teaching (`guide_step`).
//!
//! Targets can be resolved either by Windows UI Automation (preferred: exact
//! physical bounds of a control in the foreground window) or by coordinates in
//! the space of the last screenshot / physical screen.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::Manager;

use super::screen;
use super::{handler, low, object, parse, str_arg, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};
use crate::overlay;
use crate::platform::display::{self, Rect};
use crate::platform::{self, uia, winmgr};

const DEFAULT_POINT_SECONDS: f64 = 8.0;
const DEFAULT_GUIDE_TIMEOUT_SECS: u64 = 120;
const DEFAULT_CLICK_MARGIN_CSS: f64 = 12.0;

#[derive(Deserialize, Clone, Default)]
struct ElementTarget {
    name: Option<String>,
    control_type: Option<String>,
    automation_id: Option<String>,
    snapshot_id: Option<u64>,
    element_id: Option<usize>,
    window: Option<String>,
}

impl ElementTarget {
    fn is_present(&self) -> bool {
        self.snapshot_id.is_some()
            || self.element_id.is_some()
            || self.name.as_deref().is_some_and(|s| !s.trim().is_empty())
            || self.control_type.as_deref().is_some_and(|s| !s.trim().is_empty())
            || self.automation_id.as_deref().is_some_and(|s| !s.trim().is_empty())
    }
}

#[derive(Deserialize)]
struct PointArgs {
    element: Option<ElementTarget>,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
    #[serde(default = "default_space")]
    space: String,
    capture_id: Option<u64>,
    label: Option<String>,
    caption: Option<String>,
    #[serde(default = "default_true")]
    arrow: bool,
    tone: Option<String>,
    seconds: Option<f64>,
    #[serde(default = "default_true")]
    replace: bool,
}

#[derive(Deserialize)]
struct GuideStepArgs {
    text: String,
    step: Option<u32>,
    total: Option<u32>,
    element: Option<ElementTarget>,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
    #[serde(default = "default_space")]
    space: String,
    capture_id: Option<u64>,
    label: Option<String>,
    #[serde(default = "default_wait")]
    wait: String,
    #[serde(default)]
    done: bool,
    margin: Option<f64>,
    timeout_seconds: Option<u64>,
}

fn default_space() -> String {
    "image".into()
}
fn default_true() -> bool {
    true
}
fn default_wait() -> String {
    "click".into()
}

#[derive(Debug, Clone)]
struct ResolvedTarget {
    rect: Rect,
    source: &'static str,
    element_name: Option<String>,
    role: Option<&'static str>,
}

/// Build a comfortable ring rectangle around a single screen point `(cx, cy)`.
pub fn point_to_ring_rect(cx: i32, cy: i32, scale: f64) -> Rect {
    let s = if scale > 0.0 && scale.is_finite() { scale } else { 1.0 };
    let size = (48.0 * s).round() as i32;
    Rect {
        x: cx - size / 2,
        y: cy - size / 2,
        width: size,
        height: size,
    }
}

/// True when physical cursor position `(px, py)` falls inside `rect` expanded by `margin`.
pub fn point_in_target_rect(rect: Rect, px: i32, py: i32, margin: i32) -> bool {
    if rect.width <= 0 || rect.height <= 0 {
        return false;
    }
    let m = margin.max(0);
    px >= rect.x - m
        && px <= rect.x + rect.width + m
        && py >= rect.y - m
        && py <= rect.y + rect.height + m
}

/// Edge-triggered user input event observed while waiting for a guided click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitInputEvent {
    None,
    ClickedInside,
    ClickedOutside,
    EscPressed,
}

/// Pure edge-detector for left mouse click and Escape key during `guide_step`.
/// Ignores buttons already held down when the step begins.
#[derive(Debug, Clone, Copy)]
pub struct ClickWaitState {
    was_lbutton_down: bool,
    was_esc_down: bool,
}

impl ClickWaitState {
    pub fn new(lbutton_down: bool, esc_down: bool) -> Self {
        ClickWaitState {
            was_lbutton_down: lbutton_down,
            was_esc_down: esc_down,
        }
    }

    pub fn poll(
        &mut self,
        rect: Rect,
        margin: i32,
        px: i32,
        py: i32,
        lbutton_down: bool,
        esc_down: bool,
    ) -> WaitInputEvent {
        let esc_edge = esc_down && !self.was_esc_down;
        let click_edge = lbutton_down && !self.was_lbutton_down;
        self.was_esc_down = esc_down;
        self.was_lbutton_down = lbutton_down;

        if esc_edge {
            return WaitInputEvent::EscPressed;
        }
        if click_edge {
            if point_in_target_rect(rect, px, py, margin) {
                return WaitInputEvent::ClickedInside;
            }
            return WaitInputEvent::ClickedOutside;
        }
        WaitInputEvent::None
    }
}

fn target_window_hwnd(
    window_query: Option<&str>,
    island_hwnd: i64,
    history: &winmgr::ForegroundHistory,
) -> Option<i64> {
    if let Some(q) = window_query.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(w) = winmgr::find_windows(q).into_iter().next() {
            return Some(w.hwnd);
        }
    }
    #[cfg(windows)]
    let own_pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
    #[cfg(not(windows))]
    let own_pid = std::process::id();

    if let Some(fg) = winmgr::foreground_window() {
        if fg.hwnd != island_hwnd && fg.pid != own_pid {
            return Some(fg.hwnd);
        }
    }
    history
        .recent()
        .into_iter()
        .find(|e| e.hwnd != island_hwnd)
        .map(|e| e.hwnd)
        .or_else(|| winmgr::list_windows().into_iter().next().map(|w| w.hwnd))
}

fn resolve_coords(
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
    space: &str,
    capture_id: Option<u64>,
) -> AppResult<ResolvedTarget> {
    let (Some(x), Some(y)) = (x, y) else {
        return Err(AppError::invalid("Укажите целевой элемент (element) или координаты (x, y)"));
    };
    let rect = match (w, h) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => screen::to_screen_rect(x, y, w, h, space, capture_id)?,
        _ => {
            let (sx, sy) = screen::to_screen_point(x, y, space, capture_id)?;
            let scale = display::monitor_for_point(sx, sy).map(|m| m.scale).unwrap_or(1.0);
            point_to_ring_rect(sx, sy, scale)
        }
    };
    Ok(ResolvedTarget {
        rect,
        source: "coords",
        element_name: None,
        role: None,
    })
}

async fn resolve_target(
    ctx: &ToolCtx,
    element: Option<ElementTarget>,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
    space: String,
    capture_id: Option<u64>,
) -> AppResult<ResolvedTarget> {
    let has_coords = x.is_some() && y.is_some();
    if let Some(el) = element.filter(ElementTarget::is_present) {
        let island_hwnd = ctx.state().island.hwnd() as i64;
        let history = ctx.state().history.clone();
        let uia_res = platform::blocking(move || {
            if let (Some(sid), Some(eid)) = (el.snapshot_id, el.element_id) {
                let m = uia::element_rect(sid, eid)?;
                return Ok(ResolvedTarget {
                    rect: m.rect,
                    source: "uia",
                    element_name: if m.name.is_empty() { None } else { Some(m.name) },
                    role: Some(m.role),
                });
            }
            let hwnd = target_window_hwnd(el.window.as_deref(), island_hwnd, &history)
                .ok_or_else(|| AppError::not_found("Не найдено целевое окно для поиска элемента"))?;
            let q = uia::ElementQuery {
                name: el.name,
                control_type: el.control_type,
                automation_id: el.automation_id,
            };
            let m = uia::find_element(winmgr::to_hwnd(hwnd), &q)?;
            Ok(ResolvedTarget {
                rect: m.rect,
                source: "uia",
                element_name: if m.name.is_empty() { None } else { Some(m.name) },
                role: Some(m.role),
            })
        })
        .await;

        match uia_res {
            Ok(resolved) => return Ok(resolved),
            Err(_) if has_coords => {
                // Fall back to explicit coordinates when UIA did not expose the control.
                return resolve_coords(x, y, w, h, &space, capture_id);
            }
            Err(e) => return Err(e),
        }
    }

    resolve_coords(x, y, w, h, &space, capture_id)
}

fn should_speak_guide(ctx: &ToolCtx) -> bool {
    let settings = ctx.state().store.settings();
    if settings.voice.auto_speak_replies {
        return true;
    }
    ctx.app
        .try_state::<Arc<crate::voice::VoiceManager>>()
        .is_some_and(|v| v.is_active())
}

#[cfg(windows)]
fn read_live_input() -> Option<(i32, i32, bool, bool)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut pt = POINT::default();
    if unsafe { GetCursorPos(&mut pt) }.is_err() {
        return None;
    }
    let lbutton = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
    let esc = unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) } < 0;
    Some((pt.x, pt.y, lbutton, esc))
}

#[cfg(not(windows))]
fn read_live_input() -> Option<(i32, i32, bool, bool)> {
    None
}

enum WaitOutcome {
    Clicked(i32, i32),
    EscCancelled,
    UiCancelled,
    TurnCancelled,
    Timeout,
}

async fn wait_for_click(ctx: &ToolCtx, rect: Rect, margin: i32, timeout: Duration) -> AppResult<WaitOutcome> {
    let app = ctx.app.clone();
    let cancel = ctx.cancel.clone();
    platform::blocking(move || {
        let (init_l, init_esc) = read_live_input().map(|(_, _, l, e)| (l, e)).unwrap_or((false, false));
        let mut detector = ClickWaitState::new(init_l, init_esc);
        let deadline = Instant::now() + timeout;

        loop {
            if cancel.is_cancelled() {
                return Ok(WaitOutcome::TurnCancelled);
            }
            if overlay::is_guide_cancelled() {
                return Ok(WaitOutcome::UiCancelled);
            }
            if Instant::now() >= deadline {
                return Ok(WaitOutcome::Timeout);
            }
            if let Some((px, py, lbutton, esc)) = read_live_input() {
                match detector.poll(rect, margin, px, py, lbutton, esc) {
                    WaitInputEvent::EscPressed => return Ok(WaitOutcome::EscCancelled),
                    WaitInputEvent::ClickedInside => return Ok(WaitOutcome::Clicked(px, py)),
                    WaitInputEvent::ClickedOutside => {
                        // Brief grace window in case the click hit the overlay's own cancel button.
                        std::thread::sleep(Duration::from_millis(45));
                        if overlay::is_guide_cancelled() {
                            return Ok(WaitOutcome::UiCancelled);
                        }
                        if cancel.is_cancelled() {
                            return Ok(WaitOutcome::TurnCancelled);
                        }
                        overlay::pulse(&app);
                    }
                    WaitInputEvent::None => {}
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    })
    .await
}

async fn screen_point(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: PointArgs = parse(args)?;
    let target = resolve_target(&ctx, a.element, a.x, a.y, a.w, a.h, a.space, a.capture_id).await?;
    let seconds = a.seconds.unwrap_or(DEFAULT_POINT_SECONDS).clamp(1.0, 60.0);
    let tone = a.tone.as_deref().unwrap_or("amber");
    let r = target.rect;

    let mut items = Vec::new();
    items.push(json!({
        "kind": "ring",
        "id": format!("point-ring-{}", uuid::Uuid::new_v4().simple()),
        "x": r.x,
        "y": r.y,
        "w": r.width,
        "h": r.height,
        "label": a.label,
        "tone": tone,
        "seconds": seconds,
    }));
    if a.arrow {
        items.push(json!({
            "kind": "arrow",
            "id": format!("point-arrow-{}", uuid::Uuid::new_v4().simple()),
            "to": { "x": r.x + r.width / 2, "y": r.y + r.height / 2 },
            "targetRect": { "x": r.x, "y": r.y, "w": r.width, "h": r.height },
            "tone": tone,
            "seconds": seconds,
        }));
    }
    if let Some(text) = a.caption.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        items.push(json!({
            "kind": "caption",
            "id": format!("point-caption-{}", uuid::Uuid::new_v4().simple()),
            "text": text,
            "targetRect": { "x": r.x, "y": r.y, "w": r.width, "h": r.height },
            "tone": tone,
            "seconds": seconds,
        }));
    }

    overlay::draw(&ctx.app, items, a.replace)?;
    let what = a
        .label
        .as_deref()
        .or(target.element_name.as_deref())
        .unwrap_or("область на экране");
    Ok(ToolOutput::ok(
        format!("Подсветил: {what}"),
        json!({
            "rect": r,
            "source": target.source,
            "element": target.element_name,
            "role": target.role,
            "seconds": seconds,
        }),
    ))
}

async fn screen_clear(ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    overlay::clear(&ctx.app)?;
    Ok(ToolOutput::ok("Подсветка экрана убрана", json!({ "cleared": true })))
}

async fn guide_step(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: GuideStepArgs = parse(args)?;
    let text = a.text.trim().to_string();
    if text.is_empty() {
        return Err(AppError::invalid("Текст шага обучения (text) не может быть пустым"));
    }

    let has_target = a.element.as_ref().is_some_and(ElementTarget::is_present) || (a.x.is_some() && a.y.is_some());
    let wait_mode = a.wait.trim().to_lowercase();
    let is_final = a.done || matches!((a.step, a.total), (Some(s), Some(t)) if t > 0 && s >= t);

    // Pure completion step without a control to click: show «Готово» briefly and clear.
    if a.done && !has_target {
        overlay::set_guide_active(false);
        let items = vec![json!({
            "kind": "caption",
            "id": "guide-done",
            "text": text,
            "badge": "Готово",
            "step": a.step,
            "total": a.total,
            "tone": "ok",
            "seconds": 2.4,
        })];
        overlay::draw(&ctx.app, items, true)?;
        if should_speak_guide(&ctx) {
            crate::hub::request_tts(&ctx.app, &text, None, None, true);
        }
        return Ok(ToolOutput::ok("Обучение завершено", json!({ "status": "done", "completed": true })));
    }

    let target = if has_target {
        Some(resolve_target(&ctx, a.element, a.x, a.y, a.w, a.h, a.space, a.capture_id).await?)
    } else if wait_mode == "click" {
        return Err(AppError::invalid("Для guide_step с wait=\"click\" нужно указать element или координаты (x, y)"));
    } else {
        None
    };

    if wait_mode == "none" {
        let seconds = if is_final { 3.5 } else { 8.0 };
        let mut items = Vec::new();
        if let Some(ref t) = target {
            let r = t.rect;
            items.push(json!({
                "kind": "ring",
                "id": "guide-ring",
                "x": r.x,
                "y": r.y,
                "w": r.width,
                "h": r.height,
                "label": a.label,
                "tone": if is_final { "ok" } else { "amber" },
                "seconds": seconds,
            }));
            items.push(json!({
                "kind": "arrow",
                "id": "guide-arrow",
                "to": { "x": r.x + r.width / 2, "y": r.y + r.height / 2 },
                "targetRect": { "x": r.x, "y": r.y, "w": r.width, "h": r.height },
                "tone": if is_final { "ok" } else { "amber" },
                "seconds": seconds,
            }));
        }
        items.push(json!({
            "kind": "caption",
            "id": "guide-caption",
            "text": text,
            "badge": if is_final { Value::String("Готово".into()) } else { Value::Null },
            "step": a.step,
            "total": a.total,
            "targetRect": target.as_ref().map(|t| json!({ "x": t.rect.x, "y": t.rect.y, "w": t.rect.width, "h": t.rect.height })),
            "tone": if is_final { "ok" } else { "amber" },
            "seconds": seconds,
        }));
        overlay::set_guide_active(!is_final);
        overlay::draw(&ctx.app, items, true)?;
        if should_speak_guide(&ctx) {
            crate::hub::request_tts(&ctx.app, &text, None, None, true);
        }
        return Ok(ToolOutput::ok(
            if is_final { "Готово" } else { "Шаг показан" },
            json!({
                "status": if is_final { "done" } else { "shown" },
                "rect": target.as_ref().map(|t| t.rect),
                "source": target.as_ref().map(|t| t.source),
            }),
        ));
    }

    // wait == "click": keep items on screen until user clicks inside target (+ margin) or cancels.
    let target = target.expect("checked above");
    let r = target.rect;
    let mon_scale = display::monitor_for_point(r.x + r.width / 2, r.y + r.height / 2)
        .map(|m| m.scale)
        .unwrap_or(1.0);
    let margin_px = (a.margin.unwrap_or(DEFAULT_CLICK_MARGIN_CSS).clamp(0.0, 64.0) * mon_scale).round() as i32;
    let timeout = Duration::from_secs(a.timeout_seconds.unwrap_or(DEFAULT_GUIDE_TIMEOUT_SECS).clamp(5, 180));

    overlay::reset_guide_cancel();
    overlay::set_guide_active(true);

    let items = vec![
        json!({
            "kind": "ring",
            "id": "guide-ring",
            "x": r.x,
            "y": r.y,
            "w": r.width,
            "h": r.height,
            "label": a.label,
            "tone": "amber",
            "persistent": true,
        }),
        json!({
            "kind": "arrow",
            "id": "guide-arrow",
            "to": { "x": r.x + r.width / 2, "y": r.y + r.height / 2 },
            "targetRect": { "x": r.x, "y": r.y, "w": r.width, "h": r.height },
            "tone": "amber",
            "persistent": true,
        }),
        json!({
            "kind": "caption",
            "id": "guide-caption",
            "text": text,
            "step": a.step,
            "total": a.total,
            "targetRect": { "x": r.x, "y": r.y, "w": r.width, "h": r.height },
            "hint": "Нажмите в подсвеченную область",
            "tone": "amber",
            "persistent": true,
            "closable": true,
        }),
    ];
    overlay::draw(&ctx.app, items, true)?;

    if should_speak_guide(&ctx) {
        crate::hub::request_tts(&ctx.app, &text, None, None, true);
    }

    match wait_for_click(&ctx, r, margin_px, timeout).await? {
        WaitOutcome::Clicked(cx, cy) => {
            // Let the clicked window open its menu or transition state before the next ui_inspect.
            tokio::select! {
                _ = ctx.cancel.cancelled() => {
                    let _ = overlay::clear(&ctx.app);
                    return Err(AppError::Cancelled);
                }
                _ = tokio::time::sleep(Duration::from_millis(160)) => {}
            }
            if is_final {
                let done_items = vec![json!({
                    "kind": "caption",
                    "id": "guide-done",
                    "text": "Все шаги выполнены",
                    "badge": "Готово",
                    "step": a.total,
                    "total": a.total,
                    "tone": "ok",
                    "seconds": 1.8,
                })];
                let _ = overlay::draw(&ctx.app, done_items, true);
                tokio::select! {
                    _ = ctx.cancel.cancelled() => {}
                    _ = tokio::time::sleep(Duration::from_millis(1600)) => {}
                }
                let _ = overlay::clear(&ctx.app);
                Ok(ToolOutput::ok(
                    "Готово — обучение завершено",
                    json!({
                        "status": "clicked",
                        "completed": true,
                        "clickPoint": { "x": cx, "y": cy },
                        "rect": r,
                        "source": target.source,
                    }),
                ))
            } else {
                Ok(ToolOutput::ok(
                    "Пользователь нажал нужный элемент",
                    json!({
                        "status": "clicked",
                        "completed": false,
                        "clickPoint": { "x": cx, "y": cy },
                        "rect": r,
                        "source": target.source,
                        "hint": "Re-inspect the foreground window (ui_inspect or screen_capture) before calling the next guide_step.",
                    }),
                ))
            }
        }
        WaitOutcome::EscCancelled | WaitOutcome::UiCancelled => {
            let _ = overlay::clear(&ctx.app);
            Ok(ToolOutput::ok(
                "Обучение отменено пользователем",
                json!({ "status": "cancelled" }),
            ))
        }
        WaitOutcome::TurnCancelled => {
            let _ = overlay::clear(&ctx.app);
            Err(AppError::Cancelled)
        }
        WaitOutcome::Timeout => {
            let _ = overlay::clear(&ctx.app);
            Ok(ToolOutput::ok(
                "Время ожидания клика истекло",
                json!({ "status": "timeout" }),
            ))
        }
    }
}

fn element_schema() -> Value {
    json!({
        "type": "object",
        "description": "Preferred: locate a live control via Windows UI Automation in the foreground window (or named `window`). Pass `name` / `control_type` / `automation_id`, or `snapshot_id` + `element_id` from a recent ui_inspect.",
        "properties": {
            "name": { "type": "string", "description": "Visible control label/name" },
            "control_type": { "type": "string", "description": "Control role, e.g. button, menuitem, tabitem, edit, link, checkbox" },
            "automation_id": { "type": "string", "description": "UIA AutomationId if known" },
            "snapshot_id": { "type": "integer", "description": "Snapshot ID from ui_inspect" },
            "element_id": { "type": "integer", "description": "Element [id] from ui_inspect" },
            "window": { "type": "string", "description": "Optional window title/process substring" }
        },
        "additionalProperties": false
    })
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "screen_point",
            description: "Highlight a control or area on the user's real screen with a glowing amber ring, arrow and optional \
                label for N seconds (default 8 s). Use for «покажи где…» / «где кнопка…». Prefer `element` (UIA lookup \
                in the foreground window — exact at any DPI); fall back to `x, y` (and optional `w, h`) in the last \
                screenshot (`space=\"image\"`, pass `capture_id`) or physical screen pixels (`space=\"screen\"`).",
            schema: object(
                json!({
                    "element": element_schema(),
                    "x": { "type": "number", "description": "Top-left X when w/h given, or center X for a point" },
                    "y": { "type": "number", "description": "Top-left Y when w/h given, or center Y for a point" },
                    "w": { "type": "number", "description": "Optional box width" },
                    "h": { "type": "number", "description": "Optional box height" },
                    "space": { "type": "string", "enum": ["image", "screen"] },
                    "capture_id": { "type": "integer" },
                    "label": { "type": "string", "description": "Short Russian tag near the ring" },
                    "caption": { "type": "string", "description": "Optional explanation pill" },
                    "arrow": { "type": "boolean", "default": true },
                    "tone": { "type": "string", "enum": ["amber", "ok", "warn", "danger"] },
                    "seconds": { "type": "number", "minimum": 1, "maximum": 60 },
                    "replace": { "type": "boolean", "default": true }
                }),
                &[],
            ),
            risk: low,
            title: |a| {
                let lbl = str_arg(a, "label");
                if !lbl.is_empty() {
                    format!("Показать: «{lbl}»")
                } else if let Some(n) = a.pointer("/element/name").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    format!("Показать: «{n}»")
                } else {
                    "Показать на экране".into()
                }
            },
            timeout: Duration::from_secs(10),
            parallel_safe: false,
            handler: handler(screen_point),
        },
        Tool {
            name: "screen_clear",
            description: "Remove all active highlights, arrows, captions and cards from the screen overlay.",
            schema: object(json!({}), &[]),
            risk: low,
            title: |_| "Убрать подсветку экрана".into(),
            timeout: Duration::from_secs(5),
            parallel_safe: false,
            handler: handler(screen_clear),
        },
        Tool {
            name: "guide_step",
            description: "Interactive teach mode («научи меня» / «как сделать…»): highlight one control with a ring, arrow \
                and caption («Шаг N из M · …»), speak the instruction if voice replies are on, and when wait=\"click\" \
                (default) wait for the user's own left click there (Esc cancels, ~120 s timeout). Never clicks for the \
                user. Prefer `element` (UIA lookup) over screenshot coords. After `status: \"clicked\"`, re-inspect the \
                window before calling the next guide_step because menus/dialogs appear after clicks. When step==total \
                (or done=true), shows «Готово» and clears.",
            schema: object(
                json!({
                    "text": { "type": "string", "description": "Short Russian instruction for this step" },
                    "step": { "type": "integer", "minimum": 1 },
                    "total": { "type": "integer", "minimum": 1 },
                    "element": element_schema(),
                    "x": { "type": "number" },
                    "y": { "type": "number" },
                    "w": { "type": "number" },
                    "h": { "type": "number" },
                    "space": { "type": "string", "enum": ["image", "screen"] },
                    "capture_id": { "type": "integer" },
                    "label": { "type": "string" },
                    "wait": { "type": "string", "enum": ["click", "none"], "default": "click" },
                    "done": { "type": "boolean", "default": false },
                    "margin": { "type": "number", "minimum": 0, "maximum": 64 },
                    "timeout_seconds": { "type": "integer", "minimum": 5, "maximum": 180 }
                }),
                &["text"],
            ),
            risk: low,
            title: |a| match (a.get("step").and_then(Value::as_u64), a.get("total").and_then(Value::as_u64)) {
                (Some(s), Some(t)) => format!("Подсказка: шаг {s} из {t}"),
                _ => "Подсказка на экране".into(),
            },
            timeout: Duration::from_secs(135),
            parallel_safe: false,
            handler: handler(guide_step),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Risk;

    #[test]
    fn point_to_ring_rect_centers_and_scales_with_dpi() {
        assert_eq!(point_to_ring_rect(200, 100, 1.0), Rect { x: 176, y: 76, width: 48, height: 48 });
        assert_eq!(point_to_ring_rect(200, 100, 1.5), Rect { x: 164, y: 64, width: 72, height: 72 });
        assert_eq!(point_to_ring_rect(200, 100, 2.0), Rect { x: 152, y: 52, width: 96, height: 96 });
    }

    #[test]
    fn click_in_target_respects_margin_and_edges() {
        let r = Rect { x: 100, y: 200, width: 80, height: 32 };
        assert!(point_in_target_rect(r, 140, 216, 0));
        assert!(point_in_target_rect(r, 100, 200, 0));
        assert!(point_in_target_rect(r, 180, 232, 0));
        assert!(!point_in_target_rect(r, 95, 216, 0));
        assert!(point_in_target_rect(r, 90, 216, 12));
        assert!(point_in_target_rect(r, 192, 244, 12));
        assert!(!point_in_target_rect(r, 193, 216, 12));
        assert!(!point_in_target_rect(Rect { x: 0, y: 0, width: 0, height: 10 }, 0, 5, 10));
    }

    #[test]
    fn click_wait_state_is_edge_triggered_and_ignores_held_keys() {
        let r = Rect { x: 100, y: 100, width: 60, height: 30 };
        // Mouse already pressed when step starts: must NOT trigger until released and pressed again.
        let mut s = ClickWaitState::new(true, false);
        assert_eq!(s.poll(r, 10, 120, 115, true, false), WaitInputEvent::None);
        assert_eq!(s.poll(r, 10, 120, 115, false, false), WaitInputEvent::None);

        // Fresh click outside target -> ClickedOutside.
        assert_eq!(s.poll(r, 10, 400, 400, true, false), WaitInputEvent::ClickedOutside);
        // Holding mouse down does not re-fire.
        assert_eq!(s.poll(r, 10, 120, 115, true, false), WaitInputEvent::None);
        assert_eq!(s.poll(r, 10, 120, 115, false, false), WaitInputEvent::None);

        // Fresh click inside target -> ClickedInside.
        assert_eq!(s.poll(r, 10, 125, 115, true, false), WaitInputEvent::ClickedInside);

        // Fresh Escape press -> EscPressed (takes precedence).
        assert_eq!(s.poll(r, 10, 125, 115, false, true), WaitInputEvent::EscPressed);
        assert_eq!(s.poll(r, 10, 125, 115, false, true), WaitInputEvent::None);
    }

    #[test]
    fn overlay_tools_metadata_is_valid() {
        let list = tools();
        let names: Vec<&str> = list.iter().map(|t| t.name).collect();
        assert_eq!(names, ["screen_point", "screen_clear", "guide_step"]);
        for t in &list {
            assert_eq!(t.schema["type"], "object");
            assert_eq!(t.schema["additionalProperties"], false);
            assert_eq!((t.risk)(&json!({})), Risk::Low);
            assert!(!(t.title)(&json!({"label": "Сохранить", "step": 2, "total": 4})).is_empty());
        }
        let guide = list.iter().find(|t| t.name == "guide_step").unwrap();
        assert!(guide.timeout.as_secs() >= 120);
    }
}
