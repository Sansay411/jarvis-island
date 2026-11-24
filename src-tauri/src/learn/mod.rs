//! Learning from the user, so JARVIS gets better the longer it works for
//! one person. Two sources:
//! * lessons: what the model saves with the `learn` tool, what the agent
//!   infers by itself (a target that failed, then worked under another
//!   name) and what the daily review ([`reflect`]) distils from the
//!   conversations: how the recogniser mishears this user, the user's own
//!   words for apps, rooms and devices, preferences, what humour lands;
//! * the usage log ([`habits`]): what the user does at which hour, so a bare
//!   «включи свет» means the usual light at this time of day.
//!
//! Lessons live in SQLite (`lessons`); a copy in memory serves the voice
//! thread (hearing fixes) and the agent (alias rewrites) without a query.

pub mod habits;
pub mod reflect;

use std::cmp::Reverse;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;

use chrono::Utc;
use parking_lot::RwLock;
use rusqlite::params;
use serde::Serialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::store::Store;

/// Emitted when lessons change (Settings → «Память» reloads the list).
pub const EVT: &str = "learn:changed";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// How the user wants things done or said.
    Preference,
    /// The recogniser's word → what the user said.
    Hearing,
    /// The user's word → the exact name of an app, site, room or device.
    Alias,
    /// What makes this user laugh and what doesn't.
    Humor,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s.trim() {
            "preference" => Kind::Preference,
            "hearing" => Kind::Hearing,
            "alias" => Kind::Alias,
            "humor" | "humour" => Kind::Humor,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Preference => "preference",
            Kind::Hearing => "hearing",
            Kind::Alias => "alias",
            Kind::Humor => "humor",
        }
    }

    /// The most kept per kind: the least used and oldest go first.
    fn cap(self) -> usize {
        match self {
            Kind::Preference => 80,
            Kind::Hearing => 200,
            Kind::Alias => 300,
            Kind::Humor => 40,
        }
    }

    fn keyed(self) -> bool {
        matches!(self, Kind::Hearing | Kind::Alias)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Lesson {
    pub id: i64,
    pub kind: String,
    /// Hearing: what the recogniser wrote; alias: the user's word.
    pub key: String,
    /// Hearing: what was said; alias: the exact target; else the lesson.
    pub value: String,
    /// Alias: where the name works (`app_launch.query`, `mcp__ha__*.name`);
    /// empty for aliases the model saved for itself.
    pub scope: String,
    /// What the target is («свет в спальне», «приложение»).
    pub note: String,
    /// model | auto | reflect | user
    pub source: String,
    pub hits: i64,
    pub created_at: String,
    pub updated_at: String,
}

static CACHE: LazyLock<RwLock<Vec<Lesson>>> = LazyLock::new(Default::default);
/// Settings → «Память» → «Учиться»: off, lessons are kept but not used.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Follow the setting (`agent.self_learning`).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Lower case, «ё» as «е», words joined by single spaces.
pub fn norm(s: &str) -> String {
    words_of(s).join(" ")
}

fn norm_word(w: &str) -> String {
    w.to_lowercase().replace('ё', "е")
}

fn words_of(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(norm_word).collect()
}

pub fn list(store: &Store) -> AppResult<Vec<Lesson>> {
    store.with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT id, kind, key, value, scope, note, source, hits, created_at, updated_at FROM lessons ORDER BY updated_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Lesson {
                id: r.get(0)?,
                kind: r.get(1)?,
                key: r.get(2)?,
                value: r.get(3)?,
                scope: r.get(4)?,
                note: r.get(5)?,
                source: r.get(6)?,
                hits: r.get(7)?,
                created_at: r.get(8)?,
                updated_at: r.get(9)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    })
}

/// Refresh the in-memory copy after the table changed.
pub fn reload(store: &Store) {
    match list(store) {
        Ok(l) => *CACHE.write() = l,
        Err(e) => tracing::warn!("lessons not loaded: {e}"),
    }
}

pub fn cached() -> Vec<Lesson> {
    CACHE.read().clone()
}

/// Words a hearing fix may never rewrite: they are everywhere.
const COMMON: &[&str] = &[
    "и", "в", "во", "на", "не", "да", "нет", "что", "это", "как", "так", "вот", "ну", "а", "но", "по", "за", "до", "из", "с", "со", "у",
    "к", "о", "об", "от", "я", "ты", "вы", "он", "она", "мы", "они", "мне", "его", "ее", "там", "тут", "уже", "еще", "все",
];

pub struct Saved {
    pub id: i64,
    /// An existing lesson was updated (or was already the same).
    pub existing: bool,
}

/// Save a lesson; the same one again updates it instead of adding a copy.
pub fn save(store: &Store, kind: Kind, key: &str, value: &str, scope: &str, note: &str, source: &str) -> AppResult<Saved> {
    let (key, value, note) = (key.trim(), value.trim(), note.trim());
    if value.is_empty() {
        return Err(AppError::invalid("Пустой урок"));
    }
    let key = if kind.keyed() { key } else { "" };
    if kind.keyed() {
        if key.is_empty() {
            return Err(AppError::invalid("Нужно, что услышано (heard) и что имелось в виду (meant)"));
        }
        if key.chars().count() > 60 || value.chars().count() > 120 {
            return Err(AppError::invalid("Слишком длинно: урок — это слово или короткая фраза"));
        }
        if norm(key) == norm(value) {
            return Err(AppError::invalid("Услышанное и имевшееся в виду совпадают"));
        }
    } else if value.chars().count() > 300 {
        return Err(AppError::invalid("Урок длиннее 300 символов — сформулируй короче"));
    }
    if kind == Kind::Hearing {
        let heard = words_of(key);
        let letters: usize = heard.iter().map(|w| w.chars().count()).sum();
        if letters < 3 || heard.iter().all(|w| COMMON.contains(&w.as_str())) {
            return Err(AppError::invalid("Такое слово слишком частое, чтобы всегда его исправлять"));
        }
    }
    let now = Utc::now().to_rfc3339();
    let lessons = list(store)?;
    let same = lessons.iter().find(|l| {
        l.kind == kind.as_str()
            && if kind.keyed() { norm(&l.key) == norm(key) && l.scope == scope } else { norm(&l.value) == norm(value) }
    });
    let saved = store.with_conn(|c| {
        if let Some(l) = same {
            // A keyed lesson takes the new meaning; a rule said again keeps its first wording.
            let value = if kind.keyed() { value } else { l.value.as_str() };
            c.execute(
                "UPDATE lessons SET value = ?2, note = CASE WHEN ?3 = '' THEN note ELSE ?3 END, updated_at = ?4 WHERE id = ?1",
                params![l.id, value, note, now],
            )?;
            return Ok(Saved { id: l.id, existing: true });
        }
        c.execute(
            "INSERT INTO lessons(kind, key, value, scope, note, source, created_at, updated_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![kind.as_str(), key, value, scope, note, source, now],
        )?;
        let id = c.last_insert_rowid();
        c.execute(
            "DELETE FROM lessons WHERE id IN (SELECT id FROM lessons WHERE kind = ?1 ORDER BY hits DESC, updated_at DESC LIMIT -1 OFFSET ?2)",
            params![kind.as_str(), kind.cap() as i64],
        )?;
        Ok(Saved { id, existing: false })
    })?;
    reload(store);
    Ok(saved)
}

pub fn forget(store: &Store, id: i64) -> AppResult<bool> {
    let n = store.with_conn(|c| Ok(c.execute("DELETE FROM lessons WHERE id = ?1", params![id])?))?;
    reload(store);
    Ok(n > 0)
}

/// A lesson was used: the most used survive the cap.
pub fn bump(store: &Store, id: i64) {
    let _ = store.with_conn(|c| Ok(c.execute("UPDATE lessons SET hits = hits + 1 WHERE id = ?1", params![id])?));
    if let Some(l) = CACHE.write().iter_mut().find(|l| l.id == id) {
        l.hits += 1;
    }
}

// ---------- hearing ----------

/// The recognised text with the user's known mishearings fixed.
pub fn fix_hearing(text: &str) -> String {
    if !ENABLED.load(Ordering::Relaxed) {
        return text.to_string();
    }
    let fixed = fix_with(&CACHE.read(), text);
    if fixed != text {
        tracing::info!("voice: learned hearing fix applied");
    }
    fixed
}

fn fix_with(lessons: &[Lesson], text: &str) -> String {
    let fixes: Vec<(String, String)> = lessons.iter().filter(|l| l.kind == "hearing").map(|l| (l.key.clone(), l.value.clone())).collect();
    if fixes.is_empty() {
        return text.to_string();
    }
    apply_hearing(text, &fixes)
}

/// Word spans: (start, end, normalised word).
fn spans(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            out.push((s, i, norm_word(&text[s..i])));
        }
    }
    if let Some(s) = start {
        out.push((s, text.len(), norm_word(&text[s..])));
    }
    out
}

/// «Кровайте» at the start of a sentence stays capitalised as «Закрой все».
fn match_case(original: &str, meant: &str) -> String {
    let upper = original.chars().next().is_some_and(char::is_uppercase);
    let mut chars = meant.chars();
    match chars.next() {
        Some(first) if upper && first.is_lowercase() => first.to_uppercase().chain(chars).collect(),
        _ => meant.to_string(),
    }
}

/// Replace whole words and phrases (longest first); everything else in the
/// text, punctuation included, stays as it was.
pub(crate) fn apply_hearing(text: &str, fixes: &[(String, String)]) -> String {
    let mut fixes: Vec<(Vec<String>, &str)> =
        fixes.iter().map(|(h, m)| (words_of(h), m.as_str())).filter(|(h, m)| !h.is_empty() && !m.trim().is_empty()).collect();
    fixes.sort_by_key(|(h, _)| Reverse(h.len()));
    let mut text = text.to_string();
    for (heard, meant) in fixes {
        let sp = spans(&text);
        let n = heard.len();
        let mut out = String::with_capacity(text.len());
        let (mut last, mut i, mut changed) = (0, 0, false);
        while i + n <= sp.len() {
            if sp[i..i + n].iter().zip(&heard).all(|(s, h)| &s.2 == h) {
                let (s, e) = (sp[i].0, sp[i + n - 1].1);
                out.push_str(&text[last..s]);
                out.push_str(&match_case(&text[s..e], meant.trim()));
                last = e;
                i += n;
                changed = true;
            } else {
                i += 1;
            }
        }
        if changed {
            out.push_str(&text[last..]);
            text = out;
        }
    }
    text
}

// ---------- aliases ----------

/// Arguments that name a target: a learned alias may stand in for them.
pub const TARGET_KEYS: &[&str] = &["name", "query", "target", "area", "app", "title", "device", "room", "entity_id", "project"];

/// Where an alias works: one built-in tool's argument, or one argument of
/// every tool of an MCP server (Home Assistant takes the same `name` in
/// HassTurnOn, HassTurnOff, HassLightSet…).
pub fn alias_scope(tool: &str, key: &str) -> String {
    if let Some(rest) = tool.strip_prefix(crate::mcp::PREFIX) {
        if let Some((server, _)) = rest.split_once("__") {
            return format!("{}{server}__*.{key}", crate::mcp::PREFIX);
        }
    }
    format!("{tool}.{key}")
}

/// The arguments with learned aliases put in, and the lessons used.
pub fn rewrite_alias(tool: &str, args: &Value) -> Option<(Value, Vec<i64>)> {
    rewrite_with(&CACHE.read(), tool, args)
}

pub(crate) fn rewrite_with(lessons: &[Lesson], tool: &str, args: &Value) -> Option<(Value, Vec<i64>)> {
    let obj = args.as_object()?;
    let mut out = obj.clone();
    let mut used = Vec::new();
    for key in TARGET_KEYS {
        let Some(said) = obj.get(*key).and_then(Value::as_str) else { continue };
        let scope = alias_scope(tool, key);
        let said = norm(said);
        if let Some(l) = lessons.iter().find(|l| l.kind == "alias" && l.scope == scope && norm(&l.key) == said) {
            out.insert((*key).to_string(), Value::String(l.value.clone()));
            used.push(l.id);
        }
    }
    (!used.is_empty()).then(|| (Value::Object(out), used))
}

/// The user's request names it already («…и на кухне»): a success with this
/// value answers another part of the request, not the failed call.
fn mentioned(request: &str, value: &str) -> bool {
    let said = words_of(request);
    words_of(value).iter().filter(|w| w.chars().count() >= 4).any(|w| {
        let stem: String = w.chars().take(4).collect();
        said.iter().any(|s| s.starts_with(&stem))
    })
}

/// A call that failed and the same call that then worked with one target
/// argument changed: the failed value is the user's word for the working
/// one («ночник» → «Bedside lamp»). Returns (scope, from, to).
pub fn infer_alias(failed: &[(String, Value)], tool: &str, ok_args: &Value, request: &str) -> Option<(String, String, String)> {
    let ok = ok_args.as_object()?;
    for (ftool, fargs) in failed.iter().rev() {
        if ftool != tool {
            continue;
        }
        let Some(f) = fargs.as_object() else { continue };
        let mut differing = Vec::new();
        let mut compatible = true;
        for (k, fv) in f {
            match (ok.get(k), fv) {
                (Some(ov), _) if ov == fv => {}
                (Some(Value::String(o)), Value::String(from)) if TARGET_KEYS.contains(&k.as_str()) => {
                    differing.push((k.clone(), from.trim(), o.trim()))
                }
                _ => {
                    compatible = false;
                    break;
                }
            }
        }
        if !compatible || differing.len() != 1 {
            continue;
        }
        let (key, from, to) = &differing[0];
        if from.is_empty() || to.is_empty() || from.chars().count() > 60 || to.chars().count() > 120 || norm(from) == norm(to) {
            continue;
        }
        if mentioned(request, to) {
            continue;
        }
        return Some((alias_scope(tool, key), from.to_string(), to.to_string()));
    }
    None
}

// ---------- prompt ----------

fn scope_label(scope: &str, note: &str) -> String {
    if !note.is_empty() {
        return note.to_string();
    }
    let key = scope.rsplit('.').next().unwrap_or("");
    if scope.starts_with(crate::mcp::PREFIX) {
        return match key {
            "area" | "room" => "комната".into(),
            _ => "устройство или сервис".into(),
        };
    }
    match scope.split('.').next().unwrap_or("") {
        "app_launch" | "apps_find" => "приложение".into(),
        "open_target" => "сайт или файл".into(),
        "vscode_open" | "projects_list" => "проект".into(),
        _ => String::new(),
    }
}

/// Lessons for the system prompt, most used and newest first, within a budget.
pub fn prompt_block(lessons: &[Lesson]) -> String {
    if lessons.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<&Lesson> = lessons.iter().collect();
    sorted.sort_by(|a, b| b.hits.cmp(&a.hits).then_with(|| b.updated_at.cmp(&a.updated_at)));
    let pick = |kind: &str, max: usize| -> Vec<&Lesson> { sorted.iter().copied().filter(|l| l.kind == kind).take(max).collect() };
    let mut out = String::from(
        "\n\nWhat you have learned about working with this user (apply it naturally, never announce it; if a lesson turns out wrong, `learn` forget its id):",
    );
    let plain = |l: &Lesson| format!("[{}] {}", l.id, l.value);
    let prefs: Vec<String> = pick("preference", 40).into_iter().map(plain).collect();
    if !prefs.is_empty() {
        out.push_str(&format!("\n- Preferences: {}", prefs.join("; ")));
    }
    let humor: Vec<String> = pick("humor", 15).into_iter().map(plain).collect();
    if !humor.is_empty() {
        out.push_str(&format!("\n- Humour: {}", humor.join("; ")));
    }
    let aliases: Vec<String> = pick("alias", 60)
        .into_iter()
        .map(|l| {
            let what = scope_label(&l.scope, &l.note);
            if what.is_empty() {
                format!("[{}] «{}» → «{}»", l.id, l.key, l.value)
            } else {
                format!("[{}] «{}» → «{}» ({what})", l.id, l.key, l.value)
            }
        })
        .collect();
    if !aliases.is_empty() {
        out.push_str(&format!("\n- The user's own words for things: {}", aliases.join("; ")));
    }
    let hearing: Vec<String> = pick("hearing", 30).into_iter().map(|l| format!("[{}] «{}» → «{}»", l.id, l.key, l.value)).collect();
    if !hearing.is_empty() {
        out.push_str(&format!("\n- Speech recognition mishears this user (already fixed in the text you get): {}", hearing.join("; ")));
    }
    if out.len() > 6000 {
        let mut cut = 6000;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lesson(id: i64, kind: &str, key: &str, value: &str, scope: &str) -> Lesson {
        Lesson {
            id,
            kind: kind.into(),
            key: key.into(),
            value: value.into(),
            scope: scope.into(),
            note: String::new(),
            source: "auto".into(),
            hits: 0,
            created_at: "2026-11-24T10:00:00Z".into(),
            updated_at: "2026-11-24T10:00:00Z".into(),
        }
    }

    #[test]
    fn hearing_fixes_replace_whole_words_and_keep_the_rest() {
        let fixes = vec![
            ("кровайте окна".to_string(), "закрой все окна".to_string()),
            ("ночнек".to_string(), "ночник".to_string()),
            ("джанкис".to_string(), "Джарвис".to_string()),
        ];
        assert_eq!(apply_hearing("Кровайте окна, пожалуйста.", &fixes), "Закрой все окна, пожалуйста.");
        assert_eq!(apply_hearing("Джанкис, включи ночнек!", &fixes), "Джарвис, включи ночник!");
        // Parts of other words stay untouched.
        assert_eq!(apply_hearing("ночнеками", &fixes), "ночнеками");
        assert_eq!(apply_hearing("", &fixes), "");
        // «ё» and case do not matter for the match.
        let yo = vec![("свёт".to_string(), "свет".to_string())];
        assert_eq!(apply_hearing("СВЕТ свёт", &yo), "Свет свет");
    }

    #[test]
    fn aliases_rewrite_only_their_own_tool_or_server() {
        let lessons = vec![
            lesson(1, "alias", "ночник", "Bedside lamp", "mcp__home__*.name"),
            lesson(2, "alias", "телега", "Telegram", "app_launch.query"),
            lesson(3, "alias", "зал", "Гостиная", "mcp__home__*.area"),
        ];
        let (args, used) = rewrite_with(&lessons, "mcp__home__intent__HassTurnOff", &json!({"name": "Ночник", "domain": ["light"]})).unwrap();
        assert_eq!(args, json!({"name": "Bedside lamp", "domain": ["light"]}));
        assert_eq!(used, vec![1]);
        let (args, _) = rewrite_with(&lessons, "mcp__home__intent__HassTurnOn", &json!({"area": "зал", "domain": ["light"]})).unwrap();
        assert_eq!(args["area"], "Гостиная");
        assert_eq!(rewrite_with(&lessons, "app_launch", &json!({"query": "Телега"})).unwrap().0, json!({"query": "Telegram"}));
        assert!(rewrite_with(&lessons, "apps_find", &json!({"query": "телега"})).is_none());
        assert!(rewrite_with(&lessons, "mcp__other__HassTurnOn", &json!({"name": "ночник"})).is_none());
    }

    #[test]
    fn a_retry_that_worked_teaches_an_alias_unless_it_answers_another_part() {
        let tool = "mcp__home__intent__HassTurnOn";
        let failed = vec![(tool.to_string(), json!({"name": "ночник"}))];
        let got = infer_alias(&failed, tool, &json!({"name": "Bedside lamp", "domain": ["light"]}), "включи ночник");
        assert_eq!(got, Some(("mcp__home__*.name".into(), "ночник".into(), "Bedside lamp".into())));
        // «…в зале и на кухне»: the kitchen light is another part of the request.
        let failed = vec![(tool.to_string(), json!({"name": "свет в зале"}))];
        assert_eq!(infer_alias(&failed, tool, &json!({"name": "Свет на кухне"}), "включи свет в зале и на кухне"), None);
        // Another tool, or two arguments changed: nothing learned.
        assert_eq!(infer_alias(&failed, "mcp__home__intent__HassTurnOff", &json!({"name": "Люстра"}), "x"), None);
        let failed = vec![("app_launch".to_string(), json!({"query": "телега", "x": 1}))];
        assert_eq!(infer_alias(&failed, "app_launch", &json!({"query": "Telegram", "x": 2}), "открой телегу"), None);
        let failed = vec![("app_launch".to_string(), json!({"query": "телега"}))];
        assert_eq!(
            infer_alias(&failed, "app_launch", &json!({"query": "Telegram"}), "открой телегу"),
            Some(("app_launch.query".into(), "телега".into(), "Telegram".into()))
        );
    }

    #[test]
    fn lessons_are_saved_once_validated_and_used_for_hearing() {
        let store = Store::in_memory().unwrap();
        let a = save(&store, Kind::Hearing, "ночнек", "ночник", "", "", "model").unwrap();
        assert!(!a.existing);
        let b = save(&store, Kind::Hearing, "Ночнек", "ночник в спальне", "", "", "model").unwrap();
        assert!(b.existing);
        assert_eq!(a.id, b.id);
        assert!(save(&store, Kind::Hearing, "и", "в", "", "", "model").is_err());
        assert!(save(&store, Kind::Hearing, "свет", "Свет", "", "", "model").is_err());
        assert!(save(&store, Kind::Preference, "", "", "", "", "model").is_err());
        save(&store, Kind::Preference, "", "Любит тёплый свет вечером", "", "", "model").unwrap();
        assert!(save(&store, Kind::Preference, "", "любит тёплый свет вечером!", "", "", "reflect").unwrap().existing);
        assert_eq!(list(&store).unwrap().len(), 2);
        // Other tests reload the shared copy with their own stores: read this one.
        let fix = |text: &str| fix_with(&list(&store).unwrap(), text);
        assert_eq!(fix("включи ночнек"), "включи ночник в спальне");
        assert!(forget(&store, a.id).unwrap());
        assert_eq!(fix("включи ночнек"), "включи ночнек");
        assert!(prompt_block(&list(&store).unwrap()).contains("Любит тёплый свет вечером"));
    }

    #[test]
    fn prompt_block_names_what_an_alias_is() {
        let block = prompt_block(&[lesson(7, "alias", "телега", "Telegram", "app_launch.query"), lesson(8, "alias", "зал", "Гостиная", "mcp__ha__*.area")]);
        assert!(block.contains("[7] «телега» → «Telegram» (приложение)"));
        assert!(block.contains("[8] «зал» → «Гостиная» (комната)"));
        assert_eq!(prompt_block(&[]), "");
    }
}
