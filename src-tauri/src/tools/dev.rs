//! Developer workflows: VS Code, Claude Code CLI, OpenAI Codex CLI,
//! Windows Terminal, project discovery and background job supervision.
//!
//! Invocation details were verified against the installed CLIs
//! (claude 2.1: `-p`, `--output-format stream-json`, `--verbose`,
//! `--permission-mode plan|acceptEdits|…`; codex 0.162: `exec --json -C
//! -s read-only|workspace-write --skip-git-repo-check`, prompt on stdin).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Local};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::Manager;

use super::{handler, low, object, parse, str_arg, Risk, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};
use crate::jobs::{self, JobStatus, Parsed};
use crate::platform::supervisor::{self, Spec};
use crate::platform::{self, apps, winmgr};

// ---------- discovery ----------

pub fn vscode_exe() -> Option<PathBuf> {
    if let Some(cmd) = supervisor::which("code") {
        // …\Microsoft VS Code\bin\code(.cmd) → …\Microsoft VS Code\Code.exe
        if let Some(root) = cmd.parent().and_then(Path::parent) {
            let exe = root.join("Code.exe");
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    let local = dirs::data_local_dir()?.join(r"Programs\Microsoft VS Code\Code.exe");
    local.is_file().then_some(local)
}

/// Codex is an npm package; run its JS entry with node directly so prompts
/// never pass through cmd.exe argument parsing.
pub fn codex_entry() -> Option<(PathBuf, PathBuf)> {
    let shim = supervisor::which("codex.cmd").or_else(|| supervisor::which("codex"))?;
    let script = shim.parent()?.join(r"node_modules\@openai\codex\bin\codex.js");
    let node = shim.parent().map(|d| d.join("node.exe")).filter(|p| p.is_file()).or_else(|| supervisor::which("node"))?;
    script.is_file().then_some((node, script))
}

pub fn claude_exe() -> Option<PathBuf> {
    supervisor::which("claude").filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")))
}

async fn version_of(program: PathBuf, args: Vec<String>, cancel: tokio_util::sync::CancellationToken) -> Option<String> {
    let spec = Spec { program: program.display().to_string(), args, cwd: None, env: vec![], timeout: Duration::from_secs(15), stdin: None };
    let r = supervisor::run(spec, cancel, None).await.ok()?;
    r.success.then(|| r.output.lines().next().unwrap_or("").trim().to_string())
}

async fn dev_tools_status(ctx: ToolCtx, _: Value) -> AppResult<ToolOutput> {
    let c = ctx.cancel.clone();
    let code = match vscode_exe() {
        Some(p) => json!({ "path": p, "version": version_of(p.clone(), vec!["--version".into()], c.clone()).await }),
        None => Value::Null,
    };
    let claude = match claude_exe() {
        Some(p) => json!({ "path": p, "version": version_of(p.clone(), vec!["--version".into()], c.clone()).await }),
        None => Value::Null,
    };
    let codex = match codex_entry() {
        Some((node, script)) => json!({ "path": script, "version": version_of(node, vec![script.display().to_string(), "--version".into()], c.clone()).await }),
        None => Value::Null,
    };
    let git = match supervisor::which("git") {
        Some(p) => json!({ "path": p, "version": version_of(p.clone(), vec!["--version".into()], c.clone()).await }),
        None => Value::Null,
    };
    let terminal = supervisor::which("wt").is_some();
    Ok(ToolOutput::ok(
        format!(
            "VS Code: {}, Claude Code: {}, Codex: {}, git: {}",
            if code.is_null() { "нет" } else { "есть" },
            if claude.is_null() { "нет" } else { "есть" },
            if codex.is_null() { "нет" } else { "есть" },
            if git.is_null() { "нет" } else { "есть" },
        ),
        json!({ "vscode": code, "claude": claude, "codex": codex, "git": git, "windowsTerminal": terminal }),
    ))
}

// ---------- projects ----------

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn file_uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file:///")?;
    Some(percent_decode(rest).replace('/', "\\"))
}

/// Folders VS Code knows about, most relevant first: the last active window,
/// open windows, hot-exit backups and profile associations from
/// `storage.json` (VS Code 1.142 layout), plus the classic recently-opened
/// list from `state.vscdb` when present (older versions).
pub fn vscode_recent() -> Vec<String> {
    let Some(dir) = dirs::config_dir().map(|d| d.join(r"Code\User\globalStorage")) else { return vec![] };
    let mut out: Vec<String> = Vec::new();
    let mut push = |uri: Option<&str>| {
        if let Some(p) = uri.and_then(file_uri_to_path) {
            if !out.iter().any(|x| x.eq_ignore_ascii_case(&p)) {
                out.push(p);
            }
        }
    };
    if let Some(j) = std::fs::read_to_string(dir.join("storage.json")).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        push(j.pointer("/windowsState/lastActiveWindow/folder").and_then(Value::as_str));
        for w in j.pointer("/windowsState/openedWindows").and_then(Value::as_array).into_iter().flatten() {
            push(w.get("folder").and_then(Value::as_str));
        }
        for f in j.pointer("/backupWorkspaces/folders").and_then(Value::as_array).into_iter().flatten() {
            push(f.get("folderUri").and_then(Value::as_str));
        }
        if let Some(ws) = j.pointer("/profileAssociations/workspaces").and_then(Value::as_object) {
            for k in ws.keys() {
                push(Some(k.as_str()));
            }
        }
    }
    for p in vscode_recent_db(&dir.join("state.vscdb")) {
        if !out.iter().any(|x| x.eq_ignore_ascii_case(&p)) {
            out.push(p);
        }
    }
    out
}

fn vscode_recent_db(db: &Path) -> Vec<String> {
    if !db.is_file() {
        return vec![];
    }
    let uri = format!("file:{}?mode=ro&immutable=1", db.display().to_string().replace('\\', "/"));
    let Ok(conn) = rusqlite::Connection::open_with_flags(
        uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return vec![];
    };
    let raw: Option<String> = conn
        .query_row("SELECT value FROM ItemTable WHERE key = 'history.recentlyOpenedPathsList'", [], |r| r.get(0))
        .ok();
    let Some(v) = raw.and_then(|r| serde_json::from_str::<Value>(&r).ok()) else { return vec![] };
    v.get("entries")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| e.get("folderUri").and_then(Value::as_str).and_then(file_uri_to_path))
                .collect()
        })
        .unwrap_or_default()
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn scan_git_repos(root: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if out.len() > 200 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_lowercase();
        if name.starts_with('.') || name == "node_modules" || name == "appdata" {
            continue;
        }
        if p.join(".git").exists() {
            out.push(p);
        } else if depth > 0 {
            scan_git_repos(&p, depth - 1, out);
        }
    }
}

async fn projects_list(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let query = str_arg(&args, "query").to_lowercase();
    let roots = ctx.state().store.settings().agent.project_roots;
    let data = platform::blocking(move || {
        let mut found: Vec<(String, &'static str)> = Vec::new();
        for r in &roots {
            found.push((r.clone(), "settings"));
        }
        for r in vscode_recent() {
            found.push((r, "vscode-recent"));
        }
        let mut repos = Vec::new();
        if let Some(home) = dirs::home_dir() {
            for sub in ["", "Documents", "source/repos", "projects", "dev", "code", "Desktop"] {
                scan_git_repos(&home.join(sub), if sub.is_empty() { 0 } else { 2 }, &mut repos);
            }
        }
        for r in repos {
            found.push((r.display().to_string(), "git"));
        }
        let mut seen = std::collections::HashSet::new();
        let mut list: Vec<Value> = Vec::new();
        for (path, source) in found {
            let key = path.to_lowercase().trim_end_matches('\\').to_string();
            if !seen.insert(key) || !Path::new(&path).is_dir() {
                continue;
            }
            if !query.is_empty() && !path.to_lowercase().contains(&query) {
                continue;
            }
            let m = modified(Path::new(&path));
            list.push(json!({
                "path": path,
                "name": Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()),
                "source": source,
                "git": Path::new(&path).join(".git").exists(),
                "modified": m.map(|t| DateTime::<Local>::from(t).format("%d.%m.%Y %H:%M").to_string()),
                "_ts": m.and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
            }));
        }
        // VS Code recents keep their own order (most recent first); others by mtime.
        list.sort_by(|a, b| {
            let rank = |v: &Value| match v["source"].as_str() { Some("settings") => 0, Some("vscode-recent") => 1, _ => 2 };
            rank(a).cmp(&rank(b)).then(if rank(a) == 2 { b["_ts"].as_u64().cmp(&a["_ts"].as_u64()) } else { std::cmp::Ordering::Equal })
        });
        for v in &mut list {
            v.as_object_mut().map(|o| o.remove("_ts"));
        }
        Ok(json!({ "projects": list }))
    })
    .await?;
    let n = data["projects"].as_array().map(Vec::len).unwrap_or(0);
    Ok(ToolOutput::ok(format!("Найдено проектов: {n}"), data))
}

// ---------- VS Code ----------

#[derive(Deserialize)]
struct VsCodeArgs {
    path: String,
    line: Option<u32>,
    #[serde(default)]
    new_window: bool,
}

async fn vscode_open(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: VsCodeArgs = parse(args)?;
    let exe = vscode_exe().ok_or_else(|| AppError::not_found("Visual Studio Code не найден"))?;
    let target = PathBuf::from(&a.path);
    if !target.exists() {
        return Ok(ToolOutput::fail(format!("Путь не существует: {}", a.path), json!({})));
    }
    let mut argv = Vec::new();
    if a.new_window {
        argv.push("--new-window".to_string());
    } else {
        argv.push("--reuse-window".to_string());
    }
    match a.line {
        Some(l) if target.is_file() => {
            argv.push("--goto".into());
            argv.push(format!("{}:{l}", target.display()));
        }
        _ => argv.push(target.display().to_string()),
    }
    // Code.exe hands the request to a running instance and exits quickly.
    let spec = Spec { program: exe.display().to_string(), args: argv, cwd: None, env: vec![], timeout: Duration::from_secs(20), stdin: None };
    let _ = supervisor::run(spec, ctx.cancel.clone(), None).await?;

    let folder_name = if target.is_dir() { target.file_name() } else { target.file_name() }
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(15) {
        let windows = platform::blocking(|| Ok(winmgr::list_windows())).await?;
        if let Some(w) = windows.iter().find(|w| w.process.eq_ignore_ascii_case("code.exe") && w.title.to_lowercase().contains(&folder_name)) {
            let w = w.clone();
            let focused = platform::blocking(move || winmgr::focus(w.hwnd).or(Ok(w))).await?;
            return Ok(ToolOutput::ok(format!("VS Code: «{}»", focused.title), json!({ "window": focused, "verified": true })));
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    Ok(ToolOutput::fail(
        "VS Code получил команду, но окно с этим проектом не появилось за 15 с",
        json!({ "verified": false }),
    ))
}

// ---------- coding agents ----------

fn claude_parser() -> jobs::Parser {
    Box::new(|line: &str| {
        let mut p = Parsed::default();
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            let t = line.trim();
            if !t.is_empty() {
                p.display = Some(t.chars().take(300).collect());
            }
            return p;
        };
        match v["type"].as_str() {
            Some("assistant") => {
                if let Some(items) = v.pointer("/message/content").and_then(Value::as_array) {
                    for it in items {
                        match it["type"].as_str() {
                            Some("text") => {
                                let t = it["text"].as_str().unwrap_or("").trim().to_string();
                                if t.contains("Failed to authenticate") || t.contains("/login") {
                                    p.error = Some("Claude Code не авторизован: запустите `claude` в терминале и выполните /login".into());
                                }
                                if !t.is_empty() {
                                    p.display = Some(t.chars().take(300).collect());
                                }
                            }
                            Some("tool_use") => {
                                let name = it["name"].as_str().unwrap_or("tool");
                                let detail = it["input"]
                                    .get("command")
                                    .or_else(|| it["input"].get("file_path"))
                                    .or_else(|| it["input"].get("pattern"))
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                p.display = Some(format!("→ {name} {}", detail.chars().take(160).collect::<String>()).trim().to_string());
                            }
                            _ => {}
                        }
                    }
                }
            }
            Some("result") => {
                if let Some(r) = v["result"].as_str() {
                    p.final_text = Some(r.to_string());
                }
                if v["is_error"].as_bool() == Some(true) && p.error.is_none() {
                    p.error = Some(format!("Claude Code завершился с ошибкой ({})", v["subtype"].as_str().unwrap_or("error")));
                }
                p.display = Some(format!(
                    "Готово: {} шаг(ов), {:.1} c",
                    v["num_turns"].as_u64().unwrap_or(0),
                    v["duration_ms"].as_f64().unwrap_or(0.0) / 1000.0
                ));
            }
            _ => {}
        }
        p
    })
}

fn codex_parser() -> jobs::Parser {
    Box::new(|line: &str| {
        let mut p = Parsed::default();
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            // Codex logs diagnostics on stderr; surface only real errors.
            if line.contains(" ERROR ") && !line.contains("rmcp::") && !line.contains("skill") {
                p.display = Some(line.chars().take(240).collect());
            }
            return p;
        };
        match v["type"].as_str() {
            Some("item.completed") | Some("item.started") => {
                let item = &v["item"];
                let done = v["type"] == "item.completed";
                match item["type"].as_str() {
                    Some("agent_message") if done => {
                        let t = item["text"].as_str().unwrap_or("").to_string();
                        p.display = Some(t.chars().take(300).collect());
                        p.final_text = Some(t);
                    }
                    Some("command_execution") => {
                        let cmd = item["command"].as_str().unwrap_or("");
                        p.display = Some(if done {
                            format!("✓ {} (код {})", cmd.chars().take(160).collect::<String>(), item["exit_code"].as_i64().unwrap_or(0))
                        } else {
                            format!("→ {}", cmd.chars().take(160).collect::<String>())
                        });
                    }
                    Some("file_change") if done => {
                        let files: Vec<String> = item["changes"]
                            .as_array()
                            .map(|c| c.iter().filter_map(|x| x["path"].as_str().map(str::to_string)).collect())
                            .unwrap_or_default();
                        p.display = Some(format!("Изменены файлы: {}", files.join(", ")));
                    }
                    Some("error") => p.error = item["message"].as_str().map(str::to_string),
                    _ => {}
                }
            }
            Some("turn.failed") | Some("error") => {
                let msg = v.pointer("/error/message").or_else(|| v.get("message")).and_then(Value::as_str).unwrap_or("ошибка");
                p.error = Some(format!("Codex: {msg}"));
                p.display = p.error.clone();
            }
            Some("turn.completed") => {
                p.display = Some(format!(
                    "Готово (токены: вход {}, выход {})",
                    v.pointer("/usage/input_tokens").and_then(Value::as_u64).unwrap_or(0),
                    v.pointer("/usage/output_tokens").and_then(Value::as_u64).unwrap_or(0)
                ));
            }
            _ => {}
        }
        p
    })
}

#[derive(Deserialize)]
struct AgentRunArgs {
    agent: String,
    project_dir: String,
    task: String,
    #[serde(default = "review")]
    mode: String,
    wait_secs: Option<u64>,
}

fn review() -> String {
    "review".into()
}

async fn coding_agent_run(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: AgentRunArgs = parse(args)?;
    let dir = PathBuf::from(&a.project_dir);
    if !dir.is_dir() {
        return Ok(ToolOutput::fail(format!("Папка проекта не найдена: {}", a.project_dir), json!({})));
    }
    let edit = a.mode == "edit";
    let (kind, label, spec, parser) = match a.agent.as_str() {
        "claude" => {
            let exe = claude_exe().ok_or_else(|| AppError::not_found("Claude Code CLI не установлен (claude.exe не найден в PATH)"))?;
            let spec = Spec {
                program: exe.display().to_string(),
                args: vec![
                    "-p".into(),
                    "--output-format".into(),
                    "stream-json".into(),
                    "--verbose".into(),
                    "--permission-mode".into(),
                    if edit { "acceptEdits".into() } else { "plan".into() },
                ],
                cwd: Some(dir.clone()),
                env: vec![],
                timeout: Duration::from_secs(45 * 60),
                stdin: Some(a.task.clone()),
            };
            ("claude", "Claude Code", spec, claude_parser())
        }
        "codex" => {
            let (node, script) = codex_entry().ok_or_else(|| AppError::not_found("Codex CLI не установлен (npm-пакет @openai/codex не найден)"))?;
            let spec = Spec {
                program: node.display().to_string(),
                args: vec![
                    script.display().to_string(),
                    "exec".into(),
                    "--json".into(),
                    "--skip-git-repo-check".into(),
                    "-C".into(),
                    dir.display().to_string(),
                    "-s".into(),
                    if edit { "workspace-write".into() } else { "read-only".into() },
                    "-".into(),
                ],
                cwd: Some(dir.clone()),
                env: vec![],
                timeout: Duration::from_secs(45 * 60),
                stdin: Some(a.task.clone()),
            };
            ("codex", "Codex", spec, codex_parser())
        }
        other => return Err(AppError::invalid(format!("Неизвестный агент «{other}» (claude или codex)"))),
    };

    let project = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let title = format!("{label}: {project}");
    let mgr = jobs::manager(&ctx.app);
    let label_owned = label.to_string();
    let id = mgr.start(
        kind,
        title.clone(),
        spec,
        parser,
        Box::new(move |app, view| {
            let minutes = view.finished_at.map(|f| (f - view.started_at).num_seconds()).unwrap_or(0);
            let ok = view.status == JobStatus::Succeeded;
            let head = match view.status {
                JobStatus::Succeeded => format!("**{label_owned}** закончил работу над «{project}» за {} мин {} с.", minutes / 60, minutes % 60),
                JobStatus::Cancelled => format!("**{label_owned}** остановлен."),
                _ => format!("**{label_owned}** завершился с ошибкой: {}", view.error.clone().unwrap_or_default()),
            };
            let body = view.final_text.clone().map(|t| format!("\n\n{t}")).unwrap_or_default();
            let text = format!("{head}{}", body.chars().take(6000).collect::<String>());
            app.state::<std::sync::Arc<crate::agent::Agent>>().post_report(text, ok);
        }),
    );

    // Catch fast failures (not installed, not logged in) within the turn.
    let wait = Duration::from_secs(a.wait_secs.unwrap_or(25).min(120));
    let started = Instant::now();
    loop {
        let view = mgr.get(&id, 8)?;
        if view.status != JobStatus::Running {
            let ok = view.status == JobStatus::Succeeded;
            return Ok(ToolOutput {
                ok,
                summary: if ok {
                    format!("{title}: завершено")
                } else {
                    format!("{title}: {}", view.error.clone().unwrap_or_else(|| "ошибка".into()))
                },
                data: json!({ "job": view }),
                images: vec![],
            });
        }
        if started.elapsed() >= wait || ctx.cancel.is_cancelled() {
            return Ok(ToolOutput::ok(
                format!("{title}: работает в фоне (задача {id})"),
                json!({ "job": view, "note": "The job keeps running; its final result will be posted to the chat automatically. Use job_status to check progress." }),
            ));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn job_status(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let mgr = jobs::manager(&ctx.app);
    let id = str_arg(&args, "job_id");
    if id.is_empty() {
        let list = mgr.list();
        return Ok(ToolOutput::ok(format!("Фоновых задач: {}", list.len()), json!({ "jobs": list })));
    }
    let tail = args.get("tail").and_then(Value::as_u64).unwrap_or(15) as usize;
    let v = mgr.get(id, tail.min(100))?;
    Ok(ToolOutput::ok(format!("{}: {:?}", v.title, v.status), json!({ "job": v })))
}

async fn job_cancel(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let id = str_arg(&args, "job_id").to_string();
    jobs::manager(&ctx.app).cancel(&id)?;
    Ok(ToolOutput::ok("Задача остановлена", json!({ "job_id": id })))
}

// ---------- terminal ----------

async fn terminal_open(_: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let cwd = str_arg(&args, "cwd").to_string();
    let command = str_arg(&args, "command").to_string();
    let dir = if cwd.is_empty() { dirs::home_dir().unwrap_or_default() } else { PathBuf::from(&cwd) };
    if !dir.is_dir() {
        return Ok(ToolOutput::fail(format!("Папка не найдена: {}", dir.display()), json!({})));
    }
    let has_wt = supervisor::which("wt").is_some();
    platform::blocking(move || {
        let before: std::collections::HashSet<i64> = winmgr::list_windows().into_iter().map(|w| w.hwnd).collect();
        let d = dir.display().to_string();
        if has_wt {
            let mut a = format!("-d \"{d}\"");
            if !command.is_empty() {
                a.push_str(&format!(" powershell -NoExit -Command \"{}\"", command.replace('"', "`\"")));
            }
            apps::shell_open("wt.exe", Some(&a), Some(&d))?;
        } else {
            let a = if command.is_empty() { "-NoExit".to_string() } else { format!("-NoExit -Command \"{}\"", command.replace('"', "`\"")) };
            apps::shell_open("powershell.exe", Some(&a), Some(&d))?;
        }
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(250));
            if let Some(w) = winmgr::list_windows().into_iter().find(|w| !before.contains(&w.hwnd)) {
                return Ok(ToolOutput::ok(format!("Терминал открыт: {}", d), json!({ "window": w, "verified": true })));
            }
        }
        Ok(ToolOutput::ok(format!("Терминал запущен в {d} (новое окно не обнаружено)"), json!({ "verified": false })))
    })
    .await
}

fn agent_risk(a: &Value) -> Risk {
    if str_arg(a, "mode") == "edit" {
        Risk::High
    } else {
        Risk::Moderate
    }
}

fn terminal_risk(a: &Value) -> Risk {
    let c = str_arg(a, "command");
    if c.is_empty() {
        Risk::Low
    } else {
        super::shell::classify(c).max(Risk::Moderate)
    }
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "dev_tools_status",
            description: "Check which developer tools are installed and their versions: VS Code, Claude Code CLI, \
                OpenAI Codex CLI, git, Windows Terminal.",
            schema: object(json!({}), &[]),
            risk: low,
            title: |_| "Проверить инструменты разработчика".into(),
            timeout: Duration::from_secs(40),
            parallel_safe: true,
            handler: handler(dev_tools_status),
        },
        Tool {
            name: "projects_list",
            description: "List the user's code projects: folders from settings, VS Code's recently opened folders \
                (most recent first) and git repositories in common locations. Use it to resolve \"my project\" / \
                \"my last project\". Ask the user if several candidates fit.",
            schema: object(json!({ "query": { "type": "string" } }), &[]),
            risk: low,
            title: |_| "Найти проекты".into(),
            timeout: Duration::from_secs(15),
            parallel_safe: true,
            handler: handler(projects_list),
        },
        Tool {
            name: "vscode_open",
            description: "Open a folder or file in Visual Studio Code (reusing the current window unless new_window) \
                and verify that a VS Code window for it appears. `line` jumps to a line in a file.",
            schema: object(
                json!({ "path": { "type": "string" }, "line": { "type": "integer" }, "new_window": { "type": "boolean" } }),
                &["path"],
            ),
            risk: low,
            title: |a| format!("VS Code: {}", Path::new(str_arg(a, "path")).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
            timeout: Duration::from_secs(40),
            parallel_safe: false,
            handler: handler(vscode_open),
        },
        Tool {
            name: "coding_agent_run",
            description: "Run Claude Code (agent=claude) or OpenAI Codex (agent=codex) non-interactively in a project \
                folder with a task written for that agent. mode=review (default) is read-only (analysis, \
                architecture review, explaining code); mode=edit lets it modify files. Runs as a background job: \
                returns quickly with a job id while it keeps working; the final result is posted to the chat \
                automatically. Write the task in the user's language with full context.",
            schema: object(
                json!({
                    "agent": { "type": "string", "enum": ["claude", "codex"] },
                    "project_dir": { "type": "string" },
                    "task": { "type": "string" },
                    "mode": { "type": "string", "enum": ["review", "edit"] },
                    "wait_secs": { "type": "integer", "minimum": 0, "maximum": 120 }
                }),
                &["agent", "project_dir", "task"],
            ),
            risk: agent_risk,
            title: |a| {
                let who = if str_arg(a, "agent") == "codex" { "Codex" } else { "Claude Code" };
                let project = Path::new(str_arg(a, "project_dir")).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                format!("{who} в «{project}»{}", if str_arg(a, "mode") == "edit" { " (с правками)" } else { "" })
            },
            timeout: Duration::from_secs(150),
            parallel_safe: false,
            handler: handler(coding_agent_run),
        },
        Tool {
            name: "job_status",
            description: "Status and recent output of a background job (or all jobs when job_id is omitted).",
            schema: object(json!({ "job_id": { "type": "string" }, "tail": { "type": "integer" } }), &[]),
            risk: low,
            title: |_| "Статус фоновой задачи".into(),
            timeout: Duration::from_secs(5),
            parallel_safe: true,
            handler: handler(job_status),
        },
        Tool {
            name: "job_cancel",
            description: "Stop a running background job (kills its whole process tree).",
            schema: object(json!({ "job_id": { "type": "string" } }), &["job_id"]),
            risk: low,
            title: |_| "Остановить фоновую задачу".into(),
            timeout: Duration::from_secs(5),
            parallel_safe: false,
            handler: handler(job_cancel),
        },
        Tool {
            name: "terminal_open",
            description: "Open Windows Terminal (or PowerShell) in a folder, optionally running a command there \
                visibly for the user. For commands whose output you need, use shell_run instead.",
            schema: object(json!({ "cwd": { "type": "string" }, "command": { "type": "string" } }), &[]),
            risk: terminal_risk,
            title: |a| format!("Терминал в {}", Path::new(str_arg(a, "cwd")).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "домашней папке".into())),
            timeout: Duration::from_secs(15),
            parallel_safe: false,
            handler: handler(terminal_open),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_vscode_uris() {
        assert_eq!(file_uri_to_path("file:///c%3A/Users/x/My%20App").as_deref(), Some("c:\\Users\\x\\My App"));
    }

    #[test]
    fn parses_codex_stream() {
        let mut p = codex_parser();
        let r = p(r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"ok"}}"#);
        assert_eq!(r.final_text.as_deref(), Some("ok"));
        let r = p(r#"{"type":"turn.failed","error":{"message":"quota"}}"#);
        assert!(r.error.unwrap().contains("quota"));
    }

    #[test]
    fn detects_claude_auth_failure() {
        let mut p = claude_parser();
        let r = p(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Failed to authenticate: OAuth session expired"}]}}"#);
        assert!(r.error.unwrap().contains("/login"));
        let r = p(r#"{"type":"result","subtype":"success","is_error":false,"result":"Архитектура в порядке","num_turns":3,"duration_ms":12000}"#);
        assert_eq!(r.final_text.as_deref(), Some("Архитектура в порядке"));
    }
}
