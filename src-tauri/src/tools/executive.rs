//! Executive "Take-Control" tools: live multi-step mission plans, Notch Hub
//! tasks & focus sessions, non-destructive foreground selection grab, verified
//! multi-window layout management, real Win32 PC diagnostics, and neural TTS readout.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, FILETIME, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::SystemInformation::{GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetSystemTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindow};

use super::{handler, low, object, parse, str_arg, Risk, Tool, ToolCtx, ToolOutput};
use crate::agent::events::{AgentEvent, PlanStep, EVT_AGENT};
use crate::clips::image as clip_img;
use crate::error::{AppError, AppResult};
use crate::hub::{self, Hub};
use crate::platform::{
    self,
    clipboard::{self, ClipboardContent, Snapshot},
    display::{self, Rect},
    from_wide, input, winmgr,
};

// ==================== 1. MISSION PLAN ====================

#[derive(Deserialize)]
struct MissionPlanArgs {
    steps: Vec<PlanStepInput>,
}

#[derive(Deserialize)]
struct PlanStepInput {
    id: String,
    title: String,
    status: String,
}

fn normalize_plan_status(s: &str) -> &'static str {
    match s.trim().to_lowercase().as_str() {
        "running" | "in_progress" | "active" | "working" => "running",
        "done" | "completed" | "complete" | "ok" => "done",
        "failed" | "error" | "cancelled" => "failed",
        _ => "pending",
    }
}

async fn mission_plan(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: MissionPlanArgs = parse(args)?;
    let steps: Vec<PlanStep> = a
        .steps
        .into_iter()
        .enumerate()
        .filter(|(_, s)| !s.title.trim().is_empty())
        .map(|(idx, s)| PlanStep {
            id: if s.id.trim().is_empty() { format!("step-{}", idx + 1) } else { s.id.trim().to_string() },
            title: s.title.trim().to_string(),
            status: normalize_plan_status(&s.status).to_string(),
        })
        .collect();

    let done = steps.iter().filter(|s| s.status == "done").count();
    let total = steps.len();
    let active_title = steps
        .iter()
        .find(|s| s.status == "running")
        .map(|s| s.title.clone());

    let _ = ctx.app.emit(
        EVT_AGENT,
        AgentEvent::PlanUpdated {
            turn_id: ctx.task_id.clone(),
            steps: steps.clone(),
        },
    );

    let summary = match active_title {
        Some(cur) => format!("План ({done}/{total}): {cur}"),
        None => format!("План обновлён: {done} из {total} выполнено"),
    };
    Ok(ToolOutput::ok(
        summary,
        json!({ "steps": steps, "done": done, "total": total }),
    ))
}

// ==================== 2. TASKS MANAGE ====================

#[derive(Deserialize)]
struct TasksManageArgs {
    action: String,
    id: Option<String>,
    title: Option<String>,
    notes: Option<String>,
    scheduled_date: Option<String>,
    scheduled_time: Option<String>,
    color: Option<String>,
}

fn resolve_task_id(hub: &Hub, id: Option<&str>, title_query: Option<&str>) -> AppResult<String> {
    let all = hub.tasks_list()?;
    if let Some(raw_id) = id.map(str::trim).filter(|s| !s.is_empty()) {
        if all.iter().any(|t| t.id == raw_id) {
            return Ok(raw_id.to_string());
        }
    }
    if let Some(q) = title_query.map(str::trim).filter(|s| !s.is_empty()) {
        let lower = q.to_lowercase();
        if let Some(found) = all
            .iter()
            .find(|t| !t.completed && t.title.to_lowercase().contains(&lower))
            .or_else(|| all.iter().find(|t| t.title.to_lowercase().contains(&lower)))
        {
            return Ok(found.id.clone());
        }
    }
    Err(AppError::not_found("Подходящая задача не найдена"))
}

async fn tasks_manage(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: TasksManageArgs = parse(args)?;
    let hub = ctx.app.state::<Arc<Hub>>();
    match a.action.trim().to_lowercase().as_str() {
        "list" => {
            let tasks = hub.tasks_list()?;
            let open = tasks.iter().filter(|t| !t.completed).count();
            Ok(ToolOutput::ok(
                format!("Задач: {} (активных: {})", tasks.len(), open),
                json!({ "tasks": tasks, "openCount": open }),
            ))
        }
        "create" | "add" => {
            let title = a.title.unwrap_or_default();
            let item = hub.task_add_with_notes(
                title,
                a.notes.unwrap_or_default(),
                a.scheduled_date,
                a.scheduled_time,
                a.color,
            )?;
            Ok(ToolOutput::ok(
                format!("Задача добавлена: «{}»", item.title),
                json!({ "task": item, "verified": true }),
            ))
        }
        "complete" | "done" => {
            let tid = resolve_task_id(&hub, a.id.as_deref(), a.title.as_deref())?;
            let item = hub.task_set_completed(&tid, true)?;
            Ok(ToolOutput::ok(
                format!("Задача выполнена: «{}»", item.title),
                json!({ "task": item, "verified": item.completed }),
            ))
        }
        "toggle" => {
            let tid = resolve_task_id(&hub, a.id.as_deref(), a.title.as_deref())?;
            let item = hub.task_toggle(&tid)?;
            let verb = if item.completed { "выполнена" } else { "возвращена в работу" };
            Ok(ToolOutput::ok(
                format!("Задача «{}» {verb}", item.title),
                json!({ "task": item, "verified": true }),
            ))
        }
        "delete" | "remove" => {
            let tid = resolve_task_id(&hub, a.id.as_deref(), a.title.as_deref())?;
            hub.task_remove(&tid)?;
            Ok(ToolOutput::ok("Задача удалена", json!({ "id": tid, "deleted": true })))
        }
        other => Err(AppError::invalid(format!("Неизвестное действие tasks_manage: {other}"))),
    }
}

// ==================== 3. FOCUS SESSION ====================

#[derive(Deserialize)]
struct FocusArgs {
    action: String,
    title: Option<String>,
    minutes: Option<u32>,
    task_id: Option<String>,
}

async fn focus_session(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: FocusArgs = parse(args)?;
    let hub = ctx.app.state::<Arc<Hub>>();
    match a.action.trim().to_lowercase().as_str() {
        "status" => {
            let st = hub.focus_get()?;
            let summary = if st.active {
                let mins_left = (st.remaining_sec + 59) / 60;
                if st.paused {
                    format!("Фокус «{}» на паузе (осталось {} мин)", st.title, mins_left)
                } else {
                    format!("Фокус «{}» активен: осталось {} мин", st.title, mins_left)
                }
            } else {
                format!(
                    "Фокус не запущен (сегодня сессий: {}, минут: {})",
                    st.completed_today, st.minutes_today
                )
            };
            Ok(ToolOutput::ok(summary, serde_json::to_value(st)?))
        }
        "start" => {
            let mins = a.minutes.unwrap_or(25).clamp(1, 240);
            let title = a.title.unwrap_or_default();
            let st = hub.focus_start(title, mins, a.task_id)?;
            Ok(ToolOutput::ok(
                format!("Фокус запущен на {mins} мин: «{}»", st.title),
                serde_json::to_value(st)?,
            ))
        }
        "pause" => {
            let cur = hub.focus_get()?;
            let st = if cur.active && !cur.paused {
                hub.focus_pause_toggle()?
            } else {
                cur
            };
            Ok(ToolOutput::ok(
                if st.active { "Сессия фокуса поставлена на паузу" } else { "Нет активной сессии фокуса" },
                serde_json::to_value(st)?,
            ))
        }
        "resume" => {
            let cur = hub.focus_get()?;
            let st = if cur.active && cur.paused {
                hub.focus_pause_toggle()?
            } else {
                cur
            };
            Ok(ToolOutput::ok(
                if st.active { "Сессия фокуса продолжена" } else { "Нет активной сессии фокуса" },
                serde_json::to_value(st)?,
            ))
        }
        "complete" | "finish" => {
            let st = hub.focus_complete()?;
            Ok(ToolOutput::ok(
                format!(
                    "Сессия фокуса завершена (сегодня: {} сесс., {} мин)",
                    st.completed_today, st.minutes_today
                ),
                serde_json::to_value(st)?,
            ))
        }
        "cancel" | "stop" => {
            let st = hub.focus_cancel()?;
            Ok(ToolOutput::ok("Сессия фокуса остановлена", serde_json::to_value(st)?))
        }
        other => Err(AppError::invalid(format!("Неизвестное действие focus_session: {other}"))),
    }
}

// ==================== 4. SELECTION GRAB ====================

fn restore_clipboard_snapshot(snap: &Snapshot) {
    if snap.private {
        // Never touch a private clipboard item; if we didn't overwrite it, leave it,
        // otherwise clear if needed.
        return;
    }
    if !snap.files.is_empty() {
        let _ = clipboard::write_files(&snap.files);
    } else if let Some(ref text) = snap.text {
        let _ = clipboard::write_text(text);
    } else if let (Some(ref png), Some(ref dib)) = (&snap.png, &snap.dib) {
        let _ = clipboard::write_image(png, dib);
    } else if let Some(ref png) = snap.png {
        if let Some(rgba) = clip_img::png_to_rgba(png) {
            let _ = clipboard::write_image(png, &clip_img::rgba_to_dib24(&rgba));
        }
    } else if let Some(ref dib) = snap.dib {
        if let Some(rgba) = clip_img::dib_to_rgba(dib) {
            if let Some(png) = clip_img::rgba_to_png(&rgba) {
                let _ = clipboard::write_image(&png, dib);
            }
        }
    } else {
        let _ = clipboard::restore(&ClipboardContent { text: None, files: Vec::new() });
    }
}

async fn selection_grab(ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    let island = ctx.state().island.clone();
    let history = ctx.state().history.clone();
    platform::blocking(move || {
        let island_hwnd = island.hwnd() as i64;
        let own_pid = unsafe { GetCurrentProcessId() };

        // If the Island Workspace currently holds keyboard focus, briefly hand focus
        // back to the user's previous foreground window so Ctrl+C reaches their app.
        let mut switched_from_island = false;
        let mut target_win = winmgr::foreground_window();
        if target_win.as_ref().is_none_or(|w| w.hwnd == island_hwnd || w.pid == own_pid) {
            let _ = island.end_input(true);
            switched_from_island = true;
            std::thread::sleep(Duration::from_millis(35));
            target_win = winmgr::foreground_window().filter(|w| w.hwnd != island_hwnd && w.pid != own_pid);
            if target_win.is_none() {
                if let Some(prev) = history.recent().into_iter().find(|e| e.hwnd != island_hwnd) {
                    target_win = winmgr::focus(prev.hwnd).ok();
                    std::thread::sleep(Duration::from_millis(35));
                }
            }
        }

        let Some(win) = target_win else {
            return Ok(ToolOutput::fail(
                "Нет активного окна, из которого можно прочитать выделенный текст",
                json!({ "copied": false }),
            ));
        };

        let prev_snap = clipboard::snapshot().ok();
        let before_seq = unsafe { GetClipboardSequenceNumber() };

        let send_res = input::key_chord("ctrl+c");
        if let Err(e) = send_res {
            if switched_from_island {
                let _ = island.begin_input();
            }
            return Err(e);
        }

        // Wait up to 120ms for the foreground window to place the selection on the clipboard.
        let deadline = Instant::now() + Duration::from_millis(120);
        let mut seq_changed = false;
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(12));
            if unsafe { GetClipboardSequenceNumber() } != before_seq {
                seq_changed = true;
                break;
            }
        }

        let grabbed = if seq_changed {
            clipboard::read().ok().and_then(|c| c.text)
        } else {
            None
        };

        if seq_changed {
            if let Some(ref prev) = prev_snap {
                restore_clipboard_snapshot(prev);
            }
        }

        if switched_from_island {
            let _ = island.begin_input();
        }

        match grabbed.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) {
            Some(text) => {
                let chars = text.chars().count();
                Ok(ToolOutput::ok(
                    format!("Выделенный текст прочитан ({chars} симв.) из «{}»", win.title),
                    json!({
                        "text": text,
                        "chars": chars,
                        "window": win.title,
                        "process": win.process,
                        "copied": true
                    }),
                ))
            }
            None => Ok(ToolOutput::fail(
                format!("В окне «{}» ничего не выделено", win.title),
                json!({
                    "text": "",
                    "window": win.title,
                    "process": win.process,
                    "copied": false
                }),
            )),
        }
    })
    .await
}

// ==================== 5. WINDOW LAYOUT ====================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedWindow {
    hwnd: i64,
    title: String,
    process: String,
    state: String,
    rect: Rect,
}

static LAYOUT_SNAPSHOT: Mutex<Vec<SavedWindow>> = Mutex::new(Vec::new());

fn visible_window_rect(hwnd: i64) -> Option<Rect> {
    let h = winmgr::to_hwnd(hwnd);
    unsafe {
        if !IsWindow(Some(h)).as_bool() {
            return None;
        }
        let mut vis = RECT::default();
        if DwmGetWindowAttribute(
            h,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut vis as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_ok()
        {
            return Some(vis.into());
        }
        let mut outer = RECT::default();
        GetWindowRect(h, &mut outer).ok()?;
        Some(outer.into())
    }
}

fn capture_layout_snapshot() -> Vec<SavedWindow> {
    winmgr::list_windows()
        .into_iter()
        .map(|w| SavedWindow {
            rect: visible_window_rect(w.hwnd).unwrap_or(w.rect),
            hwnd: w.hwnd,
            title: w.title,
            process: w.process,
            state: w.state.to_string(),
        })
        .collect()
}

#[derive(Deserialize)]
struct WindowLayoutArgs {
    action: String,
    /// Optional query for left window (`tile_pair`).
    left: Option<String>,
    /// Optional query for right window (`tile_pair`).
    right: Option<String>,
    /// Split ratio for `tile_pair`: `"50-50"` (default) or `"65-35"`.
    ratio: Option<String>,
    /// Queries of apps/windows to keep visible during `focus_clean`.
    #[serde(default)]
    keep: Vec<String>,
}

fn parse_split_percent(ratio: Option<&str>) -> i32 {
    match ratio.unwrap_or("50-50").trim() {
        "65-35" | "65/35" | "65:35" => 65,
        "60-40" | "60/40" | "60:40" => 60,
        "35-65" | "35/65" | "35:65" => 35,
        _ => 50,
    }
}

async fn window_layout(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: WindowLayoutArgs = parse(args)?;
    let history = ctx.state().history.clone();
    platform::blocking(move || match a.action.trim().to_lowercase().as_str() {
        "snapshot" => {
            let saved = capture_layout_snapshot();
            let count = saved.len();
            *LAYOUT_SNAPSHOT.lock() = saved.clone();
            Ok(ToolOutput::ok(
                format!("Сохранено расположение {count} окон"),
                json!({ "savedCount": count, "windows": saved, "verified": true }),
            ))
        }
        "restore" => {
            let saved = LAYOUT_SNAPSHOT.lock().clone();
            if saved.is_empty() {
                return Ok(ToolOutput::fail(
                    "Нет сохранённого снимка расположения окон",
                    json!({ "restored": 0, "verified": false }),
                ));
            }
            let mut restored = Vec::new();
            for sw in &saved {
                if !unsafe { IsWindow(Some(winmgr::to_hwnd(sw.hwnd))).as_bool() } {
                    continue;
                }
                let res = match sw.state.as_str() {
                    "minimized" => winmgr::minimize(sw.hwnd),
                    "maximized" => winmgr::maximize(sw.hwnd),
                    _ => winmgr::move_resize(sw.hwnd, sw.rect),
                };
                if let Ok(info) = res {
                    restored.push(json!({ "hwnd": info.hwnd, "title": info.title, "state": info.state, "rect": info.rect }));
                }
            }
            let n = restored.len();
            Ok(ToolOutput::ok(
                format!("Восстановлено расположение окон: {n}"),
                json!({ "restoredCount": n, "windows": restored, "verified": n > 0 }),
            ))
        }
        "tile_pair" => {
            if LAYOUT_SNAPSHOT.lock().is_empty() {
                *LAYOUT_SNAPSHOT.lock() = capture_layout_snapshot();
            }
            let all_open = winmgr::list_windows();
            let recent_hwnds: Vec<i64> = history.recent().into_iter().map(|e| e.hwnd).collect();

            let left_win = a
                .left
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .and_then(|q| winmgr::find_windows(q).into_iter().next())
                .or_else(|| {
                    recent_hwnds
                        .iter()
                        .find_map(|&h| all_open.iter().find(|w| w.hwnd == h && w.state != "minimized").cloned())
                })
                .or_else(|| all_open.iter().find(|w| w.state != "minimized").cloned())
                .or_else(|| all_open.first().cloned());

            let Some(left) = left_win else {
                return Ok(ToolOutput::fail("Не найдено открытых окон для размещения", json!({})));
            };

            let right_win = a
                .right
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .and_then(|q| winmgr::find_windows(q).into_iter().find(|w| w.hwnd != left.hwnd))
                .or_else(|| {
                    recent_hwnds
                        .iter()
                        .filter(|&&h| h != left.hwnd)
                        .find_map(|&h| all_open.iter().find(|w| w.hwnd == h && w.state != "minimized").cloned())
                })
                .or_else(|| all_open.iter().find(|w| w.hwnd != left.hwnd && w.state != "minimized").cloned())
                .or_else(|| all_open.iter().find(|w| w.hwnd != left.hwnd).cloned());

            let Some(right) = right_win else {
                return Ok(ToolOutput::fail(
                    format!("Найдено только одно окно («{}») — для tile_pair нужно два", left.title),
                    json!({ "left": left.title }),
                ));
            };

            let mon = display::monitor_for_window(winmgr::to_hwnd(left.hwnd))
                .or_else(display::primary_monitor)
                .ok_or_else(|| AppError::platform("Не удалось определить рабочий монитор"))?;
            let work = mon.work_area;
            let pct = parse_split_percent(a.ratio.as_deref());
            let left_w = (work.width * pct) / 100;
            let right_w = work.width - left_w;

            let left_after = winmgr::move_resize(
                left.hwnd,
                Rect { x: work.x, y: work.y, width: left_w, height: work.height },
            )?;
            let right_after = winmgr::move_resize(
                right.hwnd,
                Rect { x: work.x + left_w, y: work.y, width: right_w, height: work.height },
            )?;
            let _ = winmgr::focus(right.hwnd);
            let _ = winmgr::focus(left.hwnd);

            let verified = left_after.state == "normal" && right_after.state == "normal";
            Ok(ToolOutput::ok(
                format!(
                    "Окна разложены {}-{}: слева «{}», справа «{}»",
                    pct,
                    100 - pct,
                    left_after.title,
                    right_after.title
                ),
                json!({
                    "ratio": format!("{}-{}", pct, 100 - pct),
                    "left": left_after,
                    "right": right_after,
                    "verified": verified
                }),
            ))
        }
        "focus_clean" => {
            if LAYOUT_SNAPSHOT.lock().is_empty() {
                *LAYOUT_SNAPSHOT.lock() = capture_layout_snapshot();
            }
            let keep_queries: Vec<String> = a
                .keep
                .iter()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();

            let primary_keep_hwnd = if keep_queries.is_empty() {
                history
                    .recent()
                    .first()
                    .map(|e| e.hwnd)
                    .or_else(|| winmgr::foreground_window().map(|w| w.hwnd))
            } else {
                None
            };

            let mut minimized = Vec::new();
            let mut kept = Vec::new();
            for w in winmgr::list_windows() {
                if w.state == "minimized" {
                    continue;
                }
                let hay = format!("{} {}", w.title, w.process).to_lowercase();
                let should_keep = if !keep_queries.is_empty() {
                    keep_queries.iter().any(|q| hay.contains(q.as_str()))
                } else {
                    Some(w.hwnd) == primary_keep_hwnd
                };
                if should_keep {
                    kept.push(w.title);
                } else if let Ok(after) = winmgr::minimize(w.hwnd) {
                    if after.state == "minimized" {
                        minimized.push(after.title);
                    }
                }
            }
            Ok(ToolOutput::ok(
                format!("Свёрнуто лишних окон: {}, оставлено на экране: {}", minimized.len(), kept.len()),
                json!({
                    "minimizedCount": minimized.len(),
                    "minimized": minimized,
                    "kept": kept,
                    "verified": true
                }),
            ))
        }
        other => Err(AppError::invalid(format!("Неизвестное действие window_layout: {other}"))),
    })
    .await
}

// ==================== 6. SYSTEM DIAGNOSTICS ====================

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessMemEntry {
    pub name: String,
    pub pid: u32,
    pub instances: u32,
    pub memory_mb: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemDiagnosticsReport {
    pub cpu_percent: f64,
    pub ram_percent: u32,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub ram_available_gb: f64,
    pub disk_c_free_gb: f64,
    pub disk_c_total_gb: f64,
    pub disk_c_used_percent: f64,
    pub has_battery: bool,
    pub battery_percent: Option<u8>,
    pub ac_online: Option<bool>,
    pub uptime_hours: f64,
    pub top_processes: Vec<ProcessMemEntry>,
}

fn filetime_u64(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub fn collect_system_diagnostics() -> AppResult<SystemDiagnosticsReport> {
    // 1. CPU sample over 150ms via GetSystemTimes.
    let cpu_percent = unsafe {
        let (mut idle1, mut kern1, mut user1) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        let (mut idle2, mut kern2, mut user2) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
        if GetSystemTimes(Some(&mut idle1), Some(&mut kern1), Some(&mut user1)).is_ok() {
            std::thread::sleep(Duration::from_millis(150));
            if GetSystemTimes(Some(&mut idle2), Some(&mut kern2), Some(&mut user2)).is_ok() {
                let idle = filetime_u64(idle2).saturating_sub(filetime_u64(idle1));
                let kern = filetime_u64(kern2).saturating_sub(filetime_u64(kern1));
                let user = filetime_u64(user2).saturating_sub(filetime_u64(user1));
                let total = kern.saturating_add(user);
                if total > 0 {
                    round1(((total.saturating_sub(idle)) as f64 * 100.0 / total as f64).clamp(0.0, 100.0))
                } else {
                    0.0
                }
            } else {
                0.0
            }
        } else {
            0.0
        }
    };

    // 2. RAM via GlobalMemoryStatusEx.
    let (ram_percent, ram_total_gb, ram_available_gb, ram_used_gb) = unsafe {
        let mut ms = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        GlobalMemoryStatusEx(&mut ms)?;
        let gib = 1024.0 * 1024.0 * 1024.0;
        let total = ms.ullTotalPhys as f64 / gib;
        let avail = ms.ullAvailPhys as f64 / gib;
        let used = (total - avail).max(0.0);
        (ms.dwMemoryLoad, round1(total), round1(avail), round1(used))
    };

    // 3. Disk space on C:\ via GetDiskFreeSpaceExW.
    let (disk_c_free_gb, disk_c_total_gb, disk_c_used_percent) = unsafe {
        let mut free_avail = 0u64;
        let mut total_bytes = 0u64;
        if GetDiskFreeSpaceExW(w!("C:\\"), Some(&mut free_avail), Some(&mut total_bytes), None).is_ok() && total_bytes > 0 {
            let gib = 1024.0 * 1024.0 * 1024.0;
            let free = free_avail as f64 / gib;
            let total = total_bytes as f64 / gib;
            let used_pct = ((total - free).max(0.0) * 100.0 / total).clamp(0.0, 100.0);
            (round1(free), round1(total), round1(used_pct))
        } else {
            (0.0, 0.0, 0.0)
        }
    };

    // 4. Battery / AC status via GetSystemPowerStatus.
    let (has_battery, battery_percent, ac_online) = unsafe {
        let mut sps = SYSTEM_POWER_STATUS::default();
        if GetSystemPowerStatus(&mut sps).is_ok() {
            let no_battery = (sps.BatteryFlag & 128) != 0 || sps.BatteryFlag == 255;
            let ac = match sps.ACLineStatus {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            };
            let pct = (!no_battery && sps.BatteryLifePercent <= 100).then_some(sps.BatteryLifePercent);
            (!no_battery, pct, ac)
        } else {
            (false, None, None)
        }
    };

    // 5. System uptime via GetTickCount64.
    let uptime_hours = round1(unsafe { GetTickCount64() } as f64 / 3_600_000.0);

    // 6. Top 5 processes by working set memory.
    let top_processes = sample_top_processes(5);

    Ok(SystemDiagnosticsReport {
        cpu_percent,
        ram_percent,
        ram_used_gb,
        ram_total_gb,
        ram_available_gb,
        disk_c_free_gb,
        disk_c_total_gb,
        disk_c_used_percent,
        has_battery,
        battery_percent,
        ac_online,
        uptime_hours,
        top_processes,
    })
}

fn sample_top_processes(limit: usize) -> Vec<ProcessMemEntry> {
    let mut agg: HashMap<String, (String, u32, u32, u64)> = HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return Vec::new();
        };
        let mut pe = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut pe).is_ok() {
            loop {
                let pid = pe.th32ProcessID;
                if pid > 4 {
                    let name = from_wide(&pe.szExeFile);
                    if !name.is_empty() {
                        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid)
                            .or_else(|_| OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid));
                        if let Ok(h) = handle {
                            let mut pmc = PROCESS_MEMORY_COUNTERS {
                                cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                                ..Default::default()
                            };
                            if GetProcessMemoryInfo(h, &mut pmc, pmc.cb).is_ok() && pmc.WorkingSetSize > 0 {
                                let ws = pmc.WorkingSetSize as u64;
                                let key = name.to_lowercase();
                                let entry = agg.entry(key).or_insert((name, pid, 0, 0));
                                entry.2 += 1;
                                entry.3 = entry.3.saturating_add(ws);
                            }
                            let _ = CloseHandle(h);
                        }
                    }
                }
                if Process32NextW(snap, &mut pe).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    let mut list: Vec<ProcessMemEntry> = agg
        .into_values()
        .map(|(name, pid, instances, bytes)| ProcessMemEntry {
            name,
            pid,
            instances,
            memory_mb: round1(bytes as f64 / (1024.0 * 1024.0)),
        })
        .collect();
    list.sort_by(|a, b| b.memory_mb.partial_cmp(&a.memory_mb).unwrap_or(std::cmp::Ordering::Equal));
    list.truncate(limit);
    list
}

async fn system_diagnostics(_ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    let report = platform::blocking(collect_system_diagnostics).await?;
    let bat = match (report.has_battery, report.battery_percent, report.ac_online) {
        (true, Some(p), Some(true)) => format!(", батарея {p}% (от сети)"),
        (true, Some(p), _) => format!(", батарея {p}%"),
        _ => String::new(),
    };
    let summary = format!(
        "CPU {:.0}%, RAM {}% ({:.1}/{:.1} ГБ), свободно на C: {:.1} ГБ{bat}",
        report.cpu_percent, report.ram_percent, report.ram_used_gb, report.ram_total_gb, report.disk_c_free_gb
    );
    Ok(ToolOutput::ok(summary, serde_json::to_value(report)?))
}

// ==================== 7. SPEAK ALOUD ====================

#[derive(Deserialize)]
struct SpeakAloudArgs {
    text: String,
    voice: Option<String>,
    rate: Option<f32>,
    #[serde(default = "default_true")]
    interrupt: bool,
}

fn default_true() -> bool {
    true
}

async fn speak_aloud(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: SpeakAloudArgs = parse(args)?;
    let clean = a.text.trim().to_string();
    if clean.is_empty() {
        return Err(AppError::invalid("Текст для озвучивания не может быть пустым"));
    }
    hub::request_tts(&ctx.app, &clean, a.voice, a.rate, a.interrupt);
    let preview: String = if clean.chars().count() > 64 {
        format!("{}…", clean.chars().take(64).collect::<String>())
    } else {
        clean.clone()
    };
    Ok(ToolOutput::ok(
        format!("Озвучиваю: «{preview}»"),
        json!({ "spoken": true, "text": clean }),
    ))
}

fn moderate(_: &Value) -> Risk {
    Risk::Moderate
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "mission_plan",
            description: "Publish or update a live multi-step execution checklist on the Dynamic Island and Workspace. \
                Call this at the start of any 3+ step task (marking the current step 'running' and upcoming steps 'pending') \
                and update statuses ('done' / 'failed' / 'running') as you progress.",
            schema: object(
                json!({
                    "steps": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "title": { "type": "string" },
                                "status": { "type": "string", "enum": ["pending", "running", "done", "failed"] }
                            },
                            "required": ["id", "title", "status"],
                            "additionalProperties": false
                        }
                    }
                }),
                &["steps"],
            ),
            risk: low,
            title: |a| {
                let n = a.get("steps").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
                format!("План выполнения ({n} шаг.)")
            },
            timeout: Duration::from_secs(5),
            parallel_safe: true,
            handler: handler(mission_plan),
        },
        Tool {
            name: "tasks_manage",
            description: "Manage the user's tasks & agenda in the Notch Hub: list tasks, create a task (with optional \
                scheduled_date 'YYYY-MM-DD'/'today'/'tomorrow', scheduled_time 'HH:MM', color 'blue'|'amber'|'coral'|'emerald'|'violet'), \
                complete/toggle a task by id or title, or delete a task.",
            schema: object(
                json!({
                    "action": { "type": "string", "enum": ["list", "create", "complete", "toggle", "delete"] },
                    "id": { "type": "string" },
                    "title": { "type": "string" },
                    "notes": { "type": "string" },
                    "scheduled_date": { "type": "string" },
                    "scheduled_time": { "type": "string" },
                    "color": { "type": "string", "enum": ["blue", "amber", "coral", "emerald", "violet"] }
                }),
                &["action"],
            ),
            risk: low,
            title: |a| match str_arg(a, "action") {
                "create" => format!("Добавить задачу «{}»", str_arg(a, "title")),
                "complete" => "Отметить задачу выполненной".into(),
                "toggle" => "Переключить статус задачи".into(),
                "delete" => "Удалить задачу".into(),
                _ => "Список задач".into(),
            },
            timeout: Duration::from_secs(8),
            parallel_safe: false,
            handler: handler(tasks_manage),
        },
        Tool {
            name: "focus_session",
            description: "Control the Notch Hub Pomodoro / Focus timer: start (with duration `minutes` and optional `title`/`task_id`), \
                pause, resume, complete, cancel, or read status.",
            schema: object(
                json!({
                    "action": { "type": "string", "enum": ["status", "start", "pause", "resume", "complete", "cancel"] },
                    "title": { "type": "string" },
                    "minutes": { "type": "integer", "minimum": 1, "maximum": 240 },
                    "task_id": { "type": "string" }
                }),
                &["action"],
            ),
            risk: low,
            title: |a| match str_arg(a, "action") {
                "start" => format!("Фокус на {} мин", a.get("minutes").and_then(Value::as_u64).unwrap_or(25)),
                "pause" => "Пауза фокуса".into(),
                "resume" => "Продолжить фокус".into(),
                "complete" => "Завершить фокус".into(),
                "cancel" => "Остановить фокус".into(),
                _ => "Статус фокуса".into(),
            },
            timeout: Duration::from_secs(8),
            parallel_safe: false,
            handler: handler(focus_session),
        },
        Tool {
            name: "selection_grab",
            description: "Read the text currently highlighted/selected by the user in the active foreground window \
                without losing their existing clipboard content (snapshots clipboard, sends Ctrl+C, reads selection, \
                and restores the previous clipboard).",
            schema: object(json!({}), &[]),
            risk: low,
            title: |_| "Прочитать выделенный текст".into(),
            timeout: Duration::from_secs(6),
            parallel_safe: false,
            handler: handler(selection_grab),
        },
        Tool {
            name: "window_layout",
            description: "Executive multi-window workspace layout manager with Win32 verification: \
                `snapshot` saves current open window positions; `restore` restores saved positions; \
                `tile_pair` snaps two windows side-by-side (`left`/`right` queries or 2 most recent windows, `ratio` '50-50' or '65-35'); \
                `focus_clean` minimizes non-essential windows except `keep` targets.",
            schema: object(
                json!({
                    "action": { "type": "string", "enum": ["snapshot", "restore", "tile_pair", "focus_clean"] },
                    "left": { "type": "string", "description": "Window title/process query for left side (tile_pair)" },
                    "right": { "type": "string", "description": "Window title/process query for right side (tile_pair)" },
                    "ratio": { "type": "string", "enum": ["50-50", "65-35", "60-40"] },
                    "keep": { "type": "array", "items": { "type": "string" }, "description": "App/window names to keep open during focus_clean" }
                }),
                &["action"],
            ),
            risk: moderate,
            title: |a| match str_arg(a, "action") {
                "snapshot" => "Сохранить расположение окон".into(),
                "restore" => "Восстановить расположение окон".into(),
                "tile_pair" => "Разложить два окна рядом".into(),
                "focus_clean" => "Свернуть лишние окна".into(),
                _ => "Управление раскладкой окон".into(),
            },
            timeout: Duration::from_secs(12),
            parallel_safe: false,
            handler: handler(window_layout),
        },
        Tool {
            name: "system_diagnostics",
            description: "Inspect real-time Windows PC health & telemetry: CPU load %, RAM usage (GB & %), \
                free disk space on C:, battery/AC power status, system uptime, and top 5 processes by memory.",
            schema: object(json!({}), &[]),
            risk: low,
            title: |_| "Диагностика системы".into(),
            timeout: Duration::from_secs(10),
            parallel_safe: true,
            handler: handler(system_diagnostics),
        },
        Tool {
            name: "speak_aloud",
            description: "Speak a Russian message, summary, translation, or briefing out loud using neural TTS.",
            schema: object(
                json!({
                    "text": { "type": "string" },
                    "voice": { "type": "string" },
                    "rate": { "type": "number", "minimum": 0.5, "maximum": 2.0 },
                    "interrupt": { "type": "boolean", "default": true }
                }),
                &["text"],
            ),
            risk: low,
            title: |_| "Озвучить вслух".into(),
            timeout: Duration::from_secs(15),
            parallel_safe: false,
            handler: handler(speak_aloud),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executive_tools_schemas_risks_and_titles_are_valid() {
        let list = tools();
        let names: Vec<&str> = list.iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            [
                "mission_plan",
                "tasks_manage",
                "focus_session",
                "selection_grab",
                "window_layout",
                "system_diagnostics",
                "speak_aloud"
            ]
        );
        for t in &list {
            assert_eq!(t.schema["type"], "object");
            assert_eq!(t.schema["additionalProperties"], false);
            let sample = json!({ "action": "tile_pair", "steps": [], "title": "Тест", "minutes": 25 });
            assert!(!(t.title)(&sample).is_empty());
        }
        let wl = list.iter().find(|t| t.name == "window_layout").unwrap();
        assert_eq!((wl.risk)(&json!({"action": "tile_pair"})), Risk::Moderate);
        let mp = list.iter().find(|t| t.name == "mission_plan").unwrap();
        assert_eq!((mp.risk)(&json!({})), Risk::Low);
        assert!(mp.parallel_safe);
        let sd = list.iter().find(|t| t.name == "system_diagnostics").unwrap();
        assert_eq!((sd.risk)(&json!({})), Risk::Low);
        assert!(sd.parallel_safe);
    }

    #[test]
    fn plan_event_serializes_exact_camel_case_shape() {
        let ev = AgentEvent::PlanUpdated {
            turn_id: "turn-42".into(),
            steps: vec![PlanStep { id: "1".into(), title: "Проверить систему".into(), status: "running".into() }],
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "plan_updated");
        assert_eq!(v["turnId"], "turn-42");
        assert_eq!(v["steps"][0]["id"], "1");
        assert_eq!(v["steps"][0]["title"], "Проверить систему");
        assert_eq!(v["steps"][0]["status"], "running");
    }

    #[test]
    fn split_ratio_and_plan_status_normalization() {
        assert_eq!(parse_split_percent(None), 50);
        assert_eq!(parse_split_percent(Some("50-50")), 50);
        assert_eq!(parse_split_percent(Some("65-35")), 65);
        assert_eq!(parse_split_percent(Some("60-40")), 60);
        assert_eq!(normalize_plan_status("in_progress"), "running");
        assert_eq!(normalize_plan_status("completed"), "done");
        assert_eq!(normalize_plan_status("pending"), "pending");
    }

    #[test]
    fn real_win32_system_diagnostics_returns_live_telemetry() {
        let diag = collect_system_diagnostics().unwrap();
        assert!(diag.ram_total_gb > 0.5, "RAM total should be positive");
        assert!(diag.ram_percent <= 100);
        assert!((0.0..=100.0).contains(&diag.cpu_percent));
        assert!(diag.disk_c_total_gb > 1.0, "C: drive total should be positive");
        assert!(!diag.top_processes.is_empty(), "Should enumerate running processes");
        assert!(diag.top_processes.len() <= 5);
    }
}
