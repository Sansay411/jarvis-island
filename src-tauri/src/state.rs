//! Shared application state managed by Tauri.

use std::sync::Arc;
use std::time::Instant;

use crate::platform::apps::AppIndex;
use crate::platform::island::Island;
use crate::platform::winmgr::ForegroundHistory;

pub struct AppState {
    pub island: Arc<Island>,
    pub history: Arc<ForegroundHistory>,
    pub apps: Arc<AppIndex>,
    pub started_at: Instant,
}
