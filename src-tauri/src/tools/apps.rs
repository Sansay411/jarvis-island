//! Applications, windows and shell targets (URLs, folders, files, settings).

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};

use super::{handler, low, object, parse, str_arg, Risk, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};
use crate::platform::{self, apps, display::Rect, winmgr};

async fn apps_find(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let query = str_arg(&args, "query").to_string();
    let index = ctx.state().apps.clone();
    let found = platform::blocking(move || index.search(&query, 8)).await?;
    Ok(ToolOutput::ok(
        if found.is_empty() { "Подходящих приложений не найдено".into() } else { format!("Найдено приложений: {}", found.len()) },
        json!({ "apps": found }),
    ))
}

#[derive(Deserialize)]
struct LaunchArgs {
    query: Option<String>,
    id: Option<String>,
    #[serde(default = "yes")]
    reuse_existing: bool,
}

fn yes() -> bool {
    true
}

async fn app_launch(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: LaunchArgs = parse(args)?;
    let index = ctx.state().apps.clone();
    platform::blocking(move || {
        let app = if let Some(id) = a.id.as_deref().filter(|s| !s.is_empty()) {
            index.by_id(id)?.ok_or_else(|| AppError::not_found(format!("Приложение с id «{id}» не найдено")))?
        } else {
            let q = a.query.clone().unwrap_or_default();
            let found = index.search_scored(&q, 5)?;
            let Some((top_score, top)) = found.first().cloned() else {
                return Ok(ToolOutput::fail(
                    format!("Приложение «{q}» не найдено среди установленных"),
                    json!({ "hint": "Try apps_find with a shorter or English/localized name" }),
                ));
            };
            // Several equally weak matches: ask instead of guessing.
            let close: Vec<_> = found.iter().filter(|(s, _)| *s == top_score).collect();
            if top_score < 70 && close.len() > 1 {
                return Ok(ToolOutput::fail(
                    format!("Неоднозначно: подходят {}", close.iter().map(|(_, a)| a.name.as_str()).collect::<Vec<_>>().join(", ")),
                    json!({ "candidates": close.iter().map(|(_, a)| a).collect::<Vec<_>>() }),
                ));
            }
            top
        };

        if a.reuse_existing {
            if let Some(w) = apps::running_windows(&app).into_iter().next() {
                let focused = winmgr::focus(w.hwnd)?;
                return Ok(ToolOutput::ok(
                    format!("{} уже открыт — переключился на окно «{}»", app.name, focused.title),
                    json!({ "app": app, "window": focused, "launched": false, "verified": true }),
                ));
            }
        }

        let out = apps::launch(&app, Duration::from_secs(15))?;
        Ok(if out.verified {
            let title = out.window.as_ref().map(|w| w.title.clone()).unwrap_or_default();
            ToolOutput::ok(format!("{} запущен, окно «{title}»", app.name), json!({ "app": out.app, "window": out.window, "verified": true }))
        } else {
            ToolOutput::fail(
                format!("{}: команда запуска отправлена, но окно не появилось за 15 с", app.name),
                json!({ "app": out.app, "verified": false, "note": out.note }),
            )
        })
    })
    .await
}

async fn windows_list(_: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let query = str_arg(&args, "query").to_string();
    let list = platform::blocking(move || {
        Ok(if query.is_empty() { winmgr::list_windows() } else { winmgr::find_windows(&query) })
    })
    .await?;
    let compact: Vec<Value> = list
        .iter()
        .map(|w| json!({ "hwnd": w.hwnd, "title": w.title, "process": w.process, "state": w.state, "foreground": w.foreground, "monitor": w.monitor }))
        .collect();
    Ok(ToolOutput::ok(format!("Окон: {}", list.len()), json!({ "windows": compact })))
}

#[derive(Deserialize)]
struct WindowArgs {
    action: String,
    hwnd: Option<i64>,
    query: Option<String>,
    region: Option<String>,
    rect: Option<Rect>,
}

async fn window_action(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: WindowArgs = parse(args)?;
    let history = ctx.state().history.clone();
    platform::blocking(move || {
        let target = if a.action == "previous" {
            let prev = history.previous().ok_or_else(|| AppError::not_found("Нет предыдущего окна в истории"))?;
            prev.hwnd
        } else if let Some(h) = a.hwnd {
            h
        } else if let Some(q) = a.query.as_deref().filter(|q| !q.is_empty()) {
            let found = winmgr::find_windows(q);
            match found.len() {
                0 => return Ok(ToolOutput::fail(format!("Окно «{q}» не найдено"), json!({}))),
                _ => found[0].hwnd,
            }
        } else {
            return Err(AppError::invalid("Нужен hwnd или query"));
        };

        let (summary, data) = match a.action.as_str() {
            "focus" | "previous" => {
                let w = winmgr::focus(target)?;
                (format!("Переключился на «{}»", w.title), json!({ "window": w, "verified": true }))
            }
            "minimize" => {
                let w = winmgr::minimize(target)?;
                (format!("«{}» свёрнуто", w.title), json!({ "window": w, "verified": w.state == "minimized" }))
            }
            "maximize" => {
                let w = winmgr::maximize(target)?;
                (format!("«{}» развёрнуто", w.title), json!({ "window": w, "verified": w.state == "maximized" }))
            }
            "restore" => {
                let w = winmgr::restore(target)?;
                (format!("«{}» восстановлено", w.title), json!({ "window": w }))
            }
            "close" => {
                let title = winmgr::describe(winmgr::to_hwnd(target)).map(|w| w.title).unwrap_or_default();
                let out = winmgr::close(target)?;
                if out.closed {
                    (format!("«{title}» закрыто"), json!({ "closed": true }))
                } else {
                    return Ok(ToolOutput::fail(
                        format!("«{title}» не закрылось: {}", out.still_open_reason.unwrap_or_default()),
                        json!({ "closed": false }),
                    ));
                }
            }
            "snap" => {
                let region = a.region.as_deref().unwrap_or("left");
                let w = winmgr::snap(target, region)?;
                (format!("«{}» размещено: {region}", w.title), json!({ "window": w }))
            }
            "move" => {
                let rect = a.rect.ok_or_else(|| AppError::invalid("Для move нужен rect"))?;
                let w = winmgr::move_resize(target, rect)?;
                (format!("«{}» перемещено", w.title), json!({ "window": w }))
            }
            other => return Err(AppError::invalid(format!("Неизвестное действие окна: {other}"))),
        };
        Ok(ToolOutput::ok(summary, data))
    })
    .await
}

fn window_risk(a: &Value) -> Risk {
    if str_arg(a, "action") == "close" {
        Risk::Moderate
    } else {
        Risk::Low
    }
}

const RUNNABLE: &[&str] = &["exe", "bat", "cmd", "ps1", "vbs", "js", "msi", "lnk", "scr", "com", "reg", "jar", "appx", "msix"];

fn target_kind(target: &str) -> &'static str {
    let t = target.trim().to_lowercase();
    if t.starts_with("http://") || t.starts_with("https://") {
        "url"
    } else if t.starts_with("ms-settings:") {
        "settings"
    } else if t.contains("://") || (t.contains(':') && !(t.len() > 2 && t.as_bytes()[1] == b':')) {
        // Any other scheme (mailto:, steam:, custom protocol handlers).
        "uri"
    } else {
        let ext = Path::new(&t).extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
        if RUNNABLE.contains(&ext.as_str()) {
            "program"
        } else if Path::new(target.trim()).is_dir() {
            "folder"
        } else {
            "file"
        }
    }
}

fn open_risk(a: &Value) -> Risk {
    match target_kind(str_arg(a, "target")) {
        "url" | "settings" | "folder" | "file" => Risk::Low,
        "program" => Risk::High,
        _ => Risk::Moderate,
    }
}

async fn open_target(_: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let target = str_arg(&args, "target").trim().to_string();
    if target.is_empty() {
        return Err(AppError::invalid("Пустая цель"));
    }
    let kind = target_kind(&target);
    if (kind == "file" || kind == "folder") && !Path::new(&target).exists() {
        return Ok(ToolOutput::fail(format!("Путь не существует: {target}"), json!({ "exists": false })));
    }
    platform::blocking(move || {
        let before = winmgr::foreground_window().map(|w| w.hwnd);
        apps::shell_open(&target, None, None)?;
        // Verification: something new should come to the foreground.
        for _ in 0..12 {
            std::thread::sleep(Duration::from_millis(250));
            if let Some(fg) = winmgr::foreground_window() {
                if Some(fg.hwnd) != before {
                    return Ok(ToolOutput::ok(
                        format!("Открыто: {target} → «{}»", fg.title),
                        json!({ "kind": kind, "window": fg, "verified": true }),
                    ));
                }
            }
        }
        Ok(ToolOutput::ok(
            format!("Команда открытия отправлена: {target} (новое окно не стало активным)"),
            json!({ "kind": kind, "verified": false }),
        ))
    })
    .await
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "apps_find",
            description: "Search installed applications (Start menu: desktop and Store apps). Names are often \
                localized on this Russian Windows (e.g. \"Проводник\", \"Терминал\", \"Блокнот\", \"Калькулятор\"). \
                Returns ids usable with app_launch.",
            schema: object(json!({ "query": { "type": "string" } }), &["query"]),
            risk: low,
            title: |a| format!("Найти приложение «{}»", str_arg(a, "query")),
            timeout: Duration::from_secs(10),
            parallel_safe: true,
            handler: handler(apps_find),
        },
        Tool {
            name: "app_launch",
            description: "Open an installed application by name (`query`) or exact `id` from apps_find, and wait until \
                its window appears (verified). If the app is already running, its window is focused instead unless \
                reuse_existing=false. Returns candidates instead of launching when the name is ambiguous.",
            schema: object(
                json!({
                    "query": { "type": "string" },
                    "id": { "type": "string" },
                    "reuse_existing": { "type": "boolean", "default": true }
                }),
                &[],
            ),
            risk: low,
            title: |a| {
                let n = a.get("query").and_then(Value::as_str).or_else(|| a.get("id").and_then(Value::as_str)).unwrap_or("");
                format!("Открыть {n}")
            },
            timeout: Duration::from_secs(25),
            parallel_safe: true,
            handler: handler(app_launch),
        },
        Tool {
            name: "windows_list",
            description: "List open application windows (title, process, state, hwnd), optionally filtered by words \
                in the title or process name.",
            schema: object(json!({ "query": { "type": "string" } }), &[]),
            risk: low,
            title: |_| "Список окон".into(),
            timeout: Duration::from_secs(6),
            parallel_safe: true,
            handler: handler(windows_list),
        },
        Tool {
            name: "window_action",
            description: "Act on a window chosen by `hwnd` or by `query` (words in title/process): focus, minimize, \
                maximize, restore, close (polite WM_CLOSE — the app may ask to save), snap to a region \
                (left/right/top/bottom/top-left/top-right/bottom-left/bottom-right/center/maximize), move to `rect` \
                (physical pixels). action=previous switches back to the previously used window.",
            schema: object(
                json!({
                    "action": { "type": "string", "enum": ["focus", "minimize", "maximize", "restore", "close", "snap", "move", "previous"] },
                    "hwnd": { "type": "integer" },
                    "query": { "type": "string" },
                    "region": { "type": "string" },
                    "rect": { "type": "object", "properties": { "x": {"type":"integer"}, "y": {"type":"integer"}, "width": {"type":"integer"}, "height": {"type":"integer"} } }
                }),
                &["action"],
            ),
            risk: window_risk,
            title: |a| {
                let who = a.get("query").and_then(Value::as_str).unwrap_or("окно");
                match str_arg(a, "action") {
                    "focus" => format!("Переключиться на {who}"),
                    "minimize" => format!("Свернуть {who}"),
                    "maximize" => format!("Развернуть {who}"),
                    "restore" => format!("Восстановить {who}"),
                    "close" => format!("Закрыть {who}"),
                    "snap" => format!("Разместить {who}"),
                    "move" => format!("Переместить {who}"),
                    _ => "Вернуться к предыдущему окну".into(),
                }
            },
            timeout: Duration::from_secs(8),
            parallel_safe: false,
            handler: handler(window_action),
        },
        Tool {
            name: "open_target",
            description: "Open a URL in the default browser, a folder in Explorer, a document with its default app, \
                or a Windows Settings page (ms-settings:display, ms-settings:sound, ms-settings:bluetooth, \
                ms-settings:network, ms-settings:apps …). Running programs/scripts this way requires confirmation.",
            schema: object(json!({ "target": { "type": "string" } }), &["target"]),
            risk: open_risk,
            title: |a| format!("Открыть {}", str_arg(a, "target")),
            timeout: Duration::from_secs(10),
            parallel_safe: false,
            handler: handler(open_target),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_targets() {
        assert_eq!(target_kind("https://github.com"), "url");
        assert_eq!(target_kind("ms-settings:display"), "settings");
        assert_eq!(target_kind("C:\\tools\\setup.exe"), "program");
        assert_eq!(target_kind("C:\\x\\report.pdf"), "file");
        assert_eq!(open_risk(&json!({"target": "C:\\a\\run.ps1"})), Risk::High);
        assert_eq!(open_risk(&json!({"target": "https://ya.ru"})), Risk::Low);
    }
}
