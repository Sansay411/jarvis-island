//! The daily review: once a day, while JARVIS is idle, the model reads the
//! latest conversations and writes down what it learned about working with
//! this user (preferences, humour that landed, misheard words, the user's
//! own names for things, lasting facts). The next prompts carry the result,
//! so the same correction is never needed twice.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::params;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use super::{Kind, Lesson};
use crate::ai::types::{ChatMessage, Role};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::{Memory, Store};

const KV_AT: &str = "learn.reflected_at";
/// Fewer requests than this since the last review: nothing worth reading.
const MIN_USER_MESSAGES: usize = 6;
const MAX_LINES: usize = 140;
const EVERY: chrono::Duration = chrono::Duration::hours(20);
/// At most this many lessons and facts from one review.
const MAX_ITEMS: usize = 8;

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15 * 60)).await;
        loop {
            match due(&app) {
                Some(since) => match run(&app, &since).await {
                    Ok(n) => tracing::info!(learned = n, "daily review done"),
                    Err(e) => tracing::info!("daily review skipped: {e}"),
                },
                None => {}
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

/// The time the review should read from, when one is due now.
fn due(app: &AppHandle) -> Option<String> {
    let state = app.state::<AppState>();
    let settings = state.store.settings();
    if !settings.agent.self_learning || !settings.privacy.store_history {
        return None;
    }
    if app.try_state::<Arc<crate::agent::Agent>>().is_some_and(|a| a.is_busy()) {
        return None;
    }
    let last = state.store.kv_get(KV_AT).and_then(|v| DateTime::parse_from_rfc3339(&v).ok()).map(|t| t.with_timezone(&Utc));
    let now = Utc::now();
    if last.is_some_and(|t| now - t < EVERY) {
        return None;
    }
    Some(last.unwrap_or(now - chrono::Duration::days(7)).to_rfc3339())
}

fn cut(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}…", flat.chars().take(max).collect::<String>())
    }
}

/// Stored messages since `since`, oldest first.
fn messages_since(store: &Store, since: &str) -> AppResult<Vec<ChatMessage>> {
    let mut out: Vec<ChatMessage> = store.with_conn(|c| {
        let mut stmt = c.prepare("SELECT payload FROM messages WHERE created_at > ?1 ORDER BY id DESC LIMIT 400")?;
        let rows = stmt.query_map(params![since], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(Result::ok).filter_map(|p| serde_json::from_str::<ChatMessage>(&p).ok()).collect())
    })?;
    out.reverse();
    Ok(out)
}

/// The conversation as short lines for the review, and how many of them
/// are the user's requests.
pub(crate) fn transcript(msgs: &[ChatMessage]) -> (Vec<String>, usize) {
    let mut lines = Vec::new();
    let mut user = 0;
    for m in msgs {
        let text = m.joined_text();
        match m.role {
            Role::User if text.starts_with("[tool-image]") => {}
            Role::User => {
                user += 1;
                lines.push(format!("Пользователь: {}", cut(&text, 240)));
            }
            Role::Assistant => {
                if !text.trim().is_empty() {
                    lines.push(format!("Джарвис: {}", cut(&text, 200)));
                }
                for c in &m.tool_calls {
                    lines.push(format!("  → {}({})", c.name, cut(&c.arguments, 120)));
                }
            }
            Role::Tool => {
                let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                let ok = v.get("ok").and_then(Value::as_bool).unwrap_or(false);
                let what = v.get("summary").or_else(|| v.get("error")).and_then(Value::as_str).unwrap_or("");
                lines.push(format!("  = {}: {}", if ok { "ok" } else { "ошибка" }, cut(what, 100)));
            }
            _ => {}
        }
    }
    if lines.len() > MAX_LINES {
        lines.drain(..lines.len() - MAX_LINES);
    }
    (lines, user)
}

fn prompt(lines: &[String], lessons: &[Lesson], memories: &[Memory]) -> String {
    let known: Vec<String> = lessons
        .iter()
        .take(120)
        .map(|l| match l.kind.as_str() {
            "hearing" | "alias" => format!("[{}] {}: «{}» → «{}»", l.id, l.kind, l.key, l.value),
            _ => format!("[{}] {}: {}", l.id, l.kind, l.value),
        })
        .collect();
    let facts: Vec<String> = memories.iter().take(80).map(|m| m.text.clone()).collect();
    format!(
        "Уже известно (не повторяй; если разговор что-то опроверг — верни его id в forget):\n{known}\n\n\
         Факты о пользователе в памяти: {facts}\n\n\
         Разговоры за последнее время:\n{lines}\n\n\
         Выпиши только то, что подтверждено разговором и пригодится снова:\n\
         - preference — как пользователь хочет, чтобы Джарвис делал или отвечал (его поправки, недовольство, похвала за конкретное). Формулируй правилом: «Отвечать короче, без вопроса в конце», «Свет вечером — тёплый, 30–40 %».\n\
         - humor — какие шутки зашли (засмеялся, подхватил) или раздражали.\n\
         - hearing — слово, которое распознавание речи явно исказило (пользователь повторил или поправил): heard — как записано, meant — что сказано. Только уверенные случаи.\n\
         - alias — слово пользователя для конкретного приложения, сайта, комнаты или устройства, когда видно, какое точное имя сработало: heard — слово пользователя, meant — точное имя, text — что это.\n\
         - facts — устойчивые факты о самом пользователе, которых нет в памяти.\n\
         Не больше {MAX_ITEMS} пунктов всего; лучше ни одного, чем выдумка. Пароли, ключи, номера карт, настроение на час и разовые просьбы не записывай.\n\
         Ответ только JSON: {{\"lessons\":[{{\"kind\":\"preference\",\"text\":\"…\"}},{{\"kind\":\"hearing\",\"heard\":\"…\",\"meant\":\"…\"}},{{\"kind\":\"alias\",\"heard\":\"…\",\"meant\":\"…\",\"text\":\"…\"}},{{\"kind\":\"humor\",\"text\":\"…\"}}],\"facts\":[\"…\"],\"forget\":[]}}",
        known = if known.is_empty() { "ничего".to_string() } else { known.join("\n") },
        facts = if facts.is_empty() { "нет".to_string() } else { facts.join("; ") },
        lines = lines.join("\n"),
    )
}

/// Secrets never become lessons.
fn looks_secret(text: &str) -> bool {
    let lower = text.to_lowercase();
    let mut run = 0;
    let long_number = text.chars().any(|c| {
        run = if c.is_ascii_digit() { run + 1 } else { 0 };
        run >= 12
    });
    long_number || ["парол", "password", "token", "токен", "api key", "api-ключ", "cvv"].iter().any(|w| lower.contains(w))
}

/// What the review answered, applied: (lessons saved, facts saved, forgotten).
pub(crate) fn apply(store: &Store, reply: &str) -> (usize, usize, usize) {
    let (Some(a), Some(b)) = (reply.find('{'), reply.rfind('}')) else { return (0, 0, 0) };
    let Ok(v) = serde_json::from_str::<Value>(&reply[a..=b]) else { return (0, 0, 0) };
    let text = |x: &Value, k: &str| x.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let mut budget = MAX_ITEMS;
    let mut saved = 0;
    for l in v.get("lessons").and_then(Value::as_array).into_iter().flatten() {
        if budget == 0 {
            break;
        }
        let Some(kind) = Kind::parse(&text(l, "kind")) else { continue };
        let (key, value, note) = match kind {
            Kind::Hearing | Kind::Alias => (text(l, "heard"), text(l, "meant"), if kind == Kind::Alias { text(l, "text") } else { String::new() }),
            _ => (String::new(), text(l, "text"), String::new()),
        };
        if looks_secret(&key) || looks_secret(&value) || looks_secret(&note) {
            continue;
        }
        if let Ok(s) = super::save(store, kind, &key, &value, "", &note, "reflect") {
            budget -= 1;
            if !s.existing {
                saved += 1;
            }
        }
    }
    let norm = super::norm;
    let mut facts = 0;
    let known: Vec<String> = store.memories().unwrap_or_default().iter().map(|m| norm(&m.text)).collect();
    for f in v.get("facts").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        let f = f.trim();
        if budget == 0 || f.is_empty() || f.chars().count() > 200 || looks_secret(f) || known.contains(&norm(f)) {
            continue;
        }
        if store.add_memory(f).is_ok() {
            budget -= 1;
            facts += 1;
        }
    }
    let mut forgotten = 0;
    for id in v.get("forget").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_i64).take(5) {
        if super::forget(store, id).unwrap_or(false) {
            forgotten += 1;
        }
    }
    (saved, facts, forgotten)
}

/// Review the conversations since `since` now. Returns how many things changed.
pub async fn run(app: &AppHandle, since: &str) -> AppResult<usize> {
    let state = app.state::<AppState>();
    let store = state.store.clone();
    let (lines, user) = transcript(&messages_since(&store, since)?);
    if user < MIN_USER_MESSAGES {
        return Err(AppError::invalid(format!("мало новых разговоров ({user})")));
    }
    let client = crate::ai::client_from_settings(&store.settings(), &store)?;
    let lessons = super::list(&store)?;
    let memories = store.memories().unwrap_or_default();
    let msgs = [
        ChatMessage::text(
            Role::System,
            "Ты — модуль самообучения ассистента JARVIS. Читаешь его разговоры с пользователем и выписываешь, что запомнить, \
             чтобы в следующий раз понимать пользователя с полуслова и не повторять ошибок. Отвечаешь строго JSON.",
        ),
        ChatMessage::text(Role::User, prompt(&lines, &lessons, &memories)),
    ];
    let cancel = tokio_util::sync::CancellationToken::new();
    let done = tokio::time::timeout(Duration::from_secs(90), client.complete(&msgs, &[], &cancel, Some(1200), |_| {}))
        .await
        .map_err(|_| AppError::Timeout("модель не ответила за 90 с".into()))??;
    let (saved, facts, forgotten) = apply(&store, &done.message.joined_text());
    store.kv_set(KV_AT, &Utc::now().to_rfc3339())?;
    let changed = saved + facts + forgotten;
    if changed > 0 {
        let _ = app.emit(super::EVT, ());
    }
    tracing::info!(saved, facts, forgotten, "lessons from the daily review");
    Ok(changed)
}

/// «Разобрать разговоры сейчас» in Settings: the last week, whatever the timer says.
pub async fn run_now(app: &AppHandle) -> AppResult<usize> {
    let since = (Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    run(app, &since).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::types::ToolCall;

    #[test]
    fn transcript_is_short_lines_and_counts_requests() {
        let mut call = ChatMessage::text(Role::Assistant, "");
        call.tool_calls.push(ToolCall { id: "1".into(), name: "app_launch".into(), arguments: r#"{"query":"телега"}"#.into() });
        let msgs = vec![
            ChatMessage::text(Role::User, "Открой телегу"),
            call,
            ChatMessage::tool_result("1", r#"{"ok":false,"summary":"Не нашёл «телега»"}"#),
            ChatMessage::text(Role::Assistant, "Не нашёл такое приложение."),
            ChatMessage::text(Role::User, "[tool-image] скрин"),
            ChatMessage::text(Role::User, "Телеграм же!"),
        ];
        let (lines, user) = transcript(&msgs);
        assert_eq!(user, 2);
        assert_eq!(lines[0], "Пользователь: Открой телегу");
        assert_eq!(lines[1], r#"  → app_launch({"query":"телега"})"#);
        assert_eq!(lines[2], "  = ошибка: Не нашёл «телега»");
        assert_eq!(lines.last().unwrap(), "Пользователь: Телеграм же!");
    }

    #[test]
    fn review_answers_are_validated_before_they_are_kept() {
        let store = Store::in_memory().unwrap();
        store.add_memory("Живёт в Караганде").unwrap();
        let reply = r#"Вот: {"lessons":[
            {"kind":"preference","text":"Отвечать короче, без вопроса в конце"},
            {"kind":"alias","heard":"телега","meant":"Telegram","text":"мессенджер"},
            {"kind":"hearing","heard":"и","meant":"в"},
            {"kind":"preference","text":"Пароль от почты qwerty"},
            {"kind":"mood","text":"устал"}
        ],"facts":["живёт в Караганде","Работает дизайнером"],"forget":[999]}"#;
        let (saved, facts, forgotten) = apply(&store, reply);
        assert_eq!((saved, facts, forgotten), (2, 1, 0));
        let mem: Vec<String> = store.memories().unwrap().into_iter().map(|m| m.text).collect();
        assert_eq!(mem, vec!["Живёт в Караганде", "Работает дизайнером"]);
        assert_eq!(apply(&store, "не JSON"), (0, 0, 0));
    }
}
