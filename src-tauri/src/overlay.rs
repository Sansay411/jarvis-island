//! Overlay window bridge stub. Replaced at merge by the full overlay module.

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::error::AppResult;

pub fn draw(app: &AppHandle, items: Vec<Value>, replace: bool) -> AppResult<()> {
    app.emit("overlay:draw", json!({ "items": items, "replace": replace }))?;
    Ok(())
}

pub fn clear(app: &AppHandle) -> AppResult<()> {
    app.emit("overlay:clear", json!({}))?;
    Ok(())
}
