//! The user's own Telegram bot, run inside JARVIS (long polling; no server
//! of ours). It serves one chat — the one bound with «/start CODE» from
//! Settings — and turns the phone into a remote for the planner and the
//! mentor: today's plan, «План дня», quiz cards as native Telegram quizzes,
//! a workout, focus on the PC, stats, quick «/add завтра 18:00 зал», the
//! evening review, reminders before events, and plain messages to JARVIS.
//! Works while the PC is on; Telegram keeps messages for a day meanwhile.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Datelike, Duration as CDur, Local, NaiveDate, NaiveTime, Weekday};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::store::secrets;

const API: &str = "https://api.telegram.org/bot";

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotStatus {
    pub has_token: bool,
    pub username: String,
    pub bound: bool,
    pub running: bool,
    pub error: Option<String>,
    /// t.me link that binds the chat (while pairing).
    pub link: Option<String>,
}

#[derive(Default)]
struct Inner {
    username: String,
    token_seen: String,
    pairing: Option<String>,
    error: Option<String>,
    running: bool,
    /// poll id → (card id, right option)
    polls: HashMap<String, (String, usize)>,
    /// chat → review answers so far
    review: Option<Vec<String>>,
}

#[derive(Default)]
pub struct Bot {
    inner: Mutex<Inner>,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn http() -> reqwest::Client {
    crate::ai::openai::http_client()
}

async fn call(token: &str, method: &str, body: Value, timeout: u64) -> AppResult<Value> {
    let res = http().post(format!("{API}{token}/{method}")).json(&body).timeout(Duration::from_secs(timeout)).send().await?;
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if v.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(v.get("result").cloned().unwrap_or(Value::Null));
    }
    let code = v.get("error_code").and_then(Value::as_i64).unwrap_or(0);
    let desc = v.get("description").and_then(Value::as_str).unwrap_or("ошибка Telegram");
    Err(match code {
        401 | 404 => AppError::PermissionDenied("Telegram не принял токен бота — проверьте его в @BotFather".into()),
        409 => AppError::Provider("Этот бот уже читает сообщения в другой программе".into()),
        _ => AppError::Provider(format!("Telegram: {desc}")),
    })
}

fn menu() -> Value {
    json!({
        "keyboard": [["Сегодня", "План дня"], ["Карточка", "Разминка"], ["Фокус 25", "Статистика"]],
        "resize_keyboard": true,
        "is_persistent": true
    })
}

impl Bot {
    pub fn status(&self, app: &AppHandle) -> BotStatus {
        let s = app.state::<crate::state::AppState>().store.settings().telegram;
        let i = self.inner.lock();
        let username = if i.username.is_empty() { s.bot_username.clone() } else { i.username.clone() };
        BotStatus {
            has_token: secrets::exists(secrets::TELEGRAM_BOT_TOKEN),
            link: i.pairing.as_ref().filter(|_| !username.is_empty()).map(|c| format!("https://t.me/{username}?start={c}")),
            username,
            bound: s.chat_id != 0,
            running: i.running,
            error: i.error.clone(),
        }
    }

    /// A fresh one-time code for «/start CODE».
    pub fn pair(&self) -> String {
        let code = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        self.inner.lock().pairing = Some(code.clone());
        code
    }
}

async fn send(token: &str, chat: i64, text: &str, markup: Option<Value>) -> AppResult<()> {
    let mut body = json!({ "chat_id": chat, "text": text.chars().take(4000).collect::<String>(), "parse_mode": "HTML", "disable_web_page_preview": true });
    if let Some(m) = markup {
        body["reply_markup"] = m;
    }
    call(token, "sendMessage", body, 20).await.map(|_| ())
}

fn hm(s: &str) -> &str {
    s.get(11..16).unwrap_or("")
}

/// Today's plan as a message.
fn today_text(app: &AppHandle) -> (String, Option<Value>) {
    let today = Local::now().date_naive();
    let mut lines = vec![format!("<b>{}</b>", esc(&Local::now().format("%A, %d.%m").to_string()).replace("Monday", "Понедельник").replace("Tuesday", "Вторник").replace("Wednesday", "Среда").replace("Thursday", "Четверг").replace("Friday", "Пятница").replace("Saturday", "Суббота").replace("Sunday", "Воскресенье"))];
    if let Some(p) = app.try_state::<Arc<crate::planner::Planner>>() {
        let evs = p.day(today).unwrap_or_default();
        if evs.is_empty() {
            lines.push("В календаре пусто — нажми «План дня», и я заполню свободное время.".into());
        }
        for e in evs {
            let when = if e.all_day { "весь день".to_string() } else { format!("{}–{}", hm(&e.start), hm(&e.end)) };
            lines.push(format!("{} {} · {}", if e.done { "✓" } else { "•" }, when, esc(&e.title)));
        }
    }
    let mut buttons = Vec::new();
    if let Some(hub) = app.try_state::<Arc<crate::hub::Hub>>() {
        let d = today.format("%Y-%m-%d").to_string();
        let tasks: Vec<_> = hub.tasks_list().unwrap_or_default().into_iter().filter(|t| !t.completed && t.scheduled_date.as_deref().is_none_or(|x| x <= d.as_str())).take(6).collect();
        if !tasks.is_empty() {
            lines.push(String::new());
            lines.push("<b>Задачи</b>".into());
        }
        for t in tasks {
            lines.push(format!("◦ {}{}", esc(&t.title), t.scheduled_time.as_deref().map(|x| format!(" · {x}")).unwrap_or_default()));
            buttons.push(vec![json!({ "text": format!("✓ {}", t.title.chars().take(30).collect::<String>()), "callback_data": format!("task:{}", t.id) })]);
        }
    }
    if let Some(rems) = app.try_state::<Arc<crate::reminders::Reminders>>() {
        let items: Vec<_> = rems
            .list()
            .into_iter()
            .filter(|r| {
                !r.done
                    && r.kind != "timer"
                    && (r.repeat.is_some()
                        || chrono::DateTime::parse_from_rfc3339(&r.due_at)
                            .is_ok_and(|d| d.with_timezone(&Local).date_naive() == today))
            })
            .take(8)
            .collect();
        if !items.is_empty() {
            lines.push(String::new());
            lines.push("<b>Напоминания</b>".into());
            for r in items {
                let due_local = chrono::DateTime::parse_from_rfc3339(&r.due_at).ok().map(|d| d.with_timezone(&Local));
                let when = match due_local {
                    Some(d) if d.date_naive() == today => d.format("%H:%M").to_string(),
                    Some(d) => d.format("%d.%m %H:%M").to_string(),
                    None => String::new(),
                };
                let rep = r.repeat_label.as_deref().map(|l| format!(" ({})", esc(l))).unwrap_or_default();
                let prefix = if r.kind == "action" { "↻ " } else { "• " };
                lines.push(format!("{prefix}{when} · {}{rep}", esc(&r.text)));
            }
        }
    }
    (lines.join("\n"), (!buttons.is_empty()).then(|| json!({ "inline_keyboard": buttons })))
}

fn stats_text(app: &AppHandle) -> String {
    let Some(m) = app.try_state::<Arc<crate::mentor::Mentor>>() else { return "Наставник не запущен".into() };
    let Ok(s) = m.state() else { return "Не удалось прочитать статистику".into() };
    format!(
        "<b>Уровень {} · {}</b> ({} / {} XP)\nСерия: {} дн. (рекорд {})\nСегодня: {} XP из {} · фокус {} мин\nКороткие видео: {} из {} мин{}",
        s.level,
        esc(&s.level_name),
        s.xp_total,
        s.level_to,
        s.streak,
        s.best_streak,
        s.today_xp,
        s.goal_xp,
        s.focus_min_today,
        s.short.used_sec / 60,
        s.short.limit_sec / 60,
        s.strict.map(|r| format!("\nСейчас закрыты: {}", esc(&r))).unwrap_or_default()
    )
}

/// «завтра 18:00 зал 45 мин» → (date, time, minutes, title).
pub fn parse_add(text: &str, today: NaiveDate) -> (NaiveDate, Option<NaiveTime>, u32, String) {
    let mut date = today;
    let mut time = None;
    let mut minutes = 60;
    let mut rest = Vec::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut i = 0;
    let weekdays = [("понедельник", Weekday::Mon), ("вторник", Weekday::Tue), ("среду", Weekday::Wed), ("среда", Weekday::Wed), ("четверг", Weekday::Thu), ("пятницу", Weekday::Fri), ("пятница", Weekday::Fri), ("субботу", Weekday::Sat), ("суббота", Weekday::Sat), ("воскресенье", Weekday::Sun)];
    while i < words.len() {
        let w = words[i].to_lowercase();
        let w = w.trim_matches(|c: char| c == ',' || c == '.');
        let next = words.get(i + 1).map(|x| x.to_lowercase());
        if w == "сегодня" {
            date = today;
        } else if w == "завтра" {
            date = today + CDur::days(1);
        } else if w == "послезавтра" {
            date = today + CDur::days(2);
        } else if w == "в" && next.as_deref().is_some_and(|n| weekdays.iter().any(|(name, _)| n.starts_with(name))) {
        } else if let Some((_, wd)) = weekdays.iter().find(|(name, _)| w.starts_with(name)) {
            let ahead = (wd.num_days_from_monday() as i64 - today.weekday().num_days_from_monday() as i64 + 7) % 7;
            date = today + CDur::days(if ahead == 0 { 7 } else { ahead });
        } else if let Ok(t) = NaiveTime::parse_from_str(w, "%H:%M") {
            time = Some(t);
        } else if let (Ok(n), Some(unit)) = (w.parse::<u32>(), next.as_deref()) {
            if unit.starts_with("мин") {
                minutes = n.clamp(5, 600);
                i += 1;
            } else if unit.starts_with('ч') {
                minutes = (n * 60).clamp(5, 600);
                i += 1;
            } else {
                rest.push(words[i]);
            }
        } else if let Some(d) = NaiveDate::parse_from_str(&format!("{w}.{}", today.year()), "%d.%m.%Y").ok() {
            date = if d < today { d.with_year(today.year() + 1).unwrap_or(d) } else { d };
        } else if w != "в" || next.as_deref().and_then(|n| NaiveTime::parse_from_str(n, "%H:%M").ok()).is_none() {
            rest.push(words[i]);
        }
        i += 1;
    }
    let title = rest.join(" ").trim().to_string();
    (date, time, minutes, if title.is_empty() { "Дело".into() } else { title })
}

async fn send_card(app: &AppHandle, bot: &Bot, token: &str, chat: i64) -> AppResult<()> {
    let m = app.state::<Arc<crate::mentor::Mentor>>();
    let cards = m.feed(6)?;
    let Some(card) = cards.iter().find(|c| matches!(c.kind.as_str(), "quiz" | "code" | "word")).or(cards.first()).cloned() else {
        return send(token, chat, "Карточки закончились — загляну за новыми.", None).await;
    };
    let d = &card.data;
    let s = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    match card.kind.as_str() {
        "quiz" | "code" | "word" => {
            let question = match card.kind.as_str() {
                "word" => format!("{} [{}] — это…", s("word"), s("ipa")),
                "code" => format!("{}\n\n{}", s("title"), s("code")),
                _ => s("q"),
            };
            let options: Vec<String> = d.get("options").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(|t| t.chars().take(100).collect())).collect()).unwrap_or_default();
            let answer = d.get("answer").and_then(Value::as_u64).unwrap_or(0) as usize;
            let explain: String = if card.kind == "word" { format!("{} — {}", s("ex"), s("exRu")) } else { s("explain") }.chars().take(190).collect();
            let res = call(
                token,
                "sendPoll",
                json!({ "chat_id": chat, "question": question.chars().take(290).collect::<String>(), "options": options.iter().map(|o| json!({ "text": o })).collect::<Vec<_>>(), "type": "quiz", "correct_option_id": answer, "is_anonymous": false, "explanation": explain }),
                20,
            )
            .await?;
            if let Some(id) = res.pointer("/poll/id").and_then(Value::as_str) {
                bot.inner.lock().polls.insert(id.to_string(), (card.id.clone(), answer));
            }
            Ok(())
        }
        _ => {
            let text = format!("<b>{}</b>\n{}", esc(&s("title")), esc(&s("body")));
            send(token, chat, &text, Some(json!({ "inline_keyboard": [[{ "text": "Сделал", "callback_data": format!("card:{}", card.id) }]] }))).await
        }
    }
}

async fn handle_text(app: &AppHandle, bot: &Bot, token: &str, chat: i64, text: &str) -> AppResult<()> {
    let t = text.trim();
    let lower = t.to_lowercase();
    // The evening review is a three-question conversation.
    let in_review = bot.inner.lock().review.clone();
    if let Some(mut answers) = in_review {
        if !t.starts_with('/') {
            answers.push(t.to_string());
            let prompts = ["Что ты сегодня узнал? Своими словами.", "Главное дело на завтра — одно."];
            if answers.len() < 3 {
                bot.inner.lock().review = Some(answers.clone());
                return send(token, chat, prompts[answers.len() - 1], None).await;
            }
            bot.inner.lock().review = None;
            let m = app.state::<Arc<crate::mentor::Mentor>>();
            let r = m.review(&answers[0], &answers[1], &answers[2])?;
            return send(token, chat, &format!("Итог записан: +{} XP. Главное на завтра — в задачах. Серия: {} дн.", r.xp, r.streak), Some(menu())).await;
        }
        bot.inner.lock().review = None;
    }
    if lower == "/start" || lower == "/help" {
        return send(token, chat, "Я Джарвис. Кнопки внизу — план, карточки, разминка, фокус. «/add завтра 18:00 зал 45 мин» — в календарь. Любой другой текст — просьба мне на компьютере.", Some(menu())).await;
    }
    if lower == "сегодня" || lower == "/today" {
        let (text, kb) = today_text(app);
        return send(token, chat, &text, kb).await;
    }
    if lower == "статистика" || lower == "/stats" {
        return send(token, chat, &stats_text(app), None).await;
    }
    if lower == "карточка" || lower == "/learn" {
        return send_card(app, bot, token, chat).await;
    }
    if lower == "разминка" || lower == "/move" {
        let w = app.state::<Arc<crate::mentor::Mentor>>().workout();
        let list: Vec<String> = w
            .as_array()
            .map(|a| a.iter().map(|m| format!("• {} — {}", m["title"].as_str().unwrap_or(""), m["reps"].as_u64().map(|r| format!("{r} раз")).unwrap_or_else(|| format!("{} с", m["seconds"].as_u64().unwrap_or(30))))).collect())
            .unwrap_or_default();
        return send(token, chat, &format!("<b>Разминка на три минуты</b>\n{}", esc(&list.join("\n"))), Some(json!({ "inline_keyboard": [[{ "text": "Сделал", "callback_data": "workout" }]] }))).await;
    }
    if lower.starts_with("фокус") || lower.starts_with("/focus") {
        let mins: u32 = lower.split_whitespace().nth(1).and_then(|n| n.parse().ok()).unwrap_or(25);
        app.state::<Arc<crate::hub::Hub>>().focus_start("Фокус из Telegram".into(), mins.clamp(5, 180), None)?;
        return send(token, chat, &format!("Фокус на {mins} мин запущен на компьютере. Короткие видео закрыты до конца."), None).await;
    }
    if lower.starts_with("план дня") || lower.starts_with("/plan") {
        // «план дня: отчёт до обеда, вечером зал» — the rest is the wishes.
        let wishes = lower.trim_start_matches("план дня").trim_start_matches("/plan").trim_start_matches([':', ' ', ',']).trim().to_string();
        let p = app.state::<Arc<crate::planner::Planner>>().inner().clone();
        let now = Local::now().naive_local();
        let blocks = crate::planner::suggest::suggest(app, &p, now.date(), now, Some(wishes.as_str()).filter(|w| !w.is_empty())).await?;
        if blocks.is_empty() {
            return send(token, chat, "Свободного времени сегодня почти не осталось.", None).await;
        }
        let lines: Vec<String> = blocks.iter().map(|b| format!("• {}–{} {}", hm(&b.start), hm(&b.end), esc(&b.title))).collect();
        let payload = serde_json::to_string(&blocks).unwrap_or_default();
        let _ = app.state::<crate::state::AppState>().store.kv_set("tg_last_plan", &payload);
        return send(token, chat, &format!("<b>План на свободное время</b>\n{}", lines.join("\n")), Some(json!({ "inline_keyboard": [[{ "text": "Добавить в календарь", "callback_data": "plan:accept" }]] }))).await;
    }
    if lower == "/review" || lower == "итог" || lower == "итог дня" {
        bot.inner.lock().review = Some(Vec::new());
        return send(token, chat, "Итог дня, три вопроса. Что сегодня получилось?", None).await;
    }
    if let Some(rest) = t.strip_prefix("/add").or_else(|| t.strip_prefix("Добавь ")).or_else(|| t.strip_prefix("добавь ")) {
        let (date, time, minutes, title) = parse_add(rest, Local::now().date_naive());
        let ds = date.format("%Y-%m-%d").to_string();
        match time {
            Some(tm) => {
                let start = date.and_time(tm);
                let input = crate::planner::EventInput {
                    title: title.clone(),
                    start: start.format("%Y-%m-%dT%H:%M").to_string(),
                    end: Some((start + CDur::minutes(minutes as i64)).format("%Y-%m-%dT%H:%M").to_string()),
                    ..Default::default()
                };
                app.state::<Arc<crate::planner::Planner>>().create(input)?;
                return send(token, chat, &format!("В календаре: {} {}, {} — «{}»", date.format("%d.%m"), tm.format("%H:%M"), crate::mentor::minutes_ru(minutes), esc(&title)), None).await;
            }
            None => {
                app.state::<Arc<crate::hub::Hub>>().task_add(title.clone(), Some(ds), None, None)?;
                return send(token, chat, &format!("Задача на {}: «{}»", date.format("%d.%m"), esc(&title)), None).await;
            }
        }
    }
    if t.starts_with('/') {
        return send(token, chat, "Такой команды нет. /help — что я умею.", None).await;
    }
    if !app.state::<crate::state::AppState>().store.settings().telegram.agent {
        return send(token, chat, "Просьбы с телефона выключены в настройках Джарвиса.", None).await;
    }
    let _ = call(token, "sendChatAction", json!({ "chat_id": chat, "action": "typing" }), 10).await;
    let agent = app.state::<Arc<crate::agent::Agent>>().inner().clone();
    if agent.is_busy() {
        return send(token, chat, "Джарвис сейчас занят задачей на компьютере — повтори через минуту.", None).await;
    }
    let answer = match tokio::time::timeout(Duration::from_secs(180), agent.remote_turn(t.to_string())).await {
        Ok(Ok(a)) => a,
        Ok(Err(e)) => format!("Не получилось: {e}"),
        Err(_) => "Задача идёт дольше трёх минут — результат будет на компьютере.".into(),
    };
    send(token, chat, &esc(&answer), None).await
}

async fn handle_callback(app: &AppHandle, token: &str, chat: i64, id: &str, data: &str) -> AppResult<()> {
    let mut note = "Готово";
    if let Some(task) = data.strip_prefix("task:") {
        app.state::<Arc<crate::hub::Hub>>().task_set_completed(task, true)?;
        note = "Задача выполнена: +10 XP";
    } else if let Some(card) = data.strip_prefix("card:") {
        app.state::<Arc<crate::mentor::Mentor>>().answer(card, true, true, 15)?;
        note = "Засчитано: +15 XP";
    } else if data == "workout" {
        app.state::<Arc<crate::mentor::Mentor>>().complete("workout", "body", "Разминка из Telegram", 3.0, 3, 40, false)?;
        note = "Разминка засчитана: +40 XP и +3 минуты";
    } else if data == "plan:accept" {
        let store = &app.state::<crate::state::AppState>().store;
        let blocks: Vec<crate::planner::suggest::Suggestion> = store.kv_get("tg_last_plan").and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
        let p = app.state::<Arc<crate::planner::Planner>>();
        for b in &blocks {
            let _ = p.create(crate::planner::EventInput { title: b.title.clone(), start: b.start.clone(), end: Some(b.end.clone()), category: Some(b.category.clone()), notes: Some(b.why.clone()), ..Default::default() });
        }
        let _ = store.kv_set("tg_last_plan", "");
        note = "План в календаре";
    } else if let Some(rest) = data.strip_prefix("focus:") {
        let mins: u32 = rest.parse().unwrap_or(25);
        app.state::<Arc<crate::hub::Hub>>().focus_start("Блок из календаря".into(), mins.clamp(5, 180), None)?;
        note = "Фокус запущен на компьютере";
    }
    let _ = call(token, "answerCallbackQuery", json!({ "callback_query_id": id, "text": note }), 10).await;
    let _ = chat;
    Ok(())
}

async fn handle(app: &AppHandle, bot: &Bot, token: &str, u: &Value) -> AppResult<()> {
    let store = app.state::<crate::state::AppState>().store.clone();
    let bound = store.settings().telegram.chat_id;
    if let Some(msg) = u.get("message") {
        let chat = msg.pointer("/chat/id").and_then(Value::as_i64).unwrap_or(0);
        let text = msg.get("text").and_then(Value::as_str).unwrap_or("");
        if let Some(code) = text.strip_prefix("/start ").map(str::trim) {
            let pairing = bot.inner.lock().pairing.clone();
            if pairing.as_deref() == Some(code) {
                let mut s = store.settings();
                s.telegram.chat_id = chat;
                store.save_settings(&s)?;
                bot.inner.lock().pairing = None;
                return send(token, chat, "Готово, этот чат привязан к Джарвису. Утром пришлю план, вечером — итог дня, перед событиями — напоминания.", Some(menu())).await;
            }
        }
        if chat != bound || bound == 0 {
            return send(token, chat, "Это личный бот Джарвиса. Привязать чат можно в настройках Джарвиса на компьютере.", None).await;
        }
        return handle_text(app, bot, token, chat, text).await;
    }
    if let Some(cb) = u.get("callback_query") {
        let chat = cb.pointer("/message/chat/id").and_then(Value::as_i64).unwrap_or(0);
        if chat != bound || bound == 0 {
            return Ok(());
        }
        let id = cb.get("id").and_then(Value::as_str).unwrap_or("");
        let data = cb.get("data").and_then(Value::as_str).unwrap_or("");
        return handle_callback(app, token, chat, id, data).await;
    }
    if let Some(pa) = u.get("poll_answer") {
        let user = pa.pointer("/user/id").and_then(Value::as_i64).unwrap_or(0);
        if user != bound {
            return Ok(());
        }
        let poll = pa.get("poll_id").and_then(Value::as_str).unwrap_or("");
        let chosen = pa.pointer("/option_ids/0").and_then(Value::as_u64).map(|x| x as usize);
        let entry = bot.inner.lock().polls.remove(poll);
        if let (Some((card, right)), Some(chosen)) = (entry, chosen) {
            let ok = chosen == right;
            app.state::<Arc<crate::mentor::Mentor>>().answer(&card, ok, false, if ok { 10 } else { 3 })?;
            if ok {
                let _ = send(token, bound, "Верно: +10 XP. Ещё одну?", Some(json!({ "keyboard": [["Карточка", "Статистика"]], "resize_keyboard": true, "one_time_keyboard": true }))).await;
            }
        }
    }
    Ok(())
}

fn due(at: &str, now: NaiveTime) -> bool {
    NaiveTime::parse_from_str(at.trim(), "%H:%M").is_ok_and(|t| now >= t && now < t + CDur::minutes(30))
}

/// Morning plan, evening review, reminders before events.
async fn schedule(app: &AppHandle, token: &str) -> AppResult<()> {
    let store = app.state::<crate::state::AppState>().store.clone();
    let s = store.settings().telegram;
    if s.chat_id == 0 {
        return Ok(());
    }
    let now = Local::now();
    let today = now.date_naive().format("%Y-%m-%d").to_string();
    let once = |key: String| -> bool {
        if store.kv_get(&key).is_some() {
            return false;
        }
        let _ = store.kv_set(&key, "1");
        true
    };
    if due(&s.morning, now.time()) && once(format!("tg_morning_{today}")) {
        let (plan, kb) = today_text(app);
        let if_then = store.settings().mentor.if_then;
        let tail = if if_then.trim().is_empty() { String::new() } else { format!("\n\nЕсли потянет в ленту — «{}».", esc(&if_then)) };
        let _ = send(token, s.chat_id, &format!("Доброе утро. {}{}", plan, tail), kb).await;
    }
    if due(&s.evening, now.time()) && once(format!("tg_evening_{today}")) {
        let _ = send(token, s.chat_id, &format!("{}\n\nНапиши «итог» — подведём день за две минуты.", stats_text(app)), None).await;
    }
    if s.remind_before > 0 {
        if let Some(p) = app.try_state::<Arc<crate::planner::Planner>>() {
            for e in p.day(now.date_naive()).unwrap_or_default().into_iter().filter(|e| !e.all_day && !e.done) {
                let Some(crate::planner::When::At(start)) = crate::planner::parse_when(&e.start) else { continue };
                let lead = (start - now.naive_local()).num_minutes();
                if lead >= 0 && lead <= s.remind_before as i64 && once(format!("tg_remind_{}_{}", e.id, e.start)) {
                    let mins = match crate::planner::parse_when(&e.end) {
                        Some(crate::planner::When::At(end)) => (end - start).num_minutes().clamp(5, 180),
                        _ => 25,
                    };
                    let _ = send(
                        token,
                        s.chat_id,
                        &format!("Через {} мин: <b>{}</b> ({}–{})", lead, esc(&e.title), hm(&e.start), hm(&e.end)),
                        Some(json!({ "inline_keyboard": [[{ "text": "Начать фокус на компьютере", "callback_data": format!("focus:{mins}") }]] })),
                    )
                    .await;
                }
            }
        }
    }
    Ok(())
}

pub fn spawn(app: AppHandle, bot: Arc<Bot>) {
    // Scheduled messages.
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(45)).await;
                let enabled = app.state::<crate::state::AppState>().store.settings().telegram.enabled;
                if let (true, Ok(Some(token))) = (enabled, secrets::get(secrets::TELEGRAM_BOT_TOKEN)) {
                    if let Err(e) = schedule(&app, &token).await {
                        tracing::debug!("telegram schedule: {e}");
                    }
                }
            }
        });
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(7)).await;
        let mut offset: i64 = 0;
        loop {
            let enabled = app.state::<crate::state::AppState>().store.settings().telegram.enabled;
            let token = secrets::get(secrets::TELEGRAM_BOT_TOKEN).ok().flatten().filter(|t| t.contains(':'));
            let Some(token) = token.filter(|_| enabled) else {
                bot.inner.lock().running = false;
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            };
            if bot.inner.lock().token_seen != token {
                match call(&token, "getMe", json!({}), 20).await {
                    Ok(me) => {
                        let name = me.get("username").and_then(Value::as_str).unwrap_or("").to_string();
                        let store = app.state::<crate::state::AppState>().store.clone();
                        let mut s = store.settings();
                        if s.telegram.bot_username != name {
                            s.telegram.bot_username = name.clone();
                            let _ = store.save_settings(&s);
                        }
                        let mut i = bot.inner.lock();
                        i.username = name;
                        i.token_seen = token.clone();
                        i.error = None;
                    }
                    Err(e) => {
                        bot.inner.lock().error = Some(e.to_string());
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        continue;
                    }
                }
            }
            bot.inner.lock().running = true;
            match call(&token, "getUpdates", json!({ "offset": offset, "timeout": 50, "allowed_updates": ["message", "callback_query", "poll_answer"] }), 65).await {
                Ok(Value::Array(updates)) => {
                    bot.inner.lock().error = None;
                    for u in updates {
                        offset = offset.max(u.get("update_id").and_then(Value::as_i64).unwrap_or(0) + 1);
                        if let Err(e) = handle(&app, &bot, &token, &u).await {
                            tracing::warn!("telegram update: {e}");
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    bot.inner.lock().error = Some(e.to_string());
                    tokio::time::sleep(Duration::from_secs(15)).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_add_understands_dates_times_and_lengths() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap(); // Monday
        let (d, t, m, title) = parse_add("завтра 18:00 зал 45 мин", today);
        assert_eq!((d.day(), t.map(|x| x.format("%H:%M").to_string()), m, title.as_str()), (24, Some("18:00".into()), 45, "зал"));
        let (d, t, m, title) = parse_add(" в пятницу в 9:30 английский 1 ч", today);
        assert_eq!((d.day(), t.is_some(), m, title.as_str()), (27, true, 60, "английский"));
        let (d, t, _, title) = parse_add("купить молоко", today);
        assert_eq!((d, t, title.as_str()), (today, None, "купить молоко"));
        let (d, _, _, title) = parse_add("05.12 отчёт по проекту", today);
        assert_eq!((d.month(), d.day(), title.as_str()), (12, 5, "отчёт по проекту"));
        // Monday on a Monday: next week.
        let (d, _, _, _) = parse_add("в понедельник созвон 10:00", today);
        assert_eq!(d.day(), 30);
        assert_eq!(esc("<b>&"), "&lt;b&gt;&amp;");
    }
}
