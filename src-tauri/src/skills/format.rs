//! Pure parser, serializer and Windows-safe slug generator for `SKILL.md`.
//!
//! Each skill lives at `<app_local_data>/skills/<slug>/SKILL.md` with
//! YAML-like front matter (`name`, `description`, `created`, `source`,
//! `enabled`, `uses`, `last_used`, optional `source_url`) followed by a
//! Markdown body. Parsing is tolerant to UTF-8 BOM, CRLF line endings,
//! missing fields, quoted scalars, block scalars and unknown keys.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const MAX_SLUG_LEN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    Learned,
    #[default]
    User,
    Installed,
}

impl SkillSource {
    pub fn as_str(self) -> &'static str {
        match self {
            SkillSource::Learned => "learned",
            SkillSource::User => "user",
            SkillSource::Installed => "installed",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "learned" => SkillSource::Learned,
            "installed" => SkillSource::Installed,
            _ => SkillSource::User,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub created: String,
    pub source: SkillSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    pub enabled: bool,
    pub uses: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub created: String,
    pub source: SkillSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    pub enabled: bool,
    pub uses: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used: Option<String>,
}

impl From<&Skill> for SkillSummary {
    fn from(s: &Skill) -> Self {
        SkillSummary {
            slug: s.slug.clone(),
            name: s.name.clone(),
            description: s.description.clone(),
            created: s.created.clone(),
            source: s.source,
            source_url: s.source_url.clone(),
            enabled: s.enabled,
            uses: s.uses,
            last_used: s.last_used.clone(),
        }
    }
}

const WIN_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

fn translit_char(c: char) -> Option<&'static str> {
    Some(match c {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'д' => "d",
        'е' | 'ё' => "e",
        'ж' => "zh",
        'з' => "z",
        'и' => "i",
        'й' => "y",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "kh",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "shch",
        'ъ' | 'ь' => "",
        'ы' => "y",
        'э' => "e",
        'ю' => "yu",
        'я' => "ya",
        _ => return None,
    })
}

/// Produce a Windows-safe ASCII slug from a Russian or Latin skill name.
pub fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for ch in input.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if let Some(rep) = translit_char(ch) {
            if !rep.is_empty() {
                out.push_str(rep);
                last_dash = false;
            }
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
        if out.len() >= MAX_SLUG_LEN {
            break;
        }
    }
    let mut slug = out.trim_matches('-').to_string();
    if slug.len() > MAX_SLUG_LEN {
        slug.truncate(MAX_SLUG_LEN);
        slug = slug.trim_end_matches('-').to_string();
    }
    if slug.is_empty() {
        return "skill".to_string();
    }
    if WIN_RESERVED.contains(&slug.as_str()) {
        return format!("skill-{slug}");
    }
    slug
}

/// Validate a slug before using it as a directory name on Windows.
pub fn is_valid_slug(slug: &str) -> bool {
    if slug.is_empty() || slug.len() > MAX_SLUG_LEN {
        return false;
    }
    if !slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        return false;
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return false;
    }
    !WIN_RESERVED.contains(&slug)
}

pub fn require_valid_slug(slug: &str) -> AppResult<&str> {
    if is_valid_slug(slug) {
        Ok(slug)
    } else {
        Err(AppError::invalid("Некорректный идентификатор навыка"))
    }
}

fn unquote_yaml(raw: &str) -> String {
    let s = raw.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        return s[1..s.len() - 1].replace("''", "'");
    }
    s.to_string()
}

fn quote_yaml(val: &str) -> String {
    let trimmed = val.trim();
    let needs_quote = val.is_empty()
        || val != trimmed
        || val.contains('\n')
        || val.contains('\r')
        || val.contains(": ")
        || val.ends_with(':')
        || val.contains(" #")
        || val.starts_with('#')
        || val.contains('"')
        || val.contains('\'')
        || val.contains('\\')
        || matches!(
            val.chars().next(),
            Some('-' | '?' | ':' | ',' | '[' | ']' | '{' | '}' | '&' | '*' | '!' | '|' | '>' | '%' | '@' | '`')
        )
        || matches!(
            val.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "~"
        );
    if !needs_quote {
        return val.to_string();
    }
    let mut out = String::with_capacity(val.len() + 2);
    out.push('"');
    for c in val.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn parse_bool(raw: &str) -> bool {
    !matches!(raw.trim().to_ascii_lowercase().as_str(), "false" | "no" | "0" | "off")
}

fn first_heading(body: &str) -> Option<String> {
    for line in body.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("# ") {
            let h = rest.trim();
            if !h.is_empty() {
                return Some(h.to_string());
            }
        }
    }
    None
}

fn first_summary_line(body: &str) -> String {
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with("```") || t == "---" {
            continue;
        }
        let cleaned = t.trim_start_matches(['-', '*', '>', ' ']).trim();
        if !cleaned.is_empty() {
            return cleaned.chars().take(160).collect();
        }
    }
    String::new()
}

/// Split optional front matter (`---` ... `---`) from the Markdown body.
fn split_front_matter(normalized: &str) -> (Option<&str>, &str) {
    let mut lines = normalized.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return (None, "");
    };
    if first.trim_end_matches('\n').trim() != "---" {
        return (None, normalized);
    }
    let fm_start = first.len();
    let mut pos = fm_start;
    for line in lines {
        let trimmed = line.trim_end_matches('\n').trim();
        if trimmed == "---" || trimmed == "..." {
            let body_start = pos + line.len();
            return (Some(&normalized[fm_start..pos]), &normalized[body_start..]);
        }
        pos += line.len();
    }
    // Unclosed front matter: treat whole document as body so nothing is lost.
    (None, normalized)
}

/// Parse a `SKILL.md` document. Tolerant to UTF-8 BOM, CRLF, missing fields,
/// quoted values and YAML block scalars.
pub fn parse_skill_md(raw: &str, fallback_slug: &str) -> Skill {
    let without_bom = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let normalized = without_bom.replace("\r\n", "\n").replace('\r', "\n");
    let (fm_block, body_raw) = split_front_matter(&normalized);

    let mut name = String::new();
    let mut description = String::new();
    let mut created = String::new();
    let mut source = SkillSource::User;
    let mut source_url: Option<String> = None;
    let mut enabled = true;
    let mut uses: u64 = 0;
    let mut last_used: Option<String> = None;

    if let Some(fm) = fm_block {
        let lines: Vec<&str> = fm.lines().collect();
        let mut idx = 0;
        while idx < lines.len() {
            let line = lines[idx];
            idx += 1;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            let key = k.trim().to_ascii_lowercase();
            let raw_val = v.trim();
            let value = if matches!(raw_val, "|" | ">" | "|-" | ">-" | "|+" | ">+") {
                let folded = raw_val.starts_with('>');
                let mut parts = Vec::new();
                while idx < lines.len() {
                    let next = lines[idx];
                    if next.starts_with(' ') || next.starts_with('\t') || next.trim().is_empty() {
                        parts.push(next.trim());
                        idx += 1;
                    } else {
                        break;
                    }
                }
                while parts.last().is_some_and(|p| p.is_empty()) {
                    parts.pop();
                }
                if folded {
                    parts.join(" ").trim().to_string()
                } else {
                    parts.join("\n").trim().to_string()
                }
            } else {
                unquote_yaml(raw_val)
            };

            match key.as_str() {
                "name" | "title" => name = value,
                "description" => description = value,
                "created" => created = value,
                "source" => source = SkillSource::parse(&value),
                "source_url" | "sourceurl" => {
                    let t = value.trim().to_string();
                    source_url = (!t.is_empty()).then_some(t);
                }
                "enabled" => enabled = parse_bool(&value),
                "uses" => uses = value.trim().parse::<u64>().unwrap_or(0),
                "last_used" | "lastused" => {
                    let t = value.trim().to_string();
                    last_used = (!t.is_empty()).then_some(t);
                }
                _ => {}
            }
        }
    }

    let body = body_raw.trim_start_matches('\n').trim_end().to_string();
    if name.trim().is_empty() {
        name = first_heading(&body)
            .or_else(|| (!fallback_slug.trim().is_empty()).then(|| fallback_slug.trim().to_string()))
            .unwrap_or_else(|| "Навык".to_string());
    }
    if description.trim().is_empty() {
        description = first_summary_line(&body);
    }

    let slug = if is_valid_slug(fallback_slug) {
        fallback_slug.to_string()
    } else {
        slugify(&name)
    };

    Skill {
        slug,
        name: name.trim().to_string(),
        description: description.trim().to_string(),
        created: created.trim().to_string(),
        source,
        source_url,
        enabled,
        uses,
        last_used,
        body,
    }
}

/// Serialize a skill to LF `SKILL.md` with YAML front matter and Markdown body.
pub fn serialize_skill_md(skill: &Skill) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", quote_yaml(&skill.name)));
    out.push_str(&format!("description: {}\n", quote_yaml(&skill.description)));
    if !skill.created.trim().is_empty() {
        out.push_str(&format!("created: {}\n", quote_yaml(&skill.created)));
    }
    out.push_str(&format!("source: {}\n", skill.source.as_str()));
    if let Some(url) = skill.source_url.as_deref().filter(|u| !u.trim().is_empty()) {
        out.push_str(&format!("source_url: {}\n", quote_yaml(url)));
    }
    out.push_str(&format!("enabled: {}\n", skill.enabled));
    if skill.uses > 0 {
        out.push_str(&format!("uses: {}\n", skill.uses));
    }
    if let Some(lu) = skill.last_used.as_deref().filter(|u| !u.trim().is_empty()) {
        out.push_str(&format!("last_used: {}\n", quote_yaml(lu)));
    }
    out.push_str("---\n");
    let body = skill.body.replace("\r\n", "\n").replace('\r', "\n");
    let body = body.trim_start_matches('\n').trim_end();
    if !body.is_empty() {
        out.push('\n');
        out.push_str(body);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_fields_and_special_chars() {
        let original = Skill {
            slug: "vygruzka-otcheta".into(),
            name: "Выгрузка отчёта: 1С".into(),
            description: "Когда нужно выгрузить отчёт за неделю #работа \"срочно\"".into(),
            created: "2026-11-24T10:00:00Z".into(),
            source: SkillSource::Learned,
            source_url: Some("https://raw.githubusercontent.com/org/repo/main/SKILL.md".into()),
            enabled: false,
            uses: 7,
            last_used: Some("2026-11-24T12:30:00Z".into()),
            body: "## Цель\nСобрать таблицу.\n\n## Шаги\n1. Открыть папку.\n2. Сохранить CSV.".into(),
        };
        let md = serialize_skill_md(&original);
        assert!(!md.contains('\r'));
        let parsed = parse_skill_md(&md, &original.slug);
        assert_eq!(parsed, original);
    }

    #[test]
    fn parses_bom_crlf_missing_fields_and_block_scalars() {
        let raw = "\u{feff}---\r\nname: 'Deploy: staging'\r\ndescription: >\r\n  Run tests and push\r\n  to staging server.\r\nunknown_field: ignored\r\n---\r\n\r\n# Steps\r\n1. Build\r\n2. Ship\r\n";
        let s = parse_skill_md(raw, "deploy-staging");
        assert_eq!(s.slug, "deploy-staging");
        assert_eq!(s.name, "Deploy: staging");
        assert_eq!(s.description, "Run tests and push to staging server.");
        assert_eq!(s.source, SkillSource::User);
        assert!(s.enabled);
        assert_eq!(s.uses, 0);
        assert_eq!(s.last_used, None);
        assert_eq!(s.body, "# Steps\n1. Build\n2. Ship");
    }

    #[test]
    fn parses_markdown_without_front_matter() {
        let raw = "# Сборка релиза\n\nЗапустить тесты и собрать пакет.\n";
        let s = parse_skill_md(raw, "");
        assert_eq!(s.name, "Сборка релиза");
        assert_eq!(s.slug, "sborka-reliza");
        assert_eq!(s.description, "Запустить тесты и собрать пакет.");
        assert!(s.enabled);
    }

    #[test]
    fn slugify_handles_russian_punctuation_length_and_windows_reserved() {
        assert_eq!(slugify("Выгрузка отчёта за месяц!"), "vygruzka-otcheta-za-mesyats");
        assert_eq!(slugify("  Deploy / Staging_v2  "), "deploy-staging-v2");
        assert_eq!(slugify("CON"), "skill-con");
        assert_eq!(slugify("nul"), "skill-nul");
        assert_eq!(slugify("COM1"), "skill-com1");
        assert_eq!(slugify("!!!"), "skill");
        let long = slugify(&"очень длинное название ".repeat(10));
        assert!(long.len() <= MAX_SLUG_LEN);
        assert!(is_valid_slug(&long));
    }

    #[test]
    fn rejects_unsafe_slugs() {
        for bad in ["", "../etc", "a/b", "a\\b", "con", "LPT1", "Upper", "-dash", "dash-", "foo.bar"] {
            assert!(!is_valid_slug(bad), "should reject {bad}");
        }
        assert!(is_valid_slug("report-export-2026"));
    }
}
