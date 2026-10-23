//! «В Избранное»: the original files go to the user's own Saved Messages
//! through the Telegram Desktop already installed and signed in on this PC.
//!
//! Telegram's own shortcut Ctrl+0 (command `self_chat` in its
//! shortcuts-default.json) opens Saved Messages; the window title then names
//! the open chat. JARVIS presses Enter only after the title says
//! «Избранное» / «Saved Messages» — never into another chat.

use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};
use crate::platform::{apps, clipboard, input, winmgr};

/// Titles of the self chat in Telegram's languages we expect.
const SAVED_TITLES: &[&str] = &["избранное", "saved messages", "збережене", "избранные"];

/// The chat name from a Telegram window title: bidi marks and unread
/// counters («(3) Избранное», «Избранное (3)») removed.
pub fn chat_title(window_title: &str) -> String {
    let t: String = window_title.chars().filter(|c| !matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).collect();
    let mut t = t.trim().to_string();
    // With several accounts the title reads «chat @ account».
    if let Some(at) = t.find(" @ ") {
        t.truncate(at);
    }
    if t.starts_with('(') {
        if let Some(end) = t.find(')') {
            t = t[end + 1..].trim().to_string();
        }
    }
    if t.ends_with(')') {
        if let Some(start) = t.rfind(" (") {
            if t[start + 2..t.len() - 1].chars().all(|c| c.is_ascii_digit()) {
                t.truncate(start);
            }
        }
    }
    t.trim().trim_start_matches("Telegram —").trim().to_string()
}

pub fn is_saved_messages(window_title: &str) -> bool {
    SAVED_TITLES.contains(&chat_title(window_title).to_lowercase().as_str())
}

fn telegram_window() -> Option<winmgr::WindowInfo> {
    winmgr::list_windows().into_iter().find(|w| w.process.eq_ignore_ascii_case("telegram.exe"))
}

fn telegram_exe() -> Option<std::path::PathBuf> {
    let p = dirs::data_dir()?.join("Telegram Desktop").join("Telegram.exe");
    p.is_file().then_some(p)
}

/// Bring Telegram's window up (it may live in the tray), launching it if needed.
fn ensure_window() -> AppResult<winmgr::WindowInfo> {
    if let Some(w) = telegram_window() {
        return Ok(w);
    }
    let exe = telegram_exe().ok_or_else(|| AppError::not_found("Telegram Desktop не найден. Установите его и войдите в аккаунт."))?;
    // A second start of Telegram.exe shows the running instance's window.
    apps::shell_open(&exe.to_string_lossy(), None, None)?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(150));
        if let Some(w) = telegram_window() {
            return Ok(w);
        }
    }
    Err(AppError::Timeout("Окно Telegram не появилось за 10 секунд".into()))
}

fn current_title(hwnd: i64) -> String {
    winmgr::describe(winmgr::to_hwnd(hwnd)).map(|w| w.title).unwrap_or_default()
}

fn wait_for(timeout: Duration, mut ok: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        if ok() {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(80));
    }
}

pub struct Sent {
    pub count: usize,
    pub chat: String,
}

/// Send the files as they are. Blocking (a few seconds); restores the
/// clipboard and the previously active window.
pub fn send_to_saved(paths: &[String]) -> AppResult<Sent> {
    for p in paths {
        let m = std::fs::metadata(p).map_err(|_| AppError::not_found(format!("Файл не найден: {p}")))?;
        if m.is_dir() {
            return Err(AppError::invalid("Папку Telegram не отправит — сначала сожмите её в ZIP"));
        }
    }
    let previous = winmgr::foreground_window();
    let saved_clipboard = clipboard::read().ok();
    let win = ensure_window()?;
    let result = (|| -> AppResult<Sent> {
        clipboard::write_files(paths)?;
        winmgr::focus(win.hwnd)?;
        if !wait_for(Duration::from_millis(1500), || winmgr::foreground_window().is_some_and(|f| f.hwnd == win.hwnd)) {
            return Err(AppError::platform("Не удалось вывести окно Telegram на передний план"));
        }
        input::key_chord("ctrl+0")?;
        if !wait_for(Duration::from_millis(2500), || is_saved_messages(&current_title(win.hwnd))) {
            let title = chat_title(&current_title(win.hwnd));
            return Err(AppError::platform(format!(
                "Telegram не открыл «Избранное» (открыт чат «{title}»). Файлы не отправлены. Проверьте, что Telegram разблокирован и сочетание Ctrl+0 не переназначено."
            )));
        }
        input::key_chord("ctrl+v")?;
        // The send box appears after Telegram reads the files.
        std::thread::sleep(Duration::from_millis(900 + 150 * paths.len().min(10) as u64));
        // Still the same window and chat — otherwise Enter must not be pressed.
        let fg = winmgr::foreground_window();
        if !fg.as_ref().is_some_and(|f| f.hwnd == win.hwnd) || !is_saved_messages(&current_title(win.hwnd)) {
            return Err(AppError::platform("Окно Telegram потеряло фокус до отправки — файлы не отправлены"));
        }
        input::key_chord("enter")?;
        std::thread::sleep(Duration::from_millis(400));
        Ok(Sent { count: paths.len(), chat: chat_title(&current_title(win.hwnd)) })
    })();
    // Telegram has read the clipboard by now: put the user's content back.
    std::thread::sleep(Duration::from_millis(250));
    if let Some(prev) = saved_clipboard {
        let _ = clipboard::restore(&prev);
    }
    if let Some(p) = previous.filter(|p| p.hwnd != win.hwnd) {
        let _ = winmgr::focus(p.hwnd);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_saved_messages_title_only() {
        assert!(is_saved_messages("\u{200e}Избранное"));
        assert!(is_saved_messages("(3) Избранное"));
        assert!(is_saved_messages("Saved Messages (12)"));
        assert!(!is_saved_messages("\u{200e}Noxen"));
        assert!(!is_saved_messages("Избранное обсуждение"));
        assert!(!is_saved_messages("Telegram"));
        assert_eq!(chat_title("\u{200e}Noxen"), "Noxen");
        assert!(is_saved_messages("\u{200e}Избранное @ \u{200e}Noxen"));
        assert!(!is_saved_messages("Нинок @ Noxen"));
    }
}
