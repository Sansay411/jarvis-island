pub use super::data_winmgr::*;
use super::native;
use crate::error::{AppError, AppResult};
use crate::platform::{display::Rect, HWND};
use chrono::Utc;
use core_foundation::{
    array::CFArray,
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
use core_graphics::{geometry::{CGPoint, CGSize}, window};
use parking_lot::Mutex;
use serde_json::json;
use std::{
    collections::{HashMap, VecDeque},
    sync::LazyLock,
};

static RESTORE: LazyLock<Mutex<HashMap<i64, Rect>>> = LazyLock::new(Default::default);

type Dict = CFDictionary<CFString, CFType>;
fn number(d: &Dict, key: &str) -> Option<f64> {
    d.find(&CFString::new(key))?
        .downcast::<CFNumber>()?
        .to_f64()
}
fn text(d: &Dict, key: &str) -> String {
    d.find(&CFString::new(key))
        .and_then(|v| v.downcast::<CFString>())
        .map(|s| s.to_string())
        .unwrap_or_default()
}
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(element: CFTypeRef, attribute: core_foundation::string::CFStringRef, value: *mut CFTypeRef) -> i32;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, timeout: f32) -> i32;
    fn AXValueGetValue(value: CFTypeRef, kind: u32, output: *mut std::ffi::c_void) -> bool;
}

fn ax_attribute(element: &CFType, name: &str) -> Option<CFType> {
    let name = CFString::new(name);
    let mut value = std::ptr::null();
    let status = unsafe { AXUIElementCopyAttributeValue(element.as_CFTypeRef(), name.as_concrete_TypeRef(), &mut value) };
    if status != 0 || value.is_null() { return None; }
    Some(unsafe { CFType::wrap_under_create_rule(value) })
}

struct AxWindow { title: String, rect: Rect, minimized: bool }

fn ax_windows(pid: u32) -> Option<Vec<AxWindow>> {
    let raw = unsafe { AXUIElementCreateApplication(pid as i32) };
    if raw.is_null() { return None; }
    let app = unsafe { CFType::wrap_under_create_rule(raw) };
    unsafe { AXUIElementSetMessagingTimeout(app.as_CFTypeRef(), 0.1); }
    let raw = ax_attribute(&app, "AXWindows")?.downcast::<CFArray>()?;
    let windows: CFArray<CFType> = unsafe { TCFType::wrap_under_get_rule(raw.as_concrete_TypeRef()) };
    let mut result = Vec::new();
    for w in windows.iter() {
        let Some(minimized) = ax_attribute(&w, "AXMinimized").and_then(|v| v.downcast::<CFBoolean>()) else { continue };
        let Some(position) = ax_attribute(&w, "AXPosition") else { continue };
        let Some(size) = ax_attribute(&w, "AXSize") else { continue };
        let mut point = CGPoint::new(0.0, 0.0);
        let mut extent = CGSize::new(0.0, 0.0);
        let valid = unsafe {
            AXValueGetValue(position.as_CFTypeRef(), 1, &mut point as *mut _ as *mut std::ffi::c_void)
                && AXValueGetValue(size.as_CFTypeRef(), 2, &mut extent as *mut _ as *mut std::ffi::c_void)
        };
        if !valid { continue; }
        let title = ax_attribute(&w, "AXTitle").and_then(|v| v.downcast::<CFString>()).map(|v| v.to_string()).unwrap_or_default();
        result.push(AxWindow { title, minimized: minimized.into(), rect: Rect {
            x: point.x as i32, y: point.y as i32, width: extent.width as i32, height: extent.height as i32,
        } });
    }
    Some(result)
}

fn window_state(rect: Rect, title: &str, windows: Option<&[AxWindow]>) -> &'static str {
    let Some(windows) = windows else { return "normal" };
    let matches: Vec<_> = windows.iter().filter(|w| {
        (w.rect.x as i64 - rect.x as i64).abs() < 3 && (w.rect.y as i64 - rect.y as i64).abs() < 3
            && (w.rect.width as i64 - rect.width as i64).abs() < 3 && (w.rect.height as i64 - rect.height as i64).abs() < 3
    }).collect();
    let named: Vec<_> = windows.iter().filter(|w| !title.is_empty() && w.title == title).collect();
    let matched = if matches.len() == 1 { Some(matches[0]) } else if named.len() == 1 { Some(named[0]) } else { None };
    if matched.is_some_and(|w| w.minimized) { "minimized" } else { "normal" }
}

pub fn to_hwnd(h: i64) -> HWND {
    HWND(h as isize)
}
pub fn describe(hwnd: HWND) -> Option<WindowInfo> {
    list_windows().into_iter().find(|w| w.hwnd == hwnd.0 as i64)
}

pub fn list_windows() -> Vec<WindowInfo> {
    let Some(ids) = window::create_window_list(window::kCGWindowListOptionAll, 0) else {
        return vec![];
    };
    let Some(list) = window::create_description_from_array(ids) else {
        return vec![];
    };
    let front = native::jxa(
        "return Number($.NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier);",
        json!(null),
    )
    .ok()
    .and_then(|v| v.as_u64())
    .unwrap_or(0) as u32;
    let mut marked = false;
    let mut accessibility = HashMap::new();
    list.iter()
        .filter_map(|d| {
            if number(&d, "kCGWindowLayer")? != 0.0 {
                return None;
            }
            let raw = d
                .find(&CFString::new("kCGWindowBounds"))?
                .downcast::<CFDictionary>()?;
            let bounds: Dict = unsafe { TCFType::wrap_under_get_rule(raw.as_concrete_TypeRef()) };
            let rect = Rect {
                x: number(&bounds, "X")? as i32,
                y: number(&bounds, "Y")? as i32,
                width: number(&bounds, "Width")? as i32,
                height: number(&bounds, "Height")? as i32,
            };
            if rect.width < 40 || rect.height < 20 {
                return None;
            }
            let pid = number(&d, "kCGWindowOwnerPID")? as u32;
            let title = text(&d, "kCGWindowName");
            let ax = accessibility.entry(pid).or_insert_with(|| ax_windows(pid));
            let state = window_state(rect, &title, ax.as_deref());
            let onscreen = d.find(CFString::new("kCGWindowIsOnscreen"))
                .and_then(|v| v.downcast::<CFBoolean>()).map(bool::from).unwrap_or(false);
            let foreground = pid == front && !marked && state != "minimized" && onscreen;
            marked |= foreground;
            let process = text(&d, "kCGWindowOwnerName");
            Some(WindowInfo {
                hwnd: number(&d, "kCGWindowNumber")? as i64,
                title,
                pid,
                class_name: process.clone(),
                process,
                state,
                foreground,
                rect,
                monitor: String::new(),
            })
        })
        .collect()
}
pub fn foreground_window() -> Option<WindowInfo> {
    list_windows().into_iter().find(|w| w.foreground)
}

const AX_ACTION: &str = r#"
const se = Application('System Events');
const processes = se.applicationProcesses.whose({unixId:a.pid})();
if (!processes.length) throw Error('Приложение уже закрыто');
const p = processes[0];
const wins = p.windows();
const matches = wins.filter(w => {
    try { const q=w.position(), s=w.size(); return Math.abs(q[0]-a.rect.x)<3 && Math.abs(q[1]-a.rect.y)<3 && Math.abs(s[0]-a.rect.width)<3 && Math.abs(s[1]-a.rect.height)<3; } catch(e) { return false; }
});
const named = a.title ? wins.filter(w => w.name()===a.title) : [];
const w = matches.length===1 ? matches[0] : named.length===1 ? named[0] : wins.length===1 ? wins[0] : null;
if (!w) throw Error('Не удалось однозначно сопоставить окно; разрешите запись экрана для чтения заголовков');
if (a.action==='focus') {
    p.frontmost=true;
    w.attributes.byName('AXMinimized').value=false;
    w.actions.byName('AXRaise').perform();
} else if (a.action==='minimize') {
    w.attributes.byName('AXMinimized').value=true;
} else if (a.action==='close') {
    const buttons=w.buttons.whose({subrole:'AXCloseButton'})();
    if (!buttons.length) throw Error('У окна нет кнопки закрытия');
    buttons[0].click();
} else {
    w.attributes.byName('AXMinimized').value=false;
    if (a.target) { w.position=[a.target.x,a.target.y]; w.size=[a.target.width,a.target.height]; }
}
return true;
"#;

fn act(hwnd: i64, action: &str, target: Option<Rect>) -> AppResult<WindowInfo> {
    let before = describe(to_hwnd(hwnd)).ok_or_else(|| AppError::not_found("Окно уже закрыто"))?;
    native::jxa(
        AX_ACTION,
        json!({"pid":before.pid,"title":before.title,"rect":before.rect,"action":action,"target":target}),
    )?;
    let mut after = describe(to_hwnd(hwnd)).unwrap_or(before);
    if action == "minimize" {
        after.state = "minimized";
    }
    Ok(after)
}
pub fn focus(hwnd: i64) -> AppResult<WindowInfo> {
    act(hwnd, "focus", None)
}
pub fn minimize(hwnd: i64) -> AppResult<WindowInfo> {
    act(hwnd, "minimize", None)
}
pub fn maximize(hwnd: i64) -> AppResult<WindowInfo> {
    let mut w = snap(hwnd, "maximize")?;
    w.state = "maximized";
    Ok(w)
}
pub fn restore(hwnd: i64) -> AppResult<WindowInfo> {
    let rect = RESTORE.lock().get(&hwnd).copied();
    let restored = act(hwnd, "restore", rect)?;
    RESTORE.lock().remove(&hwnd);
    Ok(restored)
}
pub fn close(hwnd: i64) -> AppResult<CloseOutcome> {
    act(hwnd, "close", None)?;
    std::thread::sleep(std::time::Duration::from_millis(100));
    let closed = describe(to_hwnd(hwnd)).is_none();
    if closed {
        RESTORE.lock().remove(&hwnd);
    }
    Ok(CloseOutcome {
        closed,
        still_open_reason: (!closed)
            .then(|| "Окно осталось открытым: возможно, требуется сохранить изменения".into()),
    })
}
pub fn move_resize(hwnd: i64, rect: Rect) -> AppResult<WindowInfo> {
    if rect.width <= 0 || rect.height <= 0 {
        return Err(AppError::invalid("Размеры окна должны быть положительными"));
    }
    let before = describe(to_hwnd(hwnd)).ok_or_else(|| AppError::not_found("Окно уже закрыто"))?;
    let after = act(hwnd, "resize", Some(rect))?;
    RESTORE.lock().entry(hwnd).or_insert(before.rect);
    Ok(after)
}

fn work_area(rect: Rect) -> AppResult<Rect> {
    let v = native::jxa(
        r#"
const screens=$.NSScreen.screens, top=screens.objectAtIndex(0).frame.size.height;
const all=[];
for(let i=0;i<screens.count;i++) {
    const f=screens.objectAtIndex(i).visibleFrame;
    all.push({x:f.origin.x,y:top-f.origin.y-f.size.height,width:f.size.width,height:f.size.height});
}
return all.find(s => a.x+a.width/2 >= s.x && a.x+a.width/2 < s.x+s.width && a.y+a.height/2 >= s.y && a.y+a.height/2 < s.y+s.height) || all[0];
"#,
        serde_json::to_value(rect)?,
    )?;
    serde_json::from_value(v)
        .map_err(|_| AppError::platform("Не удалось определить рабочую область монитора"))
}
fn snap_rect(w: Rect, region: &str) -> AppResult<Rect> {
    let (hw, hh) = (w.width / 2, w.height / 2);
    Ok(match region {
        "maximize" => w,
        "left" => Rect { width: hw, ..w },
        "right" => Rect {
            x: w.x + hw,
            width: w.width - hw,
            ..w
        },
        "top" => Rect { height: hh, ..w },
        "bottom" => Rect {
            y: w.y + hh,
            height: w.height - hh,
            ..w
        },
        "top-left" => Rect {
            width: hw,
            height: hh,
            ..w
        },
        "top-right" => Rect {
            x: w.x + hw,
            width: w.width - hw,
            height: hh,
            ..w
        },
        "bottom-left" => Rect {
            y: w.y + hh,
            width: hw,
            height: w.height - hh,
            ..w
        },
        "bottom-right" => Rect {
            x: w.x + hw,
            y: w.y + hh,
            width: w.width - hw,
            height: w.height - hh,
        },
        "center" => {
            let width = w.width * 62 / 100;
            let height = w.height * 72 / 100;
            Rect {
                x: w.x + (w.width - width) / 2,
                y: w.y + (w.height - height) / 2,
                width,
                height,
            }
        }
        _ => {
            return Err(AppError::invalid(format!(
                "Неизвестная область размещения: {region}"
            )))
        }
    })
}
pub fn snap(hwnd: i64, region: &str) -> AppResult<WindowInfo> {
    let before = describe(to_hwnd(hwnd)).ok_or_else(|| AppError::not_found("Окно уже закрыто"))?;
    move_resize(hwnd, snap_rect(work_area(before.rect)?, region)?)
}
pub fn find_windows(query: &str) -> Vec<WindowInfo> {
    let words: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return vec![];
    }
    list_windows()
        .into_iter()
        .filter(|w| {
            let hay = format!("{} {}", w.title, w.process).to_lowercase();
            words.iter().all(|s| hay.contains(s))
        })
        .collect()
}

#[derive(Default)]
pub struct ForegroundHistory {
    entries: Mutex<VecDeque<ForegroundEntry>>,
}
impl ForegroundHistory {
    pub(super) fn record_info(&self, info: WindowInfo) {
        if info.pid == std::process::id() || info.process.eq_ignore_ascii_case("jarvis") {
            return;
        }
        let mut entries = self.entries.lock();
        entries.retain(|e| e.hwnd != info.hwnd);
        entries.push_front(ForegroundEntry {
            hwnd: info.hwnd,
            title: info.title,
            process: info.process,
            at: Utc::now(),
        });
        entries.truncate(16);
    }
    pub fn record(&self, hwnd: HWND) {
        if let Some(info) = describe(hwnd) {
            self.record_info(info);
        }
    }
    pub fn recent(&self) -> Vec<ForegroundEntry> {
        let live = list_windows();
        let mut entries = self.entries.lock();
        entries.retain(|e| live.iter().any(|w| w.hwnd == e.hwnd));
        entries.iter().cloned().collect()
    }
    pub fn previous(&self) -> Option<ForegroundEntry> {
        let current = foreground_window().map(|w| w.hwnd);
        self.recent().into_iter().find(|e| Some(e.hwnd) != current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ax_state_does_not_confuse_other_spaces_with_minimized_windows() {
        let rect = Rect { x:0,y:0,width:800,height:600 };
        let mut windows = vec![AxWindow { title:"Test".into(),rect,minimized:false }];
        assert_eq!(window_state(rect, "Test", Some(&windows)), "normal");
        windows[0].minimized = true;
        assert_eq!(window_state(rect, "Test", Some(&windows)), "minimized");
        assert_eq!(window_state(rect, "Test", None), "normal");
        assert_eq!(window_state(rect, "Test", Some(&[])), "normal");
        assert_eq!(window_state(Rect { x:900,..rect }, "", Some(&windows)), "normal");
    }
    #[test]
    fn snap_covers_odd_sized_areas_and_validates_regions() {
        let area = Rect {
            x: -101,
            y: 24,
            width: 101,
            height: 81,
        };
        let left = snap_rect(area, "left").unwrap();
        let right = snap_rect(area, "right").unwrap();
        assert_eq!(left.width + right.width, area.width);
        assert_eq!(right.x, left.x + left.width);
        assert_eq!(snap_rect(area, "bottom-right").unwrap().height, 41);
        assert!(snap_rect(area, "").is_err());
    }
    #[test]
    fn history_deduplicates_and_excludes_own_app() {
        let history = ForegroundHistory::default();
        let info = |hwnd| WindowInfo {
            hwnd,
            title: "тест".into(),
            process: "Other".into(),
            pid: u32::MAX,
            class_name: String::new(),
            state: "normal",
            foreground: true,
            rect: Rect::default(),
            monitor: String::new(),
        };
        history.record_info(info(1));
        history.record_info(info(2));
        history.record_info(info(1));
        assert_eq!(history.entries.lock().len(), 2);
        assert_eq!(history.entries.lock()[0].hwnd, 1);
        let mut own = info(3);
        own.pid = std::process::id();
        history.record_info(own);
        assert_eq!(history.entries.lock().len(), 2);
    }
}
