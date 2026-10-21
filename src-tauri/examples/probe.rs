//! Developer probe for native integrations, runnable without the UI:
//!   cargo run --example probe -- <audio|brightness|media|windows|apps QUERY|monitors|fullscreen>
use jarvis_lib::platform::{self, apps, audio, brightness, display, media, winmgr};

fn main() {
    platform::com::ensure_mta();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("all");
    let t = std::time::Instant::now();
    let out = match cmd {
        "audio" => serde_json::to_string_pretty(&audio::get_state()),
        "sessions" => serde_json::to_string_pretty(&audio::list_app_sessions()),
        "devices" => serde_json::to_string_pretty(&audio::list_output_devices()),
        "brightness" => serde_json::to_string_pretty(&brightness::list()),
        "media" => serde_json::to_string_pretty(&media::snapshot()),
        "windows" => serde_json::to_string_pretty(&winmgr::list_windows()),
        "monitors" => serde_json::to_string_pretty(&display::list_monitors()),
        "apps" => {
            let idx = apps::AppIndex::default();
            serde_json::to_string_pretty(&idx.search(args.get(1).map(String::as_str).unwrap_or(""), 10))
        }
        other => {
            eprintln!("unknown probe {other}");
            std::process::exit(2);
        }
    }
    .unwrap();
    println!("{out}\n-- {} ms", t.elapsed().as_millis());
}
