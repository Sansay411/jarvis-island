//! Installed application discovery and launching.
//!
//! The source of truth is the shell's `AppsFolder` (the Start menu's "All
//! apps"): it lists classic desktop apps and packaged (UWP/MSIX) apps alike,
//! and every entry can be launched with `shell:AppsFolder\<parsing name>`.
//! Start-menu shortcuts are additionally resolved to learn executable paths,
//! which makes launch verification and "is it running?" checks precise.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use serde::Serialize;
use windows::core::{Interface, GUID, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellLinkW, SHGetKnownFolderItem,
    SHGetKnownFolderPath, ShellExecuteW, ShellLink, KF_FLAG_DEFAULT, SIGDN, SIGDN_NORMALDISPLAY,
    SIGDN_PARENTRELATIVEPARSING,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::winmgr::{self, WindowInfo};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEntry {
    pub name: String,
    /// AppsFolder parsing name (AUMID for packaged apps, path-like otherwise).
    pub id: String,
    pub exe: Option<String>,
    /// "packaged" | "desktop"
    pub kind: &'static str,
}

#[derive(Default)]
pub struct AppIndex {
    entries: RwLock<Vec<AppEntry>>,
    built_at: RwLock<Option<Instant>>,
}

fn take_pwstr(p: PWSTR) -> String {
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

fn display_name(item: &IShellItem, kind: SIGDN) -> Option<String> {
    unsafe { item.GetDisplayName(kind).ok().map(take_pwstr) }
}

fn enumerate_apps_folder() -> AppResult<Vec<(String, String)>> {
    unsafe {
        let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
        let en: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
        let mut out = Vec::new();
        loop {
            let mut items = [None];
            let mut fetched = 0u32;
            if en.Next(&mut items, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = items[0].take() else { break };
            if let (Some(name), Some(id)) = (
                display_name(&item, SIGDN_NORMALDISPLAY),
                display_name(&item, SIGDN_PARENTRELATIVEPARSING),
            ) {
                out.push((name, id));
            }
        }
        Ok(out)
    }
}

/// `{KNOWNFOLDERID}\rest\app.exe` → absolute path.
fn resolve_known_folder(id: &str) -> Option<String> {
    let rest = id.strip_prefix('{')?;
    let (guid, tail) = rest.split_once('}')?;
    let guid = GUID::try_from(guid).ok()?;
    let base = unsafe { SHGetKnownFolderPath(&guid, KF_FLAG_DEFAULT, None).ok().map(take_pwstr)? };
    Some(format!("{base}{tail}"))
}

fn resolve_lnk(path: &Path) -> Option<String> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(path.as_os_str()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        let mut fd = WIN32_FIND_DATAW::default();
        link.GetPath(&mut buf, &mut fd, 0).ok()?;
        let target = super::from_wide(&buf);
        target.to_lowercase().ends_with(".exe").then_some(target)
    }
}

fn start_menu_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(pd) = std::env::var("ProgramData") {
        dirs.push(PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(ad) = std::env::var("APPDATA") {
        dirs.push(PathBuf::from(ad).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    dirs
}

fn scan_shortcuts(dir: &Path, depth: usize, out: &mut HashMap<String, String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth < 4 {
                scan_shortcuts(&p, depth + 1, out);
            }
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
            if let (Some(stem), Some(target)) = (p.file_stem(), resolve_lnk(&p)) {
                out.entry(stem.to_string_lossy().to_lowercase()).or_insert(target);
            }
        }
    }
}

fn build() -> AppResult<Vec<AppEntry>> {
    let mut lnk_targets = HashMap::new();
    for d in start_menu_dirs() {
        scan_shortcuts(&d, 0, &mut lnk_targets);
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (name, id) in enumerate_apps_folder()? {
        if !seen.insert(id.to_lowercase()) {
            continue;
        }
        // Packaged apps are identified by an AUMID: "PackageFamily!AppId".
        let packaged = id.contains('!') && !id.contains('\\');
        let exe = if packaged {
            None
        } else if id.starts_with('{') {
            resolve_known_folder(&id)
        } else if Path::new(&id).is_absolute() {
            Some(id.clone())
        } else {
            None
        }
        .filter(|p| p.to_lowercase().ends_with(".exe"))
        .or_else(|| lnk_targets.get(&name.to_lowercase()).cloned());
        out.push(AppEntry { name, id, exe, kind: if packaged { "packaged" } else { "desktop" } });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Relevance of an app for a free-text query (0 = no match).
fn score(app: &AppEntry, query: &str) -> u32 {
    let q = normalize(query);
    if q.is_empty() {
        return 0;
    }
    let name = normalize(&app.name);
    let exe = app
        .exe
        .as_deref()
        .and_then(|e| Path::new(e).file_stem())
        .map(|s| normalize(&s.to_string_lossy()))
        .unwrap_or_default();
    if name == q || exe == q {
        return 100;
    }
    if name.starts_with(&q) {
        return 85;
    }
    if exe.starts_with(&q) {
        return 80;
    }
    if name.contains(&q) {
        return 70;
    }
    // Every query word appears in the name (e.g. "vs code" → "Visual Studio Code"
    // won't match, but "studio code" will); initials ("vsc") are matched too.
    let words: Vec<&str> = q.split(' ').collect();
    if words.iter().all(|w| name.contains(w)) {
        return 60;
    }
    let initials: String = name.split(' ').filter_map(|w| w.chars().next()).collect();
    if initials.len() >= 2 && (initials == q.replace(' ', "") || initials.starts_with(&q.replace(' ', ""))) {
        return 55;
    }
    0
}

impl AppIndex {
    pub fn ensure_fresh(&self, max_age: Duration) -> AppResult<()> {
        let stale = self.built_at.read().is_none_or(|t| t.elapsed() > max_age);
        if stale {
            let started = Instant::now();
            let entries = build()?;
            tracing::info!(count = entries.len(), ms = started.elapsed().as_millis() as u64, "app index built");
            *self.entries.write() = entries;
            *self.built_at.write() = Some(Instant::now());
        }
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> AppResult<Vec<AppEntry>> {
        Ok(self.search_scored(query, limit)?.into_iter().map(|(_, a)| a).collect())
    }

    /// Search returning relevance scores (100 = exact name match).
    pub fn search_scored(&self, query: &str, limit: usize) -> AppResult<Vec<(u32, AppEntry)>> {
        self.ensure_fresh(Duration::from_secs(600))?;
        let mut scored: Vec<(u32, AppEntry)> = self
            .entries
            .read()
            .iter()
            .filter_map(|a| {
                let s = score(a, query);
                (s > 0).then(|| (s, a.clone()))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.len().cmp(&b.1.name.len())));
        scored.truncate(limit);
        Ok(scored)
    }

    pub fn all(&self) -> AppResult<Vec<AppEntry>> {
        self.ensure_fresh(Duration::from_secs(600))?;
        Ok(self.entries.read().clone())
    }

    pub fn by_id(&self, id: &str) -> AppResult<Option<AppEntry>> {
        self.ensure_fresh(Duration::from_secs(600))?;
        Ok(self.entries.read().iter().find(|a| a.id.eq_ignore_ascii_case(id)).cloned())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOutcome {
    pub app: AppEntry,
    /// A window belonging to the app was observed after launching.
    pub verified: bool,
    pub window: Option<WindowInfo>,
    pub note: Option<String>,
}

pub fn shell_open(target: &str, args: Option<&str>, cwd: Option<&str>) -> AppResult<()> {
    let target_w = HSTRING::from(target);
    let args_w = args.map(HSTRING::from);
    let cwd_w = cwd.map(HSTRING::from);
    let res = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("open"),
            &target_w,
            args_w.as_ref().map(|a| PCWSTR(a.as_ptr())).unwrap_or(PCWSTR::null()),
            cwd_w.as_ref().map(|c| PCWSTR(c.as_ptr())).unwrap_or(PCWSTR::null()),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute returns a value > 32 on success.
    if res.0 as isize <= 32 {
        return Err(AppError::platform(format!("Windows не смогла открыть «{target}» (код {})", res.0 as isize)));
    }
    Ok(())
}

fn exe_name(path: &str) -> Option<String> {
    Path::new(path).file_name().map(|n| n.to_string_lossy().to_lowercase())
}

/// Launch an app and wait (up to `timeout`) for one of its windows to appear
/// or come to the foreground.
pub fn launch(app: &AppEntry, timeout: Duration) -> AppResult<LaunchOutcome> {
    let before: HashSet<i64> = winmgr::list_windows().into_iter().map(|w| w.hwnd).collect();
    shell_open(&format!("shell:AppsFolder\\{}", app.id), None, None)?;

    let exe = app.exe.as_deref().and_then(exe_name);
    let name_lc = app.name.to_lowercase();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        let windows = winmgr::list_windows();
        let matches_app = |w: &WindowInfo| match &exe {
            Some(e) => w.process.to_lowercase() == *e,
            None => w.title.to_lowercase().contains(&name_lc) || !before.contains(&w.hwnd),
        };
        // Prefer the foreground window if it belongs to the app (covers
        // single-instance apps that just re-activate an existing window).
        if let Some(fg) = windows.iter().find(|w| w.foreground) {
            if matches_app(fg) && (exe.is_some() || !before.contains(&fg.hwnd)) {
                return Ok(LaunchOutcome { app: app.clone(), verified: true, window: Some(fg.clone()), note: None });
            }
        }
        if let Some(new) = windows.iter().find(|w| !before.contains(&w.hwnd) && matches_app(w)) {
            return Ok(LaunchOutcome { app: app.clone(), verified: true, window: Some(new.clone()), note: None });
        }
    }
    Ok(LaunchOutcome {
        app: app.clone(),
        verified: false,
        window: None,
        note: Some("Команда запуска принята, но окно приложения не появилось за отведённое время".into()),
    })
}

/// Windows of a running app (by executable name), if any.
pub fn running_windows(app: &AppEntry) -> Vec<WindowInfo> {
    let Some(exe) = app.exe.as_deref().and_then(exe_name) else { return Vec::new() };
    winmgr::list_windows().into_iter().filter(|w| w.process.to_lowercase() == exe).collect()
}
