//! Settings → MCP: the servers JARVIS borrows tools from. Changes apply at
//! once (no save bar); env values and headers go to Credential Manager and
//! never come back to the UI — it only sees their names.

use std::sync::Arc;

use tauri::State;

use crate::error::AppResult;
use crate::mcp::{Hub, Secrets, ServerConfig, ServerView};

type HubState<'a> = State<'a, Arc<Hub>>;

#[tauri::command]
pub fn mcp_list(hub: HubState<'_>) -> Vec<ServerView> {
    hub.views()
}

/// Add or edit a server; `secrets: None` keeps the stored values.
#[tauri::command]
pub async fn mcp_save(hub: HubState<'_>, config: ServerConfig, secrets: Option<Secrets>, previous_id: Option<String>) -> AppResult<()> {
    hub.inner().clone().upsert(config, secrets, previous_id).await
}

#[tauri::command]
pub async fn mcp_import(hub: HubState<'_>, text: String) -> AppResult<Vec<String>> {
    hub.inner().clone().import(&text).await
}

#[tauri::command]
pub fn mcp_remove(hub: HubState<'_>, id: String) -> AppResult<()> {
    hub.remove(&id)
}

/// Restart the server and read its tools again; returns their count.
#[tauri::command]
pub async fn mcp_refresh(hub: HubState<'_>, id: String) -> AppResult<usize> {
    hub.inner().clone().refresh(&id).await
}

#[tauri::command]
pub async fn mcp_set_enabled(hub: HubState<'_>, id: String, enabled: bool) -> AppResult<()> {
    hub.inner().clone().set_enabled(&id, enabled).await
}

#[tauri::command]
pub fn mcp_set_tool(hub: HubState<'_>, id: String, tool: String, enabled: bool) -> AppResult<()> {
    hub.set_tool_enabled(&id, &tool, enabled)
}
