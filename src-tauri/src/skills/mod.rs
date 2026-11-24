//! Reusable JARVIS skills stored as `<app_local_data>/skills/<slug>/SKILL.md`.
//!
//! Skills are Markdown instructions the user taught JARVIS, asked it to
//! remember, or installed from a link after reviewing a preview. They are
//! indexed concisely in the system prompt, searched on demand and never
//! override safety rules or confirmation gates.

pub mod fetch;
pub mod format;
pub mod search;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use format::{is_valid_slug, parse_skill_md, require_valid_slug, serialize_skill_md, slugify};
pub use format::{Skill, SkillSource, SkillSummary};
pub use search::SkillMatch;

pub const MAX_BODY_BYTES: usize = 16 * 1024;
pub const MAX_SKILLS: usize = 200;
pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_DESC_CHARS: usize = 400;
pub const EVT_SKILLS_CHANGED: &str = "skills:changed";
const PREVIEW_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSaveInput {
    /// Existing slug when editing an existing skill; omitted to derive from `name`.
    pub slug: Option<String>,
    pub name: String,
    pub description: String,
    pub body: String,
    pub enabled: Option<bool>,
    pub source: Option<SkillSource>,
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInstallPreview {
    pub url: String,
    pub raw_url: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub body: String,
    pub size_bytes: usize,
    pub exists: bool,
}

#[derive(Clone)]
struct CachedPreview {
    preview: SkillInstallPreview,
    fetched_at: Instant,
}

pub struct SkillStore {
    root: PathBuf,
    app: Option<AppHandle>,
    io_lock: Mutex<()>,
    previews: Mutex<HashMap<String, CachedPreview>>,
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

fn validate_fields(name: &str, description: &str, body: &str) -> AppResult<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::invalid("Укажите название навыка"));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::invalid(format!(
            "Название навыка слишком длинное (максимум {MAX_NAME_CHARS} символов)"
        )));
    }
    if description.trim().chars().count() > MAX_DESC_CHARS {
        return Err(AppError::invalid(format!(
            "Описание навыка слишком длинное (максимум {MAX_DESC_CHARS} символов)"
        )));
    }
    let body_trimmed = body.trim();
    if body_trimmed.is_empty() {
        return Err(AppError::invalid("Текст инструкции навыка не может быть пустым"));
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(AppError::invalid(format!(
            "Инструкция навыка слишком большая (максимум {} КБ)",
            MAX_BODY_BYTES / 1024
        )));
    }
    Ok(())
}

impl SkillStore {
    pub fn new(root: PathBuf, app: Option<AppHandle>) -> Self {
        SkillStore {
            root,
            app,
            io_lock: Mutex::new(()),
            previews: Mutex::new(HashMap::new()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn emit_changed(&self) {
        if let Some(app) = &self.app {
            let _ = app.emit(EVT_SKILLS_CHANGED, ());
        }
    }

    fn skill_path(&self, slug: &str) -> PathBuf {
        self.root.join(slug).join("SKILL.md")
    }

    fn load_all_unlocked(&self) -> AppResult<Vec<Skill>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(slug) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if !is_valid_slug(&slug) {
                continue;
            }
            let md_path = path.join("SKILL.md");
            if !md_path.is_file() {
                continue;
            }
            if let Ok(raw) = std::fs::read_to_string(&md_path) {
                out.push(parse_skill_md(&raw, &slug));
            }
        }
        Ok(out)
    }

    fn write_skill_unlocked(&self, skill: &Skill) -> AppResult<()> {
        require_valid_slug(&skill.slug)?;
        let dir = self.root.join(&skill.slug);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("SKILL.md");
        let tmp = dir.join("SKILL.md.tmp");
        let content = serialize_skill_md(skill);
        std::fs::write(&tmp, content.as_bytes())?;
        if let Err(e) = std::fs::rename(&tmp, &path) {
            // Windows rename may fail over an existing file; fall back to direct write.
            let _ = std::fs::remove_file(&tmp);
            std::fs::write(&path, content.as_bytes()).map_err(|_| AppError::from(e))?;
        }
        Ok(())
    }

    fn resolve_unlocked(&self, name_or_slug: &str) -> AppResult<Option<Skill>> {
        let needle = name_or_slug.trim();
        if needle.is_empty() {
            return Ok(None);
        }
        if is_valid_slug(needle) {
            let p = self.skill_path(needle);
            if p.is_file() {
                let raw = std::fs::read_to_string(&p)?;
                return Ok(Some(parse_skill_md(&raw, needle)));
            }
        }
        let slug_guess = slugify(needle);
        let p = self.skill_path(&slug_guess);
        if p.is_file() {
            let raw = std::fs::read_to_string(&p)?;
            return Ok(Some(parse_skill_md(&raw, &slug_guess)));
        }
        let norm_needle = search::normalize_phrase(needle);
        let all = self.load_all_unlocked()?;
        Ok(all
            .into_iter()
            .find(|s| search::normalize_phrase(&s.name) == norm_needle || s.slug == needle))
    }

    /// All skills ordered by most recently used, then usage count, then newest.
    pub fn list(&self) -> AppResult<Vec<SkillSummary>> {
        let _g = self.io_lock.lock();
        let mut all = self.load_all_unlocked()?;
        sort_by_recency(&mut all);
        Ok(all.iter().map(SkillSummary::from).collect())
    }

    /// Enabled skills ordered most recently used first, for the system prompt index.
    pub fn index_entries(&self) -> Vec<SkillSummary> {
        let _g = self.io_lock.lock();
        let mut all = self.load_all_unlocked().unwrap_or_default();
        all.retain(|s| s.enabled);
        sort_by_recency(&mut all);
        all.iter().map(SkillSummary::from).collect()
    }

    /// Read a skill by slug or name. When `record_use` is true, bumps `uses`
    /// and updates `last_used`.
    pub fn read(&self, name_or_slug: &str, record_use: bool) -> AppResult<Skill> {
        let mut skill = {
            let _g = self.io_lock.lock();
            let Some(mut s) = self.resolve_unlocked(name_or_slug)? else {
                return Err(AppError::not_found(format!(
                    "Навык «{}» не найден",
                    name_or_slug.trim()
                )));
            };
            if record_use {
                s.uses = s.uses.saturating_add(1);
                s.last_used = Some(now_rfc3339());
                self.write_skill_unlocked(&s)?;
            }
            s
        };
        if record_use {
            self.emit_changed();
        }
        // Keep body normalized.
        skill.body = skill.body.replace("\r\n", "\n");
        Ok(skill)
    }

    /// Create or update a skill, enforcing body size and total skill count limits.
    pub fn save(&self, input: SkillSaveInput) -> AppResult<Skill> {
        validate_fields(&input.name, &input.description, &input.body)?;
        let saved = {
            let _g = self.io_lock.lock();
            let slug = match input.slug.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(explicit) => {
                    require_valid_slug(explicit)?;
                    explicit.to_string()
                }
                None => slugify(&input.name),
            };
            let existing = if self.skill_path(&slug).is_file() {
                std::fs::read_to_string(self.skill_path(&slug))
                    .ok()
                    .map(|raw| parse_skill_md(&raw, &slug))
            } else {
                None
            };
            if existing.is_none() {
                let count = self.load_all_unlocked()?.len();
                if count >= MAX_SKILLS {
                    return Err(AppError::invalid(format!(
                        "Достигнут предел навыков ({MAX_SKILLS}). Удалите ненужные, чтобы сохранить новый."
                    )));
                }
            }
            let created = existing
                .as_ref()
                .map(|e| e.created.clone())
                .filter(|c| !c.is_empty())
                .unwrap_or_else(now_rfc3339);
            let source = input
                .source
                .or_else(|| existing.as_ref().map(|e| e.source))
                .unwrap_or(SkillSource::User);
            let source_url = input
                .source_url
                .or_else(|| existing.as_ref().and_then(|e| e.source_url.clone()));
            let enabled = input
                .enabled
                .or_else(|| existing.as_ref().map(|e| e.enabled))
                .unwrap_or(true);
            let uses = existing.as_ref().map(|e| e.uses).unwrap_or(0);
            let last_used = existing.as_ref().and_then(|e| e.last_used.clone());
            let body = input
                .body
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .trim_start_matches('\n')
                .trim_end()
                .to_string();

            let skill = Skill {
                slug,
                name: input.name.trim().to_string(),
                description: input.description.trim().to_string(),
                created,
                source,
                source_url,
                enabled,
                uses,
                last_used,
                body,
            };
            self.write_skill_unlocked(&skill)?;
            skill
        };
        self.emit_changed();
        Ok(saved)
    }

    pub fn set_enabled(&self, slug: &str, enabled: bool) -> AppResult<SkillSummary> {
        let summary = {
            let _g = self.io_lock.lock();
            require_valid_slug(slug)?;
            let p = self.skill_path(slug);
            if !p.is_file() {
                return Err(AppError::not_found(format!("Навык «{slug}» не найден")));
            }
            let raw = std::fs::read_to_string(&p)?;
            let mut skill = parse_skill_md(&raw, slug);
            skill.enabled = enabled;
            self.write_skill_unlocked(&skill)?;
            SkillSummary::from(&skill)
        };
        self.emit_changed();
        Ok(summary)
    }

    pub fn delete(&self, name_or_slug: &str) -> AppResult<SkillSummary> {
        let summary = {
            let _g = self.io_lock.lock();
            let Some(skill) = self.resolve_unlocked(name_or_slug)? else {
                return Err(AppError::not_found(format!(
                    "Навык «{}» не найден",
                    name_or_slug.trim()
                )));
            };
            require_valid_slug(&skill.slug)?;
            let dir = self.root.join(&skill.slug);
            if dir.exists() {
                std::fs::remove_dir_all(&dir)?;
            }
            SkillSummary::from(&skill)
        };
        self.emit_changed();
        Ok(summary)
    }

    pub fn find(&self, query: &str, limit: usize) -> AppResult<Vec<SkillMatch>> {
        let _g = self.io_lock.lock();
        let all = self.load_all_unlocked()?;
        Ok(search::find_skills(&all, query, limit))
    }

    /// Fetch and parse a remote `SKILL.md` for preview without saving it.
    /// Caches the exact parsed payload briefly so confirmation installs the
    /// exact text that was shown to the user.
    pub async fn preview_install(
        &self,
        url: &str,
        cancel: &CancellationToken,
    ) -> AppResult<SkillInstallPreview> {
        let trimmed = url.trim().to_string();
        let (raw_url, text) = fetch::fetch_skill_text(&trimmed, cancel).await?;
        let parsed = parse_skill_md(&text, "");
        validate_fields(&parsed.name, &parsed.description, &parsed.body)?;
        let exists = {
            let _g = self.io_lock.lock();
            self.skill_path(&parsed.slug).is_file()
        };
        let preview = SkillInstallPreview {
            url: trimmed.clone(),
            raw_url: raw_url.clone(),
            slug: parsed.slug,
            name: parsed.name,
            description: parsed.description,
            body: parsed.body,
            size_bytes: text.len(),
            exists,
        };
        {
            let mut cache = self.previews.lock();
            let now = Instant::now();
            cache.retain(|_, v| now.duration_since(v.fetched_at) < PREVIEW_TTL);
            let entry = CachedPreview { preview: preview.clone(), fetched_at: now };
            cache.insert(trimmed, entry.clone());
            cache.insert(raw_url, entry);
        }
        Ok(preview)
    }

    /// Save a previously previewed remote skill (or fetch fresh if cache expired)
    /// marked with `source: installed`.
    pub async fn commit_install(
        &self,
        url: &str,
        cancel: &CancellationToken,
    ) -> AppResult<Skill> {
        let key = url.trim();
        let cached = {
            let mut cache = self.previews.lock();
            cache
                .remove(key)
                .filter(|c| c.fetched_at.elapsed() < PREVIEW_TTL)
                .map(|c| c.preview)
        };
        let preview = match cached {
            Some(p) => p,
            None => self.preview_install(key, cancel).await?,
        };
        self.save(SkillSaveInput {
            slug: Some(preview.slug),
            name: preview.name,
            description: preview.description,
            body: preview.body,
            enabled: Some(true),
            source: Some(SkillSource::Installed),
            source_url: Some(preview.raw_url),
        })
    }
}

fn sort_by_recency(skills: &mut [Skill]) {
    skills.sort_by(|a, b| {
        b.last_used
            .as_deref()
            .unwrap_or("")
            .cmp(a.last_used.as_deref().unwrap_or(""))
            .then_with(|| b.uses.cmp(&a.uses))
            .then_with(|| b.created.cmp(&a.created))
            .then_with(|| a.name.cmp(&b.name))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(label: &str) -> (PathBuf, SkillStore) {
        let dir = std::env::temp_dir().join(format!(
            "jarvis-skills-test-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = SkillStore::new(dir.clone(), None);
        (dir, store)
    }

    #[test]
    fn crud_usage_tracking_and_recency_order() {
        let (dir, store) = temp_store("crud");
        let a = store
            .save(SkillSaveInput {
                slug: None,
                name: "Выгрузка отчёта".into(),
                description: "Экспорт таблицы за неделю".into(),
                body: "1. Открыть папку\n2. Экспортировать CSV".into(),
                enabled: None,
                source: Some(SkillSource::Learned),
                source_url: None,
            })
            .unwrap();
        assert_eq!(a.slug, "vygruzka-otcheta");
        assert_eq!(a.uses, 0);
        assert!(dir.join("vygruzka-otcheta").join("SKILL.md").is_file());

        let b = store
            .save(SkillSaveInput {
                slug: None,
                name: "Сборка проекта".into(),
                description: "Запуск тестов и сборки".into(),
                body: "1. cargo test".into(),
                enabled: None,
                source: Some(SkillSource::User),
                source_url: None,
            })
            .unwrap();

        // Reading without recording does not bump usage.
        let peek = store.read(&a.slug, false).unwrap();
        assert_eq!(peek.uses, 0);
        assert!(peek.last_used.is_none());

        // Reading by Russian display name records usage and puts it first.
        let used = store.read("Выгрузка отчёта", true).unwrap();
        assert_eq!(used.uses, 1);
        assert!(used.last_used.is_some());

        let index = store.index_entries();
        assert_eq!(index[0].slug, a.slug);
        assert_eq!(index[1].slug, b.slug);

        // Disabling hides it from the prompt index.
        store.set_enabled(&a.slug, false).unwrap();
        let index = store.index_entries();
        assert_eq!(index.len(), 1);
        assert_eq!(index[0].slug, b.slug);

        // Deleting removes directory.
        store.delete(&a.slug).unwrap();
        assert!(!dir.join(&a.slug).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn enforces_body_size_and_skill_count_limits() {
        let (dir, store) = temp_store("limits");
        let too_big = "x".repeat(MAX_BODY_BYTES + 1);
        assert!(store
            .save(SkillSaveInput {
                slug: None,
                name: "Большой".into(),
                description: "Слишком длинный".into(),
                body: too_big,
                enabled: None,
                source: None,
                source_url: None,
            })
            .is_err());

        // Fill up to MAX_SKILLS directly on disk to keep the test fast.
        for i in 0..MAX_SKILLS {
            let slug = format!("skill-{i:03}");
            let sdir = dir.join(&slug);
            std::fs::create_dir_all(&sdir).unwrap();
            std::fs::write(
                sdir.join("SKILL.md"),
                format!("---\nname: Skill {i}\ndescription: Desc {i}\nsource: user\nenabled: true\n---\n\nBody\n"),
            )
            .unwrap();
        }
        // Creating #201 fails.
        assert!(store
            .save(SkillSaveInput {
                slug: None,
                name: "Лишний навык".into(),
                description: "Не поместится".into(),
                body: "Шаги".into(),
                enabled: None,
                source: None,
                source_url: None,
            })
            .is_err());
        // Updating an existing skill still works at the cap.
        assert!(store
            .save(SkillSaveInput {
                slug: Some("skill-000".into()),
                name: "Skill 0 updated".into(),
                description: "Updated".into(),
                body: "Новые шаги".into(),
                enabled: None,
                source: None,
                source_url: None,
            })
            .is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
