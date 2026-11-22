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
    refresh: Mutex<()>,
}
#[derive(Default)]
struct Cache {
    updated: Option<Instant>,
    entries: Vec<AppEntry>,
    aliases: BTreeMap<String, Vec<String>>,
}

fn localized_names(info: &plist::Value) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(dict) = info.as_dictionary() {
        for (key, value) in dict {
            if matches!(key.as_str(), "CFBundleDisplayName" | "CFBundleName") {
                if let Some(name) = value.as_string().filter(|s| !s.trim().is_empty()) {
                    names.push(name.to_owned());
                }
            } else if value.as_dictionary().is_some() {
                names.extend(localized_names(value));
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

fn parse_strings(bytes: &[u8]) -> Option<plist::Value> {
    if let Ok(value) = plist::Value::from_reader(std::io::Cursor::new(bytes)) { return Some(value); }
    let text = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes[0] == 0xff;
        if bytes.len() % 2 != 0 { return None; }
        let words: Vec<_> = bytes[2..].chunks_exact(2).map(|p| {
            if little { u16::from_le_bytes([p[0], p[1]]) } else { u16::from_be_bytes([p[0], p[1]]) }
        }).collect();
        String::from_utf16(&words).ok()?
    } else { std::str::from_utf8(bytes).ok()?.trim_start_matches('\u{feff}').to_owned() };
    plist::Value::from_reader_ascii(std::io::Cursor::new(format!("{{{text}}}"))).ok()
}

fn locale_variants(locale: &str) -> Vec<String> {
    let locale = locale.split(['.', '@']).next().unwrap_or("").replace('_', "-");
    if locale.is_empty() || !locale.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return vec![];
    }
    let mut names = vec![locale.clone(), locale.replace('-', "_")];
    if let Some((base, _)) = locale.split_once('-') { names.push(base.into()); }
    names.sort(); names.dedup(); names
}

fn localization_names() -> Vec<String> {
    let mut names = vec!["Russian".into(), "English".into(), "Base".into()];
    for locale in ["ru-RU", "en-US"] { names.extend(locale_variants(locale)); }
    for key in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            for locale in value.split(':') {
                names.extend(locale_variants(locale));
            }
        }
    }
    names.sort(); names.dedup(); names
}

fn bundle_aliases(path: &Path, locales: &[String]) -> Vec<String> {
    let resources = path.join("Contents/Resources");
    let mut names = Vec::new();
    if let Ok(table) = plist::Value::from_file(resources.join("InfoPlist.loctable")) {
        if let Some(table) = table.as_dictionary() {
            for locale in locales {
                if let Some(info) = table.get(locale) { names.extend(localized_names(info)); }
            }
        }
    }
    let files = locales.iter().map(|locale| resources.join(format!("{locale}.lproj/InfoPlist.strings")));
    for file in files.into_iter().filter(|p| p.is_file()) {
        let info = std::fs::read(&file).ok().and_then(|data| parse_strings(&data));
        if let Some(info) = info { names.extend(localized_names(&info)); }
    }
    names.sort();
    names.dedup();
    names
}

fn alias_score(app: &AppEntry, aliases: &[String], query: &str) -> u32 {
    aliases.iter().fold(score(app, query), |best, name| {
        let alias = AppEntry { name: name.clone(), ..app.clone() };
        best.max(score(&alias, query))
    })
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
        let initialized = {
            let cache = self.cache.lock();
            if cache.updated.is_some_and(|t| t.elapsed() < max_age) { return Ok(()); }
            cache.updated.is_some()
        };
        let _refresh = if initialized {
            let Some(guard) = self.refresh.try_lock() else { return Ok(()); };
            guard
        } else { self.refresh.lock() };
        if self.cache.lock().updated.is_some_and(|t| t.elapsed() < max_age) { return Ok(()); }
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
        let mut cache = Cache { entries: entries.into_values().collect(), ..Default::default() };
        cache.entries.sort_by_key(|a| a.name.to_lowercase());
        let locales = localization_names();
        cache.aliases = cache.entries.iter().map(|app| {
            (app.id.clone(), app.exe.as_deref().map(|p| bundle_aliases(Path::new(p), &locales)).unwrap_or_default())
        }).collect();
        cache.updated = Some(Instant::now());
        *self.cache.lock() = cache;
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
        let cache = self.cache.lock();
        let mut found: Vec<_> = cache.entries
            .iter()
            .filter_map(|a| {
                let score = alias_score(a, cache.aliases.get(&a.id).map(Vec::as_slice).unwrap_or(&[]), query);
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
    fn localized_regions_try_hyphens_underscores_and_language_fallbacks() {
        let expected = vec!["ru".to_string(), "ru-RU".to_string(), "ru_RU".to_string()];
        assert_eq!(locale_variants("ru-RU"), expected);
        assert_eq!(locale_variants("ru_RU.UTF-8"), expected);
        assert_eq!(locale_variants("en"), ["en"]);
        assert!(locale_variants("").is_empty());
        assert!(locale_variants("../ru").is_empty());
        let locales = localization_names();
        for name in ["ru", "ru-RU", "ru_RU", "en", "English", "Base"] {
            assert!(locales.iter().any(|candidate| candidate == name));
        }
    }
    #[test]
    fn parses_binary_and_utf16_and_text_strings_without_a_subprocess() {
        let text = r#"/* label */ "CFBundleDisplayName" = "Калькулятор";"#;
        assert_eq!(localized_names(&parse_strings(text.as_bytes()).unwrap()), ["Калькулятор"]);
        for little in [true, false] {
            let mut bytes = if little { vec![0xff, 0xfe] } else { vec![0xfe, 0xff] };
            for word in text.encode_utf16() { bytes.extend(if little { word.to_le_bytes() } else { word.to_be_bytes() }); }
            assert_eq!(localized_names(&parse_strings(&bytes).unwrap()), ["Калькулятор"]);
        }
        let value = parse_strings(text.as_bytes()).unwrap();
        let mut binary = Vec::new(); value.to_writer_binary(&mut binary).unwrap();
        assert_eq!(localized_names(&parse_strings(&binary).unwrap()), ["Калькулятор"]);
        assert!(parse_strings(b"invalid strings").is_none());
        assert!(parse_strings(&[0xff, 0xfe, 0]).is_none());
        assert!(localized_names(&parse_strings(b"").unwrap()).is_empty());
    }
    #[test]
    fn searches_stale_cache_while_a_refresh_is_running() {
        let index = AppIndex::default();
        index.cache.lock().updated = Some(Instant::now() - Duration::from_secs(1000));
        let _refresh = index.refresh.lock();
        assert!(index.search("", 10).unwrap().is_empty());
        assert!(index.cache.try_lock().is_some());
    }
    #[test]
    fn localized_bundle_names_match_without_replacing_english_names() {
        let info = plist::Value::from_reader_xml(std::io::Cursor::new(r#"<plist version="1.0"><dict><key>ru</key><dict><key>CFBundleDisplayName</key><string>Калькулятор</string><key>CFBundleName</key><string>Калькулятор</string></dict><key>en</key><dict><key>CFBundleName</key><string>Calculator</string></dict></dict></plist>"#)).unwrap();
        let aliases = localized_names(&info);
        assert_eq!(aliases.len(), 2);
        let app = AppEntry { name:"Calculator".into(),id:"com.apple.calculator".into(),exe:None,kind:"desktop" };
        assert_eq!(alias_score(&app, &aliases, "калькулятор"), 100);
        assert_eq!(alias_score(&app, &aliases, "calculator"), 100);
        assert_eq!(alias_score(&app, &aliases, "Кальк"), 80);
        assert_eq!(alias_score(&app, &aliases, "missing"), 0);
        assert!(localized_names(&plist::Value::Dictionary(Default::default())).is_empty());
        assert!(localized_names(&plist::Value::String("invalid".into())).is_empty());
    }
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
                aliases: Default::default(),
            }),
            refresh: Default::default(),
        };
        assert!(index.all().unwrap().is_empty());
        assert!(index.search("", 0).unwrap().is_empty());
        assert!(index.by_id("missing").unwrap().is_none());
    }
}
