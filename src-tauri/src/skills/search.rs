//! Pure search and ranking over skills with lightweight Russian and English
//! stemming (lowercase, `ё` -> `е`, common inflection stripping).

use std::collections::HashSet;

use serde::Serialize;

use super::format::{Skill, SkillSummary};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMatch {
    #[serde(flatten)]
    pub summary: SkillSummary,
    pub score: u32,
}

const RU_SUFFIXES: &[&str] = &[
    "иями", "ями", "ами", "овать", "евать", "иться", "ться", "ение", "ание", "ство", "ого", "его",
    "ому", "ему", "ыми", "ими", "или", "али", "яли", "ели", "ила", "ала", "яла", "ела", "ило",
    "ало", "яло", "ело", "ать", "ять", "еть", "ить", "ыть", "уть", "ий", "ый", "ой", "ая", "яя",
    "ое", "ее", "ые", "ие", "ов", "ев", "ей", "ом", "ем", "ам", "ям", "ах", "ях", "ть", "ти", "ся",
    "сь", "ил", "ал", "ял", "ел", "ла", "ло", "ли", "ет", "ит", "ут", "ют", "ат", "ят", "им", "у",
    "ю", "а", "я", "о", "е", "ы", "и", "ь",
];

const EN_SUFFIXES: &[&str] = &["ing", "ied", "ed", "es", "s"];

/// Normalize text: lowercase, `ё` -> `е`, collapse non-alphanumeric separators.
pub fn normalize_phrase(text: &str) -> String {
    text.to_lowercase()
        .replace('ё', "е")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Lightweight Russian/English stemmer for matching inflected query words.
pub fn stem_word(word: &str) -> String {
    let w = word.to_lowercase().replace('ё', "е");
    let char_count = w.chars().count();
    if char_count <= 3 {
        return w;
    }
    let has_cyrillic = w.chars().any(|c| ('а'..='я').contains(&c));
    if has_cyrillic {
        for suf in RU_SUFFIXES {
            if let Some(base) = w.strip_suffix(suf) {
                if base.chars().count() >= 3 {
                    return base.to_string();
                }
            }
        }
        return w;
    }
    if w.is_ascii() {
        for suf in EN_SUFFIXES {
            if let Some(base) = w.strip_suffix(suf) {
                if base.chars().count() >= 3 {
                    return base.to_string();
                }
            }
        }
    }
    w
}

/// Tokenize and stem a string into unique normalized stems.
pub fn stemmed_tokens(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in text.split(|c: char| !c.is_alphanumeric()) {
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        let chars = t.chars().count();
        if chars == 1 && !t.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let stemmed = stem_word(t);
        if seen.insert(stemmed.clone()) {
            out.push(stemmed);
        }
    }
    out
}

fn token_hit(needle: &str, haystack: &[String], exact_pts: u32, prefix_pts: u32) -> u32 {
    if haystack.iter().any(|t| t == needle) {
        return exact_pts;
    }
    if needle.chars().count() >= 4
        && haystack
            .iter()
            .any(|t| t.starts_with(needle) || (t.chars().count() >= 4 && needle.starts_with(t.as_str())))
    {
        return prefix_pts;
    }
    0
}

/// Score a skill against a query. Zero means no match.
pub fn score_skill(skill: &Skill, query: &str) -> u32 {
    let norm_q = normalize_phrase(query);
    if norm_q.is_empty() {
        return 0;
    }
    let norm_name = normalize_phrase(&skill.name);
    let norm_slug = normalize_phrase(&skill.slug);
    let norm_desc = normalize_phrase(&skill.description);

    let mut score = 0u32;
    if norm_q == norm_name || norm_q == norm_slug {
        score += 200;
    } else if norm_name.contains(&norm_q) || norm_slug.contains(&norm_q) {
        score += 80;
    } else if norm_desc.contains(&norm_q) {
        score += 40;
    }

    let q_tokens = stemmed_tokens(query);
    if !q_tokens.is_empty() {
        let name_tokens = stemmed_tokens(&format!("{} {}", skill.name, skill.slug));
        let desc_tokens = stemmed_tokens(&skill.description);
        let body_tokens = stemmed_tokens(&skill.body);

        let mut matched_tokens = 0usize;
        for qt in &q_tokens {
            let n = token_hit(qt, &name_tokens, 30, 15);
            let d = token_hit(qt, &desc_tokens, 18, 9);
            let b = token_hit(qt, &body_tokens, 6, 3);
            let best = n.max(d).max(b);
            if best > 0 {
                matched_tokens += 1;
                // Reward multi-field matches, weighted toward name/description.
                score += n + d + b;
            }
        }
        if matched_tokens == q_tokens.len() && q_tokens.len() >= 2 {
            score += 25;
        }
    }

    if score > 0 {
        score += skill.uses.min(10) as u32;
    }
    score
}

/// Rank enabled skills matching `query`, returning up to `limit` top results.
pub fn find_skills(skills: &[Skill], query: &str, limit: usize) -> Vec<SkillMatch> {
    let limit = limit.clamp(1, 50);
    let mut ranked: Vec<(u32, &Skill)> = skills
        .iter()
        .filter(|s| s.enabled)
        .filter_map(|s| {
            let score = score_skill(s, query);
            (score > 0).then_some((score, s))
        })
        .collect();

    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.last_used.cmp(&a.1.last_used))
            .then_with(|| b.1.uses.cmp(&a.1.uses))
            .then_with(|| a.1.name.cmp(&b.1.name))
    });

    ranked
        .into_iter()
        .take(limit)
        .map(|(score, s)| SkillMatch { summary: SkillSummary::from(s), score })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::format::SkillSource;

    fn sample(slug: &str, name: &str, description: &str, body: &str, enabled: bool) -> Skill {
        Skill {
            slug: slug.into(),
            name: name.into(),
            description: description.into(),
            created: "2026-11-20T10:00:00Z".into(),
            source: SkillSource::Learned,
            source_url: None,
            enabled,
            uses: 0,
            last_used: None,
            body: body.into(),
        }
    }

    #[test]
    fn russian_stemming_matches_yo_and_inflections() {
        assert_eq!(stem_word("отчёты"), stem_word("отчета"));
        assert_eq!(stem_word("выгрузить"), stem_word("выгрузил"));
        assert_eq!(stem_word("таблицами"), stem_word("таблицы"));
        assert_eq!(stem_word("deploying"), stem_word("deployed"));
    }

    #[test]
    fn ranks_name_above_description_above_body_and_skips_disabled() {
        let skills = vec![
            sample("body-only", "Общая заметка", "Разные дела", "Иногда нужна выгрузка отчёта в Excel.", true),
            sample("desc-hit", "Еженедельная рутина", "Выгрузка отчёта по продажам", "Открыть браузер.", true),
            sample("name-hit", "Выгрузка отчёта", "Быстрый экспорт данных", "Шаг 1.", true),
            sample("disabled", "Выгрузка отчёта", "Отключённый навык", "Шаг 1.", false),
        ];
        let found = find_skills(&skills, "выгрузить отчеты", 5);
        let slugs: Vec<&str> = found.iter().map(|m| m.summary.slug.as_str()).collect();
        assert_eq!(slugs, vec!["name-hit", "desc-hit", "body-only"]);
        assert!(found[0].score > found[1].score);
        assert!(found[1].score > found[2].score);
    }

    #[test]
    fn empty_or_unrelated_query_returns_nothing() {
        let skills = vec![sample("git-release", "Релиз в Git", "Сборка тега", "cargo test", true)];
        assert!(find_skills(&skills, "   ", 5).is_empty());
        assert!(find_skills(&skills, "приготовить борщ", 5).is_empty());
    }
}
