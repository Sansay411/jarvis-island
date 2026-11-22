use serde::Serialize;
use std::{path::PathBuf, time::Duration};

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
