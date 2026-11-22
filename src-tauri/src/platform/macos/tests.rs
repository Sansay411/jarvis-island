use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::error::AppError;
use crate::platform::{
    self, process,
    supervisor::{self, Spec},
};

fn spec(script: &str) -> Spec {
    Spec {
        program: "/bin/zsh".into(),
        args: vec!["-lc".into(), script.into()],
        cwd: None,
        env: vec![],
        timeout: Duration::from_secs(5),
        stdin: None,
    }
}

#[tokio::test]
async fn blocking_preserves_results_and_errors() {
    assert_eq!(platform::blocking(|| Ok(42)).await.unwrap(), 42);
    assert!(platform::blocking(|| Ok(Vec::<u8>::new()))
        .await
        .unwrap()
        .is_empty());
    assert!(matches!(
        platform::blocking(|| Err::<(), _>(AppError::invalid("тест"))).await,
        Err(AppError::InvalidInput(_))
    ));
    assert_eq!(platform::in_background(|| 7), 7);
}

#[test]
fn process_lookup_handles_current_and_missing_processes() {
    let pid = std::process::id();
    assert!(!process::process_path(pid).unwrap().is_empty());
    assert!(!process::process_name(pid).unwrap().is_empty());
    assert!(process::process_path(u32::MAX).is_none());
}

#[test]
fn which_requires_an_executable_file() {
    use std::os::unix::fs::PermissionsExt;
    assert!(supervisor::which("zsh").is_some());
    assert!(supervisor::which("").is_none());
    assert!(supervisor::which("jarvis-nonexistent-program-for-test").is_none());
    let path = std::env::temp_dir().join(format!("jarvis-which-{}", uuid::Uuid::new_v4()));
    std::fs::write(&path, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(supervisor::which(path.to_str().unwrap()).is_none());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        supervisor::which(path.to_str().unwrap()),
        Some(path.clone())
    );
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn supervisor_captures_stdin_environment_and_stderr() {
    let mut input = spec("cat; printf '%s' \"$JARVIS_PORT_TEST\"; printf failure >&2; exit 7");
    input.stdin = Some("hello\n".into());
    input.env.push(("JARVIS_PORT_TEST".into(), "world".into()));
    let result = supervisor::run(input, CancellationToken::new(), None)
        .await
        .unwrap();
    assert_eq!(result.exit_code, Some(7));
    assert!(!result.success);
    assert!(!result.cancelled && !result.timed_out);
    assert!(result.output.contains("hello"));
    assert!(result.output.contains("world"));
    assert!(result.output.contains("! failure"));
}

#[tokio::test]
async fn supervisor_handles_empty_output_and_missing_program() {
    let result = supervisor::run(spec("exit 0"), CancellationToken::new(), None)
        .await
        .unwrap();
    assert!(result.success);
    assert!(result.output.is_empty());
    let mut input = spec("");
    input.program = "/jarvis/nonexistent/program".into();
    assert!(matches!(
        supervisor::run(input, CancellationToken::new(), None).await,
        Err(AppError::NotFound(_))
    ));
}

#[tokio::test]
async fn supervisor_bounds_unterminated_lines() {
    let result = supervisor::run(
        spec("head -c 200000 /dev/zero | tr '\\0' x"),
        CancellationToken::new(),
        None,
    )
    .await
    .unwrap();
    assert!(result.success);
    assert!(result.output.chars().filter(|c| *c == 'x').count() <= 64 * 1024);
}

#[tokio::test]
async fn supervisor_timeout_kills_the_group() {
    let mut input = spec("sleep 30");
    input.timeout = Duration::from_millis(100);
    let result = supervisor::run(input, CancellationToken::new(), None)
        .await
        .unwrap();
    assert!(result.timed_out);
    assert!(!result.success && !result.cancelled);
}

#[tokio::test]
async fn cancellation_kills_descendants() {
    use std::sync::Arc;
    let marker = std::env::temp_dir().join(format!("jarvis-cancel-{}", uuid::Uuid::new_v4()));
    let mut input =
        spec("(sleep 0.5; printf escaped > \"$JARVIS_CANCEL_MARKER\") & echo ready; wait");
    input.env.push((
        "JARVIS_CANCEL_MARKER".into(),
        marker.to_string_lossy().into_owned(),
    ));
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let sink = Arc::new(move |line: &str| {
        if line == "ready" {
            signal.cancel();
        }
    });
    let result = supervisor::run(input, cancel, Some(sink)).await.unwrap();
    assert!(result.cancelled);
    assert!(!result.success && !result.timed_out);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(
        !marker.exists(),
        "descendant survived process-group cancellation"
    );
}

#[test]
fn unavailable_capabilities_report_unsupported() {
    assert!(matches!(
        platform::audio::get_state(),
        Err(AppError::Unsupported(_))
    ));
    assert!(matches!(
        platform::clipboard::write_text(""),
        Err(AppError::Unsupported(_))
    ));
    assert!(platform::winmgr::list_windows().is_empty());
}

#[test]
fn model_paths_preserve_unicode() {
    let path = std::path::Path::new("/Users/тест/модель.onnx");
    assert_eq!(
        crate::voice::models::native_path(path).unwrap(),
        path.to_str().unwrap()
    );
    assert_eq!(
        crate::voice::models::native_path(std::path::Path::new("")).unwrap(),
        ""
    );
}
