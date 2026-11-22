pub use super::data_apps::*;
use super::{native, winmgr};
use crate::error::{AppError, AppResult};
use crate::platform::winmgr::WindowInfo;
use parking_lot::Mutex;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct AppIndex {
    cache: Mutex<Cache>,
}
#[derive(Default)]
struct Cache {
    updated: Option<Instant>,
    entries: Vec<AppEntry>,
}

fn bundle_entry(path: &Path, info: &plist::Value) -> Option<AppEntry> {
    let dict = info.as_dictionary()?;
    let value = |key| {
        dict.get(key)
            .and_then(plist::Value::as_string)
            .filter(|s| !s.is_empty())
    };
    let name = value("CFBundleDisplayName")
        .or_else(|| value("CFBundleName"))
        .map(str::to_owned)
        .or_else(|| path.file_stem()?.to_str().map(str::to_owned))?;
    let path = path.to_string_lossy().into_owned();
    Some(AppEntry {
        name,
        id: value("CFBundleIdentifier").unwrap_or(&path).into(),
        exe: Some(path),
        kind: "desktop",
    })
}

fn scan(root: &Path, depth: usize, entries: &mut BTreeMap<String, AppEntry>) {
    let Ok(dir) = std::fs::read_dir(root) else {
        return;
    };
    for entry in dir.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
            && path.is_dir()
        {
            if let Ok(info) = plist::Value::from_file(path.join("Contents/Info.plist")) {
                if let Some(app) = bundle_entry(&path, &info) {
                    entries.insert(app.id.clone(), app);
                }
            }
        } else if depth > 0
            && entry.file_type().is_ok_and(|t| t.is_dir())
            && !entry.file_name().to_string_lossy().starts_with('.')
        {
            scan(&path, depth - 1, entries);
        }
    }
}

fn score(app: &AppEntry, query: &str) -> u32 {
    let q = query.trim().to_lowercase();
    let name = app.name.to_lowercase();
    let hay = format!("{} {} {}", name, app.id, app.exe.as_deref().unwrap_or("")).to_lowercase();
    if q.is_empty() {
        1
    } else if name == q {
        100
    } else if name.starts_with(&q) {
        80
    } else if q.split_whitespace().all(|w| hay.contains(w)) {
        50
    } else {
        0
    }
}

impl AppIndex {
    pub fn ensure_fresh(&self, max_age: Duration) -> AppResult<()> {
        let mut cache = self.cache.lock();
        if cache.updated.is_some_and(|t| t.elapsed() < max_age) {
            return Ok(());
        }
        let mut entries = BTreeMap::new();
        let mut roots = vec![
            PathBuf::from("/System/Applications"),
            PathBuf::from("/Applications"),
        ];
        if let Some(home) = dirs::home_dir() {
            roots.push(home.join("Applications"));
        }
        for root in roots {
            scan(&root, 4, &mut entries);
        }
        cache.entries = entries.into_values().collect();
        cache.entries.sort_by_key(|a| a.name.to_lowercase());
        cache.updated = Some(Instant::now());
        Ok(())
    }
    pub fn search(&self, query: &str, limit: usize) -> AppResult<Vec<AppEntry>> {
        Ok(self
            .search_scored(query, limit)?
            .into_iter()
            .map(|(_, app)| app)
            .collect())
    }
    pub fn search_scored(&self, query: &str, limit: usize) -> AppResult<Vec<(u32, AppEntry)>> {
        self.ensure_fresh(Duration::from_secs(600))?;
        let mut found: Vec<_> = self
            .cache
            .lock()
            .entries
            .iter()
            .filter_map(|a| {
                let score = score(a, query);
                (score > 0).then(|| (score, a.clone()))
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        found.truncate(limit);
        Ok(found)
    }
    pub fn all(&self) -> AppResult<Vec<AppEntry>> {
        self.ensure_fresh(Duration::from_secs(600))?;
        Ok(self.cache.lock().entries.clone())
    }
    pub fn by_id(&self, id: &str) -> AppResult<Option<AppEntry>> {
        Ok(self
            .all()?
            .into_iter()
            .find(|a| a.id.eq_ignore_ascii_case(id) || a.exe.as_deref() == Some(id)))
    }
}

pub fn shell_open(target: &str, args: Option<&str>, cwd: Option<&str>) -> AppResult<()> {
    if target.trim().is_empty() {
        return Err(AppError::invalid("Не указано, что открыть"));
    }
    let args = shlex::split(args.unwrap_or(""))
        .ok_or_else(|| AppError::invalid("Некорректные кавычки в аргументах"))?;
    if cwd.is_some_and(|p| !Path::new(p).is_dir()) {
        return Err(AppError::not_found("Рабочая папка не найдена"));
    }
    let mut cmd = std::process::Command::new("/usr/bin/open");
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    if target.ends_with(".app") || !args.is_empty() {
        cmd.arg("-a").arg(target);
    } else {
        cmd.arg("--").arg(target);
    }
    if !args.is_empty() {
        cmd.arg("--args").args(args);
    }
    let status = cmd
        .status()
        .map_err(|e| AppError::platform(format!("Не удалось открыть приложение: {e}")))?;
    if !status.success() {
        return Err(AppError::platform(
            "macOS не удалось открыть приложение или файл",
        ));
    }
    Ok(())
}

pub fn launch(app: &AppEntry, timeout: Duration) -> AppResult<LaunchOutcome> {
    if let Some(path) = &app.exe {
        native::execute("/usr/bin/open", &["-a", path], None)?;
    } else {
        native::execute("/usr/bin/open", &["-b", &app.id], None)?;
    }
    let start = Instant::now();
    let window = loop {
        if let Some(window) = running_windows(app).into_iter().next() {
            break Some(window);
        }
        if start.elapsed() >= timeout {
            break None;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let verified = window.is_some();
    Ok(LaunchOutcome {
        app: app.clone(),
        verified,
        window,
        note: (!verified).then(|| "Приложение запущено; видимое окно пока не найдено".into()),
    })
}

pub fn running_windows(app: &AppEntry) -> Vec<WindowInfo> {
    let stem = app
        .exe
        .as_deref()
        .and_then(|p| Path::new(p).file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or(&app.name);
    winmgr::list_windows()
        .into_iter()
        .filter(|w| {
            w.process.eq_ignore_ascii_case(&app.name) || w.process.eq_ignore_ascii_case(stem)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_bundle_metadata_and_searches_names_and_ids() {
        let info = plist::Value::from_reader_xml(std::io::Cursor::new(br#"<plist version="1.0"><dict><key>CFBundleDisplayName</key><string>Text Edit</string><key>CFBundleIdentifier</key><string>com.test.editor</string></dict></plist>"#)).unwrap();
        let app = bundle_entry(Path::new("/Applications/Editor.app"), &info).unwrap();
        assert_eq!(app.name, "Text Edit");
        assert_eq!(app.id, "com.test.editor");
        assert_eq!(score(&app, "text edit"), 100);
        assert!(score(&app, "com.test") > 0);
        assert_eq!(score(&app, "missing"), 0);
        assert_eq!(score(&app, ""), 1);
        assert!(
            bundle_entry(Path::new("X.app"), &plist::Value::String("invalid".into())).is_none()
        );
        assert_eq!(
            bundle_entry(
                Path::new("Записки.app"),
                &plist::Value::Dictionary(Default::default())
            )
            .unwrap()
            .name,
            "Записки"
        );
    }
    #[test]
    fn empty_index_and_zero_limit_are_valid() {
        let index = AppIndex {
            cache: Mutex::new(Cache {
                updated: Some(Instant::now()),
                entries: vec![],
            }),
        };
        assert!(index.all().unwrap().is_empty());
        assert!(index.search("", 0).unwrap().is_empty());
        assert!(index.by_id("missing").unwrap().is_none());
    }
}
