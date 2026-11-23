pub use super::data_supervisor::*;
use crate::error::{AppError, AppResult};
use std::os::unix::fs::PermissionsExt;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use tokio_util::sync::CancellationToken;
pub type LineSink = Arc<dyn Fn(&str) + Send + Sync>;
const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_RETAINED: usize = 24_000;
pub(crate) struct Job(AtomicI32);
impl Job {
    pub(crate) fn new() -> AppResult<Self> {
        Ok(Self(AtomicI32::new(0)))
    }
    pub(crate) fn contain(&self, child: &tokio::process::Child) -> AppResult<()> {
        let pid = child
            .id()
            .ok_or_else(|| AppError::platform("Процесс завершился при запуске"))?;
        self.0.store(pid as i32, Ordering::Relaxed);
        Ok(())
    }
    fn kill(&self) {
        let pid = self.0.load(Ordering::Relaxed);
        if pid > 0 {
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.kill();
    }
}
pub(crate) async fn read_line_capped<R: tokio::io::AsyncBufRead + Unpin>(
    r: &mut R,
    max: usize,
) -> std::io::Result<Option<(Vec<u8>, bool)>> {
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

pub async fn run(
    spec: Spec,
    cancel: CancellationToken,
    on_line: Option<LineSink>,
) -> AppResult<ProcessResult> {
    let started = Instant::now();
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => {
            AppError::not_found(format!("Программа «{}» не найдена", spec.program))
        }
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

    let retained = Arc::new(parking_lot::Mutex::new(Retained {
        text: String::new(),
        truncated: false,
    }));
    let mut readers = Vec::new();
    for (stream, is_err) in [
        (
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            false,
        ),
        (
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            true,
        ),
    ] {
        let Some(stream) = stream else { continue };
        let retained = retained.clone();
        let sink = on_line.clone();
        readers.push(tokio::spawn(async move {
            let mut reader = BufReader::new(stream);
            while let Ok(Some((raw, cut))) = read_line_capped(&mut reader, MAX_LINE_BYTES).await {
                let mut line = String::from_utf8_lossy(&raw)
                    .trim_end_matches('\r')
                    .to_string();
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
        if tokio::time::timeout(Duration::from_secs(2), r)
            .await
            .is_err()
        {
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

pub fn which(name: &str) -> Option<PathBuf> {
    fn executable(p: &Path) -> bool {
        p.is_file()
            && p.metadata()
                .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    if name.contains('/') {
        let p = PathBuf::from(name);
        return executable(&p).then_some(p);
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| executable(p))
}
