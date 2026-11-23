//! Updates. JARVIS reads the signed `latest.json` of its GitHub releases soon
//! after start and every six hours (Settings → Основное can turn that off).
//! A new version is announced to the island and the settings; nothing is
//! installed behind the user's back, since installing restarts JARVIS. One
//! click (island card, settings, tray) downloads the package, checks its
//! signature against the public key built into the app, installs it and
//! restarts.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// A newer version is out: `UpdateInfo`.
pub const EVT_AVAILABLE: &str = "update:available";
/// Download progress while installing: `{ downloaded, total }`.
pub const EVT_PROGRESS: &str = "update:progress";

const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    /// The release notes, as written for the release.
    pub notes: String,
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current: String,
    pub available: Option<UpdateInfo>,
    /// Unix ms of the last successful check.
    pub checked_at: Option<i64>,
    pub installing: bool,
}

#[derive(Default)]
pub struct Updates {
    available: Mutex<Option<UpdateInfo>>,
    checked_at: Mutex<Option<i64>>,
    installing: AtomicBool,
}

fn err(e: impl std::fmt::Display) -> AppError {
    AppError::internal(format!("Обновление не удалось: {e}"))
}

impl Updates {
    pub fn status(&self) -> UpdateStatus {
        UpdateStatus {
            current: env!("CARGO_PKG_VERSION").into(),
            available: self.available.lock().clone(),
            checked_at: *self.checked_at.lock(),
            installing: self.installing.load(Ordering::Relaxed),
        }
    }

    /// Ask GitHub whether a newer version is out; announce it if so.
    pub async fn check(&self, app: &AppHandle) -> AppResult<Option<UpdateInfo>> {
        let update = app.updater().map_err(err)?.check().await.map_err(err)?;
        *self.checked_at.lock() = Some(chrono::Utc::now().timestamp_millis());
        let info = update.map(|u| UpdateInfo {
            version: u.version.clone(),
            current: u.current_version.clone(),
            notes: u.body.clone().unwrap_or_default(),
            date: u.date.map(|d| d.to_string()),
        });
        let fresh = {
            let mut cur = self.available.lock();
            let fresh = info.is_some() && *cur != info;
            *cur = info.clone();
            fresh
        };
        if let Some(i) = info.as_ref().filter(|_| fresh) {
            tracing::info!(version = %i.version, "update available");
            let _ = app.emit(EVT_AVAILABLE, i);
        }
        Ok(info)
    }

    /// Download, verify, install and restart. Returns only on failure.
    pub async fn install(&self, app: &AppHandle) -> AppResult<()> {
        if self.installing.swap(true, Ordering::Relaxed) {
            return Err(AppError::invalid("Обновление уже устанавливается"));
        }
        let res = self.install_inner(app).await;
        self.installing.store(false, Ordering::Relaxed);
        res
    }

    async fn install_inner(&self, app: &AppHandle) -> AppResult<()> {
        let update = app.updater().map_err(err)?.check().await.map_err(err)?.ok_or_else(|| AppError::not_found("Уже установлена последняя версия"))?;
        tracing::info!(version = %update.version, "installing update");
        // Stop listening and talking: the installer closes the app.
        app.state::<std::sync::Arc<crate::voice::VoiceManager>>().stop();
        let progress = app.clone();
        let mut downloaded: u64 = 0;
        update
            .download_and_install(
                move |chunk, total| {
                    downloaded += chunk as u64;
                    let _ = progress.emit(EVT_PROGRESS, serde_json::json!({ "downloaded": downloaded, "total": total }));
                },
                || tracing::info!("update downloaded, installing"),
            )
            .await
            .map_err(err)?;
        app.restart();
    }
}

/// Check soon after start (not in the busy first seconds) and then every six
/// hours, while the setting allows it.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(45)).await;
        loop {
            let on = app.state::<AppState>().store.settings().updates.auto_check;
            if on && !cfg!(debug_assertions) {
                if let Err(e) = app.state::<Updates>().check(&app).await {
                    tracing::info!("update check failed: {e}");
                }
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}
