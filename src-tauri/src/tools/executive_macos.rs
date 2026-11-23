use super::{handler, low, object, str_arg, Risk, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};
use serde_json::{json, Value};
use std::time::Duration;
fn moderate(_: &Value) -> Risk {
    Risk::Moderate
}
async fn mission_plan(_ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
async fn tasks_manage(_ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
async fn focus_session(_ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
async fn selection_grab(_ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
async fn window_layout(_ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
async fn system_diagnostics(_ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
}
async fn speak_aloud(_ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    Err(AppError::unsupported("Пока не поддерживается на macOS"))
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
