//! The watcher. Every two seconds: what is in front of the user? A browser
//! tab with TikTok, Shorts, Reels or VK clips (or plain YouTube) spends the
//! day's minutes; learning videos — by title words or «это учёба» — don't.
//! When the minutes are gone, or at once during focus, a work/study/project
//! block or quiet hours, the tab closes and «Перезагрузка» opens. Costs a
//! window-title read per tick; the address bar is read only when the title
//! changes on a site where the URL matters.

use std::time::{Duration, Instant};

use std::sync::Arc;

use chrono::Utc;
use serde_json::json;
use tauri::{AppHandle, Emitter};

use super::{minutes_ru, refill, seed, site_group, site_label, Intervention, Mentor, Watching, EVT_WATCHING};
use crate::error::AppResult;
use crate::platform::{browser, winmgr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    TikTok,
    YouTube,
    Instagram,
    Vk,
    VkClips,
}

/// What the window title alone says.
pub fn hint(process: &str, title: &str, is_browser: bool) -> Option<Hint> {
    let p = process.to_lowercase();
    let t = title.to_lowercase();
    if p.contains("tiktok") || (is_browser && t.contains("tiktok")) {
        return Some(Hint::TikTok);
    }
    if !is_browser {
        return None;
    }
    if t.contains("youtube") {
        Some(Hint::YouTube)
    } else if t.contains("instagram") {
        Some(Hint::Instagram)
    } else if t.contains("клип") && (t.contains("vk") || t.contains("вконтакте")) {
        Some(Hint::VkClips)
    } else if t.contains("вконтакте") || t.contains(" vk") || t.starts_with("vk") {
        Some(Hint::Vk)
    } else {
        None
    }
}

/// The site once the address is known (or not: plain YouTube by default).
pub fn site_for(hint: Hint, url: Option<&str>) -> Option<&'static str> {
    let u = url.unwrap_or("").to_lowercase();
    match hint {
        Hint::TikTok => Some("tiktok"),
        Hint::YouTube if u.contains("/shorts/") => Some("shorts"),
        Hint::YouTube => Some("youtube"),
        Hint::Instagram if u.contains("/reels") || u.contains("/reel/") => Some("reels"),
        Hint::Instagram => None,
        Hint::VkClips => Some("vk"),
        Hint::Vk if u.contains("/clips") || u.contains("/clip") => Some("vk"),
        Hint::Vk => None,
    }
}

pub fn is_learning(title: &str, words: &[String]) -> bool {
    let t = title.to_lowercase();
    words.iter().map(|w| w.trim().to_lowercase()).any(|w| !w.is_empty() && t.contains(&w))
}

#[derive(Debug, Clone)]
struct Seen {
    site: &'static str,
    hwnd: i64,
    title: String,
    url: Option<String>,
    browser: bool,
}

type UrlCache = Option<(i64, String, Option<String>)>;

fn look(cache: UrlCache) -> (Option<Seen>, UrlCache) {
    let Some(w) = winmgr::foreground_window() else { return (None, cache) };
    if w.state == "minimized" {
        return (None, cache);
    }
    let is_browser = browser::browser_label(&w.process).is_some();
    let Some(h) = hint(&w.process, &w.title, is_browser) else { return (None, cache) };
    let url = if matches!(h, Hint::TikTok | Hint::VkClips) || !is_browser {
        None
    } else {
        match &cache {
            Some((hw, ti, u)) if *hw == w.hwnd && *ti == w.title => u.clone(),
            _ => browser::active_url(w.hwnd),
        }
    };
    let next = Some((w.hwnd, w.title.clone(), url.clone()));
    let seen = site_for(h, url.as_deref()).map(|site| Seen { site, hwnd: w.hwnd, title: w.title.clone(), url: url.clone(), browser: is_browser });
    (seen, next)
}

/// Close the tab (or the TikTok app). True when it is gone.
fn close(s: &Seen) -> bool {
    if !s.browser {
        return winmgr::close(s.hwnd).map(|o| o.closed).unwrap_or(false);
    }
    if let Ok(st) = browser::state(s.hwnd) {
        if let Some(tab) = st.tabs.iter().find(|t| t.selected) {
            if browser::close_tab(s.hwnd, None, Some(tab.index)).map(|r| r.1).unwrap_or(false) {
                return true;
            }
        }
    }
    // The tab strip is not readable: Ctrl+W, only if that same page is still in front.
    match winmgr::foreground_window() {
        Some(fg) if fg.hwnd == s.hwnd && fg.title == s.title => crate::platform::input::key_chord("ctrl+w").is_ok(),
        _ => false,
    }
}

struct Tick {
    last: Instant,
    site: Option<&'static str>,
    pending: f64,
    cache: UrlCache,
    last_close: Option<Instant>,
    last_speech: Option<Instant>,
    refill_at: Instant,
    emitted: Option<Watching>,
}

fn flush(mentor: &Mentor, t: &mut Tick, closing: bool) {
    if let Some(site) = t.site {
        let sec = t.pending.floor() as u32;
        if sec > 0 || closing {
            let _ = mentor.add_usage(site, sec, closing);
            t.pending -= sec as f64;
        }
    }
}

pub fn spawn(app: AppHandle, mentor: Arc<Mentor>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let mut t = Tick {
            last: Instant::now(),
            site: None,
            pending: 0.0,
            cache: None,
            last_close: None,
            last_speech: None,
            refill_at: Instant::now() + Duration::from_secs(90),
            emitted: None,
        };
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if let Err(e) = tick(&app, &mentor, &mut t).await {
                tracing::debug!("mentor guard: {e}");
            }
        }
    });
}

async fn tick(app: &AppHandle, mentor: &Arc<Mentor>, t: &mut Tick) -> AppResult<()> {
    let now = Instant::now();
    let dt = now.duration_since(t.last).as_secs_f64().min(5.0);
    t.last = now;
    if now >= t.refill_at {
        t.refill_at = now + Duration::from_secs(1800);
        tauri::async_runtime::spawn(refill(app.clone(), mentor.clone()));
    }
    let settings = mentor.store().settings().mentor;
    let cache = t.cache.take();
    let (seen, cache) = if settings.guard { crate::platform::blocking(move || Ok(look(cache))).await? } else { (None, cache) };
    t.cache = cache;
    let seen = seen.filter(|s| !(s.site == "youtube" && (is_learning(&s.title, &settings.learning_words) || mentor.is_allowed(&s.title))));

    let Some(s) = seen else {
        flush(mentor, t, false);
        t.site = None;
        t.pending = 0.0;
        if t.emitted.take().is_some() {
            *mentor.watching.lock() = None;
            let _ = app.emit(EVT_WATCHING, Option::<Watching>::None);
            mentor.emit_state();
        }
        return Ok(());
    };
    if t.site != Some(s.site) {
        flush(mentor, t, false);
        t.site = Some(s.site);
        t.pending = 0.0;
    }
    t.pending += dt;
    if t.pending >= 10.0 {
        flush(mentor, t, false);
    }
    let (short, yt, _) = mentor.budgets()?;
    let b = if site_group(s.site) == "youtube" { yt } else { short };
    let used = b.used_sec + t.pending.floor() as u32;
    let strict = mentor.strict_reason();
    let w = Watching { site: s.site.into(), label: site_label(s.site).into(), used_sec: used, limit_sec: b.limit_sec, strict: strict.clone() };
    *mentor.watching.lock() = Some(w.clone());
    let _ = app.emit(EVT_WATCHING, Some(&w));
    if t.emitted.is_none() {
        mentor.emit_state();
    }
    t.emitted = Some(w);

    let cooling = t.last_close.is_some_and(|c| now.duration_since(c) < Duration::from_secs(4));
    if used < b.limit_sec || cooling {
        return Ok(());
    }

    // Out of minutes (or a focus block): close, then offer something better.
    t.last_close = Some(now);
    flush(mentor, t, true);
    let target = s.clone();
    let closed = crate::platform::blocking(move || Ok(close(&target))).await.unwrap_or(false);
    let label = site_label(s.site);
    let mins = ((used + 30) / 60).max(1);
    let why = seed::WHY_LINES[(Utc::now().timestamp() as usize / 7) % seed::WHY_LINES.len()].to_string();
    let _ = mentor.log("closed", "", label, used as f64 / 60.0, 0, 0, json!({ "site": s.site, "strict": strict, "closed": closed }));
    let iv = Intervention {
        id: uuid::Uuid::new_v4().to_string(),
        reason: if strict.is_some() { "strict".into() } else { "budget".into() },
        site: s.site.into(),
        label: label.into(),
        used_sec: used,
        strict: strict.clone(),
        why,
        title: s.title.clone(),
        url: s.url.clone(),
        closed,
        if_then: settings.if_then.clone(),
        at: Utc::now().to_rfc3339(),
        screen: None,
    };
    if let Err(e) = mentor.open_reset(iv) {
        tracing::warn!("reset window: {e}");
    }
    let quiet = t.last_speech.is_some_and(|l| now.duration_since(l) < Duration::from_secs(180));
    if settings.speak && !quiet {
        t.last_speech = Some(now);
        let line = match &strict {
            Some(reason) => format!("{reason}. {label} подождёт — я закрыл."),
            None => format!("{label} уже {}. Я закрыл. Давай лучше разомнёмся или пройдём пару карточек.", minutes_ru(mins)),
        };
        crate::hub::request_tts(app, &line, None, None, false);
    }
    t.site = None;
    t.pending = 0.0;
    mentor.emit_state();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_addresses_name_the_site() {
        assert_eq!(hint("chrome.exe", "Смешные коты | TikTok - Google Chrome", true), Some(Hint::TikTok));
        assert_eq!(hint("TikTok.exe", "TikTok", false), Some(Hint::TikTok));
        assert_eq!(hint("Code.exe", "tiktok.rs - jarvis - Visual Studio Code", false), None);
        assert_eq!(hint("msedge.exe", "Rust за 10 минут - YouTube — Microsoft Edge", true), Some(Hint::YouTube));
        assert_eq!(hint("notepad.exe", "youtube.txt", false), None);
        assert_eq!(site_for(Hint::YouTube, Some("youtube.com/shorts/abc")), Some("shorts"));
        assert_eq!(site_for(Hint::YouTube, Some("https://www.youtube.com/watch?v=1")), Some("youtube"));
        assert_eq!(site_for(Hint::YouTube, None), Some("youtube"));
        assert_eq!(site_for(Hint::Instagram, Some("instagram.com/reels/xyz")), Some("reels"));
        assert_eq!(site_for(Hint::Instagram, Some("instagram.com/direct/inbox")), None);
        assert_eq!(site_for(Hint::Vk, Some("vk.com/clips")), Some("vk"));
        assert_eq!(site_for(Hint::Vk, Some("vk.com/im")), None);
        assert_eq!(site_for(Hint::Vk, None), None);
        assert_eq!(hint("chrome.exe", "Клипы | VK - Google Chrome", true), Some(Hint::VkClips));
    }

    #[test]
    fn learning_videos_are_free() {
        let words: Vec<String> = crate::store::settings::LEARNING_WORDS.iter().map(|s| s.to_string()).collect();
        assert!(is_learning("Rust ownership explained - YouTube", &words));
        assert!(is_learning("Английский для начинающих: урок 5 - YouTube", &words));
        assert!(!is_learning("ПРАНК НАД ДРУГОМ - YouTube", &words));
    }
}
