//! Built-in MCP catalog loaded from `src/settings/mcpCatalog.json` so Rust
//! (agent tools) and TypeScript (Settings) share a single source of truth.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::mcp::{Secrets, ServerConfig, TransportConfig};

const RAW_CATALOG: &str = include_str!("../../../src/settings/mcpCatalog.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldTarget {
    Env,
    Header,
    Arg,
    Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogField {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub hint: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub initial: Option<String>,
    pub target: FieldTarget,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub optional: bool,
}

impl CatalogField {
    /// Env vars and HTTP headers are secrets stored in Credential Manager.
    pub fn is_secret(&self) -> bool {
        matches!(self.target, FieldTarget::Env | FieldTarget::Header)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CatalogTransport {
    Stdio { command: String, args: Vec<String> },
    Http { url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub category: String,
    pub description: String,
    #[serde(default)]
    pub needs: Option<String>,
    pub transport: CatalogTransport,
    #[serde(default)]
    pub fields: Vec<CatalogField>,
    #[serde(default)]
    pub note: Option<String>,
}

impl CatalogEntry {
    pub fn secret_fields(&self) -> Vec<&CatalogField> {
        self.fields.iter().filter(|f| f.is_secret()).collect()
    }

    pub fn non_secret_fields(&self) -> Vec<&CatalogField> {
        self.fields.iter().filter(|f| !f.is_secret()).collect()
    }
}

static CATALOG: OnceLock<Vec<CatalogEntry>> = OnceLock::new();

pub fn catalog() -> &'static [CatalogEntry] {
    CATALOG.get_or_init(|| serde_json::from_str(RAW_CATALOG).expect("valid src/settings/mcpCatalog.json"))
}

pub fn by_id(id: &str) -> Option<&'static CatalogEntry> {
    let id = id.trim().to_ascii_lowercase();
    catalog().iter().find(|e| e.id == id)
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub entry: &'static CatalogEntry,
    pub score: u32,
}

/// Filler words people say when asking to connect a service.
const STOP_WORDS: &[&str] = &[
    "мой", "мою", "мое", "мои", "мне", "наш", "свой", "сервис", "сервер", "приложение", "интеграция", "интеграцию", "подключи",
    "подключить", "интегрируй", "настрой", "добавь", "пожалуйста", "mcp", "my", "the", "connect", "add", "integrate", "service",
    "server", "app", "to", "for", "with", "в", "на", "к", "с", "и", "по",
];

fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for ch in s.chars() {
        let c = match ch {
            'Ё' | 'ё' => 'е',
            _ => ch.to_lowercase().next().unwrap_or(ch),
        };
        if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
            out.push(c);
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let (m, n) = (av.len(), bv.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = usize::from(av[i - 1] != bv[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

fn score_term(term: &str, entry: &CatalogEntry) -> u32 {
    if term.is_empty() {
        return 0;
    }
    let term_len = term.chars().count();
    let id = normalize(&entry.id);
    let name = normalize(&entry.name);
    let aliases: Vec<String> = entry.aliases.iter().map(|a| normalize(a)).collect();

    let mut best = 0u32;

    // 1. Exact match on id, name or alias.
    if term == id || term == name || aliases.iter().any(|a| a == term) {
        return 120;
    }

    // 2. Prefix / word-exact / substring on id, name and aliases.
    for cand in std::iter::once(&id).chain(std::iter::once(&name)).chain(aliases.iter()) {
        if cand.is_empty() {
            continue;
        }
        if term_len >= 2 && cand.starts_with(term) {
            best = best.max(92);
        }
        for word in cand.split(|c: char| c == ' ' || c == '-' || c == '_') {
            if word.is_empty() {
                continue;
            }
            if word == term {
                best = best.max(96);
            } else if term_len >= 2 && word.starts_with(term) {
                best = best.max(76);
            } else if term_len >= 4 {
                let word_len = word.chars().count();
                if word_len >= 4 && word_len.abs_diff(term_len) <= 2 {
                    let dist = levenshtein(term, word);
                    if dist == 1 {
                        best = best.max(62);
                    } else if dist == 2 && term_len >= 6 && word_len >= 6 {
                        best = best.max(44);
                    }
                }
            }
        }
        if term_len >= 3 && cand.contains(term) {
            best = best.max(72);
        }
        if term_len >= 4 {
            let cand_len = cand.chars().count();
            if cand_len >= 4 && cand_len.abs_diff(term_len) <= 2 {
                let dist = levenshtein(term, cand);
                if dist == 1 {
                    best = best.max(64);
                } else if dist == 2 && term_len >= 6 && cand_len >= 6 {
                    best = best.max(46);
                }
            }
        }
    }

    // 3. Description fallback.
    if best == 0 && term_len >= 3 {
        let desc = normalize(&entry.description);
        if desc.split_whitespace().any(|w| w == term) {
            best = 36;
        } else if desc.contains(term) {
            best = 28;
        }
    }

    best
}

/// Fuzzy search over catalog ids, names, Russian/English aliases and descriptions.
pub fn search(query: &str, limit: usize) -> Vec<SearchHit> {
    let norm = normalize(query);
    if norm.is_empty() || limit == 0 {
        return Vec::new();
    }
    let tokens: Vec<&str> = norm
        .split_whitespace()
        .filter(|t| t.chars().count() >= 2 && !STOP_WORDS.contains(t))
        .collect();

    let mut hits = Vec::new();
    for entry in catalog() {
        let whole = score_term(&norm, entry);
        let token_score = if tokens.is_empty() {
            0
        } else {
            let mut sum = 0u32;
            let mut matched = 0u32;
            for t in &tokens {
                let s = score_term(t, entry);
                if s > 0 {
                    sum += s;
                    matched += 1;
                }
            }
            if matched == 0 {
                0
            } else {
                // Reward matching all meaningful tokens in a multi-word query.
                let avg = sum / tokens.len() as u32;
                if matched as usize == tokens.len() && tokens.len() > 1 {
                    avg.saturating_add(12)
                } else {
                    avg
                }
            }
        };
        let score = whole.max(token_score);
        if score >= 28 {
            hits.push(SearchHit { entry, score });
        }
    }

    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.entry.id.cmp(&b.entry.id)));
    hits.truncate(limit);
    hits
}

/// Replace `{field}` placeholders, trimming whitespace and trailing slashes.
pub fn fill(template: &str, values: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        if let Some(close) = after.find('}') {
            let key = &after[..close];
            if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                if let Some(v) = values.get(key) {
                    out.push_str(v.trim().trim_end_matches('/'));
                }
                rest = &after[close + 1..];
                continue;
            }
        }
        out.push('{');
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Resolve non-secret inputs (falling back to `initial`) and report any missing required ones.
pub fn resolve_non_secret_inputs(entry: &CatalogEntry, provided: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>, Vec<&'static CatalogField>> {
    let mut resolved = BTreeMap::new();
    let mut missing = Vec::new();
    // Look up in the static catalog so returned field refs have `'static` lifetime if needed.
    let canonical = by_id(&entry.id);
    let fields: &[CatalogField] = canonical.map(|e| e.fields.as_slice()).unwrap_or(&[]);
    for f in fields.iter().filter(|f| !f.is_secret()) {
        let val = provided
            .get(&f.key)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .or_else(|| f.initial.as_deref().map(str::trim).filter(|s| !s.is_empty()));
        match val {
            Some(v) => {
                resolved.insert(f.key.clone(), v.to_string());
            }
            None if !f.optional => missing.push(f),
            None => {}
        }
    }
    if missing.is_empty() {
        Ok(resolved)
    } else {
        Err(missing)
    }
}

/// Build a [`ServerConfig`] and [`Secrets`] strictly keeping secret fields out
/// of `non_secret_inputs` and out of the transport config.
pub fn build_server_config(
    entry: &CatalogEntry,
    non_secret_inputs: &BTreeMap<String, String>,
    secret_inputs: &BTreeMap<String, String>,
) -> AppResult<(ServerConfig, Secrets)> {
    let mut arg_values = BTreeMap::new();
    let mut env = BTreeMap::new();
    let mut headers = BTreeMap::new();

    for f in &entry.fields {
        if f.is_secret() {
            // Secrets come ONLY from `secret_inputs` (the secure key prompt), never `non_secret_inputs`.
            let val = secret_inputs.get(&f.key).map(|s| s.trim()).unwrap_or("");
            if val.is_empty() {
                if !f.optional {
                    return Err(AppError::invalid(format!("Не указан секретный параметр «{}»", f.label)));
                }
                continue;
            }
            if let Some(name) = &f.name {
                match f.target {
                    FieldTarget::Env => {
                        env.insert(name.clone(), val.to_string());
                    }
                    FieldTarget::Header => {
                        let fmt = f.format.as_deref().unwrap_or("{value}");
                        headers.insert(name.clone(), fmt.replace("{value}", val));
                    }
                    _ => {}
                }
            }
        } else {
            let val = non_secret_inputs
                .get(&f.key)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .or_else(|| f.initial.as_deref().map(str::trim).filter(|s| !s.is_empty()))
                .unwrap_or("");
            if val.is_empty() {
                if !f.optional {
                    return Err(AppError::invalid(format!("Не указан параметр «{}»", f.label)));
                }
                continue;
            }
            arg_values.insert(f.key.clone(), val.to_string());
        }
    }

    let transport = match &entry.transport {
        CatalogTransport::Stdio { command, args } => TransportConfig::Stdio {
            command: command.clone(),
            args: args.iter().map(|a| fill(a, &arg_values)).collect(),
            cwd: String::new(),
        },
        CatalogTransport::Http { url } => TransportConfig::Http { url: fill(url, &arg_values) },
    };

    let config = ServerConfig {
        id: entry.id.clone(),
        name: entry.name.clone(),
        enabled: true,
        transport,
        env_keys: env.keys().cloned().collect(),
        header_keys: headers.keys().cloned().collect(),
        disabled_tools: Vec::new(),
    };
    let secrets = Secrets { env, headers };
    Ok((config, secrets))
}

/// Strip any secret values from a message before returning it to the model or UI.
pub fn redact_secrets(text: &str, secret_values: &[String]) -> String {
    let mut out = text.to_string();
    for s in secret_values {
        let trimmed = s.trim();
        if trimmed.len() >= 4 {
            out = out.replace(trimmed, "[скрыто]");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn json_catalog_parses_and_every_entry_is_valid() {
        let entries = catalog();
        assert!(!entries.is_empty(), "catalog must not be empty");
        let mut ids = HashSet::new();
        for e in entries {
            assert!(ids.insert(&e.id), "duplicate id: {}", e.id);
            assert!(
                (2..=24).contains(&e.id.len()) && e.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "invalid id: {}",
                e.id
            );
            assert!(!e.name.trim().is_empty(), "{}: empty name", e.id);
            assert!(!e.description.trim().is_empty(), "{}: empty description", e.id);

            let field_keys: HashSet<&str> = e.fields.iter().map(|f| f.key.as_str()).collect();
            for f in &e.fields {
                if f.is_secret() {
                    assert!(f.name.as_deref().is_some_and(|n| !n.is_empty()), "{}: secret field {} missing name", e.id, f.key);
                }
            }

            let text = match &e.transport {
                CatalogTransport::Stdio { args, .. } => {
                    let pkg = args.iter().find(|a| !a.starts_with('-') && !a.starts_with('{')).expect("stdio package");
                    let at = pkg.rfind('@').expect("pinned version @");
                    assert!(
                        at > 0 && pkg[at + 1..].chars().next().is_some_and(|c| c.is_ascii_digit()),
                        "{}: package {} must be pinned with @version",
                        e.id,
                        pkg
                    );
                    args.join(" ")
                }
                CatalogTransport::Http { url } => url.clone(),
            };

            let mut rest = text.as_str();
            while let Some(open) = rest.find('{') {
                let after = &rest[open + 1..];
                if let Some(close) = after.find('}') {
                    let key = &after[..close];
                    assert!(field_keys.contains(key), "{}: undeclared placeholder {{{}}}", e.id, key);
                    rest = &after[close + 1..];
                } else {
                    break;
                }
            }
        }
    }

    #[test]
    fn alias_and_fuzzy_search_ranks_expected_services_first() {
        let top = |q: &str| search(q, 5).into_iter().next().map(|h| h.entry.id.as_str());

        assert_eq!(top("ноушн"), Some("notion"));
        assert_eq!(top("подключи мой ноушн"), Some("notion"));
        assert_eq!(top("ноушен"), Some("notion")); // fuzzy typo
        assert_eq!(top("гитхаб"), Some("github"));
        assert_eq!(top("телега"), Some("zapier"));
        assert_eq!(top("умный дом"), Some("home-assistant"));
        assert_eq!(top("обсидан"), Some("obsidian")); // fuzzy typo
        assert_eq!(top("страйп"), Some("stripe"));
        assert_eq!(top("ютуб"), Some("youtube"));
        assert_eq!(top("википедия"), Some("wikipedia"));

        // Services not in the catalog return no matches.
        assert!(search("яндекс музыка", 5).is_empty());
        assert!(search("spotify", 5).is_empty());
    }

    #[test]
    fn build_config_strictly_splits_secrets_and_non_secrets() {
        let ha = by_id("home-assistant").unwrap();
        let mut non_secret = BTreeMap::new();
        non_secret.insert("url".into(), "http://ha.local:8123/".into());
        // Attempting to smuggle a secret via non_secret_inputs must be ignored.
        non_secret.insert("token".into(), "leaked_via_model".into());

        let empty_secrets = BTreeMap::new();
        assert!(build_server_config(ha, &non_secret, &empty_secrets).is_err());

        let mut real_secrets = BTreeMap::new();
        real_secrets.insert("token".into(), "real_secret_token".into());
        let (cfg, sec) = build_server_config(ha, &non_secret, &real_secrets).unwrap();
        assert_eq!(cfg.transport, TransportConfig::Http { url: "http://ha.local:8123/api/mcp".into() });
        assert_eq!(cfg.header_keys, vec!["Authorization".to_string()]);
        assert_eq!(sec.headers.get("Authorization").map(String::as_str), Some("Bearer real_secret_token"));
        assert!(!format!("{:?}", cfg).contains("real_secret_token"));
        assert!(!format!("{:?}", cfg).contains("leaked_via_model"));

        let notion = by_id("notion").unwrap();
        let mut nsec = BTreeMap::new();
        nsec.insert("NOTION_TOKEN".into(), " ntn_123456 ".into());
        let (ncfg, nsec_out) = build_server_config(notion, &BTreeMap::new(), &nsec).unwrap();
        assert_eq!(ncfg.env_keys, vec!["NOTION_TOKEN".to_string()]);
        assert_eq!(nsec_out.env.get("NOTION_TOKEN").map(String::as_str), Some("ntn_123456"));
    }

    #[test]
    fn redact_secrets_removes_secret_values() {
        let secrets = vec!["ntn_super_secret_key".to_string()];
        let msg = "Ошибка авторизации с ключом ntn_super_secret_key на сервере";
        let clean = redact_secrets(msg, &secrets);
        assert!(!clean.contains("ntn_super_secret_key"));
        assert!(clean.contains("[скрыто]"));
    }
}
