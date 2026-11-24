//! What the user does at which hour: apps and sites opened, smart-home
//! devices switched, volume and brightness set. A short summary goes into
//! each request's context, so «включи свет» at eleven at night means the
//! light the user turns on at that hour, and «открой проект» the usual one.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, Duration, Local, Timelike, Utc};
use rusqlite::params;
use serde_json::Value;

use crate::store::Store;

/// Rows older than this are deleted; the summary looks at [`WINDOW_DAYS`].
const KEEP_DAYS: i64 = 90;
const WINDOW_DAYS: i64 = 45;

fn s<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty())
}

fn num(args: &Value, key: &str) -> Option<f64> {
    args.get(key).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())))
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

/// Russian word for a Home Assistant domain list («свет», «розетки»…).
fn domain_word(args: &Value) -> Option<String> {
    let list: Vec<String> = match args.get("domain") {
        Some(Value::String(d)) => vec![d.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    };
    let words: Vec<&str> = list
        .iter()
        .map(|d| match d.as_str() {
            "light" => "свет",
            "switch" => "розетки",
            "fan" => "вентиляторы",
            "cover" => "шторы",
            "climate" => "климат",
            "media_player" => "медиа",
            "lock" => "замки",
            "vacuum" => "пылесос",
            "scene" => "сцены",
            "script" => "сценарии",
            other => other,
        })
        .collect();
    (!words.is_empty()).then(|| words.join(", "))
}

/// One Home Assistant intent call as a habit («включает «Люстра» 40%»).
fn home_label(base: &str, args: &Value) -> Option<String> {
    let verb = match base {
        "HassTurnOn" => "включает",
        "HassTurnOff" => "выключает",
        "HassLightSet" => "свет",
        "HassClimateSetTemperature" => "температура",
        "HassSetPosition" => "положение",
        "HassMediaPause" => "пауза",
        "HassMediaUnpause" => "продолжает",
        "HassMediaNext" => "следующий трек",
        "HassSetVolume" => "громкость",
        "HassVacuumStart" => "запускает пылесос",
        "HassVacuumReturnToBase" => "пылесос на базу",
        "HassFanSetSpeed" => "вентилятор",
        b if b.starts_with("HassGet") || b.contains("Timer") || b.starts_with("HassList") || b.starts_with("HassShopping") => return None,
        b if b.starts_with("Hass") => b,
        _ => return None,
    };
    // «Выключи всё на кухне» has no domain; «температура в спальне» needs none.
    let kind = domain_word(args).or_else(|| matches!(base, "HassTurnOn" | "HassTurnOff").then(|| "всё".to_string()));
    let place = |p: String| kind.as_ref().map(|k| format!("{k} {p}")).unwrap_or(p);
    let what = s(args, "name")
        .map(|n| format!("«{n}»"))
        .or_else(|| s(args, "area").map(|a| place(format!("в «{a}»"))))
        .or_else(|| s(args, "floor").map(|f| place(format!("на этаже «{f}»"))))
        .or_else(|| domain_word(args).map(|d| format!("весь {d}")))
        .unwrap_or_default();
    let mut extra = Vec::new();
    if let Some(b) = num(args, "brightness") {
        extra.push(format!("{b:.0}%"));
    }
    if let Some(c) = s(args, "color") {
        extra.push(c.to_string());
    }
    if let Some(t) = num(args, "temperature") {
        extra.push(if t > 100.0 { format!("{t:.0}K") } else { format!("{t}°") });
    }
    for key in ["position", "volume_level", "percentage"] {
        if let Some(p) = num(args, key) {
            extra.push(format!("{p:.0}%"));
        }
    }
    let label = [verb.to_string(), what, extra.join(" ")].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
    Some(cut(&label, 90))
}

/// What a successful call says about the user's habits, if anything.
pub fn target_of(tool: &str, args: &Value) -> Option<String> {
    if let Some(rest) = tool.strip_prefix(crate::mcp::PREFIX) {
        let base = rest.rsplit("__").next().unwrap_or(rest);
        return home_label(base, args);
    }
    let action = s(args, "action").unwrap_or("");
    let label = match tool {
        "app_launch" => format!("открывает {}", s(args, "query")?),
        "open_target" => {
            let t = s(args, "target")?;
            let t = t.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.").trim_end_matches('/');
            format!("открывает {t}")
        }
        "vscode_open" => format!("открывает проект {}", s(args, "path").or_else(|| s(args, "project"))?),
        "display_brightness" => format!("яркость {:.0}%", num(args, "percent")?),
        "system_volume" if action == "set" => format!("громкость {:.0}%", num(args, "percent")?),
        "focus_session" if action == "start" => format!("фокус {:.0} мин", num(args, "minutes").unwrap_or(25.0)),
        "media_control" if action == "play" => "включает музыку".to_string(),
        _ => return None,
    };
    Some(cut(&label, 90))
}

/// Log a successful call that tells something about the user's routine.
pub fn record(store: &Store, tool: &str, args: &Value, now: DateTime<Local>) {
    let Some(target) = target_of(tool, args) else { return };
    let at = now.with_timezone(&Utc).to_rfc3339();
    let cutoff = (now - Duration::days(KEEP_DAYS)).with_timezone(&Utc).to_rfc3339();
    let res = store.with_conn(|c| {
        c.execute(
            "INSERT INTO usage_log(tool, target, hour, weekday, at) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![tool, target, now.hour(), now.weekday().num_days_from_monday(), at],
        )?;
        c.execute("DELETE FROM usage_log WHERE at < ?1", params![cutoff])?;
        Ok(())
    });
    if let Err(e) = res {
        tracing::warn!("usage log write failed: {e}");
    }
}

/// The habits line for the request context, or nothing while there is
/// too little to say.
pub fn summary(store: &Store, now: DateTime<Local>) -> Option<String> {
    let since = (now - Duration::days(WINDOW_DAYS)).with_timezone(&Utc).to_rfc3339();
    let rows: Vec<(String, u32)> = store
        .with_conn(|c| {
            let mut stmt = c.prepare("SELECT target, hour FROM usage_log WHERE at > ?1")?;
            let rows = stmt.query_map(params![since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?;
            Ok(rows.filter_map(Result::ok).collect())
        })
        .ok()?;
    summarize(&rows, now.hour())
}

/// Top targets around this hour (±1 h, at least three times) and overall
/// (at least five times).
pub(crate) fn summarize(rows: &[(String, u32)], hour: u32) -> Option<String> {
    let near = |h: u32| {
        let d = (h as i32 - hour as i32).rem_euclid(24);
        d <= 1 || d >= 23
    };
    let mut at_hour: HashMap<&str, u32> = HashMap::new();
    let mut overall: HashMap<&str, u32> = HashMap::new();
    for (target, h) in rows {
        *overall.entry(target).or_default() += 1;
        if near(*h) {
            *at_hour.entry(target).or_default() += 1;
        }
    }
    let top = |m: &HashMap<&str, u32>, min: u32, skip: &[&str], n: usize| -> Vec<(String, u32)> {
        let mut v: Vec<(String, u32)> =
            m.iter().filter(|(t, c)| **c >= min && !skip.contains(*t)).map(|(t, c)| (t.to_string(), *c)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v.truncate(n);
        v
    };
    let now_list = top(&at_hour, 3, &[], 4);
    let shown: Vec<&str> = now_list.iter().map(|(t, _)| t.as_str()).collect();
    let all_list = top(&overall, 5, &shown, 4);
    if now_list.is_empty() && all_list.is_empty() {
        return None;
    }
    let fmt = |v: &[(String, u32)]| v.iter().map(|(t, c)| format!("{t} ({c}×)")).collect::<Vec<_>>().join(", ");
    let mut parts = Vec::new();
    if !now_list.is_empty() {
        parts.push(format!("around this hour usually {}", fmt(&now_list)));
    }
    if !all_list.is_empty() {
        parts.push(format!("most often overall {}", fmt(&all_list)));
    }
    Some(format!("- The user's habits (last {WINDOW_DAYS} days; use them to resolve vague requests): {}\n", parts.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn calls_become_readable_habits() {
        let ha = |base: &str| format!("mcp__home__intent__{base}");
        assert_eq!(target_of(&ha("HassTurnOn"), &json!({"name": "Люстра", "domain": ["light"]})).as_deref(), Some("включает «Люстра»"));
        assert_eq!(target_of(&ha("HassTurnOff"), &json!({"area": "Кухня", "domain": ["light"]})).as_deref(), Some("выключает свет в «Кухня»"));
        assert_eq!(target_of(&ha("HassLightSet"), &json!({"name": "Ночник", "brightness": 30})).as_deref(), Some("свет «Ночник» 30%"));
        assert_eq!(target_of(&ha("HassClimateSetTemperature"), &json!({"area": "Спальня", "temperature": 22})).as_deref(), Some("температура в «Спальня» 22°"));
        assert_eq!(target_of(&ha("HassTurnOff"), &json!({"domain": "light"})).as_deref(), Some("выключает весь свет"));
        assert_eq!(target_of(&ha("HassGetState"), &json!({"name": "Люстра"})), None);
        assert_eq!(target_of("app_launch", &json!({"query": "Telegram"})).as_deref(), Some("открывает Telegram"));
        assert_eq!(target_of("open_target", &json!({"target": "https://www.youtube.com/"})).as_deref(), Some("открывает youtube.com"));
        assert_eq!(target_of("system_volume", &json!({"action": "set", "percent": 30})).as_deref(), Some("громкость 30%"));
        assert_eq!(target_of("system_volume", &json!({"action": "change", "delta": 10})), None);
        assert_eq!(target_of("file_read", &json!({"path": "x"})), None);
    }

    #[test]
    fn summary_lists_what_happens_around_this_hour_and_overall() {
        let mut rows = Vec::new();
        for _ in 0..6 {
            rows.push(("включает «Ночник»".to_string(), 23));
        }
        for _ in 0..3 {
            rows.push(("выключает весь свет".to_string(), 0));
        }
        for _ in 0..7 {
            rows.push(("открывает Telegram".to_string(), 10));
        }
        rows.push(("открывает Steam".to_string(), 23));
        let line = summarize(&rows, 23).unwrap();
        assert!(line.contains("around this hour usually включает «Ночник» (6×), выключает весь свет (3×)"), "{line}");
        assert!(line.contains("most often overall открывает Telegram (7×)"), "{line}");
        assert!(!line.contains("Steam"));
        assert_eq!(summarize(&rows[..2], 23), None);
    }

    #[test]
    fn records_go_to_the_log_and_come_back_in_the_summary() {
        let store = Store::in_memory().unwrap();
        let now = Local::now();
        for _ in 0..3 {
            record(&store, "app_launch", &json!({"query": "Telegram"}), now);
        }
        record(&store, "file_read", &json!({"path": "x"}), now);
        let line = summary(&store, now).unwrap();
        assert!(line.contains("открывает Telegram (3×)"), "{line}");
    }
}
