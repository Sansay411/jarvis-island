//! Connect external services from the built-in MCP catalog by asking in chat
//! or voice. Secrets never pass through the model or chat history: they are
//! collected via the island's secure key prompt and written straight to
//! Credential Manager through [`crate::mcp::Hub`].

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::Manager;

use crate::error::{AppError, AppResult};
use crate::mcp::catalog::{self, CatalogEntry};
use crate::mcp::{Hub, SecretFieldPrompt, SecretPromptPayload, ServerView, State};
use crate::tools::{handler, low, object, parse, str_arg, Risk, Tool, ToolCtx, ToolOutput};

const SECRET_PROMPT_TIMEOUT: Duration = Duration::from_secs(180);

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "connect_find",
            description: "Поиск сервиса во встроенном каталоге подключений (MCP) по названию, алиасу или описанию на русском и английском. Вызывай перед connect_service, когда пользователь просит подключить или интегрировать сервис (например, Notion, GitHub, Slack, Obsidian, умный дом). Возвращает кандидатов с описанием доступа и списком параметров (с пометкой, какие из них секретные).",
            schema: object(
                json!({
                    "query": { "type": "string", "description": "Название сервиса или задача (например: «Notion», «гитхаб», «умный дом», «телега»)" }
                }),
                &["query"],
            ),
            risk: low,
            title: |a| {
                let q = str_arg(a, "query").trim();
                if q.is_empty() {
                    "Поиск сервиса для подключения".into()
                } else {
                    format!("Поиск в каталоге: {q}")
                }
            },
            timeout: Duration::from_secs(10),
            parallel_safe: true,
            handler: handler(connect_find),
        },
        Tool {
            name: "connect_service",
            description: "Подключить сервис из встроенного каталога MCP по его id (из connect_find). Всегда требует подтверждения пользователя. В `inputs` передавай ТОЛЬКО несекретные параметры (например, `path` или `url`); секретные ключи и токены передавать нельзя — инструмент сам откроет защищённое окно ввода ключа в острове, сохранит его в диспетчер учётных данных Windows, запустит сервер и вернёт список новых возможностей.",
            schema: object(
                json!({
                    "id": { "type": "string", "description": "Идентификатор сервиса из каталога (например: notion, github, home-assistant)" },
                    "inputs": {
                        "type": "object",
                        "description": "Только несекретные параметры (например, path или url). Никогда не передавай сюда API-ключи или токены.",
                        "additionalProperties": { "type": "string" }
                    }
                }),
                &["id"],
            ),
            risk: |_| Risk::High,
            title: connect_title,
            timeout: Duration::from_secs(360),
            parallel_safe: false,
            handler: handler(connect_service),
        },
        Tool {
            name: "connections_list",
            description: "Показать список уже подключённых MCP-сервисов, их статус и доступные инструменты.",
            schema: object(json!({}), &[]),
            risk: low,
            title: |_| "Подключённые сервисы".into(),
            timeout: Duration::from_secs(10),
            parallel_safe: true,
            handler: handler(connections_list),
        },
        Tool {
            name: "connection_remove",
            description: "Отключить и удалить подключённый MCP-сервис по его id (из connections_list), включая его сохранённые ключи.",
            schema: object(
                json!({
                    "id": { "type": "string", "description": "Идентификатор подключённого сервиса" }
                }),
                &["id"],
            ),
            risk: |_| Risk::Moderate,
            title: |a| {
                let id = str_arg(a, "id").trim();
                match catalog::by_id(id) {
                    Some(e) => format!("Отключить {}", e.name),
                    None if !id.is_empty() => format!("Отключить «{id}»"),
                    None => "Отключить сервис".into(),
                }
            },
            timeout: Duration::from_secs(10),
            parallel_safe: false,
            handler: handler(connection_remove),
        },
    ]
}

fn connect_title(args: &Value) -> String {
    let id = str_arg(args, "id").trim();
    match catalog::by_id(id) {
        Some(e) => format!("Подключить {}", e.name),
        None if !id.is_empty() => format!("Подключить «{id}»"),
        None => "Подключить сервис".into(),
    }
}

/// Human-readable confirmation details for `connect_service` in the island UI.
pub fn confirmation_details(args: &Value) -> Option<String> {
    let id = str_arg(args, "id").trim();
    let entry = catalog::by_id(id)?;
    let mut lines = vec![entry.description.clone()];
    if let Some(needs) = &entry.needs {
        let label = match needs.as_str() {
            "node" => "Требуется Node.js (npx)",
            "uv" => "Требуется uv (Python)",
            "app" => "Требуется установленная программа",
            other => other,
        };
        lines.push(label.to_string());
    }
    if let Some(note) = &entry.note {
        lines.push(note.clone());
    }
    if let Some(inputs) = args.get("inputs").and_then(Value::as_object) {
        let non_secret_keys: Vec<&str> = entry.non_secret_fields().iter().map(|f| f.key.as_str()).collect();
        for (k, v) in inputs {
            if non_secret_keys.contains(&k.as_str()) {
                if let Some(s) = v.as_str().filter(|s| !s.trim().is_empty()) {
                    lines.push(format!("{k}: {}", s.trim()));
                }
            }
        }
    }
    let secrets = entry.secret_fields();
    if !secrets.is_empty() {
        let labels: Vec<&str> = secrets.iter().map(|f| f.label.as_str()).collect();
        lines.push(format!("Ключ ({}) запрошу отдельно в защищённом окне.", labels.join(", ")));
    }
    Some(lines.join("\n"))
}

#[derive(Deserialize)]
struct FindArgs {
    query: String,
}

async fn connect_find(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: FindArgs = parse(args)?;
    let query = a.query.trim();
    if query.is_empty() {
        return Err(AppError::invalid("Укажите название сервиса для поиска"));
    }
    let hub = ctx.app.state::<Arc<Hub>>();
    let hits = catalog::search(query, 5);
    if hits.is_empty() {
        return Ok(ToolOutput::ok(
            format!("«{query}» нет во встроенном каталоге подключений"),
            json!({
                "query": query,
                "notFound": true,
                "candidates": [],
                "hint": "Сервиса нет во встроенном каталоге. Пользователь может добавить любой MCP-сервер вручную в Настройки → «Подключения»."
            }),
        ));
    }

    let candidates: Vec<Value> = hits
        .iter()
        .map(|h| format_candidate(h.entry, hub.has_server(&h.entry.id)))
        .collect();
    let top_name = hits[0].entry.name.as_str();
    let summary = if hits.len() == 1 {
        format!("Нашёл в каталоге: {top_name}")
    } else {
        format!("Нашёл {} варианта, лучший — {top_name}", hits.len())
    };
    Ok(ToolOutput::ok(summary, json!({ "query": query, "notFound": false, "candidates": candidates })))
}

fn format_candidate(entry: &CatalogEntry, already_connected: bool) -> Value {
    let inputs: Vec<Value> = entry
        .fields
        .iter()
        .map(|f| {
            json!({
                "key": f.key,
                "label": f.label,
                "hint": f.hint,
                "link": f.link,
                "placeholder": f.placeholder,
                "initial": f.initial,
                "optional": f.optional,
                "secret": f.is_secret(),
            })
        })
        .collect();
    let has_secrets = entry.fields.iter().any(|f| f.is_secret());
    json!({
        "id": entry.id,
        "name": entry.name,
        "category": entry.category,
        "description": entry.description,
        "needs": entry.needs,
        "note": entry.note,
        "alreadyConnected": already_connected,
        "hasSecrets": has_secrets,
        "inputs": inputs,
    })
}

#[derive(Deserialize)]
struct ConnectArgs {
    id: String,
    #[serde(default)]
    inputs: BTreeMap<String, String>,
}

async fn connect_service(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: ConnectArgs = parse(args)?;
    let id = a.id.trim();
    let Some(entry) = catalog::by_id(id) else {
        return Ok(ToolOutput::fail(
            format!("Сервиса «{id}» нет во встроенном каталоге"),
            json!({
                "id": id,
                "notFound": true,
                "hint": "Сначала вызови connect_find. Если сервиса нет в каталоге, предложи открыть Настройки → «Подключения» для ручного добавления."
            }),
        ));
    };

    // Strip any secret field keys if the caller tried to pass them through the model.
    let mut non_secret_only = BTreeMap::new();
    for f in entry.non_secret_fields() {
        if let Some(v) = a.inputs.get(&f.key) {
            non_secret_only.insert(f.key.clone(), v.clone());
        }
    }

    let resolved_non_secret = match catalog::resolve_non_secret_inputs(entry, &non_secret_only) {
        Ok(map) => map,
        Err(missing) => {
            let list: Vec<Value> = missing
                .iter()
                .map(|f| {
                    json!({
                        "key": f.key,
                        "label": f.label,
                        "hint": f.hint,
                        "link": f.link,
                        "placeholder": f.placeholder,
                    })
                })
                .collect();
            let labels: Vec<&str> = missing.iter().map(|f| f.label.as_str()).collect();
            return Ok(ToolOutput::fail(
                format!("Для {} нужно указать: {}", entry.name, labels.join(", ")),
                json!({ "id": entry.id, "missingInputs": list }),
            ));
        }
    };

    let hub = ctx.app.state::<Arc<Hub>>().inner().clone();
    let secret_fields = entry.secret_fields();
    let secret_inputs = if secret_fields.is_empty() {
        BTreeMap::new()
    } else {
        (ctx.progress)("Жду ключ в защищённом окне…");
        let payload = SecretPromptPayload {
            prompt_id: uuid::Uuid::new_v4().to_string(),
            service_id: entry.id.clone(),
            service_name: entry.name.clone(),
            description: entry.description.clone(),
            note: entry.note.clone(),
            fields: secret_fields
                .iter()
                .map(|f| SecretFieldPrompt {
                    key: f.key.clone(),
                    label: f.label.clone(),
                    hint: f.hint.clone(),
                    link: f.link.clone(),
                    placeholder: f.placeholder.clone(),
                    optional: f.optional,
                })
                .collect(),
        };
        match hub.request_secrets(payload, &ctx.cancel, SECRET_PROMPT_TIMEOUT).await {
            Ok(Some(values)) => values,
            Ok(None) => {
                return Ok(ToolOutput::fail(
                    format!("Подключение {} отменено пользователем", entry.name),
                    json!({ "id": entry.id, "cancelled": true }),
                ));
            }
            Err(AppError::Timeout(_)) => {
                return Ok(ToolOutput::fail(
                    format!("Время ожидания ключа для {} истекло — попробуйте снова", entry.name),
                    json!({ "id": entry.id, "timedOut": true }),
                ));
            }
            Err(e) => return Err(e),
        }
    };

    // Keep track of raw secret values so we can guarantee none ever leak into output.
    let mut raw_secrets: Vec<String> = secret_inputs.values().cloned().collect();
    let (config, secrets) = catalog::build_server_config(entry, &resolved_non_secret, &secret_inputs)?;
    raw_secrets.extend(secrets.env.values().cloned());
    raw_secrets.extend(secrets.headers.values().cloned());

    (ctx.progress)(&format!("Запускаю {} и читаю инструменты…", entry.name));
    let previous_id = hub.has_server(&entry.id).then(|| entry.id.clone());
    if let Err(e) = hub.upsert(config, Some(secrets), previous_id).await {
        let msg = catalog::redact_secrets(&e.to_string(), &raw_secrets);
        return Ok(ToolOutput::fail(
            format!("Не удалось сохранить {}: {msg}", entry.name),
            json!({ "id": entry.id, "error": msg, "hint": failure_hint(entry, &msg) }),
        ));
    }

    let view = hub.view(&entry.id);
    Ok(format_connect_outcome(entry, view.as_ref(), &raw_secrets))
}

fn failure_hint(entry: &CatalogEntry, err: &str) -> String {
    let low = err.to_lowercase();
    if low.contains("401") || low.contains("403") || low.contains("unauthorized") || low.contains("forbidden") || low.contains("invalid_auth") || low.contains("token") {
        return "Проверьте токен и его права доступа, затем попробуйте подключить ещё раз или обновить ключ в Настройках → «Подключения».".into();
    }
    if entry.needs.as_deref() == Some("node") && (low.contains("npx") || low.contains("node") || low.contains("не найден") || low.contains("not found")) {
        return "Установите Node.js (nodejs.org) и повторите подключение.".into();
    }
    if entry.needs.as_deref() == Some("uv") && (low.contains("uvx") || low.contains("uv") || low.contains("не найден") || low.contains("not found")) {
        return "Установите uv (docs.astral.sh/uv) и повторите подключение.".into();
    }
    if let Some(note) = &entry.note {
        return format!("{note} Также можно проверить настройки сервера в Настройки → «Подключения».");
    }
    "Сервер добавлен в Настройки → «Подключения» — там можно посмотреть подробности и нажать «Проверить».".into()
}

fn format_connect_outcome(entry: &CatalogEntry, view: Option<&ServerView>, secret_values: &[String]) -> ToolOutput {
    let Some(v) = view else {
        return ToolOutput::fail(
            format!("Сервер {} не найден после сохранения", entry.name),
            json!({ "id": entry.id }),
        );
    };
    if v.state == State::Failed || v.error.is_some() {
        let raw_err = v.error.as_deref().unwrap_or("Сервер не ответил");
        let clean_err = catalog::redact_secrets(raw_err, secret_values);
        let hint = failure_hint(entry, &clean_err);
        return ToolOutput::fail(
            format!("{} не удалось запустить: {clean_err}", entry.name),
            json!({
                "id": entry.id,
                "name": entry.name,
                "state": v.state,
                "error": clean_err,
                "hint": hint,
            }),
        );
    }

    let tools: Vec<Value> = v
        .tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "title": catalog::redact_secrets(&t.title, secret_values),
                "description": catalog::redact_secrets(&t.description, secret_values),
                "risk": t.risk,
            })
        })
        .collect();
    let count = tools.len();
    let summary = if count == 0 {
        format!("{} подключён, но сервер не вернул список инструментов", entry.name)
    } else {
        format!("{} подключён — доступно {} {}", entry.name, count, plural_tools(count))
    };
    ToolOutput::ok(
        summary,
        json!({
            "id": entry.id,
            "name": entry.name,
            "serverInfo": catalog::redact_secrets(&v.server_info, secret_values),
            "toolCount": count,
            "tools": tools,
        }),
    )
}

fn plural_tools(n: usize) -> &'static str {
    let d = n % 10;
    let h = n % 100;
    if d == 1 && h != 11 {
        "инструмент"
    } else if (2..=4).contains(&d) && !(12..=14).contains(&h) {
        "инструмента"
    } else {
        "инструментов"
    }
}

async fn connections_list(ctx: ToolCtx, _args: Value) -> AppResult<ToolOutput> {
    let hub = ctx.app.state::<Arc<Hub>>();
    let views = hub.views();
    Ok(format_connections_list(&views))
}

fn format_connections_list(views: &[ServerView]) -> ToolOutput {
    if views.is_empty() {
        return ToolOutput::ok(
            "Подключённых сервисов пока нет",
            json!({ "count": 0, "connections": [] }),
        );
    }
    let items: Vec<Value> = views
        .iter()
        .map(|v| {
            let enabled_tools = v.tools.iter().filter(|t| t.enabled).count();
            let tool_names: Vec<&str> = v.tools.iter().filter(|t| t.enabled).take(20).map(|t| t.title.as_str()).collect();
            json!({
                "id": v.config.id,
                "name": v.config.name,
                "enabled": v.config.enabled,
                "state": v.state,
                "error": v.error,
                "hasSecrets": !v.config.env_keys.is_empty() || !v.config.header_keys.is_empty(),
                "toolCount": v.tools.len(),
                "enabledToolCount": enabled_tools,
                "tools": tool_names,
            })
        })
        .collect();
    let names: Vec<&str> = views.iter().map(|v| v.config.name.as_str()).collect();
    ToolOutput::ok(
        format!("Подключено сервисов: {} ({})", views.len(), names.join(", ")),
        json!({ "count": views.len(), "connections": items }),
    )
}

#[derive(Deserialize)]
struct RemoveArgs {
    id: String,
}

async fn connection_remove(ctx: ToolCtx, args: Value) -> AppResult<ToolOutput> {
    let a: RemoveArgs = parse(args)?;
    let id = a.id.trim();
    if id.is_empty() {
        return Err(AppError::invalid("Укажите id сервиса для отключения"));
    }
    let hub = ctx.app.state::<Arc<Hub>>();
    let Some(view) = hub.view(id) else {
        let existing: Vec<String> = hub.views().into_iter().map(|v| v.config.id).collect();
        return Ok(ToolOutput::fail(
            format!("Подключённого сервиса «{id}» нет"),
            json!({ "id": id, "connectedIds": existing }),
        ));
    };
    hub.remove(id)?;
    Ok(ToolOutput::ok(
        format!("Сервис {} отключён", view.config.name),
        json!({ "id": id, "name": view.config.name, "removed": true }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{ServerConfig, ToolView, TransportConfig};

    #[test]
    fn tool_outputs_never_contain_secret_values() {
        let entry = catalog::by_id("notion").unwrap();
        let secret = "ntn_live_secret_987654321".to_string();
        let failed_view = ServerView {
            config: ServerConfig {
                id: entry.id.clone(),
                name: entry.name.clone(),
                enabled: true,
                transport: TransportConfig::Stdio { command: "npx".into(), args: vec![], cwd: String::new() },
                env_keys: vec!["NOTION_TOKEN".into()],
                header_keys: vec![],
                disabled_tools: vec![],
            },
            state: State::Failed,
            error: Some(format!("401 Unauthorized for token {secret}")),
            server_info: String::new(),
            tools: vec![],
        };
        let out = format_connect_outcome(entry, Some(&failed_view), std::slice::from_ref(&secret));
        assert!(!out.ok);
        let serialized = serde_json::to_string(&out).unwrap();
        assert!(!serialized.contains(&secret), "secret leaked into failure output: {serialized}");
        assert!(serialized.contains("[скрыто]"));

        let ok_view = ServerView {
            config: failed_view.config.clone(),
            state: State::Running,
            error: None,
            server_info: format!("notion-mcp ({secret})"),
            tools: vec![ToolView {
                name: "search".into(),
                title: "Search pages".into(),
                description: format!("Uses {secret} to search"),
                risk: "low",
                enabled: true,
            }],
        };
        let ok_out = format_connect_outcome(entry, Some(&ok_view), std::slice::from_ref(&secret));
        assert!(ok_out.ok);
        let ok_json = serde_json::to_string(&ok_out).unwrap();
        assert!(!ok_json.contains(&secret), "secret leaked into ok output: {ok_json}");

        let list_out = format_connections_list(&[ok_view]);
        let list_json = serde_json::to_string(&list_out).unwrap();
        assert!(!list_json.contains(&secret));
        assert!(list_json.contains("\"hasSecrets\":true"));
    }

    #[test]
    fn confirmation_details_hides_smuggled_secrets() {
        let args = json!({
            "id": "home-assistant",
            "inputs": {
                "url": "http://ha.local:8123",
                "token": "super_secret_ha_token"
            }
        });
        let details = confirmation_details(&args).unwrap();
        assert!(details.contains("http://ha.local:8123"));
        assert!(!details.contains("super_secret_ha_token"));
        assert!(details.contains("защищённом окне"));
    }

    #[test]
    fn candidate_marks_secret_and_non_secret_fields() {
        let ha = catalog::by_id("home-assistant").unwrap();
        let c = format_candidate(ha, false);
        assert_eq!(c["hasSecrets"], json!(true));
        let inputs = c["inputs"].as_array().unwrap();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0]["key"], json!("url"));
        assert_eq!(inputs[0]["secret"], json!(false));
        assert_eq!(inputs[1]["key"], json!("token"));
        assert_eq!(inputs[1]["secret"], json!(true));
    }
}
