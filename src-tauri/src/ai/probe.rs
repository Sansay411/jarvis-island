//! Capability & context probing for local and custom OpenAI-compatible
//! models (Ollama, LM Studio, generic OpenAI servers).

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::LazyLock;
use std::time::Duration;

use base64::Engine;
use futures_util::future::join_all;
use image::{ImageFormat, Rgba, RgbaImage};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::types::{ChatMessage, Part, Role, ToolSpec};
use super::{is_local_url, OpenAiCompatible};
use crate::agent::{prompt, Heard};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::settings;
use crate::tools::Registry;

/// Headroom added to the static prompt + tool schema estimate so a reply and
/// one desktop screenshot fit in the context window.
pub const REPLY_ROOM_TOKENS: u64 = 2048;
pub const SCREENSHOT_ROOM_TOKENS: u64 = 1536;

/// Mixed Russian/English text and JSON schemas average ~3.5 chars per token
/// (Cyrillic runs ~2-3 chars/token, English and JSON keys ~4 chars/token).
pub const CHARS_PER_TOKEN: f64 = 3.5;

const META_TIMEOUT: Duration = Duration::from_secs(4);
const ACTIVE_TOOLS_TIMEOUT: Duration = Duration::from_secs(15);
const ACTIVE_VISION_TIMEOUT: Duration = Duration::from_secs(18);
const OLLAMA_DEFAULT_CTX: u64 = 4096;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    Yes,
    No,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Ok,
    Warn,
    Bad,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServerKind {
    Ollama,
    LmStudio,
    OpenAi,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextInfo {
    pub model_max: Option<u64>,
    pub loaded: Option<u64>,
    pub needed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelProbe {
    pub vision: Capability,
    pub tools: Capability,
    pub context: ContextInfo,
    pub verdict: Verdict,
    pub summary: String,
    pub notes: Vec<String>,
    pub server_kind: ServerKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: String,
    pub vision: Capability,
    pub tools: Capability,
    pub max_context: Option<u64>,
    pub loaded_context: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    pub suitable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelsListResult {
    pub server_kind: ServerKind,
    pub models: Vec<ModelOption>,
}

static PROBE_CACHE: LazyLock<Mutex<HashMap<(String, String), ModelProbe>>> = LazyLock::new(Default::default);

fn cache_key(base_url: &str, model: &str) -> (String, String) {
    (base_url.trim().trim_end_matches('/').to_lowercase(), model.trim().to_string())
}

pub fn cached_probe(base_url: &str, model: &str) -> Option<ModelProbe> {
    PROBE_CACHE.lock().get(&cache_key(base_url, model)).cloned()
}

pub fn store_cached_probe(base_url: &str, model: &str, probe: &ModelProbe) {
    PROBE_CACHE.lock().insert(cache_key(base_url, model), probe.clone());
}

/// Estimate tokens needed for JARVIS's real system prompt, turn context,
/// all active tool schemas, a reply, and one screenshot.
pub fn estimate_needed_tokens(system_prompt: &str, turn_context: &str, tools: &[ToolSpec]) -> u64 {
    let tools_json = serde_json::to_string(&super::openai::tool_wire_json(tools)).unwrap_or_default();
    let chars = system_prompt.chars().count() + turn_context.chars().count() + tools_json.chars().count();
    let base = (chars as f64 / CHARS_PER_TOKEN).ceil() as u64;
    base + REPLY_ROOM_TOKENS + SCREENSHOT_ROOM_TOKENS
}

/// Compute `needed` context tokens from the live app state and tool registry.
pub fn needed_tokens_for(state: &AppState, registry: &Registry, base_url: &str) -> u64 {
    let settings = state.store.settings();
    let memories = state.store.memories().unwrap_or_default();
    let mut specs = registry.specs();
    let supports_web = super::origin_of(base_url) == "https://api.deepseek.com" || crate::cloud::is_cloud(base_url);
    if !supports_web {
        specs.retain(|s| !matches!(s.name.as_str(), "web_search" | "web_fetch"));
    }
    let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
    let sys = prompt::system_prompt(&settings, &memories, &names, false);
    let ctx = prompt::turn_context(&settings, &state.history, Heard::Addressed);
    estimate_needed_tokens(&sys, &ctx, &specs)
}

/// Strip a trailing `/v1` or `/` to reach Ollama/LM Studio native REST endpoints.
pub fn native_api_root(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    trimmed.strip_suffix("/v1").unwrap_or(trimmed).trim_end_matches('/').to_string()
}

/// Compare two Ollama model names, treating an omitted tag as `:latest`.
pub fn ollama_name_matches(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s = s.trim().to_ascii_lowercase();
        if s.contains(':') { s } else { format!("{s}:latest") }
    };
    norm(a) == norm(b)
}

/// Extract `PARAMETER num_ctx <N>` from Ollama's `/api/show` `parameters` text.
pub fn parse_ollama_num_ctx(parameters: &str) -> Option<u64> {
    for line in parameters.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut parts = line.split_whitespace();
        if parts.next() == Some("num_ctx") {
            if let Some(val) = parts.next().and_then(|v| v.parse::<u64>().ok()) {
                if val > 0 {
                    return Some(val);
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaShowMeta {
    pub vision: Capability,
    pub tools: Capability,
    pub model_max: Option<u64>,
    pub num_ctx: Option<u64>,
}

/// Parse Ollama's `POST /api/show` JSON response.
pub fn parse_ollama_show(v: &Value) -> OllamaShowMeta {
    let caps: Option<Vec<String>> = v
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).map(|s| s.to_ascii_lowercase()).collect());

    let model_info = v.get("model_info").and_then(Value::as_object);
    let model_max = model_info.and_then(|m| {
        m.iter()
            .filter(|(k, _)| k.ends_with(".context_length") || *k == "context_length")
            .filter_map(|(_, val)| val.as_u64())
            .find(|&n| n > 0)
    });

    let has_vision_info = v.get("projector_info").is_some_and(|p| !p.is_null())
        || model_info.is_some_and(|m| m.keys().any(|k| k.contains(".vision.")));

    let (vision, tools) = match caps {
        Some(ref c) if !c.is_empty() => (
            if c.iter().any(|x| x == "vision") || has_vision_info { Capability::Yes } else { Capability::No },
            if c.iter().any(|x| x == "tools") { Capability::Yes } else { Capability::No },
        ),
        _ => (if has_vision_info { Capability::Yes } else { Capability::Unknown }, Capability::Unknown),
    };

    let num_ctx = v.get("parameters").and_then(Value::as_str).and_then(parse_ollama_num_ctx);
    OllamaShowMeta { vision, tools, model_max, num_ctx }
}

/// Parse Ollama's `GET /api/ps` response into `(name, context_length)` pairs.
pub fn parse_ollama_ps_list(v: &Value) -> Vec<(String, u64)> {
    v.get("models")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let name = m.get("model").or_else(|| m.get("name")).and_then(Value::as_str)?;
                    let ctx = m.get("context_length").and_then(Value::as_u64).filter(|&n| n > 0)?;
                    Some((name.to_string(), ctx))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_ollama_tags(v: &Value) -> Vec<String> {
    v.get("models")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("model").or_else(|| m.get("name")).and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Determine the context window Ollama will actually allocate for a model.
pub fn resolve_ollama_context(show: &OllamaShowMeta, ps_ctx: Option<u64>, env_ctx: Option<u64>) -> (Option<u64>, Option<String>) {
    if let Some(ctx) = ps_ctx.filter(|&n| n > 0) {
        return (Some(ctx), Some("ps".into()));
    }
    let clamp_max = |n: u64| show.model_max.map_or(n, |m| m.min(n));
    if let Some(n) = show.num_ctx.filter(|&n| n > 0) {
        return (Some(clamp_max(n)), Some("modelfile".into()));
    }
    if let Some(n) = env_ctx.filter(|&n| n > 0) {
        return (Some(clamp_max(n)), Some("env".into()));
    }
    (Some(clamp_max(OLLAMA_DEFAULT_CTX)), Some("default".into()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LmStudioModelMeta {
    pub id: String,
    pub vision: Capability,
    pub tools: Capability,
    pub max_context: Option<u64>,
    pub loaded_context: Option<u64>,
    pub state: Option<String>,
    pub is_embeddings: bool,
}

/// Parse LM Studio's `GET /api/v0/models` response.
pub fn parse_lmstudio_models(v: &Value) -> Vec<LmStudioModelMeta> {
    let Some(arr) = v.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str)?.to_string();
            let kind = m.get("type").and_then(Value::as_str).map(|s| s.to_ascii_lowercase());
            let state = m.get("state").and_then(Value::as_str).map(str::to_string);
            let max_context = m.get("max_context_length").and_then(Value::as_u64).filter(|&n| n > 0);
            let loaded_context = m.get("loaded_context_length").and_then(Value::as_u64).filter(|&n| n > 0);
            let caps: Option<Vec<String>> = m
                .get("capabilities")
                .and_then(Value::as_array)
                .map(|c| c.iter().filter_map(Value::as_str).map(|s| s.to_ascii_lowercase()).collect());

            let is_embeddings = kind.as_deref() == Some("embeddings");
            let vision = if is_embeddings {
                Capability::No
            } else if kind.as_deref() == Some("vlm") || caps.as_ref().is_some_and(|c| c.iter().any(|x| x == "vision")) {
                Capability::Yes
            } else if kind.as_deref() == Some("llm") {
                Capability::No
            } else {
                Capability::Unknown
            };
            let tools = if is_embeddings {
                Capability::No
            } else if let Some(c) = &caps {
                if c.iter().any(|x| x == "tool_use" || x == "tools") { Capability::Yes } else { Capability::No }
            } else {
                Capability::Unknown
            };

            Some(LmStudioModelMeta { id, vision, tools, max_context, loaded_context, state, is_embeddings })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiModelMeta {
    pub id: String,
    pub max_context: Option<u64>,
}

/// Parse standard OpenAI `GET /v1/models` response (plus optional vLLM `max_model_len`).
pub fn parse_openai_models(v: &Value) -> Vec<OpenAiModelMeta> {
    v.get("data")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m.get("id").and_then(Value::as_str)?.to_string();
                    let max_context = m
                        .get("max_model_len")
                        .or_else(|| m.get("context_length"))
                        .and_then(Value::as_u64)
                        .filter(|&n| n > 0);
                    Some(OpenAiModelMeta { id, max_context })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn format_k(tokens: u64) -> String {
    if tokens >= 1000 {
        if tokens % 1024 == 0 {
            format!("{}k", tokens / 1024)
        } else {
            format!("{}k", (tokens as f64 / 1000.0).round() as u64)
        }
    } else {
        tokens.to_string()
    }
}

/// Build the user-facing verdict, summary, and exact Russian remediation notes.
#[allow(clippy::too_many_arguments)]
pub fn build_probe_result(
    server_kind: ServerKind,
    vision: Capability,
    tools: Capability,
    model_max: Option<u64>,
    loaded: Option<u64>,
    needed: u64,
    source: Option<String>,
    extra_notes: Vec<String>,
) -> ModelProbe {
    let effective = loaded.or(model_max);
    let ctx_too_small = effective.is_some_and(|c| c < needed);
    let ctx_tight = effective.is_some_and(|c| c >= needed && c < needed + 2048);

    let verdict = if tools == Capability::No || ctx_too_small {
        Verdict::Bad
    } else if vision != Capability::Yes || tools != Capability::Yes || loaded.is_none() || ctx_tight {
        Verdict::Warn
    } else {
        Verdict::Ok
    };

    let mut notes = Vec::new();
    match tools {
        Capability::No => {
            notes.push("Эта модель не умеет вызывать инструменты — Джарвис сможет только болтать.".into());
        }
        Capability::Unknown => {
            notes.push("Сервер не сообщает, умеет ли модель вызывать инструменты. Нажмите «Проверить на деле».".into());
        }
        Capability::Yes => {}
    }

    match vision {
        Capability::No => {
            notes.push("Для вопросов про экран выберите модель с картинками (например Qwen-VL) в поле «Модель для экрана».".into());
        }
        Capability::Unknown => {
            notes.push("Сервер не сообщает, видит ли модель изображения. Нажмите «Проверить на деле».".into());
        }
        Capability::Yes => {}
    }

    if ctx_too_small {
        match server_kind {
            ServerKind::Ollama => {
                notes.push("В настройках Ollama поставьте Context length 32k или запустите с OLLAMA_CONTEXT_LENGTH=32768.".into());
            }
            ServerKind::LmStudio => {
                notes.push("Загрузите модель с Context Length 32k+.".into());
            }
            ServerKind::OpenAi => {
                notes.push(format!("Увеличьте лимит контекста на сервере до 32k+ (нужно минимум ~{}).", format_k(needed)));
            }
        }
    } else if loaded.is_none() {
        match server_kind {
            ServerKind::LmStudio => {
                notes.push("Модель сейчас не загружена в память. Загрузите модель с Context Length 32k+.".into());
            }
            ServerKind::OpenAi => {
                notes.push(format!("Сервер не сообщил размер контекста — убедитесь, что задано не меньше ~{} (лучше 32k+).", format_k(needed)));
            }
            ServerKind::Ollama => {}
        }
    } else if ctx_tight {
        notes.push("Контекста хватает впритык — длинные разговоры и скриншоты могут обрезаться, лучше поставить 32k+.".into());
    }

    for n in extra_notes {
        if !n.is_empty() && !notes.contains(&n) {
            notes.push(n);
        }
    }

    let summary = match verdict {
        Verdict::Ok => "Модель подходит: видит экран, вызывает инструменты и вмещает инструкции Джарвиса.".into(),
        Verdict::Bad if tools == Capability::No && ctx_too_small => {
            "Модель не подойдёт: нет вызова инструментов и слишком маленький контекст.".into()
        }
        Verdict::Bad if tools == Capability::No => {
            "Эта модель не умеет вызывать инструменты — Джарвис сможет только болтать.".into()
        }
        Verdict::Bad => format!(
            "Контекста {} не хватает для инструкций и инструментов (нужно ~{}).",
            effective.map(format_k).unwrap_or_default(),
            format_k(needed)
        ),
        Verdict::Warn if vision == Capability::No => {
            "Работает без экрана: команды и инструменты доступны, но картинки модель не видит.".into()
        }
        Verdict::Warn if loaded.is_none() && server_kind == ServerKind::LmStudio => {
            "Модель не загружена в LM Studio — проверьте Context Length при загрузке.".into()
        }
        Verdict::Warn if vision == Capability::Unknown || tools == Capability::Unknown => {
            "Часть возможностей неизвестна по метаданным — можно проверить на деле.".into()
        }
        Verdict::Warn => "Модель работает, но с небольшим запасом по контексту.".into(),
    };

    ModelProbe {
        vision,
        tools,
        context: ContextInfo { model_max, loaded, needed, source },
        verdict,
        summary,
        notes,
        server_kind,
    }
}

fn evaluate_option(
    id: String,
    vision: Capability,
    tools: Capability,
    max_context: Option<u64>,
    loaded_context: Option<u64>,
    state: Option<String>,
    is_embeddings: bool,
    needed: u64,
) -> ModelOption {
    let effective = loaded_context.or(max_context);
    let (suitable, reason) = if is_embeddings {
        (false, Some("Модель для эмбеддингов".into()))
    } else if tools == Capability::No {
        (false, Some("Не вызывает инструменты".into()))
    } else if effective.is_some_and(|c| c < needed) {
        (false, Some(format!("Контекст {} < ~{}", format_k(effective.unwrap()), format_k(needed))))
    } else if vision == Capability::No {
        (true, Some("Не видит экран".into()))
    } else {
        (true, None)
    };
    ModelOption { id, vision, tools, max_context, loaded_context, state, suitable, reason }
}

/// Tiny solid red 56x56 PNG data URL for active vision probing (at least 28px
/// per side so VLM patch encoders never reject it as too small).
pub fn tiny_red_png_data_url() -> String {
    static RED_PNG: LazyLock<String> = LazyLock::new(|| {
        let img = RgbaImage::from_pixel(56, 56, Rgba([220, 30, 30, 255]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .expect("encode tiny PNG");
        format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&buf))
    });
    RED_PNG.clone()
}

/// Interpret a model's answer to "What color is this image?" against our red PNG.
pub fn parse_color_vision_reply(text: &str) -> Capability {
    let lower = text.trim().to_ascii_lowercase();
    let lower_ru = text.trim().to_lowercase();
    if lower.is_empty() {
        return Capability::Unknown;
    }
    const RED_WORDS: &[&str] = &["red", "crimson", "scarlet", "vermilion", "ruby", "красн", "алый", "алого", "алым", "багр", "рубин"];
    if RED_WORDS.iter().any(|w| lower.contains(w) || lower_ru.contains(w)) {
        return Capability::Yes;
    }
    Capability::No
}

fn looks_like_unsupported_feature(err: &str, keywords: &[&str]) -> bool {
    let l = err.to_lowercase();
    keywords.iter().any(|k| l.contains(k))
}

async fn fetch_json(client: &reqwest::Client, url: &str, bearer: Option<&str>, timeout: Duration) -> Result<Value, reqwest::Error> {
    let mut req = client.get(url).timeout(timeout);
    if let Some(k) = bearer {
        req = req.bearer_auth(k);
    }
    let resp = req.send().await?.error_for_status()?;
    resp.json::<Value>().await
}

async fn post_json(client: &reqwest::Client, url: &str, body: &Value, timeout: Duration) -> Result<(reqwest::StatusCode, Value), reqwest::Error> {
    let resp = client.post(url).json(body).timeout(timeout).send().await?;
    let status = resp.status();
    let val = resp.json::<Value>().await.unwrap_or(Value::Null);
    Ok((status, val))
}

fn env_ollama_context_length(root: &str) -> Option<u64> {
    if !is_local_url(root) {
        return None;
    }
    std::env::var("OLLAMA_CONTEXT_LENGTH").ok().and_then(|s| s.trim().parse::<u64>().ok()).filter(|&n| n > 0)
}

async fn probe_ollama(http: &reqwest::Client, root: &str, model: &str, needed: u64) -> AppResult<ModelProbe> {
    let show_url = format!("{root}/api/show");
    let ps_url = format!("{root}/api/ps");
    let show_body = json!({ "model": model });
    let (show_res, ps_res) = tokio::join!(
        post_json(http, &show_url, &show_body, META_TIMEOUT),
        fetch_json(http, &ps_url, None, META_TIMEOUT),
    );
    let (status, show_json) = show_res.map_err(AppError::from)?;
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::not_found(format!("Модель «{model}» не найдена в Ollama")));
    }
    if !status.is_success() {
        return Err(AppError::Provider(format!("Ollama вернула HTTP {}", status.as_u16())));
    }
    let show = parse_ollama_show(&show_json);
    let ps_list = ps_res.map(|v| parse_ollama_ps_list(&v)).unwrap_or_default();
    let ps_ctx = ps_list.iter().find(|(n, _)| ollama_name_matches(n, model)).map(|(_, c)| *c);
    let env_ctx = env_ollama_context_length(root);
    let (loaded, source) = resolve_ollama_context(&show, ps_ctx, env_ctx);

    Ok(build_probe_result(ServerKind::Ollama, show.vision, show.tools, show.model_max, loaded, needed, source, Vec::new()))
}

async fn probe_lmstudio(http: &reqwest::Client, root: &str, model: &str, needed: u64) -> AppResult<Option<ModelProbe>> {
    let url = format!("{root}/api/v0/models");
    let val = match fetch_json(http, &url, None, META_TIMEOUT).await {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let models = parse_lmstudio_models(&val);
    if models.is_empty() && val.get("data").is_none() {
        return Ok(None);
    }
    let found = models
        .iter()
        .find(|m| m.id == model)
        .or_else(|| models.iter().find(|m| m.id.eq_ignore_ascii_case(model)));
    let Some(m) = found else {
        return Err(AppError::not_found(format!("Модель «{model}» не найдена в LM Studio")));
    };
    let mut extra = Vec::new();
    if m.is_embeddings {
        extra.push("Это модель эмбеддингов: она не отвечает на сообщения и не работает как агент.".into());
    }
    let source = if m.loaded_context.is_some() { Some("lmstudio".into()) } else { Some("not_loaded".into()) };
    Ok(Some(build_probe_result(
        ServerKind::LmStudio,
        m.vision,
        m.tools,
        m.max_context,
        m.loaded_context,
        needed,
        source,
        extra,
    )))
}

async fn active_probe_tools(client: &OpenAiCompatible) -> (Capability, Option<String>) {
    let dummy = ToolSpec {
        name: "ping".into(),
        description: "Echo a word back.".into(),
        parameters: json!({
            "type": "object",
            "properties": { "word": { "type": "string" } },
            "required": ["word"],
            "additionalProperties": false
        }),
    };
    let msgs = [ChatMessage::text(
        Role::User,
        "Call the ping tool with word=\"ok\". Do not reply with text, only call the tool.",
    )];
    let cancel = CancellationToken::new();
    let res = tokio::time::timeout(ACTIVE_TOOLS_TIMEOUT, client.complete(&msgs, &[dummy], &cancel, Some(64), |_| {})).await;
    match res {
        Ok(Ok(c)) if !c.message.tool_calls.is_empty() => (Capability::Yes, None),
        Ok(Ok(_)) => (Capability::No, Some("Активная проверка: модель ответила текстом и не вызвала тестовый инструмент.".into())),
        Ok(Err(e)) => {
            let msg = e.to_string();
            if looks_like_unsupported_feature(&msg, &["tool", "function"]) {
                (Capability::No, Some("Активная проверка: сервер отклонил запрос с инструментами.".into()))
            } else {
                (Capability::Unknown, Some(format!("Не удалось проверить инструменты: {msg}")))
            }
        }
        Err(_) => (Capability::Unknown, Some("Проверка инструментов не уложилась в 15 с.".into())),
    }
}

async fn active_probe_vision(client: &OpenAiCompatible) -> (Capability, Option<String>) {
    let msgs = [ChatMessage {
        role: Role::User,
        parts: vec![
            Part::Text { text: "What color is this image? Reply with one word.".into() },
            Part::Image { data_url: tiny_red_png_data_url() },
        ],
        tool_calls: Vec::new(),
        tool_call_id: None,
    }];
    let cancel = CancellationToken::new();
    let res = tokio::time::timeout(ACTIVE_VISION_TIMEOUT, client.complete(&msgs, &[], &cancel, Some(32), |_| {})).await;
    match res {
        Ok(Ok(c)) => {
            let reply = c.message.joined_text();
            match parse_color_vision_reply(&reply) {
                Capability::Yes => (Capability::Yes, None),
                Capability::No => {
                    let preview: String = reply.trim().chars().take(60).collect();
                    (Capability::No, Some(format!("Активная проверка экрана: модель не распознала красный квадрат («{preview}»).")))
                }
                Capability::Unknown => (Capability::Unknown, Some("Активная проверка экрана вернула пустой ответ.".into())),
            }
        }
        Ok(Err(e)) => {
            let msg = e.to_string();
            if looks_like_unsupported_feature(&msg, &["image", "vision", "multimodal", "content must be a string", "unsupported"]) {
                (Capability::No, Some("Активная проверка: модель или сервер не принимает изображения.".into()))
            } else {
                (Capability::Unknown, Some(format!("Не удалось проверить зрение: {msg}")))
            }
        }
        Err(_) => (Capability::Unknown, Some("Проверка зрения не уложилась в 18 с.".into())),
    }
}

/// Probe a local or custom model's capabilities and context window.
/// Results are cached per `(base_url, model)` for the session unless `refresh`
/// or `active` is requested.
pub async fn probe_model(
    state: &AppState,
    registry: &Registry,
    preset_id: &str,
    base_url: &str,
    model: &str,
    active: bool,
    refresh: bool,
) -> AppResult<ModelProbe> {
    let model = model.trim();
    if base_url.trim().is_empty() {
        return Err(AppError::invalid("Укажите адрес API сервера"));
    }
    if model.is_empty() {
        return Err(AppError::invalid("Выберите или введите название модели"));
    }
    let base_url = base_url.trim().trim_end_matches('/');

    if !active && !refresh {
        if let Some(hit) = cached_probe(base_url, model) {
            return Ok(hit);
        }
    }

    let needed = needed_tokens_for(state, registry, base_url);
    let http = super::openai::http_client();
    let root = native_api_root(base_url);
    let key = super::resolve_key_for_url(preset_id, base_url, &state.store, false)?;

    let mut result = match preset_id {
        "ollama" => probe_ollama(&http, &root, model, needed).await?,
        "lmstudio" => match probe_lmstudio(&http, &root, model, needed).await? {
            Some(p) => p,
            None => return Err(AppError::Network("Не удалось подключиться к LM Studio — проверьте, что сервер запущен.".into())),
        },
        _ => {
            // Custom preset: auto-detect LM Studio or Ollama before falling back to generic OpenAI.
            if let Some(p) = probe_lmstudio(&http, &root, model, needed).await? {
                p
            } else if let Ok(p) = probe_ollama(&http, &root, model, needed).await {
                p
            } else {
                let models_url = format!("{base_url}/models");
                let meta = fetch_json(&http, &models_url, key.as_deref(), META_TIMEOUT)
                    .await
                    .map_err(AppError::from)
                    .map(|v| parse_openai_models(&v))?;
                let model_max = meta
                    .iter()
                    .find(|m| m.id == model || m.id.eq_ignore_ascii_case(model))
                    .and_then(|m| m.max_context);
                build_probe_result(ServerKind::OpenAi, Capability::Unknown, Capability::Unknown, model_max, None, needed, None, Vec::new())
            }
        }
    };

    if active {
        let preset = settings::preset(preset_id);
        let client = OpenAiCompatible::new(base_url.to_string(), key, model.to_string(), preset.dashscope, 0.0);
        let need_tools = result.tools == Capability::Unknown;
        let need_vision = result.vision == Capability::Unknown;
        let (tools_res, vision_res) = tokio::join!(
            async { if need_tools { Some(active_probe_tools(&client).await) } else { None } },
            async { if need_vision { Some(active_probe_vision(&client).await) } else { None } },
        );
        let mut extra = Vec::new();
        if let Some((cap, note)) = tools_res {
            result.tools = cap;
            if let Some(n) = note { extra.push(n); }
        }
        if let Some((cap, note)) = vision_res {
            result.vision = cap;
            if let Some(n) = note { extra.push(n); }
        }
        result = build_probe_result(
            result.server_kind,
            result.vision,
            result.tools,
            result.context.model_max,
            result.context.loaded,
            needed,
            result.context.source,
            extra,
        );
    }

    store_cached_probe(base_url, model, &result);
    Ok(result)
}

/// Fetch installed/available models from a local or custom server with
/// capability metadata when available.
pub async fn list_models(
    state: &AppState,
    registry: &Registry,
    preset_id: &str,
    base_url: &str,
) -> AppResult<ModelsListResult> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err(AppError::invalid("Укажите адрес API сервера"));
    }
    let needed = needed_tokens_for(state, registry, base_url);
    let http = super::openai::http_client();
    let root = native_api_root(base_url);
    let key = super::resolve_key_for_url(preset_id, base_url, &state.store, false)?;

    if preset_id == "lmstudio" || (preset_id == "custom" && root.contains(":1234")) {
        if let Ok(val) = fetch_json(&http, &format!("{root}/api/v0/models"), None, META_TIMEOUT).await {
            let parsed = parse_lmstudio_models(&val);
            if !parsed.is_empty() || val.get("data").is_some() {
                let models = parsed
                    .into_iter()
                    .map(|m| evaluate_option(m.id, m.vision, m.tools, m.max_context, m.loaded_context, m.state, m.is_embeddings, needed))
                    .collect();
                return Ok(ModelsListResult { server_kind: ServerKind::LmStudio, models });
            }
        }
    }

    if preset_id == "ollama" || preset_id == "custom" {
        if let Ok(tags_val) = fetch_json(&http, &format!("{root}/api/tags"), None, META_TIMEOUT).await {
            let names = parse_ollama_tags(&tags_val);
            let ps_list = fetch_json(&http, &format!("{root}/api/ps"), None, META_TIMEOUT)
                .await
                .map(|v| parse_ollama_ps_list(&v))
                .unwrap_or_default();
            let env_ctx = env_ollama_context_length(&root);
            let show_futs = names.into_iter().take(30).map(|name| {
                let http = http.clone();
                let show_url = format!("{root}/api/show");
                let ps_ctx = ps_list.iter().find(|(n, _)| ollama_name_matches(n, &name)).map(|(_, c)| *c);
                async move {
                    let show = post_json(&http, &show_url, &json!({ "model": name }), META_TIMEOUT)
                        .await
                        .ok()
                        .filter(|(s, _)| s.is_success())
                        .map(|(_, v)| parse_ollama_show(&v));
                    match show {
                        Some(s) => {
                            let (loaded, _) = resolve_ollama_context(&s, ps_ctx, env_ctx);
                            evaluate_option(name, s.vision, s.tools, s.model_max, loaded, ps_ctx.map(|_| "loaded".into()), false, needed)
                        }
                        None => evaluate_option(name, Capability::Unknown, Capability::Unknown, None, ps_ctx, None, false, needed),
                    }
                }
            });
            let models = join_all(show_futs).await;
            return Ok(ModelsListResult { server_kind: ServerKind::Ollama, models });
        }
    }

    if preset_id == "custom" {
        if let Ok(val) = fetch_json(&http, &format!("{root}/api/v0/models"), None, META_TIMEOUT).await {
            let parsed = parse_lmstudio_models(&val);
            if !parsed.is_empty() {
                let models = parsed
                    .into_iter()
                    .map(|m| evaluate_option(m.id, m.vision, m.tools, m.max_context, m.loaded_context, m.state, m.is_embeddings, needed))
                    .collect();
                return Ok(ModelsListResult { server_kind: ServerKind::LmStudio, models });
            }
        }
    }

    let val = fetch_json(&http, &format!("{base_url}/models"), key.as_deref(), META_TIMEOUT).await?;
    let models = parse_openai_models(&val)
        .into_iter()
        .map(|m| evaluate_option(m.id, Capability::Unknown, Capability::Unknown, m.max_context, None, None, false, needed))
        .collect();
    Ok(ModelsListResult { server_kind: ServerKind::OpenAi, models })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::prompt;
    use crate::store::settings::Settings;

    #[test]
    fn native_api_root_strips_v1_and_slashes() {
        assert_eq!(native_api_root("http://localhost:11434/v1"), "http://localhost:11434");
        assert_eq!(native_api_root("http://localhost:11434/v1/"), "http://localhost:11434");
        assert_eq!(native_api_root("http://localhost:1234"), "http://localhost:1234");
        assert_eq!(native_api_root("https://host/ollama/v1"), "https://host/ollama");
    }

    #[test]
    fn ollama_show_parses_capabilities_context_and_modelfile_num_ctx() {
        let sample = json!({
            "parameters": "num_ctx                        16384\ntemperature                    0.3\nstop                           \"<|im_end|>\"",
            "details": { "family": "qwen25vl", "parameter_size": "7.6B", "quantization_level": "Q4_K_M" },
            "model_info": {
                "general.architecture": "qwen25vl",
                "qwen25vl.context_length": 32768,
                "qwen25vl.vision.image_size": 448
            },
            "capabilities": ["completion", "vision", "tools"]
        });
        let meta = parse_ollama_show(&sample);
        assert_eq!(meta.vision, Capability::Yes);
        assert_eq!(meta.tools, Capability::Yes);
        assert_eq!(meta.model_max, Some(32768));
        assert_eq!(meta.num_ctx, Some(16384));
    }

    #[test]
    fn ollama_show_handles_text_only_without_tools_and_ps_context() {
        let show_json = json!({
            "parameters": "stop \"<|endoftext|>\"",
            "model_info": {
                "general.architecture": "llama",
                "llama.context_length": 131072
            },
            "capabilities": ["completion"]
        });
        let ps_json = json!({
            "models": [
                {
                    "name": "llama3.1:8b",
                    "model": "llama3.1:8b",
                    "size": 5137025024u64,
                    "context_length": 8192
                }
            ]
        });
        let show = parse_ollama_show(&show_json);
        assert_eq!(show.vision, Capability::No);
        assert_eq!(show.tools, Capability::No);
        assert_eq!(show.model_max, Some(131072));
        assert_eq!(show.num_ctx, None);

        let ps = parse_ollama_ps_list(&ps_json);
        let ps_ctx = ps.iter().find(|(n, _)| ollama_name_matches(n, "llama3.1:8b")).map(|(_, c)| *c);
        assert_eq!(resolve_ollama_context(&show, ps_ctx, None), (Some(8192), Some("ps".into())));
        // Not loaded in ps, env OLLAMA_CONTEXT_LENGTH set vs unset default 4096.
        assert_eq!(resolve_ollama_context(&show, None, Some(32768)), (Some(32768), Some("env".into())));
        assert_eq!(resolve_ollama_context(&show, None, None), (Some(4096), Some("default".into())));
    }

    #[test]
    fn lmstudio_models_parses_vlm_llm_and_embeddings() {
        let sample = json!({
            "object": "list",
            "data": [
                {
                    "id": "qwen2.5-vl-7b-instruct",
                    "object": "model",
                    "type": "vlm",
                    "state": "loaded",
                    "max_context_length": 32768,
                    "loaded_context_length": 16384,
                    "capabilities": ["tool_use"]
                },
                {
                    "id": "qwen2.5-7b-instruct",
                    "object": "model",
                    "type": "llm",
                    "state": "not-loaded",
                    "max_context_length": 32768,
                    "capabilities": []
                },
                {
                    "id": "text-embedding-nomic-embed-text-v1.5",
                    "object": "model",
                    "type": "embeddings",
                    "state": "not-loaded",
                    "max_context_length": 2048
                }
            ]
        });
        let list = parse_lmstudio_models(&sample);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].vision, Capability::Yes);
        assert_eq!(list[0].tools, Capability::Yes);
        assert_eq!(list[0].loaded_context, Some(16384));

        assert_eq!(list[1].vision, Capability::No);
        assert_eq!(list[1].tools, Capability::No);
        assert_eq!(list[1].loaded_context, None);

        assert!(list[2].is_embeddings);
        assert_eq!(list[2].vision, Capability::No);
        assert_eq!(list[2].tools, Capability::No);
    }

    #[test]
    fn verdict_and_notes_cover_ok_warn_and_bad_cases() {
        let ok = build_probe_result(ServerKind::Ollama, Capability::Yes, Capability::Yes, Some(32768), Some(32768), 14000, Some("ps".into()), vec![]);
        assert_eq!(ok.verdict, Verdict::Ok);
        assert!(ok.notes.is_empty());

        let no_vision = build_probe_result(ServerKind::Ollama, Capability::No, Capability::Yes, Some(32768), Some(32768), 14000, Some("ps".into()), vec![]);
        assert_eq!(no_vision.verdict, Verdict::Warn);
        assert!(no_vision.notes.iter().any(|n| n.contains("Модель для экрана")));

        let small_ollama = build_probe_result(ServerKind::Ollama, Capability::Yes, Capability::Yes, Some(32768), Some(4096), 14000, Some("default".into()), vec![]);
        assert_eq!(small_ollama.verdict, Verdict::Bad);
        assert!(small_ollama.notes.iter().any(|n| n.contains("OLLAMA_CONTEXT_LENGTH=32768")));

        let no_tools_lms = build_probe_result(ServerKind::LmStudio, Capability::Yes, Capability::No, Some(32768), Some(8192), 14000, Some("lmstudio".into()), vec![]);
        assert_eq!(no_tools_lms.verdict, Verdict::Bad);
        assert!(no_tools_lms.notes.iter().any(|n| n.contains("только болтать")));
        assert!(no_tools_lms.notes.iter().any(|n| n.contains("Context Length 32k+")));
    }

    #[test]
    fn token_estimate_from_real_prompt_and_tools_is_realistic() {
        let specs = crate::tools::builtin_specs();
        let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
        let settings = Settings::default();
        let sys = prompt::system_prompt(&settings, &[], &names, false);
        let est = estimate_needed_tokens(&sys, "[Контекст запроса]\n- Local time: 12:00", &specs);
        assert!(est >= 8_000 && est <= 25_000, "unexpected needed token estimate: {est}");
    }

    #[test]
    fn color_vision_reply_detects_red_in_ru_and_en() {
        assert_eq!(parse_color_vision_reply("Red"), Capability::Yes);
        assert_eq!(parse_color_vision_reply("Красный квадрат."), Capability::Yes);
        assert_eq!(parse_color_vision_reply("Я не могу видеть изображения."), Capability::No);
        assert_eq!(parse_color_vision_reply(""), Capability::Unknown);
    }
}
