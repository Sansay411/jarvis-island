//! IPC for the mentor: its state, «Перезагрузка» and what happens in it.

use std::sync::Arc;

use serde_json::Value;
use tauri::{AppHandle, Manager, State};

use crate::error::AppResult;
use crate::hub::Hub;
use crate::mentor::{FeedCard, Intervention, Mentor, MentorState, Reward, Step};

#[tauri::command]
pub fn mentor_state(mentor: State<'_, Arc<Mentor>>) -> AppResult<MentorState> {
    mentor.state()
}

#[tauri::command]
pub fn mentor_pending(mentor: State<'_, Arc<Mentor>>) -> Option<Intervention> {
    mentor.pending()
}

/// «Перезагрузка» on request (island, planner, Telegram), optionally
/// straight on one activity.
#[tauri::command]
pub async fn mentor_open_reset(mentor: State<'_, Arc<Mentor>>, screen: Option<String>) -> AppResult<()> {
    mentor.voluntary(screen)
}

#[tauri::command]
pub fn mentor_feed(mentor: State<'_, Arc<Mentor>>, count: Option<u32>) -> AppResult<Vec<FeedCard>> {
    mentor.feed(count.unwrap_or(7).clamp(3, 12) as usize)
}

#[tauri::command]
pub fn mentor_answer(mentor: State<'_, Arc<Mentor>>, id: String, correct: bool, quick: bool, xp: u32) -> AppResult<()> {
    mentor.answer(&id, correct, quick, xp)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn mentor_complete(
    mentor: State<'_, Arc<Mentor>>,
    kind: String,
    track: String,
    title: String,
    minutes: f64,
    score: u32,
    xp: u32,
    after_close: bool,
) -> AppResult<Reward> {
    mentor.complete(&kind, &track, &title, minutes, score, xp, after_close)
}

#[tauri::command]
pub fn mentor_best(mentor: State<'_, Arc<Mentor>>, game: String) -> u32 {
    mentor.best(&game)
}

#[tauri::command]
pub fn mentor_words(mentor: State<'_, Arc<Mentor>>) -> AppResult<Vec<(String, String)>> {
    mentor.words(40)
}

#[tauri::command]
pub fn mentor_workout(mentor: State<'_, Arc<Mentor>>) -> Value {
    mentor.workout()
}

#[tauri::command]
pub async fn mentor_steps(mentor: State<'_, Arc<Mentor>>) -> AppResult<Vec<Step>> {
    let m = mentor.inner().clone();
    crate::platform::blocking(move || m.steps()).await
}

/// Ten minutes on a step: a focus session, and the project in VS Code.
#[tauri::command]
pub async fn mentor_start_step(app: AppHandle, title: String, minutes: u32, path: Option<String>) -> AppResult<()> {
    app.state::<Arc<Hub>>().focus_start(title, minutes.clamp(5, 120), None)?;
    if let Some(dir) = path.filter(|p| std::path::Path::new(p).is_dir()) {
        crate::platform::blocking(move || {
            match crate::tools::vscode_exe() {
                Some(code) => {
                    let _ = std::process::Command::new(code).arg(&dir).spawn();
                }
                None => crate::platform::apps::shell_open(&dir, None, None)?,
            }
            Ok(())
        })
        .await?;
    }
    Ok(())
}

/// «Это была учёба»: this video is free today; reopen it.
#[tauri::command]
pub async fn mentor_allow(mentor: State<'_, Arc<Mentor>>, title: String, url: Option<String>) -> AppResult<()> {
    mentor.allow_title(&title);
    let _ = mentor.log("learning", "", &title, 0.0, 0, 0, serde_json::json!({ "url": url }));
    if let Some(u) = url.filter(|u| !u.trim().is_empty()) {
        let target = if u.starts_with("http") { u } else { format!("https://{u}") };
        crate::platform::blocking(move || crate::platform::apps::shell_open(&target, None, None)).await?;
    }
    Ok(())
}

#[tauri::command]
pub fn mentor_review(mentor: State<'_, Arc<Mentor>>, went_well: String, learned: String, tomorrow: String) -> AppResult<Reward> {
    mentor.review(&went_well, &learned, &tomorrow)
}

// ---------- Telegram ----------

use crate::telegram::{Bot, BotStatus};

#[tauri::command]
pub fn telegram_status(app: AppHandle, bot: State<'_, Arc<Bot>>) -> BotStatus {
    bot.status(&app)
}

/// Save (or, with an empty string, remove) the bot token from @BotFather.
#[tauri::command]
pub fn telegram_set_token(app: AppHandle, bot: State<'_, Arc<Bot>>, token: String) -> AppResult<BotStatus> {
    let token = token.trim();
    if token.is_empty() {
        crate::store::secrets::delete(crate::store::secrets::TELEGRAM_BOT_TOKEN)?;
    } else {
        let looks_right = token.split_once(':').is_some_and(|(id, key)| id.chars().all(|c| c.is_ascii_digit()) && key.len() >= 30);
        if !looks_right {
            return Err(crate::error::AppError::invalid("Токен выглядит как «123456789:AA…» — скопируйте его из @BotFather целиком"));
        }
        crate::store::secrets::set(crate::store::secrets::TELEGRAM_BOT_TOKEN, token)?;
    }
    Ok(bot.status(&app))
}

/// A one-time link that binds the user's chat.
#[tauri::command]
pub fn telegram_pair(app: AppHandle, bot: State<'_, Arc<Bot>>) -> BotStatus {
    bot.pair();
    bot.status(&app)
}

#[tauri::command]
pub fn telegram_unbind(app: AppHandle, bot: State<'_, Arc<Bot>>, state: State<'_, crate::state::AppState>) -> AppResult<BotStatus> {
    let mut s = state.store.settings();
    s.telegram.chat_id = 0;
    state.store.save_settings(&s)?;
    Ok(bot.status(&app))
}
