use crate::error::{AppError, AppResult};
use serde_json::Value;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAX_OUTPUT: u64 = 32 * 1024 * 1024;

pub(super) fn execute(program: &str, args: &[&str], input: Option<Vec<u8>>) -> AppResult<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::platform(format!("Не удалось запустить {program}: {e}")))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = |stream: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            stream
                .take(MAX_OUTPUT + 1)
                .read_to_end(&mut out)
                .map(|_| out)
        })
    };
    let out = read(Box::new(stdout));
    let err = read(Box::new(stderr));
    let writer = input.map(|bytes| {
        let mut stdin = child.stdin.take().unwrap();
        std::thread::spawn(move || stdin.write_all(&bytes))
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if start.elapsed() < Duration::from_secs(12) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(AppError::Timeout("macOS не ответила за 12 секунд".into()));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(AppError::platform(format!(
                    "Не удалось дождаться ответа macOS: {e}"
                )));
            }
        }
    };
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let out = out
        .join()
        .map_err(|_| AppError::internal("Не удалось прочитать ответ macOS"))??;
    let err = err
        .join()
        .map_err(|_| AppError::internal("Не удалось прочитать ошибку macOS"))??;
    if !status?.success() {
        return Err(script_error(&String::from_utf8_lossy(&err)));
    }
    if out.len() as u64 > MAX_OUTPUT {
        return Err(AppError::unsupported("Ответ macOS слишком большой"));
    }
    String::from_utf8(out).map_err(|_| AppError::platform("macOS вернула некорректный текст"))
}

fn script_error(error: &str) -> AppError {
    if [
        "-1743",
        "-1719",
        "-25211",
        "not authorized",
        "not allowed assistive",
    ]
    .iter()
    .any(|s| error.contains(s))
    {
        AppError::PermissionDenied("Разрешите Джарвису «Универсальный доступ» и «Автоматизацию» в настройках конфиденциальности macOS".into())
    } else {
        AppError::platform(format!("Ошибка macOS: {}", error.trim()))
    }
}

pub(super) fn jxa(script: &str, args: Value) -> AppResult<Value> {
    let code = format!(
        r#"ObjC.import('AppKit'); ObjC.import('Foundation');
function run() {{
    const a = JSON.parse(ObjC.unwrap($.NSString.alloc.initWithDataEncoding($.NSFileHandle.fileHandleWithStandardInput.readDataToEndOfFile, $.NSUTF8StringEncoding)));
    return JSON.stringify((function() {{ {script} }})());
}}"#
    );
    let out = execute(
        "/usr/bin/osascript",
        &["-l", "JavaScript", "-e", &code],
        Some(serde_json::to_vec(&args)?),
    )?;
    serde_json::from_str(out.trim())
        .map_err(|_| AppError::platform("Не удалось разобрать ответ macOS"))
}

pub(super) fn apple(script: &str) -> AppResult<String> {
    execute("/usr/bin/osascript", &["-e", script], None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permission_errors_have_actionable_russian_messages() {
        assert!(matches!(
            script_error("Not authorized (-1743)"),
            AppError::PermissionDenied(_)
        ));
        assert!(matches!(script_error("failure"), AppError::Platform(_)));
    }
}
