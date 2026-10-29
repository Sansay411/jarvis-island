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
pub(crate) const CREATE_SUSPENDED: u32 = 0x0000_0004;
/// One output line longer than this is cut (the rest is read and dropped).
const MAX_LINE_BYTES: usize = 64 * 1024;
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

    /// Put a child started with `CREATE_SUSPENDED` into the job, then let it
    /// run. Assigning before it runs means nothing it starts can escape the
    /// job; if assignment fails the child stays suspended for the caller to kill.
    pub(crate) fn contain(&self, child: &tokio::process::Child) -> AppResult<()> {
        let (Some(raw), Some(pid)) = (child.raw_handle(), child.id()) else {
            return Err(AppError::platform("Процесс завершился при запуске"));
        };
        unsafe {
            AssignProcessToJobObject(self.0, HANDLE(raw)).map_err(|e| AppError::platform(format!("Не удалось ограничить процесс: {e}")))?;
        }
        resume_threads(pid);
        Ok(())
    }

    fn kill(&self) {
        unsafe {
            let _ = TerminateJobObject(self.0, 1);
        }
    }
}

/// Resume every thread of a process created suspended.
fn resume_threads(pid: u32) {
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32};
    use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else { return };
        let mut entry = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut ok = Thread32First(snap, &mut entry).is_ok();
        while ok {
            if entry.th32OwnerProcessID == pid {
                if let Ok(t) = OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) {
                    ResumeThread(t);
                    let _ = CloseHandle(t);
                }
            }
            ok = Thread32Next(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
}

/// Read one `\n`-terminated line, keeping at most `max` bytes of it: a
/// process that writes without newlines cannot grow our memory. Returns the
/// line (without `\n`) and whether it was cut, or `None` at end of stream.
pub(crate) async fn read_line_capped<R: tokio::io::AsyncBufRead + Unpin>(r: &mut R, max: usize) -> std::io::Result<Option<(Vec<u8>, bool)>> {
    let mut out = Vec::new();
    let (mut cut, mut any) = (false, false);
    loop {
        let buf = r.fill_buf().await?;
        if buf.is_empty() {
            return Ok(any.then_some((out, cut)));
        }
        any = true;
        let newline = buf.iter().position(|&b| b == b'\n');
        let chunk = &buf[..newline.unwrap_or(buf.len())];
        let room = max.saturating_sub(out.len());
        cut |= chunk.len() > room;
        out.extend_from_slice(&chunk[..chunk.len().min(room)]);
        let used = newline.map_or(buf.len(), |i| i + 1);
        r.consume(used);
        if newline.is_some() {
            return Ok(Some((out, cut)));
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
        .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
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

    let job = match Job::new().and_then(|j| j.contain(&child).map(|()| j)) {
        Ok(j) => j,
        Err(e) => {
            let _ = child.kill().await;
            return Err(e);
        }
    };

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
            let mut reader = BufReader::new(stream);
            while let Ok(Some((raw, cut))) = read_line_capped(&mut reader, MAX_LINE_BYTES).await {
                let mut line = String::from_utf8_lossy(&raw).trim_end_matches('\r').to_string();
                if cut {
                    line.push('…');
                }
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
        // A grandchild can keep the pipe open: stop reading rather than
        // leave the task behind.
        let abort = r.abort_handle();
        if tokio::time::timeout(Duration::from_secs(2), r).await.is_err() {
            abort.abort();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(args: &[&str]) -> Spec {
        Spec {
            program: "cmd.exe".into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: None,
            env: vec![],
            timeout: Duration::from_secs(20),
            stdin: None,
        }
    }

    #[tokio::test]
    async fn suspended_child_runs_after_joining_the_job() {
        let r = run(spec(&["/c", "echo hello"]), CancellationToken::new(), None).await.unwrap();
        assert!(r.success);
        assert!(r.output.contains("hello"));
    }

    #[tokio::test]
    async fn endless_line_is_cut_not_buffered() {
        // 200 KB on one line: kept to MAX_LINE_BYTES and marked as cut.
        let script = "powershell -NoProfile -Command \"[Console]::Out.Write('x' * 200000)\"";
        let r = run(spec(&["/c", script]), CancellationToken::new(), None).await.unwrap();
        assert!(r.success);
        assert!(r.output.chars().filter(|c| *c == 'x').count() <= MAX_LINE_BYTES);
    }
}
