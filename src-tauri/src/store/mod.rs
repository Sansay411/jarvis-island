//! Local persistence (SQLite): settings, conversations, task history, audit
//! log of consequential actions and explicit user memories. Secrets live in
//! Windows Credential Manager (see [`secrets`]).

pub mod secrets;
pub mod settings;

use std::path::Path;

use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::ai::types::ChatMessage;
use crate::error::AppResult;
use settings::Settings;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS kv (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS conversations (
    id         TEXT PRIMARY KEY,
    title      TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    payload         TEXT NOT NULL,
    tool_title      TEXT,
    tool_ok         INTEGER,
    created_at      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_conv ON messages(conversation_id, id);
CREATE TABLE IF NOT EXISTS tasks (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    request         TEXT NOT NULL,
    status          TEXT NOT NULL,
    steps           INTEGER NOT NULL DEFAULT 0,
    error           TEXT,
    started_at      TEXT NOT NULL,
    finished_at     TEXT
);
CREATE TABLE IF NOT EXISTS audit_log (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    at        TEXT NOT NULL,
    task_id   TEXT,
    tool      TEXT NOT NULL,
    risk      TEXT NOT NULL,
    args      TEXT NOT NULL,
    decision  TEXT NOT NULL,
    outcome   TEXT
);
CREATE TABLE IF NOT EXISTS memories (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    text       TEXT NOT NULL,
    created_at TEXT NOT NULL
);
"#;

pub struct Store {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessage {
    pub message: ChatMessage,
    pub tool_title: Option<String>,
    pub tool_ok: Option<bool>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: i64,
    pub text: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub request: String,
    pub status: String,
    pub steps: i64,
    pub error: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

impl Store {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    #[cfg(test)]
    pub fn in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    // ---------- settings ----------

    pub fn settings(&self) -> Settings {
        let raw: Option<String> = self
            .conn
            .lock()
            .query_row("SELECT value FROM kv WHERE key = 'settings'", [], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save_settings(&self, s: &Settings) -> AppResult<()> {
        let json = serde_json::to_string(s)?;
        self.conn.lock().execute(
            "INSERT INTO kv(key, value) VALUES('settings', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![json],
        )?;
        Ok(())
    }

    pub fn kv_get(&self, key: &str) -> Option<String> {
        self.conn
            .lock()
            .query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn kv_set(&self, key: &str, value: &str) -> AppResult<()> {
        self.conn.lock().execute(
            "INSERT INTO kv(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ---------- conversations ----------

    pub fn create_conversation(&self, id: &str) -> AppResult<()> {
        let t = now();
        self.conn.lock().execute(
            "INSERT OR IGNORE INTO conversations(id, created_at, updated_at) VALUES(?1, ?2, ?2)",
            params![id, t],
        )?;
        Ok(())
    }

    pub fn append_message(
        &self,
        conversation_id: &str,
        msg: &ChatMessage,
        tool_title: Option<&str>,
        tool_ok: Option<bool>,
    ) -> AppResult<()> {
        let payload = serde_json::to_string(msg)?;
        let t = now();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO messages(conversation_id, payload, tool_title, tool_ok, created_at) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![conversation_id, payload, tool_title, tool_ok, t],
        )?;
        conn.execute("UPDATE conversations SET updated_at = ?2 WHERE id = ?1", params![conversation_id, t])?;
        Ok(())
    }

    pub fn messages(&self, conversation_id: &str) -> AppResult<Vec<StoredMessage>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT payload, tool_title, tool_ok, created_at FROM messages WHERE conversation_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![conversation_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<bool>>(2)?, r.get::<_, String>(3)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (payload, tool_title, tool_ok, created_at) = row?;
            if let Ok(message) = serde_json::from_str::<ChatMessage>(&payload) {
                out.push(StoredMessage { message, tool_title, tool_ok, created_at });
            }
        }
        Ok(out)
    }

    pub fn latest_conversation(&self) -> Option<String> {
        self.conn
            .lock()
            .query_row("SELECT id FROM conversations ORDER BY updated_at DESC LIMIT 1", [], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn clear_history(&self) -> AppResult<()> {
        self.conn.lock().execute_batch("DELETE FROM messages; DELETE FROM conversations; DELETE FROM tasks;")?;
        Ok(())
    }

    // ---------- tasks & audit ----------

    pub fn task_started(&self, id: &str, conversation_id: &str, request: &str) -> AppResult<()> {
        self.conn.lock().execute(
            "INSERT INTO tasks(id, conversation_id, request, status, started_at) VALUES(?1, ?2, ?3, 'running', ?4)",
            params![id, conversation_id, request, now()],
        )?;
        Ok(())
    }

    pub fn task_finished(&self, id: &str, status: &str, steps: u32, error: Option<&str>) -> AppResult<()> {
        self.conn.lock().execute(
            "UPDATE tasks SET status = ?2, steps = ?3, error = ?4, finished_at = ?5 WHERE id = ?1",
            params![id, status, steps, error, now()],
        )?;
        Ok(())
    }

    pub fn recent_tasks(&self, limit: u32) -> AppResult<Vec<TaskRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, request, status, steps, error, started_at, finished_at FROM tasks ORDER BY started_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            Ok(TaskRecord {
                id: r.get(0)?,
                request: r.get(1)?,
                status: r.get(2)?,
                steps: r.get(3)?,
                error: r.get(4)?,
                started_at: r.get(5)?,
                finished_at: r.get(6)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn audit(&self, task_id: Option<&str>, tool: &str, risk: &str, args: &str, decision: &str, outcome: Option<&str>) {
        let res = self.conn.lock().execute(
            "INSERT INTO audit_log(at, task_id, tool, risk, args, decision, outcome) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![now(), task_id, tool, risk, args, decision, outcome],
        );
        if let Err(e) = res {
            tracing::error!("audit log write failed: {e}");
        }
    }

    // ---------- memories ----------

    pub fn add_memory(&self, text: &str) -> AppResult<i64> {
        let conn = self.conn.lock();
        conn.execute("INSERT INTO memories(text, created_at) VALUES(?1, ?2)", params![text, now()])?;
        Ok(conn.last_insert_rowid())
    }

    pub fn memories(&self) -> AppResult<Vec<Memory>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, text, created_at FROM memories ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok(Memory { id: r.get(0)?, text: r.get(1)?, created_at: r.get(2)? }))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn delete_memory(&self, id: i64) -> AppResult<()> {
        self.conn.lock().execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::types::Role;

    #[test]
    fn settings_roundtrip_and_defaults() {
        let s = Store::in_memory().unwrap();
        let mut cfg = s.settings();
        assert_eq!(cfg.provider.model, "qwen3-8");
        cfg.provider.model = "qwen3.8-max".into();
        s.save_settings(&cfg).unwrap();
        assert_eq!(s.settings().provider.model, "qwen3.8-max");
    }

    #[test]
    fn messages_roundtrip() {
        let s = Store::in_memory().unwrap();
        s.create_conversation("c1").unwrap();
        s.append_message("c1", &ChatMessage::text(Role::User, "привет"), None, None).unwrap();
        s.append_message("c1", &ChatMessage::tool_result("t1", "{}"), Some("Громкость"), Some(true)).unwrap();
        let m = s.messages("c1").unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].message.joined_text(), "привет");
        assert_eq!(m[1].tool_ok, Some(true));
        assert_eq!(s.latest_conversation().as_deref(), Some("c1"));
    }
}
