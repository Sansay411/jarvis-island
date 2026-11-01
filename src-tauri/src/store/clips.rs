//! Clipboard history: text, pictures and file lists copied on this PC.
//!
//! Everything stays in the local SQLite file. Identical content is stored
//! once (a repeated copy moves it to the top), pinned items never expire and
//! do not count against the limits, and pictures live under a byte budget so
//! a screenshot habit cannot fill the disk.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::Store;
use crate::error::{AppError, AppResult};

/// Characters of a text shown in the list (the full text stays in `body`).
const PREVIEW_CHARS: usize = 240;
/// Characters of a text that search looks through.
const SEARCH_CHARS: usize = 20_000;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS clips (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT NOT NULL,
    hash       TEXT NOT NULL UNIQUE,
    preview    TEXT NOT NULL,
    body       TEXT,
    image      BLOB,
    thumb      TEXT,
    width      INTEGER,
    height     INTEGER,
    size       INTEGER NOT NULL DEFAULT 0,
    source     TEXT NOT NULL DEFAULT '',
    search     TEXT NOT NULL DEFAULT '',
    pinned     INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    used_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS clips_order ON clips(pinned DESC, used_at DESC);
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipKind {
    Text,
    Image,
    Files,
}

/// What the watcher hands over for storage.
#[derive(Debug, Clone)]
pub struct NewClip {
    pub kind: ClipKind,
    pub hash: String,
    pub preview: String,
    pub body: Option<String>,
    pub image: Option<Vec<u8>>,
    pub thumb: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size: u32,
    pub source: String,
    pub search: String,
}

impl ClipKind {
    fn as_str(self) -> &'static str {
        match self {
            ClipKind::Text => "text",
            ClipKind::Image => "image",
            ClipKind::Files => "files",
        }
    }

    fn parse(s: &str) -> ClipKind {
        match s {
            "image" => ClipKind::Image,
            "files" => ClipKind::Files,
            _ => ClipKind::Text,
        }
    }
}

fn digest(kind: &str, data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(data);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The text on one line, cut to what the list can show.
fn one_line(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= PREVIEW_CHARS {
        return collapsed;
    }
    let mut cut: String = collapsed.chars().take(PREVIEW_CHARS).collect();
    cut.push('…');
    cut
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Lower-cased text that search matches against.
fn searchable(parts: &[&str]) -> String {
    let joined = parts.join(" ").to_lowercase();
    joined.chars().take(SEARCH_CHARS + 64).collect()
}

impl NewClip {
    pub fn text(text: &str, source: &str) -> NewClip {
        let head: String = text.chars().take(SEARCH_CHARS).collect();
        NewClip {
            kind: ClipKind::Text,
            hash: digest("text", text.as_bytes()),
            preview: one_line(text),
            body: Some(text.to_string()),
            image: None,
            thumb: None,
            width: None,
            height: None,
            size: text.chars().count() as u32,
            source: source.to_string(),
            search: searchable(&[&head, source]),
        }
    }

    pub fn files(paths: &[String], source: &str) -> NewClip {
        let names: Vec<&str> = paths.iter().map(|p| file_name(p)).collect();
        let mut preview = names.iter().take(3).copied().collect::<Vec<_>>().join(", ");
        if names.len() > 3 {
            preview.push_str(&format!(" и ещё {}", names.len() - 3));
        }
        let body = paths.join("\n");
        NewClip {
            kind: ClipKind::Files,
            hash: digest("files", body.as_bytes()),
            preview,
            search: searchable(&[&body, source]),
            body: Some(body),
            image: None,
            thumb: None,
            width: None,
            height: None,
            size: paths.len() as u32,
            source: source.to_string(),
        }
    }

    /// `png` is the picture as stored; `pixel_hash` identifies its pixels, so
    /// the same picture copied from two programs is one entry.
    pub fn image(png: Vec<u8>, pixel_hash: &str, width: u32, height: u32, thumb: String, source: &str) -> NewClip {
        NewClip {
            kind: ClipKind::Image,
            hash: digest("image", pixel_hash.as_bytes()),
            preview: format!("Изображение {width}×{height}"),
            body: None,
            image: Some(png),
            thumb: Some(thumb),
            width: Some(width),
            height: Some(height),
            size: 0,
            source: source.to_string(),
            search: searchable(&["изображение картинка скриншот", source]),
        }
    }
}

/// A history row as the list shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: i64,
    pub kind: ClipKind,
    pub preview: String,
    /// Characters of a text, files in a list, 0 for a picture.
    pub size: u32,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Small PNG data URL for pictures.
    pub thumb: Option<String>,
    pub source: String,
    pub pinned: bool,
    pub created_at: String,
    pub used_at: String,
}

/// A row with everything needed to put it back on the clipboard.
#[derive(Debug, Clone)]
pub struct ClipContent {
    pub clip: Clip,
    /// The full text, or the file paths one per line.
    pub body: Option<String>,
    pub image: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy)]
pub struct ClipLimits {
    /// Unpinned items kept.
    pub max_items: u32,
    /// Unpinned items older than this many days expire; 0 keeps them.
    pub keep_days: u32,
    /// Bytes of unpinned pictures kept.
    pub max_image_bytes: u64,
}

impl Store {
    pub fn clip_add(&self, new: &NewClip, limits: ClipLimits) -> AppResult<i64> {
        self.clip_add_at(new, limits, Utc::now())
    }

    /// Stores a copy, or moves the identical earlier one to the top (keeping
    /// its pin and first source), then applies the limits.
    pub(crate) fn clip_add_at(&self, new: &NewClip, limits: ClipLimits, now: DateTime<Utc>) -> AppResult<i64> {
        // Stored and pruned in one breath would hand back the id of nothing.
        if new.image.as_ref().is_some_and(|png| png.len() as u64 > limits.max_image_bytes) {
            return Err(AppError::invalid("Картинка больше лимита истории буфера"));
        }
        let conn = self.conn.lock();
        let at = stamp(now);
        let existing: Option<i64> = conn.query_row("SELECT id FROM clips WHERE hash = ?1", params![new.hash], |r| r.get(0)).optional()?;
        let id = match existing {
            Some(id) => {
                conn.execute("UPDATE clips SET used_at = ?2 WHERE id = ?1", params![id, at])?;
                id
            }
            None => {
                conn.execute(
                    "INSERT INTO clips(kind, hash, preview, body, image, thumb, width, height, size, source, search, created_at, used_at) \
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
                    params![new.kind.as_str(), new.hash, new.preview, new.body, new.image, new.thumb, new.width, new.height, new.size, new.source, new.search, at],
                )?;
                conn.last_insert_rowid()
            }
        };
        prune(&conn, limits, now)?;
        Ok(id)
    }

    /// Pinned first, then most recently used; `query` is a case-insensitive
    /// substring of the text, file names or source program.
    pub fn clips_list(&self, query: &str, limit: u32) -> AppResult<Vec<Clip>> {
        let pattern = like_pattern(query);
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM clips WHERE search LIKE ?1 ESCAPE '\\' ORDER BY pinned DESC, used_at DESC, id DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![pattern, limit], row_to_clip)?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn clip_get(&self, id: i64) -> AppResult<Option<ClipContent>> {
        let conn = self.conn.lock();
        let found = conn
            .query_row(&format!("SELECT {COLUMNS}, body, image FROM clips WHERE id = ?1"), params![id], |r| {
                Ok(ClipContent { clip: row_to_clip(r)?, body: r.get(11)?, image: r.get(12)? })
            })
            .optional()?;
        Ok(found)
    }

    pub fn clip_set_pinned(&self, id: i64, pinned: bool) -> AppResult<bool> {
        Ok(self.conn.lock().execute("UPDATE clips SET pinned = ?2 WHERE id = ?1", params![id, pinned])? == 1)
    }

    /// An item was used: it goes back to the top.
    pub fn clip_touch(&self, id: i64) -> AppResult<bool> {
        self.clip_touch_at(id, Utc::now())
    }

    pub(crate) fn clip_touch_at(&self, id: i64, now: DateTime<Utc>) -> AppResult<bool> {
        Ok(self.conn.lock().execute("UPDATE clips SET used_at = ?2 WHERE id = ?1", params![id, stamp(now)])? == 1)
    }

    pub fn clip_delete(&self, id: i64) -> AppResult<bool> {
        Ok(self.conn.lock().execute("DELETE FROM clips WHERE id = ?1", params![id])? == 1)
    }

    /// Returns how many items were removed.
    pub fn clips_clear(&self, keep_pinned: bool) -> AppResult<u32> {
        let sql = if keep_pinned { "DELETE FROM clips WHERE pinned = 0" } else { "DELETE FROM clips" };
        Ok(self.conn.lock().execute(sql, [])? as u32)
    }

    pub fn clips_prune(&self, limits: ClipLimits) -> AppResult<u32> {
        self.clips_prune_at(limits, Utc::now())
    }

    pub(crate) fn clips_prune_at(&self, limits: ClipLimits, now: DateTime<Utc>) -> AppResult<u32> {
        prune(&self.conn.lock(), limits, now)
    }
}

const COLUMNS: &str = "id, kind, preview, size, width, height, thumb, source, pinned, created_at, used_at";

fn row_to_clip(r: &Row) -> rusqlite::Result<Clip> {
    Ok(Clip {
        id: r.get(0)?,
        kind: ClipKind::parse(&r.get::<_, String>(1)?),
        preview: r.get(2)?,
        size: r.get(3)?,
        width: r.get(4)?,
        height: r.get(5)?,
        thumb: r.get(6)?,
        source: r.get(7)?,
        pinned: r.get::<_, i64>(8)? != 0,
        created_at: r.get(9)?,
        used_at: r.get(10)?,
    })
}

/// `%query%` for LIKE, with the query's own wildcards taken literally.
fn like_pattern(query: &str) -> String {
    let escaped = query.trim().to_lowercase().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

fn stamp(at: DateTime<Utc>) -> String {
    // Fixed width, so text order is time order.
    at.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Expired, over the item limit, over the picture budget: oldest go first;
/// pinned items are never touched.
fn prune(conn: &Connection, limits: ClipLimits, now: DateTime<Utc>) -> AppResult<u32> {
    let mut removed = 0;
    if limits.keep_days > 0 {
        let cutoff = stamp(now - Duration::days(i64::from(limits.keep_days)));
        removed += conn.execute("DELETE FROM clips WHERE pinned = 0 AND used_at < ?1", params![cutoff])?;
    }
    removed += conn.execute(
        "DELETE FROM clips WHERE pinned = 0 AND id NOT IN \
         (SELECT id FROM clips WHERE pinned = 0 ORDER BY used_at DESC, id DESC LIMIT ?1)",
        params![limits.max_items],
    )?;
    // Newest pictures are kept while they fit the budget; everything older
    // than the first one that does not fit goes.
    let mut over = Vec::new();
    {
        let mut stmt = conn.prepare("SELECT id, length(image) FROM clips WHERE pinned = 0 AND image IS NOT NULL ORDER BY used_at DESC, id DESC")?;
        let mut used = 0u64;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
            let (id, bytes) = row?;
            used += bytes.max(0) as u64;
            if used > limits.max_image_bytes || !over.is_empty() {
                over.push(id);
            }
        }
    }
    for id in over {
        removed += conn.execute("DELETE FROM clips WHERE id = ?1", params![id])?;
    }
    Ok(removed as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap()
    }
    fn min(n: i64) -> DateTime<Utc> {
        t0() + Duration::minutes(n)
    }
    fn limits() -> ClipLimits {
        ClipLimits { max_items: 100, keep_days: 7, max_image_bytes: 1_000_000 }
    }
    fn add(s: &Store, text: &str, at: DateTime<Utc>) -> i64 {
        s.clip_add_at(&NewClip::text(text, "chrome"), limits(), at).unwrap()
    }
    fn add_with(s: &Store, text: &str, at: DateTime<Utc>, lim: ClipLimits) -> i64 {
        s.clip_add_at(&NewClip::text(text, "chrome"), lim, at).unwrap()
    }
    fn picture(s: &Store, tag: &str, bytes: usize, at: DateTime<Utc>) -> i64 {
        let clip = NewClip::image(vec![7; bytes], tag, 10, 8, "data:image/png;base64,AA".into(), "Snip");
        s.clip_add_at(&clip, limits(), at).unwrap()
    }
    fn previews(s: &Store, query: &str) -> Vec<String> {
        s.clips_list(query, 50).unwrap().into_iter().map(|c| c.preview).collect()
    }

    #[test]
    fn lists_the_newest_copy_first() {
        let s = Store::in_memory().unwrap();
        add(&s, "a", min(0));
        add(&s, "b", min(1));
        add(&s, "c", min(2));
        assert_eq!(previews(&s, ""), ["c", "b", "a"]);
    }

    #[test]
    fn copying_the_same_text_again_moves_it_to_the_top_without_a_duplicate() {
        let s = Store::in_memory().unwrap();
        let first = add(&s, "a", min(0));
        add(&s, "b", min(1));
        let again = add(&s, "a", min(2));
        assert_eq!(again, first);
        assert_eq!(previews(&s, ""), ["a", "b"]);
    }

    #[test]
    fn a_repeated_copy_keeps_the_pin_and_the_first_source() {
        let s = Store::in_memory().unwrap();
        let id = add(&s, "a", min(0));
        s.clip_set_pinned(id, true).unwrap();
        s.clip_add_at(&NewClip::text("a", "Code"), limits(), min(1)).unwrap();
        let clip = &s.clips_list("", 10).unwrap()[0];
        assert!(clip.pinned);
        assert_eq!(clip.source, "chrome");
    }

    #[test]
    fn pinned_items_stay_on_top_and_survive_the_item_limit() {
        let s = Store::in_memory().unwrap();
        let lim = ClipLimits { max_items: 2, ..limits() };
        let a = add_with(&s, "a", min(0), lim);
        s.clip_set_pinned(a, true).unwrap();
        add_with(&s, "b", min(1), lim);
        add_with(&s, "c", min(2), lim);
        add_with(&s, "d", min(3), lim);
        assert_eq!(previews(&s, ""), ["a", "d", "c"]);
    }

    #[test]
    fn old_items_expire_but_pinned_ones_do_not() {
        let s = Store::in_memory().unwrap();
        let old = add(&s, "old", min(0));
        s.clip_set_pinned(old, true).unwrap();
        add(&s, "stale", min(0));
        add(&s, "fresh", t0() + Duration::days(8));
        assert_eq!(previews(&s, ""), ["old", "fresh"]);
    }

    #[test]
    fn zero_retention_days_keeps_everything() {
        let s = Store::in_memory().unwrap();
        let lim = ClipLimits { keep_days: 0, ..limits() };
        add_with(&s, "x", min(0), lim);
        add_with(&s, "y", t0() + Duration::days(400), lim);
        assert_eq!(previews(&s, ""), ["y", "x"]);
    }

    #[test]
    fn search_ignores_case_in_cyrillic() {
        let s = Store::in_memory().unwrap();
        add(&s, "Привет, Мир", min(0));
        add(&s, "other", min(1));
        assert_eq!(previews(&s, "привет"), ["Привет, Мир"]);
        assert_eq!(previews(&s, "МИР"), ["Привет, Мир"]);
    }

    #[test]
    fn search_treats_percent_and_underscore_literally() {
        let s = Store::in_memory().unwrap();
        add(&s, "100% готово", min(0));
        add(&s, "abc", min(1));
        assert_eq!(previews(&s, "%"), ["100% готово"]);
        assert!(previews(&s, "_").is_empty());
    }

    #[test]
    fn search_matches_file_names_and_the_source_program() {
        let s = Store::in_memory().unwrap();
        s.clip_add_at(&NewClip::files(&["C:\\docs\\Отчёт.xlsx".into(), "C:\\pics\\Фото.jpg".into()], "Explorer"), limits(), min(0)).unwrap();
        add(&s, "text", min(1));
        assert_eq!(s.clips_list("отчёт", 10).unwrap().len(), 1);
        assert_eq!(s.clips_list("explorer", 10).unwrap().len(), 1);
        assert_eq!(s.clips_list("chrome", 10).unwrap().len(), 1);
    }

    #[test]
    fn the_picture_budget_drops_the_oldest_unpinned_pictures_first() {
        let s = Store::in_memory().unwrap();
        let lim = ClipLimits { max_image_bytes: 1000, ..limits() };
        let clip = |tag: &str, at| s.clip_add_at(&NewClip::image(vec![1; 600], tag, 10, 8, "data:,".into(), "Snip"), lim, at).unwrap();
        let first = clip("a", min(0));
        let second = clip("b", min(1));
        assert!(s.clip_get(first).unwrap().is_none());
        assert!(s.clip_get(second).unwrap().is_some());
    }

    #[test]
    fn a_picture_larger_than_the_whole_budget_is_refused_instead_of_stored_and_dropped() {
        let s = Store::in_memory().unwrap();
        let lim = ClipLimits { max_image_bytes: 100, ..limits() };
        let big = NewClip::image(vec![1; 500], "big", 10, 8, "data:,".into(), "Snip");
        assert!(s.clip_add_at(&big, lim, min(0)).is_err());
        assert!(s.clips_list("", 10).unwrap().is_empty());
    }

    #[test]
    fn a_pinned_picture_is_outside_the_budget() {
        let s = Store::in_memory().unwrap();
        let lim = ClipLimits { max_image_bytes: 1000, ..limits() };
        let clip = |tag: &str, at| s.clip_add_at(&NewClip::image(vec![1; 600], tag, 10, 8, "data:,".into(), "Snip"), lim, at).unwrap();
        let keep = clip("a", min(0));
        s.clip_set_pinned(keep, true).unwrap();
        clip("b", min(1));
        clip("c", min(2));
        assert!(s.clip_get(keep).unwrap().is_some());
        assert_eq!(s.clips_list("", 10).unwrap().len(), 2);
    }

    #[test]
    fn clearing_can_keep_the_pinned_items() {
        let s = Store::in_memory().unwrap();
        let a = add(&s, "a", min(0));
        add(&s, "b", min(1));
        s.clip_set_pinned(a, true).unwrap();
        assert_eq!(s.clips_clear(true).unwrap(), 1);
        assert_eq!(previews(&s, ""), ["a"]);
        assert_eq!(s.clips_clear(false).unwrap(), 1);
        assert!(previews(&s, "").is_empty());
    }

    #[test]
    fn the_list_shows_a_short_one_line_preview_and_get_returns_the_full_text() {
        let s = Store::in_memory().unwrap();
        let long = format!("начало\n\n   {}", "я".repeat(1000));
        let id = add(&s, &long, min(0));
        let shown = &s.clips_list("", 10).unwrap()[0];
        assert!(shown.preview.starts_with("начало яя"));
        assert!(shown.preview.chars().count() <= 241);
        assert!(shown.preview.ends_with('…'));
        assert_eq!(shown.size, long.chars().count() as u32);
        assert_eq!(s.clip_get(id).unwrap().unwrap().body.as_deref(), Some(long.as_str()));
    }

    #[test]
    fn a_picture_keeps_its_png_and_size() {
        let s = Store::in_memory().unwrap();
        let id = picture(&s, "p", 50, min(0));
        let got = s.clip_get(id).unwrap().unwrap();
        assert_eq!(got.clip.kind, ClipKind::Image);
        assert_eq!((got.clip.width, got.clip.height), (Some(10), Some(8)));
        assert_eq!(got.image.unwrap(), vec![7; 50]);
        assert_eq!(got.clip.thumb.as_deref(), Some("data:image/png;base64,AA"));
    }

    #[test]
    fn the_same_picture_pixels_are_one_entry() {
        let s = Store::in_memory().unwrap();
        let a = picture(&s, "same", 50, min(0));
        let b = picture(&s, "same", 60, min(1));
        assert_eq!(a, b);
        assert_eq!(s.clips_list("", 10).unwrap().len(), 1);
    }

    #[test]
    fn a_file_list_previews_its_first_names_and_keeps_every_path() {
        let s = Store::in_memory().unwrap();
        let paths: Vec<String> = ["C:\\a\\один.txt", "C:\\a\\два.txt", "C:\\a\\три.txt", "C:\\a\\четыре.txt"].map(String::from).into();
        let id = s.clip_add_at(&NewClip::files(&paths, "Explorer"), limits(), min(0)).unwrap();
        let got = s.clip_get(id).unwrap().unwrap();
        assert_eq!(got.clip.kind, ClipKind::Files);
        assert_eq!(got.clip.preview, "один.txt, два.txt, три.txt и ещё 1");
        assert_eq!(got.clip.size, 4);
        assert_eq!(got.body.unwrap(), paths.join("\n"));
    }

    #[test]
    fn using_an_item_moves_it_to_the_top() {
        let s = Store::in_memory().unwrap();
        let a = add(&s, "a", min(0));
        add(&s, "b", min(1));
        assert!(s.clip_touch_at(a, min(2)).unwrap());
        assert_eq!(previews(&s, ""), ["a", "b"]);
    }

    #[test]
    fn deleting_and_pinning_report_whether_the_item_existed() {
        let s = Store::in_memory().unwrap();
        let a = add(&s, "a", min(0));
        assert!(s.clip_set_pinned(a, true).unwrap());
        assert!(s.clips_list("", 10).unwrap()[0].pinned);
        assert!(s.clip_delete(a).unwrap());
        assert!(!s.clip_delete(a).unwrap());
        assert!(!s.clip_set_pinned(a, true).unwrap());
        assert!(!s.clip_touch_at(a, min(1)).unwrap());
    }

    #[test]
    fn the_list_limit_is_respected() {
        let s = Store::in_memory().unwrap();
        for i in 0..5 {
            add(&s, &format!("item {i}"), min(i));
        }
        assert_eq!(s.clips_list("", 3).unwrap().len(), 3);
    }
}
