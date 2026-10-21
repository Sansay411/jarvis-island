//! Background jobs: long-running supervised processes (coding agents, long
//! builds) that outlive a single agent turn. Output is parsed into readable
//! progress lines, streamed to the island, and the verified final result is
//! posted back into the conversation when the process exits.

use std::collections::VecDeque;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult};
use crate::platform::supervisor::{self, Spec};

pub const EVT_JOBS: &str = "jobs:update";
const MAX_LINES: usize = 300;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// Parsed view of one raw output line.
#[derive(Default)]
pub struct Parsed {
    /// Human-readable progress line for the UI/model.
    pub display: Option<String>,
    /// Final answer of the tool (last agent message / result).
    pub final_text: Option<String>,
    /// A failure reported inside the stream (auth errors etc.).
    pub error: Option<String>,
}

pub type Parser = Box<dyn FnMut(&str) -> Parsed + Send>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub cwd: String,
    pub status: JobStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub final_text: Option<String>,
    pub error: Option<String>,
    pub tail: Vec<String>,
}

struct Job {
    view: JobView,
    lines: VecDeque<String>,
    cancel: CancellationToken,
}

pub struct JobManager {
    app: AppHandle,
    jobs: Mutex<Vec<Job>>,
}

pub type OnFinish = Box<dyn FnOnce(&AppHandle, &JobView) + Send>;

impl JobManager {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(JobManager { app, jobs: Mutex::new(Vec::new()) })
    }

    fn view_of(job: &Job, tail: usize) -> JobView {
        let mut v = job.view.clone();
        v.tail = job.lines.iter().rev().take(tail).rev().cloned().collect();
        v
    }

    fn emit(&self) {
        let _ = self.app.emit(EVT_JOBS, self.list());
    }

    pub fn list(&self) -> Vec<JobView> {
        self.jobs.lock().iter().rev().take(20).map(|j| Self::view_of(j, 4)).collect()
    }

    pub fn get(&self, id: &str, tail: usize) -> AppResult<JobView> {
        self.jobs
            .lock()
            .iter()
            .find(|j| j.view.id == id)
            .map(|j| Self::view_of(j, tail))
            .ok_or_else(|| AppError::not_found(format!("Задача {id} не найдена")))
    }

    pub fn running(&self) -> usize {
        self.jobs.lock().iter().filter(|j| j.view.status == JobStatus::Running).count()
    }

    pub fn cancel(&self, id: &str) -> AppResult<()> {
        let jobs = self.jobs.lock();
        let job = jobs.iter().find(|j| j.view.id == id).ok_or_else(|| AppError::not_found("Задача не найдена"))?;
        job.cancel.cancel();
        Ok(())
    }

    pub fn start(self: &Arc<Self>, kind: &str, title: String, spec: Spec, parser: Parser, on_finish: OnFinish) -> String {
        let id = format!("job-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let cancel = CancellationToken::new();
        let cwd = spec.cwd.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        self.jobs.lock().push(Job {
            view: JobView {
                id: id.clone(),
                kind: kind.to_string(),
                title,
                cwd,
                status: JobStatus::Running,
                started_at: Utc::now(),
                finished_at: None,
                exit_code: None,
                final_text: None,
                error: None,
                tail: Vec::new(),
            },
            lines: VecDeque::new(),
            cancel: cancel.clone(),
        });
        self.emit();

        let mgr = Arc::clone(self);
        let job_id = id.clone();
        let parsed_state: Arc<Mutex<(Option<String>, Option<String>)>> = Arc::new(Mutex::new((None, None)));
        let sink_state = parsed_state.clone();
        let sink_mgr = Arc::clone(self);
        let sink_id = id.clone();
        let parser_cell = Arc::new(Mutex::new(parser));
        let sink: supervisor::LineSink = Arc::new(move |raw: &str| {
            let p = (parser_cell.lock())(raw.trim_start_matches("! "));
            if let Some(t) = p.final_text {
                sink_state.lock().0 = Some(t);
            }
            if let Some(e) = p.error {
                sink_state.lock().1 = Some(e);
            }
            if let Some(line) = p.display {
                let mut jobs = sink_mgr.jobs.lock();
                if let Some(j) = jobs.iter_mut().find(|j| j.view.id == sink_id) {
                    j.lines.push_back(line);
                    while j.lines.len() > MAX_LINES {
                        j.lines.pop_front();
                    }
                }
                drop(jobs);
                sink_mgr.emit();
            }
        });

        tauri::async_runtime::spawn(async move {
            let result = supervisor::run(spec, cancel, Some(sink)).await;
            let (final_text, stream_error) = parsed_state.lock().clone();
            let view = {
                let mut jobs = mgr.jobs.lock();
                let Some(j) = jobs.iter_mut().find(|j| j.view.id == job_id) else { return };
                j.view.finished_at = Some(Utc::now());
                j.view.final_text = final_text;
                match &result {
                    Ok(r) => {
                        j.view.exit_code = r.exit_code;
                        j.view.status = if r.cancelled {
                            JobStatus::Cancelled
                        } else if r.success && stream_error.is_none() {
                            JobStatus::Succeeded
                        } else {
                            JobStatus::Failed
                        };
                        if r.timed_out {
                            j.view.error = Some("Превышено время ожидания".into());
                        } else if let Some(e) = stream_error {
                            j.view.error = Some(e);
                        } else if !r.success && !r.cancelled {
                            j.view.error = Some(format!("Процесс завершился с кодом {}", r.exit_code.unwrap_or(-1)));
                        }
                    }
                    Err(e) => {
                        j.view.status = JobStatus::Failed;
                        j.view.error = Some(e.to_string());
                    }
                }
                Self::view_of(j, 12)
            };
            tracing::info!(job = %view.id, status = ?view.status, "job finished");
            mgr.emit();
            on_finish(&mgr.app, &view);
        });
        id
    }
}

/// Convenience accessor from anywhere holding an AppHandle.
pub fn manager(app: &AppHandle) -> Arc<JobManager> {
    app.state::<Arc<JobManager>>().inner().clone()
}
