//! Safe HTTP fetcher for installing `SKILL.md` files from public URLs
//! (including GitHub blob/tree/repo links converted to `raw.githubusercontent.com`).
//!
//! Enforces public-host validation (including on redirects), text-only content
//! checks and the 64 KB download cap.

use std::time::Duration;

use tokio_util::sync::CancellationToken;
use url::{Host, Url};

use crate::error::{AppError, AppResult};

pub const MAX_FETCH_BYTES: usize = 64 * 1024;
const MAX_URL_LEN: usize = 2048;
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
const UA: &str = concat!("JARVIS/", env!("CARGO_PKG_VERSION"));

fn is_public_ipv4(ip: std::net::Ipv4Addr) -> bool {
    let oct = ip.octets();
    let cgnat = oct[0] == 100 && (oct[1] & 0b1100_0000) == 64;
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_unspecified()
        && !ip.is_broadcast()
        && !ip.is_multicast()
        && !cgnat
        && oct[0] != 0
}

/// Validate that a URL is a public HTTP(S) target without embedded credentials.
pub fn validate_public_url(raw: &str) -> AppResult<Url> {
    let text = raw.trim();
    if text.is_empty() || text.len() > MAX_URL_LEN {
        return Err(AppError::invalid("Укажите корректную HTTP(S)-ссылку на SKILL.md"));
    }
    let url = Url::parse(text).map_err(|_| AppError::invalid("Некорректная ссылка на навык"))?;
    let public = match url.host() {
        Some(Host::Domain(host)) => {
            let h = host.to_ascii_lowercase();
            h.contains('.')
                && !h.ends_with(".local")
                && !h.ends_with(".localhost")
                && !h.ends_with(".internal")
                && h != "localhost"
        }
        Some(Host::Ipv4(ip)) => is_public_ipv4(ip),
        Some(Host::Ipv6(_)) | None => false,
    };
    if !matches!(url.scheme(), "http" | "https")
        || !public
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AppError::invalid(
            "Поддерживаются только публичные HTTP(S)-ссылки без логина и пароля",
        ));
    }
    Ok(url)
}

/// Convert a GitHub file/folder/repo link or raw URL into candidate raw text URLs.
pub fn resolve_skill_urls(raw: &str) -> AppResult<Vec<String>> {
    let url = validate_public_url(raw)?;
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let segs: Vec<&str> = url
        .path_segments()
        .map(|it| it.filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();

    if host == "github.com" || host == "www.github.com" {
        if segs.len() < 2 {
            return Err(AppError::invalid(
                "Укажите ссылку на файл SKILL.md или папку в репозитории GitHub",
            ));
        }
        let owner = segs[0];
        let repo = segs[1].trim_end_matches(".git");
        if segs.len() == 2 {
            return Ok(vec![
                format!("https://raw.githubusercontent.com/{owner}/{repo}/main/SKILL.md"),
                format!("https://raw.githubusercontent.com/{owner}/{repo}/master/SKILL.md"),
            ]);
        }
        match segs[2] {
            "blob" | "raw" => {
                if segs.len() < 5 {
                    return Err(AppError::invalid("Ссылка GitHub должна указывать на файл"));
                }
                let branch = segs[3];
                let rest = segs[4..].join("/");
                return Ok(vec![format!(
                    "https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{rest}"
                )]);
            }
            "tree" => {
                if segs.len() < 4 {
                    return Err(AppError::invalid("Ссылка GitHub должна указывать на ветку или папку"));
                }
                let branch = segs[3];
                let rest = if segs.len() > 4 {
                    format!("{}/SKILL.md", segs[4..].join("/"))
                } else {
                    "SKILL.md".to_string()
                };
                return Ok(vec![format!(
                    "https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{rest}"
                )]);
            }
            _ => {
                return Err(AppError::invalid(
                    "Поддерживаются ссылки GitHub на файл (blob), папку (tree) или репозиторий",
                ));
            }
        }
    }

    if host == "raw.githubusercontent.com" {
        if segs.len() < 4 {
            return Err(AppError::invalid("Неполная raw-ссылка GitHub"));
        }
        let last = segs.last().copied().unwrap_or("");
        if last.contains('.') {
            return Ok(vec![url.to_string()]);
        }
        let base = url.to_string();
        return Ok(vec![format!("{}/SKILL.md", base.trim_end_matches('/'))]);
    }

    if host == "gist.github.com" && segs.len() >= 2 {
        return Ok(vec![format!("https://gist.githubusercontent.com/{}/{}/raw", segs[0], segs[1])]);
    }

    Ok(vec![url.to_string()])
}

/// Ensure downloaded bytes are plain text / Markdown within the 64 KB limit.
pub fn validate_text_payload(content_type: Option<&str>, bytes: &[u8]) -> AppResult<String> {
    if bytes.is_empty() {
        return Err(AppError::invalid("Файл по ссылке пуст"));
    }
    if bytes.len() > MAX_FETCH_BYTES {
        return Err(AppError::invalid(format!(
            "Файл слишком большой (лимит {} КБ)",
            MAX_FETCH_BYTES / 1024
        )));
    }
    if let Some(ct) = content_type {
        let mime = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        let allowed = mime.is_empty()
            || mime == "text/plain"
            || mime == "text/markdown"
            || mime == "text/x-markdown"
            || mime == "application/x-markdown";
        if !allowed {
            return Err(AppError::invalid(format!(
                "Ожидается текстовый файл Markdown, получен тип «{mime}»"
            )));
        }
    }
    if bytes.contains(&0) {
        return Err(AppError::invalid("Файл не похож на текст (обнаружены бинарные данные)"));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| AppError::invalid("Файл должен быть в кодировке UTF-8"))?;
    let head = text.trim_start_matches('\u{feff}').trim_start();
    let lower_head: String = head.chars().take(32).collect::<String>().to_ascii_lowercase();
    if lower_head.starts_with("<!doctype html") || lower_head.starts_with("<html") {
        return Err(AppError::invalid(
            "По ссылке открылась HTML-страница вместо текста SKILL.md",
        ));
    }
    Ok(text.to_string())
}

/// Download a `SKILL.md` text file from a public URL. Returns `(resolved_url, text)`.
pub async fn fetch_skill_text(input_url: &str, cancel: &CancellationToken) -> AppResult<(String, String)> {
    let candidates = resolve_skill_urls(input_url)?;
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("too many redirects");
        }
        if validate_public_url(attempt.url().as_str()).is_err() {
            return attempt.error("redirect to non-public address blocked");
        }
        attempt.follow()
    });
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(UA)
        .redirect(policy)
        .build()?;

    let mut last_err: Option<AppError> = None;
    for (idx, candidate) in candidates.iter().enumerate() {
        let req = client
            .get(candidate)
            .header(reqwest::header::ACCEPT, "text/plain, text/markdown;q=0.9, */*;q=0.1")
            .send();
        let mut resp = tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            r = req => match r {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(AppError::from(e));
                    continue;
                }
            }
        };

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND && idx + 1 < candidates.len() {
            last_err = Some(AppError::not_found("Файл SKILL.md по этой ссылке не найден (404)"));
            continue;
        }
        if !status.is_success() {
            return Err(AppError::Network(format!(
                "Не удалось загрузить навык (HTTP {})",
                status.as_u16()
            )));
        }

        if let Some(len) = resp.content_length() {
            if len as usize > MAX_FETCH_BYTES {
                return Err(AppError::invalid(format!(
                    "Файл слишком большой (лимит {} КБ)",
                    MAX_FETCH_BYTES / 1024
                )));
            }
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let mut buf = Vec::new();
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(AppError::Cancelled),
                c = resp.chunk() => c?,
            };
            let Some(bytes) = chunk else { break };
            if buf.len() + bytes.len() > MAX_FETCH_BYTES {
                return Err(AppError::invalid(format!(
                    "Файл слишком большой (лимит {} КБ)",
                    MAX_FETCH_BYTES / 1024
                )));
            }
            buf.extend_from_slice(&bytes);
        }

        let text = validate_text_payload(content_type.as_deref(), &buf)?;
        return Ok((candidate.clone(), text));
    }

    Err(last_err.unwrap_or_else(|| AppError::Network("Не удалось загрузить файл навыка".into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_github_blob_raw_tree_and_repo_urls() {
        assert_eq!(
            resolve_skill_urls("https://github.com/acme/tools/blob/main/skills/report/SKILL.md").unwrap(),
            vec!["https://raw.githubusercontent.com/acme/tools/main/skills/report/SKILL.md"]
        );
        assert_eq!(
            resolve_skill_urls("https://github.com/acme/tools/raw/dev/SKILL.md").unwrap(),
            vec!["https://raw.githubusercontent.com/acme/tools/dev/SKILL.md"]
        );
        assert_eq!(
            resolve_skill_urls("https://github.com/acme/tools/tree/main/skills/report").unwrap(),
            vec!["https://raw.githubusercontent.com/acme/tools/main/skills/report/SKILL.md"]
        );
        assert_eq!(
            resolve_skill_urls("https://github.com/acme/tools/tree/main").unwrap(),
            vec!["https://raw.githubusercontent.com/acme/tools/main/SKILL.md"]
        );
        assert_eq!(
            resolve_skill_urls("https://github.com/acme/tools").unwrap(),
            vec![
                "https://raw.githubusercontent.com/acme/tools/main/SKILL.md",
                "https://raw.githubusercontent.com/acme/tools/master/SKILL.md",
            ]
        );
        assert_eq!(
            resolve_skill_urls("https://raw.githubusercontent.com/acme/tools/main/skills/report/SKILL.md").unwrap(),
            vec!["https://raw.githubusercontent.com/acme/tools/main/skills/report/SKILL.md"]
        );
        assert_eq!(
            resolve_skill_urls("https://raw.githubusercontent.com/acme/tools/main/skills/report").unwrap(),
            vec!["https://raw.githubusercontent.com/acme/tools/main/skills/report/SKILL.md"]
        );
    }

    #[test]
    fn rejects_private_local_and_credential_urls() {
        for bad in [
            "",
            "file:///C:/Windows/win.ini",
            "http://localhost/SKILL.md",
            "http://127.0.0.1/SKILL.md",
            "http://10.0.0.5/SKILL.md",
            "http://192.168.1.1/SKILL.md",
            "http://169.254.169.254/latest",
            "http://100.64.0.1/SKILL.md",
            "http://[::1]/SKILL.md",
            "http://printer.local/SKILL.md",
            "https://user:pass@github.com/a/b/blob/main/SKILL.md",
        ] {
            assert!(resolve_skill_urls(bad).is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn validates_text_payload_and_size_cap() {
        assert!(validate_text_payload(Some("text/plain; charset=utf-8"), b"# Skill\nSteps").is_ok());
        assert!(validate_text_payload(Some("text/markdown"), b"---\nname: Test\n---\nBody").is_ok());
        assert!(validate_text_payload(Some("text/html"), b"<html>nope</html>").is_err());
        assert!(validate_text_payload(Some("text/plain"), b"<!DOCTYPE html><html></html>").is_err());
        assert!(validate_text_payload(Some("image/png"), b"\x89PNG").is_err());
        assert!(validate_text_payload(None, b"abc\0def").is_err());
        assert!(validate_text_payload(Some("text/plain"), &vec![b'a'; MAX_FETCH_BYTES + 1]).is_err());
    }
}
