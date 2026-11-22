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
fn capabilities_outside_this_step_report_unsupported() {
    assert!(matches!(
        platform::audio::list_app_sessions(),
        Err(AppError::Unsupported(_))
    ));
    assert!(matches!(
        platform::capture::capture_rect(platform::display::Rect::default(), None),
        Err(AppError::Unsupported(_))
    ));
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

#[test]
#[ignore = "изменяет системную громкость и буфер обмена с последующим восстановлением"]
fn macos_live_smoke() {
    use platform::{apps::AppIndex, audio, clipboard, native, winmgr};
    use serde_json::{json, Value};

    struct ClipboardRestore(Value);
    impl Drop for ClipboardRestore {
        fn drop(&mut self) {
            let result = native::jxa(
                r#"
const p=$.NSPasteboard.generalPasteboard, items=$.NSMutableArray.alloc.init;
for(const saved of a) {
    const item=$.NSPasteboardItem.alloc.init;
    for(const entry of saved) {
        const data=$.NSData.alloc.initWithBase64EncodedStringOptions(entry.data,0);
        if(!item.setDataForType(data,entry.type)) throw Error('Не удалось восстановить формат буфера обмена');
    }
    items.addObject(item);
}
p.clearContents;
if(items.count && !p.writeObjects(items)) throw Error('Не удалось восстановить буфер обмена');
return true;
"#,
                self.0.clone(),
            );
            assert!(result.is_ok(), "не удалось восстановить буфер обмена");
        }
    }
    match audio::get_state() {
        Ok(original) => {
            let restored = audio::set_volume(original.volume).expect("вернуть исходную громкость");
            assert_eq!(restored.volume, original.volume);
            assert_eq!(restored.muted, original.muted);
            println!(
                "audio: volume={}, muted={}; set_volume(original) verified",
                restored.volume, restored.muted
            );
        }
        Err(e) => println!("audio: {e}; set_volume skipped: no original device volume"),
    }
    let apps = AppIndex::default().all().unwrap();
    println!(
        "apps: count={}, first 3={:?}",
        apps.len(),
        apps.iter().take(3).map(|a| &a.name).collect::<Vec<_>>()
    );
    assert!(!apps.is_empty());
    println!("windows: count={}", winmgr::list_windows().len());
    let saved = native::jxa(r#"
const p=$.NSPasteboard.generalPasteboard, result=[];
const types=ObjC.deepUnwrap(p.types)||[];
if(types.includes('org.nspasteboard.ConcealedType')) throw Error('Буфер обмена содержит конфиденциальные данные');
const items=p.pasteboardItems;
if(items && !items.isNil()) for(let i=0;i<items.count;i++) {
    const item=items.objectAtIndex(i), saved=[];
    for(const type of ObjC.deepUnwrap(item.types)) {
        const data=item.dataForType(type);
        if(data && !data.isNil()) saved.push({type:type,data:ObjC.unwrap(data.base64EncodedStringWithOptions(0))});
    }
    result.push(saved);
}
return result;
"#, json!(null)).expect("сохранить буфер обмена");
    let restore = ClipboardRestore(saved);
    let text = "Jarvis macOS smoke — тест\n\"quotes\" \\ path";
    clipboard::write_text(text).unwrap();
    assert_eq!(clipboard::read().unwrap().text.as_deref(), Some(text));
    println!("clipboard: text write/read verified");

    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([20, 40, 60, 255]));
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    clipboard::write_image(&png, &[]).unwrap();
    assert_eq!(
        clipboard::snapshot().unwrap().png.as_deref(),
        Some(png.as_slice())
    );
    println!("clipboard: PNG write/read verified");

    let path = std::env::temp_dir().join(format!("jarvis-pasteboard-{}.txt", uuid::Uuid::new_v4()));
    std::fs::write(&path, b"smoke").unwrap();
    clipboard::write_files(&[path.to_string_lossy().into_owned()]).unwrap();
    let read = clipboard::read().unwrap();
    assert_eq!(
        read.files,
        vec![std::fs::canonicalize(&path)
            .unwrap()
            .to_string_lossy()
            .into_owned()]
    );
    std::fs::remove_file(path).unwrap();
    println!("clipboard: file list write/read verified");
    drop(restore);
    println!("clipboard: original formats restored");
}
