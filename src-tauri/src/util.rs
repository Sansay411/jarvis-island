//! Small cross-cutting helpers.

#[cfg(windows)]
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
#[cfg(windows)]
use windows::Win32::System::Threading::GetCurrentProcess;

/// Current process working set in MiB (host process only; WebView2 runs in
/// separate processes and is measured externally).
#[cfg(windows)]
pub fn working_set_mb() -> f64 {
    unsafe {
        let mut pmc = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb).is_ok() {
            pmc.WorkingSetSize as f64 / (1024.0 * 1024.0)
        } else {
            0.0
        }
    }
}

#[cfg(target_os = "macos")]
pub fn working_set_mb() -> f64 {
    std::process::Command::new("/bin/ps").args(["-p", &std::process::id().to_string(), "-o", "rss="]).output().ok()
        .and_then(|o| String::from_utf8(o.stdout).ok()).and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.0) / 1024.0
}

/// «2 ч 30 мин», «45 мин», «9 ч»: a duration the way the island writes it
/// (the voice pipeline reads the units out in words).
pub fn human_minutes(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} мин"),
        (h, 0) => format!("{h} ч"),
        (h, m) => format!("{h} ч {m} мин"),
    }
}
