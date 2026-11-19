//! The «Стрик» deck: the user's real GitHub contribution calendar, with their
//! name and avatar.
//!
//! The local mix (chat messages + agent actions + `git log` of every folder in
//! Documents) was unreadable — «565 действий за год» said nothing about where
//! the number came from. GitHub's own calendar is the number the user knows.
//!
//! Sources, best first:
//! 1. GitHub GraphQL `contributionsCollection.contributionCalendar` with the
//!    token of the GitHub CLI the user is already signed in to (`gh auth
//!    token`). Includes private contributions when the login is the signed-in
//!    account. The token stays in memory and goes only to api.github.com.
//! 2. Without the CLI: the public profile (`api.github.com/users/<login>`) and
//!    the public calendar page `github.com/users/<login>/contributions` — the
//!    same cells the profile shows (`data-date`, `data-level`, tool-tip text).
//!
//! The last result is cached in the `kv` table, so the deck opens instantly
//! and offline; it is refetched at most every `FRESH_FOR`.

use std::collections::HashMap;
use std::time::Duration;

use base64::Engine;
use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::store::Store;

const KV_LOGIN: &str = "github_login";
const KV_CACHE: &str = "github_activity";
const FRESH_FOR: chrono::Duration = chrono::Duration::minutes(20);
const UA: &str = "Jarvis-desktop";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GithubDay {
    pub date: String,
    pub count: u32,
    /// 0..=4 as GitHub shades it.
    pub level: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GithubActivity {
    pub login: String,
    pub name: String,
    /// `data:image/…;base64,…` — no remote image in the island webview.
    pub avatar: Option<String>,
    pub profile_url: String,
    pub total: u32,
    pub current_streak: u32,
    pub longest_streak: u32,
    /// Oldest first; GitHub's window (≈ 53 weeks, ends today).
    pub days: Vec<GithubDay>,
    /// `"gh"` (GraphQL with the CLI token, private included) or `"public"`.
    pub source: String,
    pub fetched_at: String,
}

/// What the deck shows: the calendar, or why there is none yet.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GithubState {
    pub login: Option<String>,
    pub activity: Option<GithubActivity>,
    /// The last refresh failed (the cached calendar, if any, is still shown).
    pub error: Option<String>,
}

pub fn saved_login(store: &Store) -> Option<String> {
    store.kv_get(KV_LOGIN).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn cached(store: &Store) -> Option<GithubActivity> {
    store.kv_get(KV_CACHE).and_then(|s| serde_json::from_str(&s).ok())
}

fn is_fresh(a: &GithubActivity) -> bool {
    chrono::DateTime::parse_from_rfc3339(&a.fetched_at)
        .is_ok_and(|t| chrono::Utc::now().signed_duration_since(t) < FRESH_FOR)
}

/// Save (or clear, with an empty string) the login and fetch its calendar.
pub fn set_login(store: &Store, login: &str) -> AppResult<GithubState> {
    let login = login.trim().trim_start_matches('@').trim_start_matches("https://github.com/").trim_end_matches('/');
    if login.is_empty() {
        store.kv_set(KV_LOGIN, "")?;
        store.kv_set(KV_CACHE, "")?;
        return Ok(GithubState { login: None, activity: None, error: None });
    }
    if !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || login.len() > 39 {
        return Err(AppError::invalid("Логин GitHub — латинские буквы, цифры и дефис"));
    }
    store.kv_set(KV_LOGIN, login)?;
    store.kv_set(KV_CACHE, "")?;
    state(store, true)
}

/// The calendar for the saved login (or the signed-in GitHub CLI account).
/// `force` skips the cache.
pub fn state(store: &Store, force: bool) -> AppResult<GithubState> {
    let cache = cached(store);
    let mut login = saved_login(store);
    if !force {
        if let Some(a) = cache.as_ref().filter(|a| is_fresh(a) && login.as_deref().is_none_or(|l| l.eq_ignore_ascii_case(&a.login))) {
            return Ok(GithubState { login: Some(a.login.clone()), activity: Some(a.clone()), error: None });
        }
    }
    let token = cli_token();
    if login.is_none() {
        // Nothing saved: the account the GitHub CLI is signed in to.
        login = token.as_deref().and_then(|t| viewer_login(t).ok());
        if let Some(l) = &login {
            store.kv_set(KV_LOGIN, l)?;
        }
    }
    let Some(login) = login else {
        return Ok(GithubState { login: None, activity: None, error: None });
    };
    match fetch(&login, token.as_deref()) {
        Ok(a) => {
            store.kv_set(KV_CACHE, &serde_json::to_string(&a).unwrap_or_default())?;
            Ok(GithubState { login: Some(login), activity: Some(a), error: None })
        }
        Err(e) => {
            tracing::warn!(%login, "github activity: {e}");
            let keep = cache.filter(|a| a.login.eq_ignore_ascii_case(&login));
            Ok(GithubState { login: Some(login), activity: keep, error: Some(e.to_string()) })
        }
    }
}

fn http() -> AppResult<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .user_agent(UA)
        .build()
        .map_err(|e| AppError::internal(e.to_string()))
}

/// The GitHub CLI's token, if it is installed and signed in.
fn cli_token() -> Option<String> {
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["auth", "token"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok().filter(|o| o.status.success())?;
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!t.is_empty()).then_some(t)
}

fn graphql(token: &str, query: &str, variables: Value) -> AppResult<Value> {
    let res = http()?
        .post("https://api.github.com/graphql")
        .bearer_auth(token)
        .json(&json!({ "query": query, "variables": variables }))
        .send()
        .map_err(|e| AppError::platform(format!("GitHub недоступен: {e}")))?;
    let status = res.status();
    let v: Value = res.json().map_err(|e| AppError::platform(format!("GitHub: {e}")))?;
    if !status.is_success() {
        return Err(AppError::platform(format!("GitHub ответил {status}")));
    }
    if let Some(err) = v["errors"].as_array().and_then(|a| a.first()) {
        return Err(AppError::platform(format!("GitHub: {}", err["message"].as_str().unwrap_or("ошибка запроса"))));
    }
    Ok(v["data"].clone())
}

fn viewer_login(token: &str) -> AppResult<String> {
    let d = graphql(token, "query { viewer { login } }", json!({}))?;
    d["viewer"]["login"].as_str().map(str::to_string).ok_or_else(|| AppError::platform("GitHub не вернул логин"))
}

const CALENDAR: &str = "login name url avatarUrl(size: 96) contributionsCollection { contributionCalendar { totalContributions weeks { contributionDays { date contributionCount contributionLevel } } } }";

fn fetch(login: &str, token: Option<&str>) -> AppResult<GithubActivity> {
    if let Some(t) = token {
        match fetch_graphql(login, t) {
            Ok(a) => return Ok(a),
            Err(e) => tracing::info!("github graphql failed, using the public page: {e}"),
        }
    }
    fetch_public(login)
}

fn fetch_graphql(login: &str, token: &str) -> AppResult<GithubActivity> {
    // The signed-in account itself: `viewer` counts private contributions.
    let me = graphql(token, &format!("query {{ viewer {{ {CALENDAR} }} }}"), json!({}))?;
    let user = if me["viewer"]["login"].as_str().is_some_and(|l| l.eq_ignore_ascii_case(login)) {
        me["viewer"].clone()
    } else {
        let d = graphql(token, &format!("query($login: String!) {{ user(login: $login) {{ {CALENDAR} }} }}"), json!({ "login": login }))?;
        if d["user"].is_null() {
            return Err(AppError::not_found(format!("На GitHub нет пользователя «{login}»")));
        }
        d["user"].clone()
    };
    let cal = &user["contributionsCollection"]["contributionCalendar"];
    let mut days = Vec::new();
    for w in cal["weeks"].as_array().into_iter().flatten() {
        for d in w["contributionDays"].as_array().into_iter().flatten() {
            let level = match d["contributionLevel"].as_str().unwrap_or("NONE") {
                "FIRST_QUARTILE" => 1,
                "SECOND_QUARTILE" => 2,
                "THIRD_QUARTILE" => 3,
                "FOURTH_QUARTILE" => 4,
                _ => 0,
            };
            days.push(GithubDay {
                date: d["date"].as_str().unwrap_or_default().to_string(),
                count: d["contributionCount"].as_u64().unwrap_or(0) as u32,
                level,
            });
        }
    }
    let login = user["login"].as_str().unwrap_or(login).to_string();
    Ok(finish(GithubActivity {
        name: user["name"].as_str().filter(|s| !s.trim().is_empty()).unwrap_or(&login).to_string(),
        avatar: user["avatarUrl"].as_str().and_then(avatar_data_url),
        profile_url: user["url"].as_str().map(str::to_string).unwrap_or_else(|| format!("https://github.com/{login}")),
        total: cal["totalContributions"].as_u64().unwrap_or(0) as u32,
        login,
        current_streak: 0,
        longest_streak: 0,
        days,
        source: "gh".into(),
        fetched_at: String::new(),
    }))
}

fn fetch_public(login: &str) -> AppResult<GithubActivity> {
    let c = http()?;
    let res = c
        .get(format!("https://api.github.com/users/{login}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| AppError::platform(format!("GitHub недоступен: {e}")))?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::not_found(format!("На GitHub нет пользователя «{login}»")));
    }
    if !res.status().is_success() {
        return Err(AppError::platform(format!("GitHub ответил {}", res.status())));
    }
    let user: Value = res.json().map_err(|e| AppError::platform(format!("GitHub: {e}")))?;
    let html = c
        .get(format!("https://github.com/users/{login}/contributions"))
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.text())
        .map_err(|e| AppError::platform(format!("Календарь GitHub не загрузился: {e}")))?;
    let days = parse_calendar_html(&html);
    if days.is_empty() {
        return Err(AppError::platform("GitHub изменил страницу календаря — не удалось прочитать вклад"));
    }
    let login = user["login"].as_str().unwrap_or(login).to_string();
    Ok(finish(GithubActivity {
        name: user["name"].as_str().filter(|s| !s.trim().is_empty()).unwrap_or(&login).to_string(),
        avatar: user["avatar_url"].as_str().and_then(|u| avatar_data_url(&format!("{u}&s=96"))),
        profile_url: user["html_url"].as_str().map(str::to_string).unwrap_or_else(|| format!("https://github.com/{login}")),
        total: days.iter().map(|d| d.count).sum(),
        login,
        current_streak: 0,
        longest_streak: 0,
        days,
        source: "public".into(),
        fetched_at: String::new(),
    }))
}

/// Cells of the public calendar page: `<td … data-date="…" id="…" data-level="…">`
/// and `<tool-tip for="…">N contributions on …</tool-tip>`.
pub fn parse_calendar_html(html: &str) -> Vec<GithubDay> {
    fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
        let key = format!("{name}=\"");
        let at = tag.find(&key)? + key.len();
        Some(&tag[at..at + tag[at..].find('"')?])
    }
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for part in html.split("<tool-tip").skip(1) {
        let Some(end) = part.find('>') else { continue };
        let Some(id) = attr(&part[..end], "for") else { continue };
        let text = &part[end + 1..part.find("</tool-tip>").unwrap_or(part.len())];
        let n = text.split_whitespace().next().and_then(|w| w.replace(',', "").parse().ok()).unwrap_or(0);
        counts.insert(id, n);
    }
    let mut days = Vec::new();
    for part in html.split("<td").skip(1) {
        let Some(end) = part.find('>') else { continue };
        let tag = &part[..end];
        let (Some(date), Some(level)) = (attr(tag, "data-date"), attr(tag, "data-level")) else { continue };
        let level: u8 = level.parse().unwrap_or(0);
        let count = attr(tag, "id").and_then(|id| counts.get(id).copied()).unwrap_or(u32::from(level > 0));
        days.push(GithubDay { date: date.to_string(), count, level: level.min(4) });
    }
    // The page lays cells out row by row (weekday by weekday).
    days.sort_by(|a, b| a.date.cmp(&b.date));
    days
}

fn avatar_data_url(url: &str) -> Option<String> {
    let res = http().ok()?.get(url).send().ok()?.error_for_status().ok()?;
    let mime = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|m| m.starts_with("image/"))
        .unwrap_or("image/png")
        .to_string();
    let bytes = res.bytes().ok()?;
    (bytes.len() < 512 * 1024).then(|| format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)))
}

/// Streaks over the calendar; today with no contribution yet does not break one.
fn finish(mut a: GithubActivity) -> GithubActivity {
    let today = Local::now().date_naive();
    a.days.retain(|d| NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").is_ok_and(|x| x <= today));
    let (cur, best) = streaks(&a.days);
    a.current_streak = cur;
    a.longest_streak = best;
    a.fetched_at = chrono::Utc::now().to_rfc3339();
    a
}

pub fn streaks(days: &[GithubDay]) -> (u32, u32) {
    let (mut best, mut run) = (0u32, 0u32);
    for d in days {
        run = if d.count > 0 { run + 1 } else { 0 };
        best = best.max(run);
    }
    let mut end = days.len();
    if days.last().is_some_and(|d| d.count == 0) {
        end = end.saturating_sub(1);
    }
    let cur = days[..end].iter().rev().take_while(|d| d.count > 0).count() as u32;
    (cur, best)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: &str, count: u32) -> GithubDay {
        GithubDay { date: date.into(), count, level: u8::from(count > 0) }
    }

    #[test]
    fn streaks_forgive_an_empty_today() {
        let d = [day("2026-11-14", 1), day("2026-11-15", 0), day("2026-11-16", 2), day("2026-11-17", 1), day("2026-11-18", 0)];
        assert_eq!(streaks(&d), (2, 2));
        let d = [day("2026-11-16", 2), day("2026-11-17", 0), day("2026-11-18", 0)];
        assert_eq!(streaks(&d), (0, 1));
    }

    /// Real network: `JARVIS_GITHUB_LOGIN=<login> cargo test --lib live_github -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_github_both_sources() {
        let login = std::env::var("JARVIS_GITHUB_LOGIN").expect("JARVIS_GITHUB_LOGIN");
        let public = fetch_public(&login).expect("public");
        println!("public: {} {} total={} days={} streak={}/{} avatar={}", public.login, public.name, public.total, public.days.len(), public.current_streak, public.longest_streak, public.avatar.as_ref().map_or(0, String::len));
        if let Some(t) = cli_token() {
            let gh = fetch_graphql(&login, &t).expect("graphql");
            println!("gh: {} {} total={} days={} streak={}/{} last={:?}", gh.login, gh.name, gh.total, gh.days.len(), gh.current_streak, gh.longest_streak, gh.days.last());
            assert!(gh.total >= public.total);
        }
        assert!(public.days.len() > 360);
    }

    #[test]
    fn public_calendar_cells_and_counts_are_read() {
        // Shape of github.com/users/<login>/contributions (2026-11-18).
        let html = r#"<table><tr>
<td tabindex="0" data-ix="0" style="width: 10px" data-date="2025-11-16" id="contribution-day-component-0-0" data-level="0" role="gridcell" class="ContributionCalendar-day"></td>
<td tabindex="0" data-ix="1" style="width: 10px" data-date="2025-11-23" id="contribution-day-component-0-1" data-level="2" role="gridcell" class="ContributionCalendar-day"></td>
</tr></table>
<tool-tip id="t1" for="contribution-day-component-0-0" popover="manual" class="sr-only">No contributions on November 16th.</tool-tip>
<tool-tip id="t2" for="contribution-day-component-0-1" popover="manual" class="sr-only">1,204 contributions on November 23rd.</tool-tip>"#;
        let days = parse_calendar_html(html);
        assert_eq!(days, vec![GithubDay { date: "2025-11-16".into(), count: 0, level: 0 }, GithubDay { date: "2025-11-23".into(), count: 1204, level: 2 }]);
    }
}
