//! Audio and video through the user's own FFmpeg (found on PATH or where
//! winget installs it). Container changes and track extraction copy the
//! streams (`-c copy`) — no re-encoding, so nothing is lost; re-encoding
//! operations are named as such in the UI.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::error::{AppError, AppResult};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn find_tool(name: &str) -> Option<PathBuf> {
    let exe = format!("{name}.exe");
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join(&exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    // winget: %LOCALAPPDATA%\Microsoft\WinGet\Links and the package folders
    // (PATH of an app started before the install does not have them yet).
    let local = dirs::data_local_dir()?;
    let links = local.join("Microsoft").join("WinGet").join("Links").join(&exe);
    if links.is_file() {
        return Some(links);
    }
    let packages = local.join("Microsoft").join("WinGet").join("Packages");
    for pkg in std::fs::read_dir(packages).ok()?.flatten() {
        if !pkg.file_name().to_string_lossy().to_lowercase().contains("ffmpeg") {
            continue;
        }
        for build in std::fs::read_dir(pkg.path()).ok()?.flatten() {
            let p = build.path().join("bin").join(&exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub fn ffmpeg() -> AppResult<PathBuf> {
    find_tool("ffmpeg").ok_or_else(|| {
        AppError::unsupported("Для аудио и видео нужен FFmpeg. Установите его командой «winget install Gyan.FFmpeg» и повторите.")
    })
}

pub fn available() -> bool {
    find_tool("ffmpeg").is_some() && find_tool("ffprobe").is_some()
}

fn command(tool: &Path) -> Command {
    let mut c = Command::new(tool);
    #[cfg(windows)]
    c.creation_flags(CREATE_NO_WINDOW);
    c.stdin(Stdio::null());
    c
}

#[derive(Debug, Clone, Default)]
pub struct Probe {
    pub duration_s: Option<f64>,
    pub video: Option<String>,
    pub audio: Option<String>,
    pub audio_bits: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub audio_streams: usize,
    pub subtitle_streams: usize,
}

/// Stream facts (ffprobe JSON), killed after `timeout`.
pub fn probe(path: &Path, timeout: Duration) -> AppResult<Probe> {
    let tool = find_tool("ffprobe").ok_or_else(|| AppError::unsupported("ffprobe не найден"))?;
    let mut child = command(&tool)
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::platform(format!("ffprobe: {e}")))?;
    let mut out = String::new();
    let mut stdout = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let _ = stdout.read_to_string(&mut out);
        out
    });
    let start = std::time::Instant::now();
    loop {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            return Err(AppError::Timeout("ffprobe не ответил вовремя".into()));
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    let text = reader.join().unwrap_or_default();
    let v: Value = serde_json::from_str(&text).map_err(|_| AppError::invalid("Файл не читается как аудио или видео"))?;
    let mut p = Probe { duration_s: v["format"]["duration"].as_str().and_then(|d| d.parse().ok()), ..Default::default() };
    for s in v["streams"].as_array().into_iter().flatten() {
        let codec = s["codec_name"].as_str().map(str::to_string);
        match s["codec_type"].as_str() {
            Some("video") if s["disposition"]["attached_pic"].as_i64() != Some(1) && p.video.is_none() => {
                p.video = codec;
                p.width = s["width"].as_u64().map(|x| x as u32);
                p.height = s["height"].as_u64().map(|x| x as u32);
            }
            Some("audio") => {
                p.audio_streams += 1;
                if p.audio.is_none() {
                    p.audio = codec;
                    p.audio_bits = s["bits_per_raw_sample"].as_str().and_then(|b| b.parse().ok()).filter(|b| *b > 0);
                }
            }
            Some("subtitle") => p.subtitle_streams += 1,
            _ => {}
        }
    }
    Ok(p)
}

/// Codecs an MP4 container carries without conversion.
pub fn mp4_compatible(p: &Probe) -> bool {
    let v_ok = p.video.as_deref().is_none_or(|v| matches!(v, "h264" | "hevc" | "av1" | "mpeg4" | "vp9"));
    let a_ok = p.audio.as_deref().is_none_or(|a| matches!(a, "aac" | "mp3" | "opus" | "flac" | "alac" | "ac3" | "eac3"));
    v_ok && a_ok && p.video.is_some()
}

/// File extension that holds this audio codec as is.
pub fn audio_container(codec: &str) -> &'static str {
    match codec {
        "aac" | "alac" => "m4a",
        "mp3" => "mp3",
        "opus" => "opus",
        "vorbis" => "ogg",
        "flac" => "flac",
        c if c.starts_with("pcm_") => "wav",
        _ => "mka",
    }
}

/// Run ffmpeg with `-progress`, reporting 0..1 against `duration`; `cancel`
/// returning true kills it. The output is verified by the caller.
pub fn run(args: &[String], duration: Option<f64>, mut progress: impl FnMut(f32) -> bool) -> AppResult<()> {
    let tool = ffmpeg()?;
    let mut child = command(&tool)
        .args(["-hide_banner", "-nostdin", "-nostats", "-loglevel", "error", "-progress", "pipe:1"])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::platform(format!("Не удалось запустить FFmpeg: {e}")))?;
    let mut stderr = child.stderr.take().expect("piped");
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let stdout = child.stdout.take().expect("piped");
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<f64>().ok()) {
            let frac = duration.filter(|d| *d > 0.0).map(|d| (us / 1e6 / d).clamp(0.0, 1.0) as f32).unwrap_or(0.0);
            if !progress(frac) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::Cancelled);
            }
        }
    }
    let status = child.wait().map_err(|e| AppError::platform(format!("FFmpeg: {e}")))?;
    let err = errors.join().unwrap_or_default();
    if !status.success() {
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("неизвестная ошибка").trim().to_string();
        return Err(AppError::platform(format!("FFmpeg: {last}")));
    }
    Ok(())
}

/// Mono 16 kHz samples of the first audio track (for recognition), handed
/// to `on_samples` as they are decoded so hours of audio never sit in memory.
/// `on_samples` returns false to cancel.
pub fn decode_pcm16k(path: &Path, mut on_samples: impl FnMut(&[f32]) -> AppResult<bool>) -> AppResult<()> {
    let tool = ffmpeg()?;
    let mut child = command(&tool)
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-ac", "1", "-ar", "16000", "-f", "s16le", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::platform(format!("Не удалось запустить FFmpeg: {e}")))?;
    let mut stdout = child.stdout.take().expect("piped");
    let mut pcm = Vec::with_capacity(32 * 1024);
    let mut any = false;
    let mut buf = vec![0u8; 64 * 1024];
    let mut carry: Option<u8> = None;
    loop {
        let n = match stdout.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                let _ = child.kill();
                return Err(AppError::platform(format!("FFmpeg: {e}")));
            }
        };
        if n == 0 {
            break;
        }
        pcm.clear();
        let mut bytes: &[u8] = &buf[..n];
        if let Some(lo) = carry.take() {
            pcm.push(i16::from_le_bytes([lo, bytes[0]]) as f32 / 32768.0);
            bytes = &bytes[1..];
        }
        let mut it = bytes.chunks_exact(2);
        for b in &mut it {
            pcm.push(i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0);
        }
        carry = it.remainder().first().copied();
        any |= !pcm.is_empty();
        match on_samples(&pcm) {
            Ok(true) => {}
            Ok(false) => {
                let _ = child.kill();
                return Err(AppError::Cancelled);
            }
            Err(e) => {
                let _ = child.kill();
                return Err(e);
            }
        }
    }
    let status = child.wait().map_err(|e| AppError::platform(format!("FFmpeg: {e}")))?;
    if !status.success() || !any {
        return Err(AppError::invalid("В файле нет звуковой дорожки, которую можно прочитать"));
    }
    Ok(())
}

/// One video frame as PNG bytes, scaled to `width` (thumbnails).
pub fn frame_png(path: &Path, at_s: f64, width: Option<u32>, timeout: Duration) -> AppResult<Vec<u8>> {
    let tool = ffmpeg()?;
    let mut c = command(&tool);
    c.args(["-hide_banner", "-loglevel", "error", "-ss", &format!("{at_s:.2}"), "-i"]).arg(path).args(["-frames:v", "1"]);
    if let Some(w) = width {
        c.args(["-vf", &format!("scale={w}:-2")]);
    }
    let mut child = c
        .args(["-f", "image2pipe", "-c:v", "png", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::platform(format!("FFmpeg: {e}")))?;
    let mut stdout = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = stdout.read_to_end(&mut v);
        v
    });
    let start = std::time::Instant::now();
    while child.try_wait().ok().flatten().is_none() {
        if start.elapsed() > timeout {
            let _ = child.kill();
            return Err(AppError::Timeout("Кадр не извлёкся вовремя".into()));
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    let png = reader.join().unwrap_or_default();
    if png.len() < 8 || &png[1..4] != b"PNG" {
        return Err(AppError::invalid("Не удалось получить кадр"));
    }
    Ok(png)
}

/// SRT timestamp.
pub fn srt_time(s: f64) -> String {
    let ms = (s.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containers_and_compatibility() {
        assert_eq!(audio_container("aac"), "m4a");
        assert_eq!(audio_container("pcm_s16le"), "wav");
        assert_eq!(audio_container("dts"), "mka");
        let p = Probe { video: Some("h264".into()), audio: Some("aac".into()), ..Default::default() };
        assert!(mp4_compatible(&p));
        let p = Probe { video: Some("vp8".into()), audio: Some("vorbis".into()), ..Default::default() };
        assert!(!mp4_compatible(&p));
        assert_eq!(srt_time(3725.5), "01:02:05,500");
    }
}
