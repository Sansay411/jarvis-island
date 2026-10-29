//! Child-process supervision: no console window, the whole process tree in a
//! Job Object (killed together on cancel/timeout/drop), line-by-line
//! stdout/stderr capture with streaming callback, bounded output retention
//! and exit-code reporting. Starting a process is never treated as success —
//! callers get the exit status.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

use crate::error::{AppError, AppResult};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Keep at most this many characters of combined output for the model.
const MAX_RETAINED: usize = 24_000;

pub(crate) struct Job(HANDLE);

unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    pub(crate) fn new() -> AppResult<Job> {
        unsafe {
            let h = CreateJobObjectW(None, None)?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )?;
            Ok(Job(h))
        }
    }

    /// Put a child (and whatever it starts later) into the job.
    pub(crate) fn assign(&self, child: &tokio::process::Child) {
        if let Some(raw) = child.raw_handle() {
            unsafe {
                if let Err(e) = AssignProcessToJobObject(self.0, HANDLE(raw)) {
                    tracing::warn!("job assignment failed: {e}");
                }
            }
        }
    }

    fn kill(&self) {
        unsafe {
            let _ = TerminateJobObject(self.0, 1);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // KILL_ON_JOB_CLOSE: closing the last handle terminates the tree.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[derive(Debug, Clone)]
pub struct Spec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
    /// Text written to stdin, then stdin is closed.
    pub stdin: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessResult {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub timed_out: bool,
    pub cancelled: bool,
    pub duration_ms: u64,
    /// Tail-biased combined output (stderr lines prefixed with "! ").
    pub output: String,
    pub output_truncated: bool,
}

pub type LineSink = Arc<dyn Fn(&str) + Send + Sync>;

struct Retained {
    text: String,
    truncated: bool,
}

impl Retained {
    fn push(&mut self, line: &str) {
        self.text.push_str(line);
        self.text.push('\n');
        if self.text.len() > MAX_RETAINED * 2 {
            // Keep the head (often the command echo / first error) and the tail.
            let head: String = self.text.chars().take(MAX_RETAINED / 4).collect();
            let tail_start = self.text.len() - MAX_RETAINED * 3 / 4;
            let mut cut = tail_start;
            while !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            self.text = format!("{head}\n… [вывод сокращён] …\n{}", &self.text[cut..]);
            self.truncated = true;
        }
    }
}

pub async fn run(spec: Spec, cancel: CancellationToken, on_line: Option<LineSink>) -> AppResult<ProcessResult> {
    let started = Instant::now();
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .stdin(if spec.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .kill_on_drop(true);
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => AppError::not_found(format!("Программа «{}» не найдена", spec.program)),
        _ => AppError::platform(format!("Не удалось запустить «{}»: {e}", spec.program)),
    })?;

    let job = Job::new()?;
    if let Some(raw) = child.raw_handle() {
        unsafe {
            if let Err(e) = AssignProcessToJobObject(job.0, HANDLE(raw)) {
                tracing::warn!("job assignment failed: {e}");
            }
        }
    }

    if let (Some(text), Some(mut stdin)) = (spec.stdin.clone(), child.stdin.take()) {
        use tokio::io::AsyncWriteExt;
        tokio::spawn(async move {
            let _ = stdin.write_all(text.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
    }

    let retained = Arc::new(parking_lot::Mutex::new(Retained { text: String::new(), truncated: false }));
    let mut readers = Vec::new();
    for (stream, is_err) in [
        (child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), false),
        (child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), true),
    ] {
        let Some(stream) = stream else { continue };
        let retained = retained.clone();
        let sink = on_line.clone();
        readers.push(tokio::spawn(async move {
            let mut lines = BufReader::new(stream).split(b'\n');
            while let Ok(Some(raw)) = lines.next_segment().await {
                let line = String::from_utf8_lossy(&raw).trim_end_matches('\r').to_string();
                let shown = if is_err { format!("! {line}") } else { line };
                if let Some(s) = &sink {
                    s(&shown);
                }
                retained.lock().push(&shown);
            }
        }));
    }

    let (mut timed_out, mut cancelled) = (false, false);
    let status = tokio::select! {
        s = child.wait() => Some(s?),
        _ = tokio::time::sleep(spec.timeout) => { timed_out = true; None }
        _ = cancel.cancelled() => { cancelled = true; None }
    };
    if status.is_none() {
        job.kill();
        let _ = child.kill().await;
    }
    for r in readers {
        let _ = tokio::time::timeout(Duration::from_secs(2), r).await;
    }
    let r = retained.lock();
    let exit_code = status.and_then(|s| s.code());
    Ok(ProcessResult {
        exit_code,
        success: status.is_some_and(|s| s.success()),
        timed_out,
        cancelled,
        duration_ms: started.elapsed().as_millis() as u64,
        output: r.text.clone(),
        output_truncated: r.truncated,
    })
}

/// Resolve an executable on PATH (also trying PATHEXT extensions).
pub fn which(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
        .split(';')
        .map(|s| s.to_lowercase())
        .collect();
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let direct = dir.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        for ext in &exts {
            let p = dir.join(format!("{name}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}
