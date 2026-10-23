//! Hardware & system state tools: volume, per-app volume, brightness, media,
//! context snapshot, bounded wait.

use std::time::Duration;

use chrono::Local;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{handler, low, object, parse, str_arg, Tool, ToolCtx, ToolOutput};
use crate::error::{AppError, AppResult};
use crate::platform::{self, audio, brightness, display, media, winmgr};

#[derive(Deserialize)]
struct VolumeArgs {
    action: String,
    percent: Option<i64>,
    delta: Option<i64>,
}

async fn system_volume(_: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: VolumeArgs = parse(args)?;
    let state = platform::blocking(move || match a.action.as_str() {
        "get" => audio::get_state(),
        "set" => {
            let p = a.percent.ok_or_else(|| AppError::invalid("Для action=set нужен percent"))?;
            audio::set_volume(p.clamp(0, 100) as u8)
        }
        "change" => {
            let d = a.delta.ok_or_else(|| AppError::invalid("Для action=change нужен delta"))?;
            audio::adjust_volume(d.clamp(-100, 100) as i16)
        }
        "mute" => audio::set_mute(true),
        "unmute" => audio::set_mute(false),
        other => Err(AppError::invalid(format!("Неизвестное действие {other}"))),
    })
    .await?;
    let summary = if state.muted {
        format!("Звук выключен ({})", state.device)
    } else {
        format!("Громкость {}% ({})", state.volume, state.device)
    };
    Ok(ToolOutput::ok(summary, json!({ "volume": state.volume, "muted": state.muted, "device": state.device, "verified": true })))
}

fn volume_title(a: &Value) -> String {
    match str_arg(a, "action") {
        "set" => format!("Громкость → {}%", a.get("percent").and_then(Value::as_i64).unwrap_or(0)),
        "change" => {
            let d = a.get("delta").and_then(Value::as_i64).unwrap_or(0);
            format!("Громкость {}{}%", if d >= 0 { "+" } else { "" }, d)
        }
        "mute" => "Выключить звук".into(),
        "unmute" => "Включить звук".into(),
        _ => "Проверить громкость".into(),
    }
}

#[derive(Deserialize)]
struct AppVolumeArgs {
    process: Option<String>,
    percent: Option<i64>,
}

async fn app_volume(_: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: AppVolumeArgs = parse(args)?;
    match (a.process, a.percent) {
        (Some(p), Some(pct)) => {
            let sessions = platform::blocking(move || audio::set_app_volume(&p, pct.clamp(0, 100) as u8)).await?;
            let s = sessions.first().cloned();
            Ok(ToolOutput::ok(
                s.map(|s| format!("{}: громкость {}%", s.process, s.volume)).unwrap_or_else(|| "Готово".into()),
                json!({ "sessions": sessions, "verified": true }),
            ))
        }
        _ => {
            let sessions = platform::blocking(audio::list_app_sessions).await?;
            Ok(ToolOutput::ok(format!("Активных аудиосеансов: {}", sessions.len()), json!({ "sessions": sessions })))
        }
    }
}

#[derive(Deserialize)]
struct BrightnessArgs {
    percent: Option<i64>,
    display: Option<String>,
}

async fn display_brightness(_: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: BrightnessArgs = parse(args)?;
    let list = match a.percent {
        Some(p) => {
            let target = a.display.clone();
            platform::blocking(move || brightness::set(p.clamp(0, 100) as u8, target.as_deref())).await?
        }
        None => platform::blocking(brightness::list).await?,
    };
    let supported: Vec<_> = list.iter().filter(|d| d.supported).collect();
    if supported.is_empty() {
        return Ok(ToolOutput::fail(
            "Ни один дисплей не поддерживает программную яркость",
            json!({ "displays": list }),
        ));
    }
    let summary = supported
        .iter()
        .map(|d| format!("{} — {}%", d.name, d.brightness.map(|b| b.to_string()).unwrap_or("?".into())))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(ToolOutput::ok(format!("Яркость: {summary}"), json!({ "displays": list, "verified": true })))
}

async fn media_control(_: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let action = str_arg(&args, "action").to_string();
    if action == "status" {
        let m = platform::blocking(media::snapshot).await?;
        let summary = if m.available {
            format!("{} — {} ({})", m.title, m.artist, m.status)
        } else {
            "Сейчас ничего не воспроизводится".into()
        };
        return Ok(ToolOutput::ok(summary, serde_json::to_value(m)?));
    }
    let act: media::MediaAction = serde_json::from_value(Value::String(action.clone()))
        .map_err(|_| AppError::invalid(format!("Неизвестное действие медиа: {action}")))?;
    let out = platform::blocking(move || media::control(act)).await?;
    let what = match act {
        media::MediaAction::PlayPause => if out.state.status == "playing" { "Воспроизведение" } else { "Пауза" },
        media::MediaAction::Play => "Воспроизведение",
        media::MediaAction::Pause => "Пауза",
        media::MediaAction::Next => "Следующий трек",
        media::MediaAction::Previous => "Предыдущий трек",
        media::MediaAction::Stop => "Остановлено",
    };
    let summary = if out.method == "media_key" {
        format!("{what}: отправлена медиаклавиша (плеер не сообщает состояние)")
    } else if out.verified {
        if out.state.title.is_empty() { what.to_string() } else { format!("{what}: {} — {}", out.state.title, out.state.artist) }
    } else {
        format!("{what}: команда принята, но состояние плеера не изменилось")
    };
    Ok(ToolOutput {
        ok: out.verified || out.method == "media_key",
        summary,
        data: json!({ "state": out.state, "method": out.method, "verified": out.verified }),
        images: vec![],
    })
}

/// Observe primitive: what is going on right now, without screenshots.
async fn context_snapshot(ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    let history = ctx.state().history.clone();
    let data = platform::blocking(move || {
        let fg = winmgr::foreground_window();
        let recent: Vec<Value> = history
            .recent()
            .into_iter()
            .take(6)
            .map(|e| json!({ "hwnd": e.hwnd, "title": e.title, "process": e.process }))
            .collect();
        let media = media::snapshot().ok().filter(|m| m.available);
        let audio = audio::get_state().ok();
        Ok(json!({
            "localTime": Local::now().format("%A, %d.%m.%Y %H:%M").to_string(),
            "foreground": fg,
            "recentWindows": recent,
            "monitors": display::list_monitors().len(),
            "media": media.map(|m| json!({ "title": m.title, "artist": m.artist, "status": m.status, "app": m.source_app })),
            "audio": audio,
            "userFolders": {
                "desktop": dirs::desktop_dir(),
                "downloads": dirs::download_dir(),
                "documents": dirs::document_dir(),
            },
        }))
    })
    .await?;
    let fg_title = data.pointer("/foreground/title").and_then(Value::as_str).unwrap_or("—").to_string();
    Ok(ToolOutput::ok(format!("Активное окно: {fg_title}"), data))
}

async fn wait(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let secs = args.get("seconds").and_then(Value::as_f64).unwrap_or(1.0).clamp(0.2, 15.0);
    tokio::select! {
        _ = ctx.cancel.cancelled() => return Err(AppError::Cancelled),
        _ = tokio::time::sleep(Duration::from_secs_f64(secs)) => {}
    }
    Ok(ToolOutput::ok(format!("Подождал {secs:.1} с"), json!({ "waited": secs })))
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "system_volume",
            description: "Read or change the master output volume of the default Windows audio device (Core Audio). \
                Returns the verified level read back after the change.",
            schema: object(
                json!({
                    "action": { "type": "string", "enum": ["get", "set", "change", "mute", "unmute"] },
                    "percent": { "type": "integer", "minimum": 0, "maximum": 100, "description": "Target level for action=set" },
                    "delta": { "type": "integer", "minimum": -100, "maximum": 100, "description": "Relative change for action=change" }
                }),
                &["action"],
            ),
            risk: low,
            title: volume_title,
            timeout: Duration::from_secs(5),
            parallel_safe: true,
            handler: handler(system_volume),
        },
        Tool {
            name: "app_volume",
            description: "List per-application audio sessions, or set one application's volume. \
                `process` is the executable name, e.g. \"spotify.exe\" or \"chrome\". Omit percent to list sessions.",
            schema: object(
                json!({
                    "process": { "type": "string" },
                    "percent": { "type": "integer", "minimum": 0, "maximum": 100 }
                }),
                &[],
            ),
            risk: low,
            title: |a| match (a.get("process").and_then(Value::as_str), a.get("percent").and_then(Value::as_i64)) {
                (Some(p), Some(v)) => format!("Громкость {p} → {v}%"),
                _ => "Аудиосеансы приложений".into(),
            },
            timeout: Duration::from_secs(5),
            parallel_safe: true,
            handler: handler(app_volume),
        },
        Tool {
            name: "display_brightness",
            description: "Read or set display brightness. Internal laptop panels use WMI, external monitors DDC/CI. \
                Without `display` the level is applied to every supported display. Omit percent to read.",
            schema: object(
                json!({
                    "percent": { "type": "integer", "minimum": 0, "maximum": 100 },
                    "display": { "type": "string", "description": "Display id from a previous read (wmi:... or ddc:...)" }
                }),
                &[],
            ),
            risk: low,
            title: |a| match a.get("percent").and_then(Value::as_i64) {
                Some(p) => format!("Яркость → {p}%"),
                None => "Проверить яркость".into(),
            },
            timeout: Duration::from_secs(8),
            parallel_safe: true,
            handler: handler(display_brightness),
        },
        Tool {
            name: "media_control",
            description: "Control the current media session (Spotify, browser video, Media Player, etc.) through \
                Windows media transport controls, or read what is playing with action=status.",
            schema: object(
                json!({ "action": { "type": "string", "enum": ["status", "play_pause", "play", "pause", "next", "previous", "stop"] } }),
                &["action"],
            ),
            risk: low,
            title: |a| match str_arg(a, "action") {
                "play" => "Воспроизвести".into(),
                "pause" => "Пауза".into(),
                "play_pause" => "Пауза / воспроизведение".into(),
                "next" => "Следующий трек".into(),
                "previous" => "Предыдущий трек".into(),
                "stop" => "Остановить".into(),
                _ => "Что играет".into(),
            },
            timeout: Duration::from_secs(6),
            parallel_safe: false,
            handler: handler(media_control),
        },
        Tool {
            name: "context_snapshot",
            description: "Observe the desktop without a screenshot: local time, foreground window, recently used \
                windows, media, volume and the user's standard folders. Use it to resolve references like \
                \"this\", \"the last window\" or \"what am I doing\".",
            schema: object(json!({}), &[]),
            risk: low,
            title: |_| "Осмотреться".into(),
            timeout: Duration::from_secs(6),
            parallel_safe: true,
            handler: handler(context_snapshot),
        },
        Tool {
            name: "wait",
            description: "Wait (0.2–15 s) before verifying something that takes time. Every second here is a second the user waits: 0.5–1 s is enough after a click or a key; app_launch and open_target already wait for their window.",
            schema: object(json!({ "seconds": { "type": "number", "minimum": 0.2, "maximum": 15 } }), &["seconds"]),
            risk: low,
            title: |a| format!("Подождать {} с", a.get("seconds").and_then(Value::as_f64).unwrap_or(1.0)),
            timeout: Duration::from_secs(20),
            parallel_safe: false,
            handler: handler(wait),
        },
    ]
}
