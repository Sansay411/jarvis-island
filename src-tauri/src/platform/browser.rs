//! Browser tab control through Windows UI Automation.
//!
//! Chromium browsers (Chrome, Edge, Yandex, Brave, Opera, Vivaldi) and
//! Firefox expose their tab strip as TabItem elements and the page as a
//! Document with a Text pattern, so tabs can be listed, switched and closed
//! and page text read without an extension or remote-debugging ports (which
//! would weaken the user's browser security). Unsupported browsers degrade to
//! "no tabs found".

use serde::Serialize;
use windows::core::{Interface, BSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::*;

use super::winmgr::{self, WindowInfo};
use crate::error::{AppError, AppResult};

pub const BROWSERS: &[(&str, &str)] = &[
    ("chrome.exe", "Chrome"),
    ("msedge.exe", "Edge"),
    ("browser.exe", "Яндекс Браузер"),
    ("brave.exe", "Brave"),
    ("opera.exe", "Opera"),
    ("vivaldi.exe", "Vivaldi"),
    ("firefox.exe", "Firefox"),
];

pub fn browser_label(process: &str) -> Option<&'static str> {
    BROWSERS.iter().find(|(p, _)| p.eq_ignore_ascii_case(process)).map(|(_, l)| *l)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub index: usize,
    pub title: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
    pub browser: String,
    pub minimized: bool,
    pub hwnd: i64,
    pub window_title: String,
    pub url: Option<String>,
    pub tabs: Vec<Tab>,
}

fn uia() -> AppResult<IUIAutomation> {
    unsafe {
        let u: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        if let Ok(u2) = u.cast::<IUIAutomation2>() {
            let _ = u2.SetConnectionTimeout(2000);
            let _ = u2.SetTransactionTimeout(4000);
        }
        Ok(u)
    }
}

/// Browser windows in Z-order (most recently used first), optionally
/// filtered by browser name/process.
pub fn browser_windows(which: Option<&str>) -> Vec<WindowInfo> {
    let want = which.map(|w| w.to_lowercase());
    winmgr::list_windows()
        .into_iter()
        .filter(|w| {
            let Some(label) = browser_label(&w.process) else { return false };
            match &want {
                None => true,
                Some(q) => label.to_lowercase().contains(q) || w.process.to_lowercase().contains(q),
            }
        })
        .collect()
}

fn find_all(u: &IUIAutomation, root: &IUIAutomationElement, control_type: i32) -> AppResult<Vec<IUIAutomationElement>> {
    unsafe {
        let cond = u.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(control_type))?;
        let arr = root.FindAll(TreeScope_Descendants, &cond)?;
        let mut out = Vec::new();
        for i in 0..arr.Length()? {
            if let Ok(e) = arr.GetElement(i) {
                out.push(e);
            }
        }
        Ok(out)
    }
}

fn tab_elements(u: &IUIAutomation, root: &IUIAutomationElement) -> AppResult<Vec<IUIAutomationElement>> {
    find_all(u, root, UIA_TabItemControlTypeId.0)
}

fn is_selected(e: &IUIAutomationElement) -> bool {
    unsafe {
        e.GetCurrentPropertyValue(UIA_SelectionItemIsSelectedPropertyId)
            .ok()
            .and_then(|v| bool::try_from(&v).ok())
            .unwrap_or(false)
    }
}

/// The address bar's value (current URL of the active tab), if exposed.
fn address(u: &IUIAutomation, root: &IUIAutomationElement) -> Option<String> {
    let edits = find_all(u, root, UIA_EditControlTypeId.0).ok()?;
    for e in edits {
        unsafe {
            let name = e.CurrentName().map(|b| b.to_string().to_lowercase()).unwrap_or_default();
            let looks_like_bar = name.contains("address") || name.contains("адрес") || name.contains("url") || name.contains("search bar");
            let value = e
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .ok()
                .and_then(|p| p.CurrentValue().ok())
                .map(|b| b.to_string())
                .unwrap_or_default();
            if looks_like_bar && !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

pub fn state(hwnd: i64) -> AppResult<BrowserState> {
    let w = winmgr::describe(winmgr::to_hwnd(hwnd)).ok_or_else(|| AppError::not_found("Окно браузера закрыто"))?;
    let u = uia()?;
    unsafe {
        let root = u.ElementFromHandle(HWND(hwnd as isize as *mut _))?;
        let tabs = tab_elements(&u, &root)?
            .iter()
            .enumerate()
            .map(|(i, e)| Tab { index: i, title: e.CurrentName().map(|b| b.to_string()).unwrap_or_default(), selected: is_selected(e) })
            .collect::<Vec<_>>();
        Ok(BrowserState {
            minimized: w.state == "minimized",
            browser: browser_label(&w.process).unwrap_or("Браузер").to_string(),
            hwnd,
            window_title: w.title,
            url: address(&u, &root),
            tabs,
        })
    }
}

fn find_tab(u: &IUIAutomation, hwnd: i64, query: Option<&str>, index: Option<usize>) -> AppResult<(IUIAutomationElement, String)> {
    unsafe {
        let root = u.ElementFromHandle(HWND(hwnd as isize as *mut _))?;
        let tabs = tab_elements(u, &root)?;
        if tabs.is_empty() {
            return Err(AppError::unsupported("Браузер не показывает вкладки через UI Automation"));
        }
        if let Some(i) = index {
            let e = tabs.get(i).cloned().ok_or_else(|| AppError::not_found(format!("Вкладки №{i} нет")))?;
            let name = e.CurrentName().map(|b| b.to_string()).unwrap_or_default();
            return Ok((e, name));
        }
        let q = query.unwrap_or("").to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        for e in tabs {
            let name = e.CurrentName().map(|b| b.to_string()).unwrap_or_default();
            let lname = name.to_lowercase();
            if !words.is_empty() && words.iter().all(|w| lname.contains(w)) {
                return Ok((e, name));
            }
        }
        Err(AppError::not_found(format!("Вкладка «{q}» не найдена")))
    }
}

pub fn switch_tab(hwnd: i64, query: Option<&str>, index: Option<usize>) -> AppResult<BrowserState> {
    // A minimised Chromium window hides its tab strip from UI Automation.
    let _ = winmgr::focus(hwnd);
    std::thread::sleep(std::time::Duration::from_millis(250));
    let u = uia()?;
    let (tab, _) = find_tab(&u, hwnd, query, index)?;
    unsafe {
        if let Ok(p) = tab.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) {
            p.Select()?;
        } else if let Ok(p) = tab.GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(UIA_LegacyIAccessiblePatternId) {
            p.DoDefaultAction()?;
        } else {
            return Err(AppError::unsupported("Вкладку нельзя выбрать через UI Automation"));
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(250));
    state(hwnd)
}

pub fn close_tab(hwnd: i64, query: Option<&str>, index: Option<usize>) -> AppResult<(String, BrowserState)> {
    let u = uia()?;
    let (tab, name) = find_tab(&u, hwnd, query, index)?;
    unsafe {
        let buttons = find_all(&u, &tab, UIA_ButtonControlTypeId.0)?;
        let close = buttons.into_iter().last().ok_or_else(|| AppError::unsupported("У вкладки нет кнопки закрытия"))?;
        close.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)?.Invoke()?;
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    Ok((name, state(hwnd)?))
}

/// Visible text of the active page (Document element's Text pattern).
pub fn page_text(hwnd: i64, max_chars: usize) -> AppResult<String> {
    let u = uia()?;
    unsafe {
        let root = u.ElementFromHandle(HWND(hwnd as isize as *mut _))?;
        let docs = find_all(&u, &root, UIA_DocumentControlTypeId.0)?;
        for d in docs {
            if let Ok(tp) = d.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
                if let Ok(range) = tp.DocumentRange() {
                    let text: BSTR = range.GetText(max_chars as i32)?;
                    let s = text.to_string();
                    if !s.trim().is_empty() {
                        return Ok(s);
                    }
                }
            }
        }
        Err(AppError::unsupported("Браузер не отдал текст страницы через UI Automation"))
    }
}
