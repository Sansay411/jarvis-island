//! Synthetic input (SendInput): mouse moves/clicks/scrolls in physical
//! screen pixels across the virtual desktop, Unicode typing and key chords.
//! UIPI applies: elevated windows (UAC, admin apps) ignore our input, which
//! is intended — JARVIS never bypasses Windows security boundaries.

use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
    VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_HOME, VK_LEFT, VK_LWIN, VK_MENU, VK_NEXT,
    VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use crate::error::{AppError, AppResult};

fn send(inputs: &[INPUT]) -> AppResult<()> {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(AppError::platform(
            "Windows заблокировала ввод (окно запущено с правами администратора или экран заблокирован)",
        ));
    }
    Ok(())
}

fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

/// Physical screen point → normalised absolute coordinates (0..65535) on the
/// virtual desktop.
fn normalise(x: i32, y: i32) -> AppResult<(i32, i32)> {
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if x < vx || y < vy || x >= vx + vw || y >= vy + vh {
            return Err(AppError::invalid(format!("Точка ({x}, {y}) за пределами экранов")));
        }
        let nx = ((x - vx) as i64 * 65535 / (vw - 1).max(1) as i64) as i32;
        let ny = ((y - vy) as i64 * 65535 / (vh - 1).max(1) as i64) as i32;
        Ok((nx, ny))
    }
}

pub fn move_to(x: i32, y: i32) -> AppResult<()> {
    let (nx, ny) = normalise(x, y)?;
    send(&[mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, nx, ny, 0)])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    Left,
    Right,
    Middle,
}

pub fn click(x: i32, y: i32, button: Button, count: u32) -> AppResult<()> {
    move_to(x, y)?;
    std::thread::sleep(Duration::from_millis(30));
    let (down, up) = match button {
        Button::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        Button::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        Button::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
    };
    for i in 0..count.clamp(1, 3) {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(60));
        }
        send(&[mouse(down, 0, 0, 0), mouse(up, 0, 0, 0)])?;
    }
    Ok(())
}

pub fn scroll(x: i32, y: i32, dy_notches: i32, dx_notches: i32) -> AppResult<()> {
    move_to(x, y)?;
    let mut inputs = Vec::new();
    if dy_notches != 0 {
        // Positive WHEEL_DELTA scrolls up; callers pass "down" as positive.
        inputs.push(mouse(MOUSEEVENTF_WHEEL, 0, 0, -dy_notches * 120));
    }
    if dx_notches != 0 {
        inputs.push(mouse(MOUSEEVENTF_HWHEEL, 0, 0, dx_notches * 120));
    }
    send(&inputs)
}

/// Type arbitrary Unicode text (independent of keyboard layout).
pub fn type_text(text: &str) -> AppResult<()> {
    for chunk in text.encode_utf16().collect::<Vec<_>>().chunks(64) {
        let mut inputs = Vec::with_capacity(chunk.len() * 2);
        for &u in chunk {
            if u == '\n' as u16 {
                inputs.push(key(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)));
                inputs.push(key(VK_RETURN, 0, KEYEVENTF_KEYUP));
                continue;
            }
            inputs.push(key(VIRTUAL_KEY(0), u, KEYEVENTF_UNICODE));
            inputs.push(key(VIRTUAL_KEY(0), u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
        send(&inputs)?;
        std::thread::sleep(Duration::from_millis(8));
    }
    Ok(())
}

fn vk_for(name: &str) -> Option<(VIRTUAL_KEY, bool)> {
    let n = name.trim().to_lowercase();
    let ext = |vk| Some((vk, true));
    Some(match n.as_str() {
        "ctrl" | "control" => (VK_CONTROL, false),
        "alt" => (VK_MENU, false),
        "shift" => (VK_SHIFT, false),
        "win" | "meta" | "super" => (VK_LWIN, true),
        "enter" | "return" => (VK_RETURN, false),
        "esc" | "escape" => (VK_ESCAPE, false),
        "tab" => (VK_TAB, false),
        "space" => (VK_SPACE, false),
        "backspace" => (VK_BACK, false),
        "delete" | "del" => return ext(VK_DELETE),
        "home" => return ext(VK_HOME),
        "end" => return ext(VK_END),
        "pageup" => return ext(VK_PRIOR),
        "pagedown" => return ext(VK_NEXT),
        "up" => return ext(VK_UP),
        "down" => return ext(VK_DOWN),
        "left" => return ext(VK_LEFT),
        "right" => return ext(VK_RIGHT),
        f if f.len() >= 2 && f.starts_with('f') && f[1..].parse::<u16>().is_ok_and(|v| (1..=24).contains(&v)) => {
            (VIRTUAL_KEY(VK_F1.0 + f[1..].parse::<u16>().unwrap() - 1), false)
        }
        c if c.chars().count() == 1 => {
            let ch = c.chars().next().unwrap().to_ascii_uppercase();
            if ch.is_ascii_alphanumeric() {
                (VIRTUAL_KEY(ch as u16), false)
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

/// Press a chord like "ctrl+shift+t", "alt+tab", "enter", "f5".
pub fn key_chord(chord: &str) -> AppResult<()> {
    let parts: Vec<&str> = chord.split('+').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Err(AppError::invalid("Пустое сочетание клавиш"));
    }
    let mut keys = Vec::new();
    for p in &parts {
        keys.push(vk_for(p).ok_or_else(|| AppError::invalid(format!("Неизвестная клавиша «{p}»")))?);
    }
    let flags = |ext: bool, up: bool| {
        let mut f = KEYBD_EVENT_FLAGS(0);
        if ext {
            f |= KEYEVENTF_EXTENDEDKEY;
        }
        if up {
            f |= KEYEVENTF_KEYUP;
        }
        f
    };
    let mut inputs = Vec::new();
    for (vk, ext) in &keys {
        inputs.push(key(*vk, 0, flags(*ext, false)));
    }
    for (vk, ext) in keys.iter().rev() {
        inputs.push(key(*vk, 0, flags(*ext, true)));
    }
    send(&inputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys() {
        assert_eq!(vk_for("ctrl").unwrap().0, VK_CONTROL);
        assert_eq!(vk_for("F5").unwrap().0 .0, VK_F1.0 + 4);
        assert_eq!(vk_for("a").unwrap().0 .0, 'A' as u16);
        assert!(vk_for("ctrlx").is_none());
    }
}
