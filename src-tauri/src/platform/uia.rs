//! Windows UI Automation: structured inspection of unfamiliar interfaces
//! and pattern-based actions (Invoke, Value, Toggle, SelectionItem,
//! ExpandCollapse, ScrollItem). Preferred over pixel clicking; the visual
//! fallback lives in `input`.
//!
//! A snapshot keeps the element references so a later action can target
//! "[12]" from the list the model saw. Elements are re-validated before use.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use windows::core::{Interface, BSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::*;

use super::display::Rect;
use super::input;
use crate::error::{AppError, AppResult};

struct Element(IUIAutomationElement);
// UIA client objects are free-threaded (MTA); JARVIS only touches them from
// COM-initialised blocking-pool threads.
unsafe impl Send for Element {}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementInfo {
    pub id: usize,
    pub role: &'static str,
    pub name: String,
    pub value: Option<String>,
    pub rect: Rect,
    pub enabled: bool,
    pub focused: bool,
    pub actions: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub snapshot_id: u64,
    pub hwnd: i64,
    pub window_title: String,
    pub elements: Vec<ElementInfo>,
    pub total_found: usize,
    pub elapsed_ms: u64,
}

struct Stored {
    id: u64,
    elements: Vec<Element>,
    infos: Vec<ElementInfo>,
}

static LAST: Mutex<Option<Stored>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn automation() -> AppResult<IUIAutomation> {
    unsafe {
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        // Don't hang on unresponsive apps.
        if let Ok(u2) = uia.cast::<IUIAutomation2>() {
            let _ = u2.SetConnectionTimeout(2000);
            let _ = u2.SetTransactionTimeout(4000);
        }
        Ok(uia)
    }
}

fn role_name(ct: UIA_CONTROLTYPE_ID) -> &'static str {
    match ct.0 {
        50000 => "button",
        50001 => "calendar",
        50002 => "checkbox",
        50003 => "combobox",
        50004 => "edit",
        50005 => "link",
        50006 => "image",
        50007 => "listitem",
        50008 => "list",
        50009 => "menu",
        50010 => "menubar",
        50011 => "menuitem",
        50012 => "progressbar",
        50013 => "radio",
        50014 => "scrollbar",
        50015 => "slider",
        50016 => "spinner",
        50017 => "statusbar",
        50018 => "tab",
        50019 => "tabitem",
        50020 => "text",
        50021 => "toolbar",
        50022 => "tooltip",
        50023 => "tree",
        50024 => "treeitem",
        50025 => "custom",
        50026 => "group",
        50027 => "thumb",
        50028 => "datagrid",
        50029 => "dataitem",
        50030 => "document",
        50031 => "splitbutton",
        50032 => "window",
        50033 => "pane",
        50034 => "header",
        50035 => "headeritem",
        50036 => "table",
        50037 => "titlebar",
        50038 => "separator",
        _ => "other",
    }
}

fn interactive(role: &str) -> bool {
    matches!(
        role,
        "button" | "checkbox" | "combobox" | "edit" | "link" | "listitem" | "menuitem" | "radio" | "slider" | "spinner"
            | "tabitem" | "treeitem" | "splitbutton" | "dataitem" | "document" | "headeritem"
    )
}

fn cached_bool(e: &IUIAutomationElement, prop: UIA_PROPERTY_ID) -> bool {
    unsafe { e.GetCachedPropertyValue(prop).ok().and_then(|v| bool::try_from(&v).ok()).unwrap_or(false) }
}

/// Inspect a window (default: the foreground window).
pub fn snapshot(hwnd: HWND, budget: usize) -> AppResult<Snapshot> {
    let started = Instant::now();
    let uia = automation()?;
    unsafe {
        let root = uia.ElementFromHandle(hwnd).map_err(|_| AppError::not_found("Окно недоступно для UI Automation"))?;
        let title = root.CurrentName().map(|b| b.to_string()).unwrap_or_default();
        let cache = uia.CreateCacheRequest()?;
        for p in [
            UIA_NamePropertyId,
            UIA_ControlTypePropertyId,
            UIA_BoundingRectanglePropertyId,
            UIA_IsEnabledPropertyId,
            UIA_HasKeyboardFocusPropertyId,
            UIA_ValueValuePropertyId,
            UIA_IsInvokePatternAvailablePropertyId,
            UIA_IsValuePatternAvailablePropertyId,
            UIA_IsTogglePatternAvailablePropertyId,
            UIA_IsSelectionItemPatternAvailablePropertyId,
            UIA_IsExpandCollapsePatternAvailablePropertyId,
            UIA_IsScrollItemPatternAvailablePropertyId,
        ] {
            cache.AddProperty(p)?;
        }
        let on_screen = uia.CreatePropertyCondition(UIA_IsOffscreenPropertyId, &VARIANT::from(false))?;
        let control = uia.ControlViewCondition()?;
        let cond = uia.CreateAndCondition(&on_screen, &control)?;
        let found = root.FindAllBuildCache(TreeScope_Descendants, &cond, &cache)?;
        let total = found.Length()? as usize;

        let mut primary = Vec::new();
        let mut secondary = Vec::new();
        for i in 0..total as i32 {
            let Ok(e) = found.GetElement(i) else { continue };
            let role = role_name(e.CachedControlType().unwrap_or_default());
            let name = e.CachedName().map(|b| b.to_string()).unwrap_or_default().trim().to_string();
            let r = e.CachedBoundingRectangle().unwrap_or_default();
            let rect: Rect = r.into();
            if rect.width <= 1 || rect.height <= 1 {
                continue;
            }
            let is_interactive = interactive(role);
            if !is_interactive && name.is_empty() {
                continue;
            }
            let mut actions = Vec::new();
            if cached_bool(&e, UIA_IsInvokePatternAvailablePropertyId) {
                actions.push("invoke");
            }
            if cached_bool(&e, UIA_IsValuePatternAvailablePropertyId) {
                actions.push("set_value");
            }
            if cached_bool(&e, UIA_IsTogglePatternAvailablePropertyId) {
                actions.push("toggle");
            }
            if cached_bool(&e, UIA_IsSelectionItemPatternAvailablePropertyId) {
                actions.push("select");
            }
            if cached_bool(&e, UIA_IsExpandCollapsePatternAvailablePropertyId) {
                actions.push("expand");
            }
            if cached_bool(&e, UIA_IsScrollItemPatternAvailablePropertyId) {
                actions.push("scroll_into_view");
            }
            actions.push("click");
            let value = e
                .GetCachedPropertyValue(UIA_ValueValuePropertyId)
                .ok()
                .and_then(|v| BSTR::try_from(&v).ok())
                .map(|b| b.to_string())
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(200).collect());
            let info = ElementInfo {
                id: 0,
                role,
                name: name.chars().take(120).collect(),
                value,
                rect,
                enabled: cached_bool(&e, UIA_IsEnabledPropertyId),
                focused: cached_bool(&e, UIA_HasKeyboardFocusPropertyId),
                actions,
            };
            if is_interactive {
                primary.push((i, e, info));
            } else {
                secondary.push((i, e, info));
            }
        }
        // Interactive elements first, labels fill the remaining budget; keep
        // document order within the final list.
        let mut chosen: Vec<_> = primary.into_iter().take(budget).collect();
        let room = budget.saturating_sub(chosen.len());
        chosen.extend(secondary.into_iter().take(room));
        chosen.sort_by_key(|(i, _, _)| *i);

        let mut elements = Vec::with_capacity(chosen.len());
        let mut infos = Vec::with_capacity(chosen.len());
        for (idx, (_, e, mut info)) in chosen.into_iter().enumerate() {
            info.id = idx;
            elements.push(Element(e));
            infos.push(info);
        }
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        *LAST.lock() = Some(Stored { id, elements, infos: infos.clone() });
        Ok(Snapshot {
            snapshot_id: id,
            hwnd: hwnd.0 as isize as i64,
            window_title: title,
            elements: infos,
            total_found: total,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }
}

/// Compact, token-efficient rendering for the model.
pub fn render(s: &Snapshot) -> String {
    let mut out = String::new();
    for e in &s.elements {
        let mut line = format!("[{}] {}", e.id, e.role);
        if !e.name.is_empty() {
            line.push_str(&format!(" \"{}\"", e.name.replace('\n', " ")));
        }
        if let Some(v) = &e.value {
            line.push_str(&format!(" value=\"{}\"", v.replace('\n', " ")));
        }
        if !e.enabled {
            line.push_str(" disabled");
        }
        if e.focused {
            line.push_str(" focused");
        }
        let acts: Vec<&str> = e.actions.iter().copied().filter(|a| *a != "click").collect();
        if !acts.is_empty() {
            line.push_str(&format!(" {{{}}}", acts.join(",")));
        }
        line.push_str(&format!(" @{},{} {}x{}\n", e.rect.x, e.rect.y, e.rect.width, e.rect.height));
        out.push_str(&line);
    }
    out
}

/// Name of an element in the latest snapshot (for risk assessment).
pub fn element_name(snapshot_id: u64, id: usize) -> Option<String> {
    let last = LAST.lock();
    let s = last.as_ref()?;
    (s.id == snapshot_id).then(|| s.infos.get(id).map(|i| i.name.clone())).flatten()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActOutcome {
    pub action: String,
    pub element: String,
    /// State read back after the action, where the pattern exposes one.
    pub after: Option<String>,
}

pub fn act(snapshot_id: u64, id: usize, action: &str, value: Option<&str>) -> AppResult<ActOutcome> {
    let (el, info) = {
        let last = LAST.lock();
        let s = last.as_ref().ok_or_else(|| AppError::invalid("Сначала нужен снимок интерфейса (ui_inspect)"))?;
        if s.id != snapshot_id {
            return Err(AppError::invalid("Снимок интерфейса устарел — сделайте новый ui_inspect"));
        }
        let el = s.elements.get(id).ok_or_else(|| AppError::invalid(format!("Элемента [{id}] нет в снимке")))?;
        (el.0.clone(), s.infos[id].clone())
    };
    let label = format!("{} \"{}\"", info.role, info.name);
    unsafe {
        // Re-validate: the element must still exist.
        let current = el.CurrentBoundingRectangle().map_err(|_| AppError::not_found(format!("{label} больше не существует — интерфейс изменился")))?;
        let after = match action {
            "invoke" => {
                el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)?.Invoke()?;
                None
            }
            "set_value" => {
                let v = value.ok_or_else(|| AppError::invalid("Для set_value нужен value"))?;
                let p = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)?;
                p.SetValue(&BSTR::from(v))?;
                std::thread::sleep(Duration::from_millis(80));
                p.CurrentValue().ok().map(|b| format!("value=\"{}\"", b.to_string().chars().take(200).collect::<String>()))
            }
            "toggle" => {
                let p = el.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)?;
                p.Toggle()?;
                std::thread::sleep(Duration::from_millis(80));
                p.CurrentToggleState().ok().map(|s| format!("toggle={}", match s.0 { 0 => "off", 1 => "on", _ => "indeterminate" }))
            }
            "select" => {
                let p = el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)?;
                p.Select()?;
                std::thread::sleep(Duration::from_millis(80));
                p.CurrentIsSelected().ok().map(|b| format!("selected={}", b.as_bool()))
            }
            "expand" | "collapse" => {
                let p = el.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId)?;
                if action == "expand" { p.Expand()? } else { p.Collapse()? }
                std::thread::sleep(Duration::from_millis(120));
                p.CurrentExpandCollapseState().ok().map(|s| format!("state={}", match s.0 { 0 => "collapsed", 1 => "expanded", 2 => "partial", _ => "leaf" }))
            }
            "scroll_into_view" => {
                el.GetCurrentPatternAs::<IUIAutomationScrollItemPattern>(UIA_ScrollItemPatternId)?.ScrollIntoView()?;
                None
            }
            "focus" => {
                el.SetFocus()?;
                None
            }
            "click" | "double_click" | "right_click" => {
                let r: Rect = current.into();
                if r.width <= 0 || r.height <= 0 {
                    return Err(AppError::invalid(format!("{label} не видно на экране")));
                }
                let (cx, cy) = (r.x + r.width / 2, r.y + r.height / 2);
                let button = if action == "right_click" { input::Button::Right } else { input::Button::Left };
                input::click(cx, cy, button, if action == "double_click" { 2 } else { 1 })?;
                None
            }
            other => return Err(AppError::invalid(format!("Неизвестное действие «{other}»"))),
        };
        Ok(ActOutcome { action: action.to_string(), element: label, after })
    }
}
