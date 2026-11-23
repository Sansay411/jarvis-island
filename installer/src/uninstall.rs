#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Jarvis's uninstaller: the same window as the setup (what will go, the
//! steps, the result). The removal itself is the NSIS uninstaller Tauri
//! builds, run silently, so shortcuts, the autostart entry and the «Apps &
//! features» record go exactly the way the package created them. On top of
//! that it clears what NSIS leaves: the voice models and caches and, if asked,
//! the settings, history and API keys.
//!
//! The NSIS hook registers this file as `UninstallString`, and a newer package
//! calls that string during updates (`/UPDATE /P _?=<folder>`). Any of those
//! switches therefore means «no window»: the call goes to `uninstall.exe`
//! unchanged and its exit code comes back.
//!
//! Safety rules (antiviruses watch uninstallers closely): no elevation, no
//! network, no shells or scripts, no killing processes by name, no
//! self-deletion tricks. Only the folder that really holds Jarvis and the
//! fixed `dev.jarvis.desktop` data folders are touched.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use jarvis_setup::{dir_size, install_dir, js_str, uninstall_temp_dir, Asset, Page};

const VERSION: &str = env!("JARVIS_VERSION");
const BUNDLE_ID: &str = "dev.jarvis.desktop";
/// «Models and cache»: everything Jarvis downloads or rebuilds by itself.
const CACHE: [&str; 3] = ["models", "tts_cache", "logs"];

fn asset(path: &str, config: &str) -> Option<Asset> {
    let (bytes, mime): (&'static [u8], &'static str) = match path {
        "/" | "/index.html" => (include_bytes!("../ui/uninstall.html"), "text/html; charset=utf-8"),
        "/uninstall.js" => (include_bytes!("../ui/uninstall.js"), "text/javascript; charset=utf-8"),
        "/config.js" => return Some((Cow::Owned(config.as_bytes().to_vec()), "text/javascript; charset=utf-8")),
        _ => return None,
    };
    Some((Cow::Borrowed(bytes), mime))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| matches!(a.to_ascii_uppercase().as_str(), "/S" | "/P" | "/UPDATE") || a.starts_with("_?=")) {
        std::process::exit(passthrough());
    }
    let demo = cfg!(debug_assertions) && args.iter().any(|a| a == "--demo");
    let from = args.iter().position(|a| a == "--from").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let dir = if demo { None } else { from.into_iter().chain(exe_dir()).chain(install_dir()).find_map(|d| checked_install_dir(&d)) };

    if let Some(d) = &dir {
        if running_inside(d) && relaunch_from_temp(d) {
            return;
        }
    }
    if !demo && !first_instance() {
        return;
    }
    let _ = std::env::set_current_dir(std::env::temp_dir());

    let config = format!("window.UNINSTALL={};", serde_json::json!({ "version": VERSION, "found": demo || dir.is_some() }));
    jarvis_setup::run("Удаление Джарвиса", uninstall_temp_dir().join("webview"), move |p| asset(p, &config), move |cmd, arg, page| {
        match cmd {
            "measure" => measure(page.clone(), demo),
            "uninstall" => {
                let page = page.clone();
                let dir = dir.clone();
                let (cache, personal) = (arg.starts_with('1'), arg.get(1..2) == Some("1"));
                std::thread::spawn(move || {
                    let result = match &dir {
                        _ if demo => rehearse(&page),
                        Some(d) => remove(&page, d, cache || personal, personal),
                        None => Err("Джарвис не найден на этом компьютере.".into()),
                    };
                    match result {
                        Ok((freed, left)) => page.eval(format!("window.uninstall.done({freed},{})", js_str(&left))),
                        Err(e) => page.eval(format!("window.uninstall.failed({})", js_str(&e))),
                    }
                });
            }
            _ => {}
        }
        false
    })
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

/// A folder is Jarvis's only if it holds both `jarvis.exe` and the NSIS
/// `uninstall.exe`, and it is not a drive root or a folder Windows owns.
/// Guards against a damaged or planted `InstallLocation`.
fn checked_install_dir(dir: &Path) -> Option<PathBuf> {
    let dir = dir.canonicalize().ok()?;
    if dir.parent().is_none() || !dir.join("jarvis.exe").is_file() || !dir.join("uninstall.exe").is_file() {
        return None;
    }
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::*;
        let owned = [
            &FOLDERID_Profile, &FOLDERID_LocalAppData, &FOLDERID_RoamingAppData, &FOLDERID_UserProgramFiles,
            &FOLDERID_Desktop, &FOLDERID_Documents, &FOLDERID_Downloads, &FOLDERID_ProgramFiles,
            &FOLDERID_ProgramFilesX86, &FOLDERID_Windows, &FOLDERID_System,
        ];
        if owned.iter().filter_map(|id| jarvis_setup::known_folder(id)?.canonicalize().ok()).any(|p| p == dir) {
            return None;
        }
    }
    Some(dir)
}

/// `C:\…` instead of the `\\?\C:\…` form `canonicalize` returns.
fn plain(p: &Path) -> String {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => s.strip_prefix(r"\\?\").unwrap_or(&s).to_string(),
    }
}

fn running_inside(dir: &Path) -> bool {
    std::env::current_exe().and_then(|e| e.canonicalize()).is_ok_and(|e| e.starts_with(dir))
}

/// A running program cannot remove its own folder, so the window runs from a
/// copy in %TEMP% (NSIS does the same with its `Au_.exe`). If the copy is in
/// use, another uninstall window is already open.
fn relaunch_from_temp(dir: &Path) -> bool {
    let tmp = uninstall_temp_dir();
    let copy = tmp.join("jarvis-uninstall.exe");
    std::fs::create_dir_all(&tmp).is_ok()
        && std::env::current_exe().and_then(|me| std::fs::copy(me, &copy)).is_ok()
        && Command::new(&copy).arg("--from").arg(plain(dir)).current_dir(&tmp).spawn().is_ok()
}

#[cfg(windows)]
fn first_instance() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    match unsafe { CreateMutexW(None, false, w!("Local\\JarvisUninstall")) } {
        // The handle stays open for the life of the process.
        Ok(_) => unsafe { GetLastError() != ERROR_ALREADY_EXISTS },
        Err(_) => true,
    }
}

#[cfg(not(windows))]
fn first_instance() -> bool {
    true
}

/// The arguments exactly as they were typed. `_?=` must stay last and
/// unquoted even when the folder has spaces, so nothing is re-parsed.
#[cfg(windows)]
fn raw_args() -> String {
    let line = unsafe { windows::Win32::System::Environment::GetCommandLineW().to_string() }.unwrap_or_default();
    let rest = match line.strip_prefix('"') {
        Some(s) => s.split_once('"').map_or("", |(_, r)| r),
        None => line.split_once(char::is_whitespace).map_or("", |(_, r)| r),
    };
    rest.trim_start().to_string()
}

/// Silent, passive and update calls: hand them to NSIS untouched.
fn passthrough() -> i32 {
    let Some(dir) = exe_dir().into_iter().chain(install_dir()).find_map(|d| checked_install_dir(&d)) else { return 2 };
    let mut cmd = Command::new(dir.join("uninstall.exe"));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.raw_arg(raw_args());
    }
    cmd.status().ok().and_then(|s| s.code()).unwrap_or(2)
}

fn data_dirs() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, FOLDERID_RoamingAppData};
        [&FOLDERID_LocalAppData, &FOLDERID_RoamingAppData].into_iter().filter_map(|id| Some(jarvis_setup::known_folder(id)?.join(BUNDLE_ID))).collect()
    }
    #[cfg(not(windows))]
    Vec::new()
}

fn cache_dirs() -> Vec<PathBuf> {
    data_dirs().into_iter().take(1).flat_map(|local| CACHE.map(|c| local.join(c))).collect()
}

fn measure(page: Page, demo: bool) {
    std::thread::spawn(move || {
        let (cache, all) = if demo {
            (1_480_000_000, 1_560_000_000)
        } else {
            (cache_dirs().iter().map(|d| dir_size(d)).sum::<u64>(), data_dirs().iter().map(|d| dir_size(d)).sum::<u64>())
        };
        page.eval(format!("window.uninstall.sizes({cache},{all})"));
    });
}

/// Closes Jarvis through the Restart Manager, which asks the windows of that
/// exact file to close and ends them if they don't. NSIS does the same.
#[cfg(windows)]
fn close_app(exe: &Path) -> Result<(), String> {
    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::System::RestartManager::{RmEndSession, RmForceShutdown, RmRegisterResources, RmShutdown, RmStartSession, CCH_RM_SESSION_KEY};
    let mut session = 0u32;
    let mut key = [0u16; CCH_RM_SESSION_KEY as usize + 1];
    unsafe {
        if RmStartSession(&mut session, None, PWSTR(key.as_mut_ptr())).is_err() {
            return Ok(());
        }
        let path = HSTRING::from(exe.as_os_str());
        let files = [PCWSTR(path.as_ptr())];
        let mut r = RmRegisterResources(session, Some(&files), None, None);
        if r.is_ok() {
            r = RmShutdown(session, RmForceShutdown.0 as u32, None);
        }
        let _ = RmEndSession(session);
        if r.is_err() {
            return Err("Не получилось закрыть Джарвиса. Закройте его через значок в трее и нажмите «Повторить».".into());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn close_app(_: &Path) -> Result<(), String> {
    Ok(())
}

/// Removes a data folder, waiting a few seconds for WebView2 processes that
/// still hold files. Links are removed, never followed.
fn remove_tree(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return true };
    if meta.file_type().is_symlink() {
        return std::fs::remove_dir(path).or_else(|_| std::fs::remove_file(path)).is_ok();
    }
    for _ in 0..12 {
        if std::fs::remove_dir_all(path).is_ok() || !path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    !path.exists()
}

/// API keys live in the Credential Manager under `JARVIS/…`.
#[cfg(windows)]
fn forget_secrets() {
    use windows::core::w;
    use windows::Win32::Security::Credentials::{CredDeleteW, CredEnumerateW, CredFree, CREDENTIALW};
    use windows::Win32::System::Registry::{RegDeleteKeyW, RegDeleteTreeW, HKEY_CURRENT_USER};
    unsafe {
        let mut count = 0u32;
        let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
        if CredEnumerateW(w!("JARVIS/*"), None, &mut count, &mut list).is_ok() && !list.is_null() {
            for &cred in std::slice::from_raw_parts(list, count as usize) {
                let _ = CredDeleteW((*cred).TargetName, (*cred).Type, None);
            }
            CredFree(list as _);
        }
        // The install folder and language NSIS remembers for the next setup.
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, w!("Software\\jarvis\\Jarvis"));
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, w!("Software\\jarvis"));
    }
}

#[cfg(not(windows))]
fn forget_secrets() {}

/// The real removal. Returns the bytes freed and a folder that could not be
/// removed, if any.
fn remove(page: &Page, dir: &Path, cache: bool, personal: bool) -> Result<(u64, String), String> {
    let progress = |p: f64| page.eval(format!("window.uninstall.progress({p:.0})"));
    let exe = dir.join("jarvis.exe");
    let nsis = dir.join("uninstall.exe");

    page.eval("window.uninstall.step(0)");
    progress(4.0);
    if exe.exists() {
        close_app(&exe)?;
    }
    progress(16.0);

    page.eval("window.uninstall.step(1)");
    let app_size = dir_size(dir);
    if nsis.exists() {
        let mut cmd = Command::new(&nsis);
        cmd.arg("/S");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.raw_arg(format!("_?={}", plain(dir)));
        }
        let mut child = cmd.spawn().map_err(|e| format!("Не удалось запустить удаление: {e}"))?;
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break status;
            }
            progress(16.0 + 56.0 * (1.0 - (-start.elapsed().as_secs_f64() / 3.0).exp()));
            std::thread::sleep(Duration::from_millis(120));
        };
        if !status.success() {
            return Err(format!("Удаление остановилось с кодом {}. Перезагрузите компьютер и попробуйте ещё раз.", status.code().unwrap_or(-1)));
        }
    }
    if exe.exists() {
        return Err("Файлы Джарвиса заняты другой программой. Перезагрузите компьютер и попробуйте ещё раз.".into());
    }
    // NSIS run in place (`_?=`) cannot delete itself; the hook's copy of this
    // file is not running either, the window is the copy from %TEMP%.
    let _ = std::fs::remove_file(&nsis);
    let _ = std::fs::remove_file(dir.join("jarvis-uninstall.exe"));
    let _ = std::fs::remove_dir(dir);
    let app_freed = app_size.saturating_sub(dir_size(dir));
    progress(76.0);

    page.eval("window.uninstall.step(2)");
    let targets = if personal { data_dirs() } else if cache { cache_dirs() } else { Vec::new() };
    let before: u64 = targets.iter().map(|t| dir_size(t)).sum();
    let left = targets.iter().find(|t| !remove_tree(t)).map(|t| plain(t)).unwrap_or_default();
    let after: u64 = targets.iter().map(|t| dir_size(t)).sum();
    if personal {
        forget_secrets();
    }
    progress(100.0);
    Ok((app_freed + before.saturating_sub(after), left))
}

/// `--demo` in debug builds: the same screens, nothing touched.
fn rehearse(page: &Page) -> Result<(u64, String), String> {
    for (step, until) in [(0, 16), (1, 76), (2, 100)] {
        page.eval(format!("window.uninstall.step({step})"));
        for p in (0..=until).step_by(2) {
            page.eval(format!("window.uninstall.progress({p})"));
            std::thread::sleep(Duration::from_millis(40));
        }
    }
    Ok((1_620_000_000, String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_drops_the_verbatim_prefix() {
        assert_eq!(plain(Path::new(r"\\?\C:\Users\a b\Jarvis")), r"C:\Users\a b\Jarvis");
        assert_eq!(plain(Path::new(r"\\?\UNC\srv\share\Jarvis")), r"\\srv\share\Jarvis");
        assert_eq!(plain(Path::new(r"C:\Jarvis")), r"C:\Jarvis");
    }

    #[test]
    fn only_a_real_jarvis_folder_counts() {
        let dir = std::env::temp_dir().join(format!("jarvis-uninstall-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(checked_install_dir(&dir).is_none(), "empty folder");
        std::fs::write(dir.join("jarvis.exe"), b"").unwrap();
        assert!(checked_install_dir(&dir).is_none(), "no uninstall.exe");
        std::fs::write(dir.join("uninstall.exe"), b"").unwrap();
        assert_eq!(checked_install_dir(&dir), Some(dir.canonicalize().unwrap()));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(checked_install_dir(Path::new(r"C:\")).is_none(), "drive root");
    }

    #[test]
    fn cache_stays_inside_the_data_folder() {
        for d in cache_dirs() {
            assert!(d.parent().unwrap().ends_with(BUNDLE_ID), "{d:?}");
        }
        assert!(data_dirs().iter().all(|d| d.ends_with(BUNDLE_ID)));
    }
}
