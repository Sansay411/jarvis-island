//! Developer probe for native integrations, runnable without the UI:
//!   cargo run --example probe -- <command> [args]
//! audio | sessions | devices | brightness | media | windows | monitors | apps Q
//! capture OUT.jpg | uia WINDOW_QUERY | uia-press WINDOW_QUERY NAME...
use jarvis_lib::platform::{self, apps, audio, brightness, capture, display, media, uia, winmgr};

fn main() {
    // Match the app: per-monitor DPI aware v2, so coordinates are physical.
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    platform::com::ensure_mta();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let t = std::time::Instant::now();
    let out = match cmd {
        "audio" => serde_json::to_string_pretty(&audio::get_state()).unwrap(),
        "sessions" => serde_json::to_string_pretty(&audio::list_app_sessions()).unwrap(),
        "devices" => serde_json::to_string_pretty(&audio::list_output_devices()).unwrap(),
        "brightness" => serde_json::to_string_pretty(&brightness::list()).unwrap(),
        "media" => serde_json::to_string_pretty(&media::snapshot()).unwrap(),
        "windows" => serde_json::to_string_pretty(&winmgr::list_windows()).unwrap(),
        "close" => {
            let w = winmgr::find_windows(&args[1]).into_iter().next().expect("window");
            serde_json::to_string_pretty(&winmgr::close(w.hwnd)).unwrap()
        }
        "devtools" => format!(
            "code={:?}
codex={:?}
claude={:?}
recent={:?}",
            jarvis_lib::tools::vscode_exe(),
            jarvis_lib::tools::codex_entry(),
            jarvis_lib::tools::claude_exe(),
            jarvis_lib::tools::vscode_recent().into_iter().take(5).collect::<Vec<_>>()
        ),
        "codex-run" => {
            let (node, script) = jarvis_lib::tools::codex_entry().expect("codex");
            let rt = tokio::runtime::Runtime::new().unwrap();
            let spec = jarvis_lib::platform::supervisor::Spec {
                program: node.display().to_string(),
                args: vec![script.display().to_string(), "exec".into(), "--json".into(), "--skip-git-repo-check".into(), "-C".into(), args[1].clone(), "-s".into(), "read-only".into(), "-".into()],
                cwd: Some(args[1].clone().into()),
                env: vec![],
                timeout: std::time::Duration::from_secs(180),
                stdin: Some(args[2].clone()),
            };
            let sink: jarvis_lib::platform::supervisor::LineSink = std::sync::Arc::new(|l: &str| { if l.starts_with('{') { println!("  | {}", l.chars().take(160).collect::<String>()) } });
            let r = rt.block_on(jarvis_lib::platform::supervisor::run(spec, tokio_util::sync::CancellationToken::new(), Some(sink))).unwrap();
            format!("exit={:?} success={} ms={}", r.exit_code, r.success, r.duration_ms)
        }
        "monitors" => serde_json::to_string_pretty(&display::list_monitors()).unwrap(),
        "apps" => {
            let idx = apps::AppIndex::default();
            serde_json::to_string_pretty(&idx.search(args.get(1).map(String::as_str).unwrap_or(""), 10)).unwrap()
        }
        "capture" => {
            let mon = display::primary_monitor().unwrap();
            let frame = capture::capture_rect(mon.bounds, None).unwrap();
            let enc = capture::encode(frame, capture::MAX_SIDE).unwrap();
            use base64::Engine;
            let b64 = enc.data_url.split(',').nth(1).unwrap();
            std::fs::write(&args[1], base64::engine::general_purpose::STANDARD.decode(b64).unwrap()).unwrap();
            format!("{}x{} scale={} capture={}ms encode={}ms bytes={}", enc.image_width, enc.image_height, enc.scale, enc.capture_ms, enc.encode_ms, enc.bytes)
        }
        "uia" => {
            let w = winmgr::find_windows(&args[1]).into_iter().next().expect("window");
            let s = uia::snapshot(winmgr::to_hwnd(w.hwnd), 220).unwrap();
            format!("snapshot {} «{}» {} of {} elements in {} ms\n{}", s.snapshot_id, s.window_title, s.elements.len(), s.total_found, s.elapsed_ms, uia::render(&s))
        }
        "uia-press" => {
            let w = winmgr::find_windows(&args[1]).into_iter().next().expect("window");
            let mut log = String::new();
            for name in &args[2..] {
                let s = uia::snapshot(winmgr::to_hwnd(w.hwnd), 400).unwrap();
                let el = s.elements.iter().find(|e| e.role == "button" && e.name == *name).expect("button");
                let o = uia::act(s.snapshot_id, el.id, "invoke", None).unwrap();
                log.push_str(&format!("{} ", o.element));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
            let s = uia::snapshot(winmgr::to_hwnd(w.hwnd), 400).unwrap();
            let result: Vec<_> = s.elements.iter().filter(|e| e.name.contains("Отображать как") || e.name.contains("Display is")).map(|e| e.name.clone()).collect();
            format!("{log}\nresult: {result:?}")
        }
        other => {
            eprintln!("unknown probe {other}");
            std::process::exit(2);
        }
    };
    println!("{out}\n-- {} ms", t.elapsed().as_millis());
}
