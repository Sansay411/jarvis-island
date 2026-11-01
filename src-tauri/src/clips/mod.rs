//! Clipboard history: what is captured, what is kept, how an entry returns
//! to the clipboard (see docs/ARCHITECTURE.md).

pub mod image;
pub mod privacy;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AppError, AppResult};
use crate::platform::clipboard::{self, Snapshot};
use crate::platform::island::LABEL as ISLAND;
use crate::platform::{self, clipwatch, input, process, winmgr};
use crate::state::AppState;
use crate::store::clips::{Clip, ClipContent, ClipKind, ClipLimits, NewClip};
use crate::store::settings::ClipboardSettings;
use crate::store::Store;

/// Event for the island: the history changed, list it again.
pub const EVT_CHANGED: &str = "clips:changed";
/// Longest text the history keeps, in characters.
pub const MAX_TEXT_CHARS: usize = 200_000;
/// Thumbnail box for pictures in the list.
const THUMB_W: u32 = 200;
const THUMB_H: u32 = 100;
/// Bytes of unpinned pictures kept in total.
const IMAGE_BUDGET: u64 = 96 * 1024 * 1024;

/// What to do with a clipboard snapshot.
#[derive(Debug)]
pub enum Prepared {
    /// Not recorded, and why (for the log; never the content).
    Skip(&'static str),
    Keep(NewClip),
}

/// Decide whether a copy is recorded, and in what form.
pub fn prepare(snap: &Snapshot, source: &str, cfg: &ClipboardSettings) -> Prepared {
    if snap.private {
        return Prepared::Skip("private");
    }
    if privacy::app_ignored(source, &cfg.ignored_apps) {
        return Prepared::Skip("ignored app");
    }
    if !snap.files.is_empty() {
        return Prepared::Keep(NewClip::files(&snap.files, source));
    }
    if let Some(text) = &snap.text {
        if text.trim().is_empty() {
            return Prepared::Skip("empty");
        }
        if text.chars().count() > MAX_TEXT_CHARS {
            return Prepared::Skip("too long");
        }
        if cfg.skip_secrets && privacy::looks_secret(text) {
            return Prepared::Skip("secret");
        }
        return Prepared::Keep(NewClip::text(text, source));
    }
    if snap.png.is_some() || snap.dib.is_some() {
        return picture(snap, source);
    }
    Prepared::Skip("empty")
}

fn picture(snap: &Snapshot, source: &str) -> Prepared {
    let pixels = snap.png.as_deref().and_then(image::png_to_rgba).or_else(|| snap.dib.as_deref().and_then(image::dib_to_rgba));
    let Some(pixels) = pixels else {
        return Prepared::Skip("unreadable picture");
    };
    // Kept as a clean PNG whatever the program offered.
    match (image::rgba_to_png(&pixels), image::thumbnail_data_url(&pixels, THUMB_W, THUMB_H)) {
        (Some(png), Some(thumb)) => Prepared::Keep(NewClip::image(png, &image::pixel_hash(&pixels), pixels.width, pixels.height, thumb, source)),
        _ => Prepared::Skip("unreadable picture"),
    }
}

fn limits(cfg: &ClipboardSettings) -> ClipLimits {
    ClipLimits { max_items: cfg.max_items.clamp(10, 1000), keep_days: cfg.keep_days.min(3650), max_image_bytes: IMAGE_BUDGET }
}

/// `chrome.exe` → `chrome`.
fn program_name(exe: &str) -> String {
    Path::new(exe).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| exe.to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
    pub shortcut: String,
}

pub struct Clips {
    app: AppHandle,
    /// Captures run one at a time, in the order the copies happened.
    serial: Mutex<()>,
}

impl Clips {
    pub fn new(app: AppHandle) -> Arc<Clips> {
        Arc::new(Clips { app, serial: Mutex::new(()) })
    }

    fn store(&self) -> Arc<Store> {
        self.app.state::<AppState>().store.clone()
    }

    fn changed(&self) {
        let _ = self.app.emit_to(ISLAND, EVT_CHANGED, ());
    }

    /// Start listening for copies. Expired entries are dropped first.
    pub fn start(self: &Arc<Self>) {
        let cfg = self.store().settings().clipboard;
        if let Err(e) = self.store().clips_prune(limits(&cfg)) {
            tracing::warn!("clipboard history: pruning failed: {e}");
        }
        let me = Arc::clone(self);
        clipwatch::spawn(move || {
            let me = Arc::clone(&me);
            tauri::async_runtime::spawn_blocking(move || me.capture());
        });
    }

    /// The clipboard changed: record it if the settings and the privacy rules
    /// allow. With the history off the clipboard is not even read.
    fn capture(&self) {
        let _one_at_a_time = self.serial.lock();
        let cfg = self.store().settings().clipboard;
        if !cfg.enabled {
            return;
        }
        let snap = match clipboard::snapshot() {
            Ok(snap) => snap,
            Err(e) => return tracing::debug!("clipboard history: {e}"),
        };
        let source = snap.owner_pid.and_then(process::process_name).map(|exe| program_name(&exe)).unwrap_or_default();
        match prepare(&snap, &source, &cfg) {
            Prepared::Skip(why) => tracing::debug!(why, "copy not recorded"),
            Prepared::Keep(clip) => match self.store().clip_add(&clip, limits(&cfg)) {
                Ok(_) => self.changed(),
                Err(e) => tracing::debug!("clipboard history: {e}"),
            },
        }
    }

    pub fn status(&self) -> Status {
        let s = self.store().settings();
        Status { enabled: s.clipboard.enabled, shortcut: s.shortcuts.clipboard }
    }

    pub fn set_enabled(&self, enabled: bool) -> AppResult<()> {
        let store = self.store();
        let mut settings = store.settings();
        settings.clipboard.enabled = enabled;
        store.save_settings(&settings)?;
        let _ = self.app.emit("settings:changed", ());
        Ok(())
    }

    pub fn list(&self, query: &str, limit: u32) -> AppResult<Vec<Clip>> {
        self.store().clips_list(query, limit.clamp(1, 200))
    }

    pub fn get(&self, id: i64) -> AppResult<ClipContent> {
        self.store().clip_get(id)?.ok_or_else(|| AppError::not_found("Этой записи уже нет в истории буфера"))
    }

    pub fn set_pinned(&self, id: i64, pinned: bool) -> AppResult<()> {
        if !self.store().clip_set_pinned(id, pinned)? {
            return Err(AppError::not_found("Этой записи уже нет в истории буфера"));
        }
        self.changed();
        Ok(())
    }

    pub fn delete(&self, id: i64) -> AppResult<()> {
        self.store().clip_delete(id)?;
        self.changed();
        Ok(())
    }

    pub fn clear(&self, keep_pinned: bool) -> AppResult<u32> {
        let removed = self.store().clips_clear(keep_pinned)?;
        self.changed();
        Ok(removed)
    }

    /// Put the entry on the clipboard and read the clipboard back to be sure
    /// it took. The entry moves to the top of the history.
    pub fn copy(&self, id: i64) -> AppResult<ClipContent> {
        let item = self.get(id)?;
        let body = item.body.clone().unwrap_or_default();
        match item.clip.kind {
            ClipKind::Text => clipboard::write_text(&body)?,
            ClipKind::Files => clipboard::write_files(&body.lines().map(String::from).collect::<Vec<_>>())?,
            ClipKind::Image => {
                let png = item.image.as_deref().ok_or_else(|| AppError::internal("У записи нет картинки"))?;
                let pixels = image::png_to_rgba(png).ok_or_else(|| AppError::internal("Не удалось прочитать картинку из истории"))?;
                clipboard::write_image(png, &image::rgba_to_dib24(&pixels))?;
            }
        }
        let back = clipboard::snapshot()?;
        let took = match item.clip.kind {
            ClipKind::Text => back.text.as_deref() == Some(body.as_str()),
            ClipKind::Files => back.files.len() == body.lines().count(),
            ClipKind::Image => back.png.is_some() || back.dib.is_some(),
        };
        if !took {
            return Err(AppError::platform("Буфер обмена не принял запись — возможно, его держит другая программа"));
        }
        let _ = self.store().clip_touch(id);
        self.changed();
        Ok(item)
    }

    /// Copy the entry and press Ctrl+V in the window that had the keyboard.
    /// Returns the program it was pasted into. What lands in that window is
    /// up to the program: the entry is on the clipboard either way.
    pub async fn paste(self: &Arc<Self>, id: i64) -> AppResult<String> {
        let me = Arc::clone(self);
        platform::blocking(move || me.paste_blocking(id)).await
    }

    fn paste_blocking(&self, id: i64) -> AppResult<String> {
        let island = self.app.state::<AppState>().island.clone();
        self.copy(id)?;
        // If the search box had the keyboard, give it back to the window it
        // came from.
        let _ = island.end_input(true);
        let island_hwnd = island.hwnd() as i64;
        let deadline = Instant::now() + Duration::from_millis(500);
        let target = loop {
            if let Some(w) = winmgr::foreground_window().filter(|w| w.hwnd != island_hwnd) {
                break w;
            }
            if Instant::now() >= deadline {
                return Err(AppError::platform("Не вижу окна, куда вставить. Запись уже в буфере — нажмите Ctrl+V."));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        // Let the window take the keyboard back before the keystroke.
        std::thread::sleep(Duration::from_millis(40));
        input::key_chord("ctrl+v")?;
        Ok(program_name(&target.process))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::clips::ClipKind;

    fn cfg() -> ClipboardSettings {
        ClipboardSettings::default()
    }

    fn text(t: &str) -> Snapshot {
        Snapshot { text: Some(t.into()), ..Default::default() }
    }

    fn kept(p: Prepared) -> NewClip {
        match p {
            Prepared::Keep(c) => c,
            Prepared::Skip(why) => panic!("expected the copy to be kept, it was skipped: {why}"),
        }
    }

    fn skipped(p: Prepared) -> &'static str {
        match p {
            Prepared::Skip(why) => why,
            Prepared::Keep(c) => panic!("expected a skip, kept {:?}", c.kind),
        }
    }

    #[test]
    fn plain_text_is_kept_with_its_source() {
        let clip = kept(prepare(&text("Встреча в четверг"), "chrome", &cfg()));
        assert_eq!(clip.kind, ClipKind::Text);
        assert_eq!(clip.body.as_deref(), Some("Встреча в четверг"));
        assert_eq!(clip.source, "chrome");
    }

    #[test]
    fn a_copy_marked_private_by_its_program_is_skipped() {
        let snap = Snapshot { private: true, text: Some("hunter2".into()), ..Default::default() };
        assert_eq!(skipped(prepare(&snap, "KeePassXC", &cfg())), "private");
    }

    #[test]
    fn copies_from_listed_programs_are_skipped() {
        assert_eq!(skipped(prepare(&text("anything"), "Bitwarden", &cfg())), "ignored app");
        let mine = ClipboardSettings { ignored_apps: vec!["telegram".into()], ..cfg() };
        assert_eq!(skipped(prepare(&text("anything"), "Telegram", &mine)), "ignored app");
        assert!(matches!(prepare(&text("anything"), "Bitwarden", &mine), Prepared::Keep(_)), "only the user's own list applies");
    }

    #[test]
    fn secrets_are_skipped_unless_the_user_turned_that_off() {
        let token = text("ghp_abcdefghijklmnopqrstuvwxyz0123456789");
        assert_eq!(skipped(prepare(&token, "Code", &cfg())), "secret");
        let off = ClipboardSettings { skip_secrets: false, ..cfg() };
        assert!(matches!(prepare(&token, "Code", &off), Prepared::Keep(_)));
    }

    #[test]
    fn blank_and_oversized_text_is_skipped() {
        assert_eq!(skipped(prepare(&text("  \n "), "Code", &cfg())), "empty");
        assert_eq!(skipped(prepare(&Snapshot::default(), "Code", &cfg())), "empty");
        assert_eq!(skipped(prepare(&text(&"я".repeat(MAX_TEXT_CHARS + 1)), "Code", &cfg())), "too long");
    }

    #[test]
    fn copied_files_are_kept_as_a_list() {
        let snap = Snapshot { files: vec!["C:\\a\\один.txt".into(), "C:\\a\\два.txt".into()], ..Default::default() };
        let clip = kept(prepare(&snap, "Explorer", &cfg()));
        assert_eq!(clip.kind, ClipKind::Files);
        assert_eq!(clip.size, 2);
    }

    fn picture() -> image::Rgba {
        image::Rgba { width: 3, height: 2, pixels: (0..6u8).flat_map(|i| [i * 30, 200 - i * 20, i * 5, 255]).collect() }
    }

    #[test]
    fn a_picture_copied_as_png_and_as_dib_is_the_same_entry() {
        let img = picture();
        let as_png = Snapshot { png: image::rgba_to_png(&img), ..Default::default() };
        let as_dib = Snapshot { dib: Some(image::rgba_to_dib24(&img)), ..Default::default() };
        let (a, b) = (kept(prepare(&as_png, "Snip", &cfg())), kept(prepare(&as_dib, "Paint", &cfg())));
        assert_eq!(a.kind, ClipKind::Image);
        assert_eq!(a.hash, b.hash);
        assert_eq!((a.width, a.height), (Some(3), Some(2)));
        assert!(a.thumb.as_deref().is_some_and(|t| t.starts_with("data:image/png;base64,")));
        assert_eq!(image::png_to_rgba(a.image.as_deref().unwrap()).unwrap(), img);
    }

    #[test]
    fn a_picture_that_cannot_be_read_is_skipped() {
        let broken = Snapshot { dib: Some(vec![1, 2, 3]), ..Default::default() };
        assert_eq!(skipped(prepare(&broken, "Paint", &cfg())), "unreadable picture");
    }
}
