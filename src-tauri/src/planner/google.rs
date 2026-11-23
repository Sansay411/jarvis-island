//! Google Calendar, both ways. The user brings their own OAuth client
//! («Desktop app» in Google Cloud Console); JARVIS signs in through the
//! system browser with PKCE and a one-shot loopback listener on 127.0.0.1,
//! keeps the refresh token in the system key store and syncs the primary
//! calendar: local changes go up at once, Google's come down every few
//! minutes over a window of five weeks back and four months ahead.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use chrono::{DateTime, Duration as CDur, Local, NaiveDate, TimeZone};
use parking_lot::Mutex;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::Emitter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::{category_color, color_by_id, color_id, parse_when, today, CalEvent, Planner, When, EVT_CHANGED};
use crate::error::{AppError, AppResult};
use crate::store::secrets;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
const API: &str = "https://www.googleapis.com/calendar/v3/";
const SCOPES: &str = "openid email https://www.googleapis.com/auth/calendar.events";
pub const EVT_SYNC: &str = "planner:sync";
const KV_ACCOUNT: &str = "google_account";
const KV_LAST_SYNC: &str = "google_last_sync";
const FORM: &str = "application/x-www-form-urlencoded";
/// Days of the calendar mirrored before and after today.
const BACK_DAYS: i64 = 35;
const AHEAD_DAYS: i64 = 120;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub connected: bool,
    pub account: String,
    pub client_id: String,
    pub has_secret: bool,
    pub syncing: bool,
    pub connecting: bool,
    pub last_sync: Option<String>,
    pub error: Option<String>,
}

#[derive(Default)]
struct RunState {
    syncing: bool,
    error: Option<String>,
    connect: Option<CancellationToken>,
}

#[derive(Default)]
pub struct Google {
    access: Mutex<Option<(String, Instant)>>,
    state: Mutex<RunState>,
    notify: Notify,
    sync_lock: tokio::sync::Mutex<()>,
}

impl Google {
    /// A local change happened: sync soon (the loop debounces).
    pub fn poke(&self) {
        self.notify.notify_one();
    }

    fn set_error(&self, e: Option<String>) {
        self.state.lock().error = e;
    }
}

fn http() -> reqwest::Client {
    crate::ai::openai::http_client()
}

fn form(pairs: &[(&str, &str)]) -> String {
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        s.append_pair(k, v);
    }
    s.finish()
}

fn client_id(planner: &Planner) -> String {
    planner.store().settings().planner.google_client_id.trim().to_string()
}

pub fn connected(planner: &Planner) -> bool {
    !client_id(planner).is_empty() && secrets::exists(secrets::GOOGLE_REFRESH_TOKEN)
}

pub fn status(planner: &Planner) -> SyncStatus {
    let st = planner.google.state.lock();
    SyncStatus {
        connected: connected(planner),
        account: planner.store().kv_get(KV_ACCOUNT).unwrap_or_default(),
        client_id: client_id(planner),
        has_secret: secrets::exists(secrets::GOOGLE_CLIENT_SECRET),
        syncing: st.syncing,
        connecting: st.connect.is_some(),
        last_sync: planner.store().kv_get(KV_LAST_SYNC).filter(|s| !s.is_empty()),
        error: st.error.clone(),
    }
}

fn emit_status(planner: &Planner) {
    if let Some(app) = planner.app() {
        let _ = app.emit(EVT_SYNC, status(planner));
    }
}

// ---------- sign-in ----------

fn page(ok: bool, text: &str) -> String {
    let title = if ok { "Готово" } else { "Не получилось" };
    format!(
        "<!doctype html><html lang=\"ru\"><meta charset=\"utf-8\"><title>Джарвис — {title}</title>\
<style>body{{margin:0;height:100vh;display:grid;place-items:center;background:#f3f4f6;color:#16161a;\
font:16px/1.5 'Segoe UI Variable Text','Segoe UI',sans-serif}}main{{text-align:center;max-width:440px}}\
.m{{display:inline-flex;gap:5px;align-items:center;justify-content:center;width:64px;height:34px;border-radius:99px;background:#0b0b0d}}\
.m i{{width:6px;height:16px;border-radius:6px;background:#ffb23f;box-shadow:0 0 8px rgba(255,178,63,.8)}}\
h1{{font-size:30px;font-weight:640;letter-spacing:-.03em;margin:22px 0 8px}}p{{color:#4a4b52;margin:0}}</style>\
<main><span class=\"m\"><i></i><i></i></span><h1>{title}</h1><p>{text}</p></main></html>"
    )
}

async fn respond(sock: &mut tokio::net::TcpStream, status: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = sock.write_all(head.as_bytes()).await;
    let _ = sock.write_all(body.as_bytes()).await;
    let _ = sock.shutdown().await;
}

/// Accept connections until Google redirects back with `code` (or `error`).
async fn wait_for_code(listener: tokio::net::TcpListener, state: &str) -> AppResult<String> {
    loop {
        let (mut sock, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let mut len = 0;
        while len < buf.len() {
            let n = sock.read(&mut buf[len..]).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            len += n;
            if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let head = String::from_utf8_lossy(&buf[..len]).to_string();
        let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
        let Ok(url) = url::Url::parse(&format!("http://127.0.0.1{path}")) else {
            respond(&mut sock, "400 Bad Request", "").await;
            continue;
        };
        let get = |k: &str| url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.to_string());
        if let Some(err) = get("error") {
            respond(&mut sock, "200 OK", &page(false, "Доступ к календарю не выдан. Вернитесь в Джарвис и попробуйте ещё раз.")).await;
            return Err(AppError::PermissionDenied(if err == "access_denied" {
                "Вы отказали Джарвису в доступе к календарю".into()
            } else {
                format!("Google вернул ошибку: {err}")
            }));
        }
        let Some(code) = get("code") else {
            respond(&mut sock, "404 Not Found", "").await;
            continue;
        };
        if get("state").as_deref() != Some(state) {
            respond(&mut sock, "400 Bad Request", &page(false, "Ответ не от того входа. Начните подключение заново.")).await;
            return Err(AppError::PermissionDenied("Ответ Google не совпал с запросом входа".into()));
        }
        respond(&mut sock, "200 OK", &page(true, "Джарвис подключён к Google Календарю. Эту вкладку можно закрыть.")).await;
        return Ok(code);
    }
}

/// The `email` claim of an ID token (shown in settings; not verified).
fn email_from_id_token(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let v: Value = serde_json::from_slice(&raw).ok()?;
    v.get("email").and_then(Value::as_str).map(str::to_string)
}

/// Sign in through the browser. Replaces any sign-in already waiting.
pub async fn connect(planner: &Arc<Planner>, client_id_in: String, client_secret_in: String) -> AppResult<SyncStatus> {
    let id = client_id_in.trim().to_string();
    let id = if id.is_empty() { client_id(planner) } else { id };
    if !id.ends_with(".apps.googleusercontent.com") {
        return Err(AppError::invalid("Client ID выглядит как «…apps.googleusercontent.com» — скопируйте его из Google Cloud Console"));
    }
    let secret_in = client_secret_in.trim().to_string();
    if !secret_in.is_empty() {
        secrets::set(secrets::GOOGLE_CLIENT_SECRET, &secret_in)?;
    }
    let secret = secrets::get(secrets::GOOGLE_CLIENT_SECRET)?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| AppError::invalid("Добавьте Client secret из Google Cloud Console"))?;
    {
        let mut s = planner.store().settings();
        if s.planner.google_client_id != id {
            s.planner.google_client_id = id.clone();
            planner.store().save_settings(&s)?;
        }
    }

    let cancel = CancellationToken::new();
    if let Some(old) = planner.google.state.lock().connect.replace(cancel.clone()) {
        old.cancel();
    }
    emit_status(planner);
    let res = sign_in(planner, &id, &secret, &cancel).await;
    {
        let mut st = planner.google.state.lock();
        // A newer sign-in (or «Отключить») cancelled ours and owns the slot.
        if !cancel.is_cancelled() {
            st.connect = None;
        }
        if let Err(e) = &res {
            if !matches!(e, AppError::Cancelled) {
                st.error = Some(e.to_string());
            }
        } else {
            st.error = None;
        }
    }
    emit_status(planner);
    res?;
    planner.google.poke();
    Ok(status(planner))
}

async fn sign_in(planner: &Arc<Planner>, id: &str, secret: &str, cancel: &CancellationToken) -> AppResult<()> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let verifier = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = uuid::Uuid::new_v4().simple().to_string();
    let url = url::Url::parse_with_params(
        AUTH_URL,
        &[
            ("client_id", id),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", SCOPES),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", state.as_str()),
            ("access_type", "offline"),
            ("prompt", "consent"),
        ],
    )
    .map_err(|e| AppError::internal(format!("Адрес входа: {e}")))?;
    let target = url.to_string();
    crate::platform::blocking(move || crate::platform::apps::shell_open(&target, None, None)).await?;

    let code = tokio::select! {
        _ = cancel.cancelled() => return Err(AppError::Cancelled),
        r = tokio::time::timeout(Duration::from_secs(300), wait_for_code(listener, &state)) => {
            r.map_err(|_| AppError::Timeout("Вход в Google не завершён за 5 минут — попробуйте ещё раз".into()))??
        }
    };

    let res = http()
        .post(TOKEN_URL)
        .header(reqwest::header::CONTENT_TYPE, FORM)
        .body(form(&[
            ("code", code.as_str()),
            ("client_id", id),
            ("client_secret", secret),
            ("redirect_uri", redirect.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier.as_str()),
        ]))
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    let ok = res.status().is_success();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if !ok {
        let why = v.get("error_description").or_else(|| v.get("error")).and_then(Value::as_str).unwrap_or("неизвестная ошибка");
        return Err(AppError::Provider(format!("Google не выдал доступ: {why}")));
    }
    let refresh = v
        .get("refresh_token")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Provider("Google не прислал ключ обновления — удалите доступ Джарвиса в аккаунте Google и подключите заново".into()))?;
    secrets::set(secrets::GOOGLE_REFRESH_TOKEN, refresh)?;
    if let (Some(tok), Some(exp)) = (v.get("access_token").and_then(Value::as_str), v.get("expires_in").and_then(Value::as_u64)) {
        *planner.google.access.lock() = Some((tok.to_string(), Instant::now() + Duration::from_secs(exp)));
    }
    let email = v.get("id_token").and_then(Value::as_str).and_then(email_from_id_token).unwrap_or_default();
    let before = planner.store().kv_get(KV_ACCOUNT).unwrap_or_default();
    if !before.is_empty() && !email.is_empty() && before != email {
        forget_account(planner)?;
    }
    planner.store().kv_set(KV_ACCOUNT, &email)?;
    tracing::info!("google calendar connected");
    Ok(())
}

/// Another account: its events leave, ours are uploaded to the new one.
fn forget_account(planner: &Planner) -> AppResult<()> {
    planner.store().with_conn(|conn| {
        conn.execute_batch(
            "DELETE FROM planner_events WHERE origin = 'google' OR deleted = 1;
             UPDATE planner_events SET google_id = NULL, sync_state = '', recurring = 0;",
        )?;
        Ok(())
    })?;
    planner.store().kv_set(KV_LAST_SYNC, "")?;
    Ok(())
}

pub async fn disconnect(planner: &Arc<Planner>) -> AppResult<SyncStatus> {
    if let Some(token) = secrets::get(secrets::GOOGLE_REFRESH_TOKEN)? {
        // Best effort: the account page also lists and revokes the access.
        let _ = http()
            .post(REVOKE_URL)
            .header(reqwest::header::CONTENT_TYPE, FORM)
            .body(form(&[("token", token.as_str())]))
            .timeout(Duration::from_secs(10))
            .send()
            .await;
    }
    secrets::delete(secrets::GOOGLE_REFRESH_TOKEN)?;
    *planner.google.access.lock() = None;
    planner.google.set_error(None);
    if let Some(c) = planner.google.state.lock().connect.take() {
        c.cancel();
    }
    emit_status(planner);
    Ok(status(planner))
}

// ---------- API ----------

async fn access_token(planner: &Planner) -> AppResult<String> {
    if let Some((tok, exp)) = planner.google.access.lock().clone() {
        if exp > Instant::now() + Duration::from_secs(60) {
            return Ok(tok);
        }
    }
    let refresh = secrets::get(secrets::GOOGLE_REFRESH_TOKEN)?.ok_or_else(|| AppError::invalid("Google Календарь не подключён"))?;
    let secret = secrets::get(secrets::GOOGLE_CLIENT_SECRET)?.unwrap_or_default();
    let id = client_id(planner);
    let res = http()
        .post(TOKEN_URL)
        .header(reqwest::header::CONTENT_TYPE, FORM)
        .body(form(&[
            ("client_id", id.as_str()),
            ("client_secret", secret.as_str()),
            ("refresh_token", refresh.as_str()),
            ("grant_type", "refresh_token"),
        ]))
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    let ok = res.status().is_success();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if !ok {
        let err = v.get("error").and_then(Value::as_str).unwrap_or("");
        if err == "invalid_grant" {
            // Revoked, or a «Testing» project's token aged out (7 days).
            let _ = secrets::delete(secrets::GOOGLE_REFRESH_TOKEN);
            return Err(AppError::PermissionDenied("Google отозвал доступ. Подключите календарь заново (в режиме «Testing» доступ живёт 7 дней).".into()));
        }
        let why = v.get("error_description").and_then(Value::as_str).unwrap_or(err);
        return Err(AppError::Provider(format!("Google: {why}")));
    }
    let tok = v.get("access_token").and_then(Value::as_str).ok_or_else(|| AppError::Provider("Google не прислал ключ доступа".into()))?.to_string();
    let exp = v.get("expires_in").and_then(Value::as_u64).unwrap_or(3600);
    *planner.google.access.lock() = Some((tok.clone(), Instant::now() + Duration::from_secs(exp)));
    Ok(tok)
}

fn events_url(calendar: &str, event: Option<&str>) -> url::Url {
    let mut u = url::Url::parse(API).expect("api base");
    {
        let mut seg = u.path_segments_mut().expect("http url");
        seg.pop_if_empty();
        seg.extend(["calendars", calendar, "events"]);
        if let Some(e) = event {
            seg.push(e);
        }
    }
    u
}

async fn call(planner: &Planner, method: reqwest::Method, url: url::Url, body: Option<&Value>) -> AppResult<(u16, Value)> {
    let token = access_token(planner).await?;
    let mut req = http().request(method, url).bearer_auth(token).timeout(Duration::from_secs(30));
    if let Some(b) = body {
        req = req.json(b);
    }
    let res = req.send().await?;
    let code = res.status().as_u16();
    let v = res.json::<Value>().await.unwrap_or(Value::Null);
    if code == 401 {
        *planner.google.access.lock() = None;
    }
    if code == 429 || code >= 500 {
        return Err(AppError::Network(format!("Google Календарь временно недоступен ({code})")));
    }
    Ok((code, v))
}

fn api_error(code: u16, v: &Value) -> AppError {
    let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("ошибка");
    if code == 403 && msg.to_lowercase().contains("has not been used") {
        return AppError::Provider("В вашем проекте Google Cloud не включён Google Calendar API — включите его и повторите".into());
    }
    AppError::Provider(format!("Google Календарь ({code}): {msg}"))
}

fn rfc3339(local: &str) -> Option<String> {
    match parse_when(local)? {
        When::At(t) => Local.from_local_datetime(&t).earliest().map(|d| d.to_rfc3339()),
        When::Day(_) => None,
    }
}

/// Our event as a Google event resource.
pub fn to_google(e: &CalEvent) -> Value {
    let (start, end) = if e.all_day {
        (json!({ "date": e.start }), json!({ "date": e.end }))
    } else {
        (json!({ "dateTime": rfc3339(&e.start) }), json!({ "dateTime": rfc3339(&e.end) }))
    };
    let mut v = json!({
        "summary": e.title,
        "description": e.notes,
        "location": e.location,
        "start": start,
        "end": end,
        "extendedProperties": { "private": { "jarvisCategory": e.category, "jarvisDone": if e.done { "1" } else { "0" } } },
    });
    // Events born in Google keep the calendar's colour unless recoloured.
    let color = if e.color.is_empty() && e.category != "google" { category_color(&e.category) } else { e.color.as_str() };
    if let Some(id) = color_id(color) {
        v["colorId"] = json!(id.to_string());
    }
    v
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct GTime {
    date: Option<String>,
    date_time: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct GExt {
    private: std::collections::HashMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GEvent {
    id: String,
    status: String,
    summary: Option<String>,
    description: Option<String>,
    location: Option<String>,
    color_id: Option<String>,
    start: GTime,
    end: GTime,
    recurring_event_id: Option<String>,
    extended_properties: Option<GExt>,
}

/// A Google event in our shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Remote {
    pub google_id: String,
    pub title: String,
    pub notes: String,
    pub location: String,
    pub start: String,
    pub end: String,
    pub all_day: bool,
    pub color: String,
    pub category: String,
    pub done: bool,
    pub recurring: bool,
}

fn local_when(t: &GTime) -> Option<(String, bool)> {
    if let Some(d) = &t.date {
        return NaiveDate::parse_from_str(d, "%Y-%m-%d").ok().map(|d| (When::Day(d).format(), true));
    }
    let dt = DateTime::parse_from_rfc3339(t.date_time.as_deref()?).ok()?;
    Some((When::At(dt.with_timezone(&Local).naive_local()).format(), false))
}

pub fn from_google(g: &GEvent) -> Option<Remote> {
    if g.id.is_empty() || g.status == "cancelled" {
        return None;
    }
    let (start, all_day) = local_when(&g.start)?;
    let (end, _) = local_when(&g.end)?;
    let private = g.extended_properties.as_ref().map(|e| &e.private);
    let category = private
        .and_then(|p| p.get("jarvisCategory"))
        .filter(|c| super::CATEGORIES.iter().any(|(id, _, _)| id == c))
        .cloned()
        .unwrap_or_else(|| "google".into());
    let color = g.color_id.as_deref().and_then(|c| c.parse::<u8>().ok()).and_then(color_by_id).unwrap_or("");
    // Our own category colour comes back as an explicit colorId: keep it implicit.
    let color = if color == category_color(&category) { "" } else { color };
    let title = g.summary.clone().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "(Без названия)".into());
    Some(Remote {
        google_id: g.id.clone(),
        title,
        notes: g.description.clone().unwrap_or_default(),
        location: g.location.clone().unwrap_or_default(),
        start,
        end,
        all_day,
        color: color.to_string(),
        category,
        done: private.and_then(|p| p.get("jarvisDone")).is_some_and(|d| d == "1"),
        recurring: g.recurring_event_id.is_some(),
    })
}

fn calendar_id(planner: &Planner) -> String {
    let c = planner.store().settings().planner.google_calendar_id.trim().to_string();
    if c.is_empty() {
        "primary".into()
    } else {
        c
    }
}

fn local_rows(planner: &Planner, filter: &str) -> AppResult<Vec<(CalEvent, String)>> {
    planner.store().with_conn(|conn| {
        let mut stmt = conn.prepare(&format!("SELECT {}, sync_state FROM planner_events WHERE {filter}", super::COLUMNS))?;
        let rows = stmt.query_map([], |r| Ok((super::row_event(r)?, r.get::<_, String>(14)?)))?;
        Ok(rows.filter_map(Result::ok).collect())
    })
}

/// Push local changes, then mirror Google's window.
pub async fn sync(planner: &Arc<Planner>) -> AppResult<SyncStatus> {
    if !connected(planner) {
        return Ok(status(planner));
    }
    let _one = planner.google.sync_lock.lock().await;
    planner.google.state.lock().syncing = true;
    emit_status(planner);
    let res = sync_inner(planner).await;
    {
        let mut st = planner.google.state.lock();
        st.syncing = false;
        st.error = res.as_ref().err().map(|e| e.to_string());
    }
    match &res {
        Ok(changed) => {
            let _ = planner.store().kv_set(KV_LAST_SYNC, &chrono::Utc::now().to_rfc3339());
            if *changed > 0 {
                if let Some(app) = planner.app() {
                    let _ = app.emit(EVT_CHANGED, ());
                }
            }
        }
        Err(e) => tracing::warn!("google sync: {e}"),
    }
    emit_status(planner);
    res.map(|_| status(planner))
}

async fn sync_inner(planner: &Arc<Planner>) -> AppResult<usize> {
    let cal = calendar_id(planner);
    let mut changed = 0;

    // 1. Deletions.
    for (e, _) in local_rows(planner, "deleted = 1 AND sync_state = 'delete' AND google_id IS NOT NULL")? {
        let gid = e.google_id.clone().unwrap_or_default();
        let (code, v) = call(planner, reqwest::Method::DELETE, events_url(&cal, Some(&gid)), None).await?;
        if code < 300 || code == 404 || code == 410 {
            planner.store().with_conn(|c| Ok(c.execute("DELETE FROM planner_events WHERE id = ?1", params![e.id])?))?;
        } else {
            return Err(api_error(code, &v));
        }
    }

    // 2. New local events, then 3. edited ones.
    for (e, _) in local_rows(planner, "deleted = 0 AND google_id IS NULL")? {
        insert_remote(planner, &cal, &e).await?;
    }
    for (e, _) in local_rows(planner, "deleted = 0 AND sync_state = 'push' AND google_id IS NOT NULL")? {
        let gid = e.google_id.clone().unwrap_or_default();
        let (code, v) = call(planner, reqwest::Method::PATCH, events_url(&cal, Some(&gid)), Some(&to_google(&e))).await?;
        match code {
            200..=299 => {
                planner.store().with_conn(|c| Ok(c.execute("UPDATE planner_events SET sync_state = '' WHERE id = ?1 AND updated_at = ?2", params![e.id, e.updated_at])?))?;
            }
            404 | 410 => insert_remote(planner, &cal, &e).await?,
            _ => return Err(api_error(code, &v)),
        }
    }

    // 4. Pull the window.
    let from = today() - CDur::days(BACK_DAYS);
    let to = today() + CDur::days(AHEAD_DAYS);
    let day_start = |d: NaiveDate| Local.from_local_datetime(&d.and_hms_opt(0, 0, 0).expect("midnight")).earliest().map(|t| t.to_rfc3339()).unwrap_or_default();
    let mut remote = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let mut u = events_url(&cal, None);
        {
            let mut q = u.query_pairs_mut();
            q.append_pair("timeMin", &day_start(from));
            q.append_pair("timeMax", &day_start(to));
            q.append_pair("singleEvents", "true");
            q.append_pair("maxResults", "2500");
            if let Some(p) = &page {
                q.append_pair("pageToken", p);
            }
        }
        let (code, v) = call(planner, reqwest::Method::GET, u, None).await?;
        if code >= 300 {
            return Err(api_error(code, &v));
        }
        let items: Vec<GEvent> = serde_json::from_value(v.get("items").cloned().unwrap_or(json!([]))).unwrap_or_default();
        remote.extend(items.iter().filter_map(from_google));
        page = v.get("nextPageToken").and_then(Value::as_str).map(str::to_string);
        if page.is_none() || remote.len() > 20_000 {
            break;
        }
    }
    changed += apply_remote(planner, &remote, &from.format("%Y-%m-%d").to_string(), &to.format("%Y-%m-%d").to_string())?;
    Ok(changed)
}

async fn insert_remote(planner: &Planner, cal: &str, e: &CalEvent) -> AppResult<()> {
    let (code, v) = call(planner, reqwest::Method::POST, events_url(cal, None), Some(&to_google(e))).await?;
    if code >= 300 {
        return Err(api_error(code, &v));
    }
    let gid = v.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
    // Edited while the upload ran: send that edit next time.
    planner.store().with_conn(|c| {
        Ok(c.execute(
            "UPDATE planner_events SET google_id = ?2, sync_state = CASE WHEN updated_at = ?3 THEN '' ELSE 'push' END WHERE id = ?1",
            params![e.id, gid, e.updated_at],
        )?)
    })?;
    Ok(())
}

/// Mirror Google's events into SQLite; local rows with unsent changes win.
pub fn apply_remote(planner: &Planner, remote: &[Remote], from: &str, to: &str) -> AppResult<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    planner.store().with_conn(|conn| {
        let tx = conn.unchecked_transaction()?;
        let mut changed = 0;
        let mut seen = HashSet::new();
        for r in remote {
            seen.insert(r.google_id.clone());
            let local: Option<(String, String, i64, String, String, String, String, String, i64, String, String, i64)> = tx
                .query_row(
                    "SELECT id, sync_state, deleted, title, notes, location, start_at, end_at, all_day, color, category, done FROM planner_events WHERE google_id = ?1",
                    params![r.google_id],
                    |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?, x.get(3)?, x.get(4)?, x.get(5)?, x.get(6)?, x.get(7)?, x.get(8)?, x.get(9)?, x.get(10)?, x.get(11)?)),
                )
                .optional()?;
            match local {
                Some((id, sync, deleted, title, notes, location, start, end, all_day, color, category, done)) => {
                    if sync.is_empty() && deleted == 0 {
                        let same = (title.as_str(), notes.as_str(), location.as_str(), start.as_str(), end.as_str(), all_day != 0, color.as_str(), category.as_str(), done != 0)
                            == (r.title.as_str(), r.notes.as_str(), r.location.as_str(), r.start.as_str(), r.end.as_str(), r.all_day, r.color.as_str(), r.category.as_str(), r.done);
                        if !same {
                            tx.execute(
                                "UPDATE planner_events SET title = ?2, notes = ?3, location = ?4, start_at = ?5, end_at = ?6, all_day = ?7, color = ?8,
                                 category = ?9, done = ?10, recurring = ?11, updated_at = ?12 WHERE id = ?1",
                                params![id, r.title, r.notes, r.location, r.start, r.end, r.all_day as i64, r.color, r.category, r.done as i64, r.recurring as i64, now],
                            )?;
                            changed += 1;
                        }
                    }
                }
                None => {
                    tx.execute(
                        "INSERT INTO planner_events(id, title, notes, location, start_at, end_at, all_day, color, category, done, origin, google_id, recurring, created_at, updated_at)
                         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'google', ?11, ?12, ?13, ?13)",
                        params![uuid::Uuid::new_v4().to_string(), r.title, r.notes, r.location, r.start, r.end, r.all_day as i64, r.color, r.category, r.done as i64, r.google_id, r.recurring as i64, now],
                    )?;
                    changed += 1;
                }
            }
        }
        // Deleted in Google: gone here too (unless edited here meanwhile).
        let stale: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT id, google_id FROM planner_events
                 WHERE google_id IS NOT NULL AND deleted = 0 AND sync_state = '' AND start_at >= ?1 AND start_at < ?2",
            )?;
            let rows = stmt.query_map(params![from, to], |x| Ok((x.get::<_, String>(0)?, x.get::<_, String>(1)?)))?;
            rows.filter_map(Result::ok).filter(|(_, g)| !seen.contains(g)).map(|(id, _)| id).collect()
        };
        for id in &stale {
            tx.execute("DELETE FROM planner_events WHERE id = ?1", params![id])?;
            changed += 1;
        }
        tx.commit()?;
        Ok(changed)
    })
}

/// Sync shortly after start, after local changes (debounced) and every
/// three minutes.
pub fn spawn(planner: Arc<Planner>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(8)).await;
        loop {
            if connected(&planner) {
                let _ = sync(&planner).await;
            }
            tokio::select! {
                _ = planner.google.notify.notified() => tokio::time::sleep(Duration::from_millis(1200)).await,
                _ = tokio::time::sleep(Duration::from_secs(180)) => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{EventInput, EventPatch};
    use crate::store::Store;

    fn planner() -> Arc<Planner> {
        Planner::new_headless(Arc::new(Store::in_memory().unwrap()))
    }

    fn gevent(v: Value) -> GEvent {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn google_events_map_to_ours() {
        let timed = from_google(&gevent(json!({
            "id": "abc", "status": "confirmed", "summary": " Созвон ", "colorId": "11",
            "start": { "dateTime": "2026-11-23T10:00:00Z" }, "end": { "dateTime": "2026-11-23T11:00:00Z" },
            "recurringEventId": "base"
        })))
        .unwrap();
        assert_eq!(timed.title, "Созвон");
        assert!(!timed.all_day && timed.recurring);
        assert_eq!(timed.color, "tomato");
        assert_eq!(timed.category, "google");
        let expect = DateTime::parse_from_rfc3339("2026-11-23T10:00:00Z").unwrap().with_timezone(&Local).format("%Y-%m-%dT%H:%M").to_string();
        assert_eq!(timed.start, expect);

        let day = from_google(&gevent(json!({
            "id": "d1", "start": { "date": "2026-11-24" }, "end": { "date": "2026-11-25" }, "colorId": "10",
            "extendedProperties": { "private": { "jarvisCategory": "sport", "jarvisDone": "1" } }
        })))
        .unwrap();
        assert!(day.all_day && day.done);
        assert_eq!((day.start.as_str(), day.end.as_str()), ("2026-11-24", "2026-11-25"));
        assert_eq!(day.category, "sport");
        // basil is sport's own colour: stored as «the calendar's».
        assert_eq!(day.color, "");
        assert_eq!(day.title, "(Без названия)");

        assert!(from_google(&gevent(json!({ "id": "x", "status": "cancelled", "start": {}, "end": {} }))).is_none());
    }

    #[test]
    fn our_events_go_up_with_colour_and_category() {
        let p = planner();
        let e = p.create(EventInput { title: "Зал".into(), start: "2026-11-25T18:00".into(), end: Some("2026-11-25T19:00".into()), category: Some("sport".into()), ..Default::default() }).unwrap().remove(0);
        let g = to_google(&e);
        assert_eq!(g["summary"], "Зал");
        assert_eq!(g["colorId"], "10");
        assert_eq!(g["extendedProperties"]["private"]["jarvisCategory"], "sport");
        let start = g["start"]["dateTime"].as_str().unwrap();
        assert_eq!(DateTime::parse_from_rfc3339(start).unwrap().with_timezone(&Local).format("%Y-%m-%dT%H:%M").to_string(), "2026-11-25T18:00");
        let all = p.create(EventInput { title: "Выходной".into(), start: "2026-11-28".into(), all_day: Some(true), ..Default::default() }).unwrap().remove(0);
        let g = to_google(&all);
        assert_eq!(g["start"]["date"], "2026-11-28");
        assert_eq!(g["end"]["date"], "2026-11-29");
    }

    #[test]
    fn remote_window_is_mirrored_and_local_edits_win() {
        let p = planner();
        let remote = |id: &str, title: &str, start: &str| Remote {
            google_id: id.into(),
            title: title.into(),
            notes: String::new(),
            location: String::new(),
            start: start.into(),
            end: start.replace("T10", "T11"),
            all_day: false,
            color: String::new(),
            category: "google".into(),
            done: false,
            recurring: false,
        };
        let r1 = remote("g1", "Встреча", "2026-11-23T10:00");
        let r2 = remote("g2", "Обед", "2026-11-24T10:00");
        assert_eq!(apply_remote(&p, &[r1.clone(), r2.clone()], "2026-11-01", "2026-12-31").unwrap(), 2);
        assert_eq!(p.list("2026-11-23", "2026-11-25").unwrap().len(), 2);
        // Same again: nothing changes.
        assert_eq!(apply_remote(&p, &[r1.clone(), r2.clone()], "2026-11-01", "2026-12-31").unwrap(), 0);

        // Edited here, not yet pushed: Google's older copy does not overwrite it.
        let local = p.list("2026-11-23", "2026-11-24").unwrap().remove(0);
        p.update(&local.id, EventPatch { title: Some("Встреча с Аней".into()), ..Default::default() }).unwrap();
        let renamed = Remote { title: "Встреча (Google)".into(), ..r1.clone() };
        apply_remote(&p, &[renamed, r2.clone()], "2026-11-01", "2026-12-31").unwrap();
        assert_eq!(p.get(&local.id).unwrap().title, "Встреча с Аней");

        // Deleted in Google (g2 missing from the window): deleted here.
        apply_remote(&p, &[r1.clone()], "2026-11-01", "2026-12-31").unwrap();
        assert!(p.list("2026-11-24", "2026-11-25").unwrap().is_empty());
        // Outside the window nothing is touched.
        apply_remote(&p, &[r2.clone()], "2026-11-01", "2026-12-31").unwrap();
        apply_remote(&p, &[], "2027-01-01", "2027-02-01").unwrap();
        assert_eq!(p.list("2026-11-24", "2026-11-25").unwrap().len(), 1);
    }

    #[test]
    fn deleting_a_synced_event_waits_for_google() {
        let p = planner();
        apply_remote(&p, &[Remote {
            google_id: "g9".into(),
            title: "Синк".into(),
            notes: String::new(),
            location: String::new(),
            start: "2026-11-23T10:00".into(),
            end: "2026-11-23T11:00".into(),
            all_day: false,
            color: String::new(),
            category: "google".into(),
            done: false,
            recurring: false,
        }], "2026-11-01", "2026-12-31")
        .unwrap();
        let e = p.list("2026-11-23", "2026-11-24").unwrap().remove(0);
        p.delete(&e.id, false).unwrap();
        // Hidden at once, kept for the remote delete.
        assert!(p.list("2026-11-23", "2026-11-24").unwrap().is_empty());
        assert_eq!(local_rows(&p, "deleted = 1 AND sync_state = 'delete'").unwrap().len(), 1);
    }

    #[test]
    fn urls_and_tokens() {
        assert_eq!(events_url("primary", None).as_str(), "https://www.googleapis.com/calendar/v3/calendars/primary/events");
        assert_eq!(events_url("me@gmail.com", Some("a_b")).as_str(), "https://www.googleapis.com/calendar/v3/calendars/me@gmail.com/events/a_b");
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"email":"me@gmail.com"}"#);
        assert_eq!(email_from_id_token(&format!("h.{payload}.s")).as_deref(), Some("me@gmail.com"));
        assert_eq!(form(&[("a", "1 2"), ("b", "ä")]), "a=1+2&b=%C3%A4");
    }
}
