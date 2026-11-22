//! Media playback via Global System Media Transport Controls (GSMTC).
//! Reads the current session's metadata, artwork, timeline and capabilities,
//! controls playback, and pushes change events to the island. Falls back to
//! virtual media keys when an app doesn't expose a controllable session.

use std::io::Cursor;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use base64::Engine;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as SessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};
use windows::Storage::Streams::{DataReader, IRandomAccessStreamReference};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP,
};

use crate::error::{AppError, AppResult};

pub const EVT_MEDIA: &str = "media:changed";
pub const EVT_ARTWORK: &str = "media:artwork";

pub use super::data_media::MediaState;

pub use super::data_media::ArtworkEvent;

pub use super::data_media::MediaAction;

/// WinRT DateTime ticks (100 ns since 1601) → Unix ms.
fn winrt_to_unix_ms(ticks: i64) -> i64 {
    (ticks - 116_444_736_000_000_000) / 10_000
}

pub fn manager() -> AppResult<SessionManager> {
    Ok(SessionManager::RequestAsync()?.join()?)
}

/// The session to show and control: one that is playing, else the one
/// Windows calls current, else any. Windows leaves «current» empty for some
/// browsers even while a session exists, and the player then never showed.
fn current_session(mgr: &SessionManager) -> Option<Session> {
    let all: Vec<Session> = mgr.GetSessions().map(|v| v.into_iter().collect()).unwrap_or_default();
    let playing = all.iter().find(|s| {
        s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).is_ok_and(|st| st == PlaybackStatus::Playing)
    });
    playing.cloned().or_else(|| mgr.GetCurrentSession().ok()).or_else(|| all.into_iter().next())
}

fn status_str(s: PlaybackStatus) -> &'static str {
    match s {
        PlaybackStatus::Playing => "playing",
        PlaybackStatus::Paused => "paused",
        PlaybackStatus::Stopped => "stopped",
        PlaybackStatus::Changing => "changing",
        PlaybackStatus::Opened => "opened",
        _ => "closed",
    }
}

fn session_state(session: &Session) -> AppResult<(MediaState, Option<IRandomAccessStreamReference>)> {
    let props = session.TryGetMediaPropertiesAsync()?.join()?;
    let info = session.GetPlaybackInfo()?;
    let controls = info.Controls()?;
    let timeline = session.GetTimelineProperties()?;

    let title = props.Title().map(|h| h.to_string()).unwrap_or_default();
    let artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
    let album = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
    let source_app = session.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();

    let end = timeline.EndTime().map(|t| t.Duration / 10_000).unwrap_or(0);
    let start = timeline.StartTime().map(|t| t.Duration / 10_000).unwrap_or(0);
    let pos = timeline.Position().map(|t| t.Duration / 10_000).unwrap_or(0);
    let sampled = timeline.LastUpdatedTime().map(|d| winrt_to_unix_ms(d.UniversalTime)).ok();
    let has_timeline = end > start;

    let site = browser_site(&source_app, &title);
    let state = MediaState {
        available: true,
        site,
        track_key: format!("{source_app}\u{1f}{title}\u{1f}{artist}\u{1f}{album}"),
        source_app,
        title,
        artist,
        album,
        status: status_str(info.PlaybackStatus()?).to_string(),
        position_ms: has_timeline.then_some(pos - start),
        duration_ms: has_timeline.then_some(end - start),
        position_sampled_at: if has_timeline { sampled } else { None },
        can_toggle: controls.IsPlayPauseToggleEnabled().unwrap_or(false)
            || controls.IsPlayEnabled().unwrap_or(false)
            || controls.IsPauseEnabled().unwrap_or(false),
        can_next: controls.IsNextEnabled().unwrap_or(false),
        can_previous: controls.IsPreviousEnabled().unwrap_or(false),
    };
    Ok((state, props.Thumbnail().ok()))
}

fn is_browser(aumid: &str) -> bool {
    let a = aumid.to_lowercase();
    ["chrome", "msedge", "firefox", "brave", "opera", "vivaldi", "browser"].iter().any(|b| a.contains(b))
}

static SITE_CACHE: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));

fn site_of(window_title: &str) -> Option<&'static str> {
    let t = window_title.to_lowercase();
    let has = |k: &str| t.contains(k);
    Some(if has("youtube") {
        "youtube"
    } else if has("\u{44f}\u{43d}\u{434}\u{435}\u{43a}\u{441} \u{43c}\u{443}\u{437}\u{44b}\u{43a}") || has("yandex music") || has("music.yandex") {
        "yandex"
    } else if has("spotify") {
        "spotify"
    } else if has("soundcloud") {
        "soundcloud"
    } else if has("vk.com") || has("vk \u{43c}\u{443}\u{437}\u{44b}\u{43a}") || has("\u{432}\u{43a}\u{43e}\u{43d}\u{442}\u{430}\u{43a}\u{442}\u{435}") {
        "vk"
    } else if has("twitch") {
        "twitch"
    } else {
        return None;
    })
}

/// Which site a browser plays from: the browser window whose title holds the
/// track title usually ends with the site's name (" - YouTube").
fn browser_site(aumid: &str, title: &str) -> String {
    if !is_browser(aumid) || title.trim().is_empty() {
        return String::new();
    }
    {
        let c = SITE_CACHE.lock();
        if c.0 == title {
            return c.1.clone();
        }
    }
    let needle = title.to_lowercase();
    let site = crate::platform::winmgr::list_windows()
        .into_iter()
        .filter(|w| w.title.to_lowercase().contains(&needle))
        .find_map(|w| site_of(&w.title))
        .unwrap_or("")
        .to_string();
    *SITE_CACHE.lock() = (title.to_string(), site.clone());
    site
}

pub fn snapshot() -> AppResult<MediaState> {
    let mgr = manager()?;
    match current_session(&mgr) {
        Some(s) => Ok(session_state(&s)?.0),
        None => Ok(MediaState::default()),
    }
}

/// Read, downscale (≤160 px) and encode the thumbnail as a JPEG data URL.
fn read_artwork(r: &IRandomAccessStreamReference) -> Option<String> {
    let stream = r.OpenReadAsync().ok()?.join().ok()?;
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    // Some apps report Size() = 0 for a stream that does hold an image:
    // read in chunks until it runs dry instead of trusting the size.
    let _ = reader.SetInputStreamOptions(windows::Storage::Streams::InputStreamOptions::Partial);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let n = reader.LoadAsync(64 * 1024).ok()?.join().ok()?;
        if n == 0 {
            break;
        }
        let start = buf.len();
        buf.resize(start + n as usize, 0);
        reader.ReadBytes(&mut buf[start..]).ok()?;
        if buf.len() > 8 * 1024 * 1024 {
            return None;
        }
    }
    if buf.is_empty() {
        return None;
    }
    let img = image::load_from_memory(&buf).ok()?;
    let thumb = img.thumbnail(160, 160).to_rgb8();
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 86)
        .encode_image(&thumb)
        .ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(out.into_inner())
    ))
}

pub fn artwork() -> AppResult<ArtworkEvent> {
    let mgr = manager()?;
    let Some(s) = current_session(&mgr) else {
        return Ok(ArtworkEvent { track_key: String::new(), data_url: None });
    };
    let (state, thumb) = session_state(&s)?;
    Ok(ArtworkEvent { track_key: state.track_key, data_url: thumb.as_ref().and_then(read_artwork) })
}

fn send_media_key(vk: VIRTUAL_KEY) {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    };
    let inputs = [key(Default::default()), key(KEYEVENTF_KEYUP)];
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}

pub use super::data_media::ControlOutcome;

pub fn control(action: MediaAction) -> AppResult<ControlOutcome> {
    let mgr = manager()?;
    let Some(session) = current_session(&mgr) else {
        // No GSMTC session: a media key still reaches apps that only
        // listen for hardware keys.
        let vk = match action {
            MediaAction::PlayPause | MediaAction::Play | MediaAction::Pause => VK_MEDIA_PLAY_PAUSE,
            MediaAction::Next => VK_MEDIA_NEXT_TRACK,
            MediaAction::Previous => VK_MEDIA_PREV_TRACK,
            MediaAction::Stop => VK_MEDIA_STOP,
        };
        send_media_key(vk);
        return Ok(ControlOutcome { state: MediaState::default(), method: "media_key", verified: false });
    };
    let before = session_state(&session)?.0;
    let accepted = match action {
        MediaAction::PlayPause => session.TryTogglePlayPauseAsync()?.join()?,
        MediaAction::Play => session.TryPlayAsync()?.join()?,
        MediaAction::Pause => session.TryPauseAsync()?.join()?,
        MediaAction::Next => session.TrySkipNextAsync()?.join()?,
        MediaAction::Previous => session.TrySkipPreviousAsync()?.join()?,
        MediaAction::Stop => session.TryStopAsync()?.join()?,
    };
    if !accepted {
        return Err(AppError::unsupported(format!(
            "Приложение {} не поддерживает это действие",
            if before.source_app.is_empty() { "плеера".to_string() } else { format!("«{}»", before.source_app) }
        )));
    }
    // Verify against the new state; apps apply commands asynchronously.
    let deadline = Instant::now() + Duration::from_millis(1200);
    loop {
        std::thread::sleep(Duration::from_millis(120));
        let after = session_state(&session)?.0;
        let verified = match action {
            MediaAction::PlayPause => after.status != before.status,
            MediaAction::Play => after.status == "playing",
            MediaAction::Pause => after.status == "paused",
            MediaAction::Stop => after.status == "stopped" || after.status == "closed",
            MediaAction::Next | MediaAction::Previous => after.track_key != before.track_key,
        };
        if verified || Instant::now() > deadline {
            return Ok(ControlOutcome { state: after, method: "session", verified });
        }
    }
}

/// Scrub playback position (`position_ms` relative to track start) via GSMTC.
pub fn seek(position_ms: i64) -> AppResult<()> {
    let mgr = manager()?;
    let Some(session) = current_session(&mgr) else {
        return Err(AppError::not_found("Нет активного медиасеанса"));
    };
    let timeline = session.GetTimelineProperties()?;
    let start_ticks = timeline.StartTime().map(|t| t.Duration).unwrap_or(0);
    let end_ticks = timeline.EndTime().map(|t| t.Duration).unwrap_or(0);
    let offset_ticks = position_ms.max(0).saturating_mul(10_000);
    let mut target_ticks = start_ticks.saturating_add(offset_ticks);
    if end_ticks > start_ticks {
        target_ticks = target_ticks.clamp(start_ticks, end_ticks);
    }
    let accepted = session.TryChangePlaybackPositionAsync(target_ticks)?.join()?;
    if !accepted {
        let app = session.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
        return Err(AppError::unsupported(format!(
            "Приложение {} не поддерживает перемотку по шкале времени",
            if app.is_empty() { "плеера".to_string() } else { format!("«{app}»") }
        )));
    }
    Ok(())
}

enum Signal {
    SessionChanged,
    Properties,
    Playback,
}

struct Bound {
    session: Session,
    tokens: [i64; 3],
}

/// Keeps GSMTC event subscriptions alive and forwards debounced changes to
/// the island. All WinRT callbacks only enqueue a signal; the worker thread
/// does the (blocking) reads.
pub struct MediaWatcher {
    bound: Mutex<Option<Bound>>,
}

impl MediaWatcher {
    pub fn spawn(app: AppHandle) -> AppResult<()> {
        let (tx, rx) = mpsc::channel::<Signal>();
        std::thread::Builder::new()
            .name("media-watcher".into())
            .spawn(move || {
                crate::platform::com::ensure_mta();
                let mgr = match manager() {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("GSMTC unavailable: {e}");
                        return;
                    }
                };
                let watcher = MediaWatcher { bound: Mutex::new(None) };
                let tx_mgr = tx.clone();
                let _mgr_token = mgr.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = tx_mgr.send(Signal::SessionChanged);
                    Ok(())
                }));
                // A session appearing or going away (a browser tab starting
                // music) may not change «current»: rebind on that too.
                let tx_list = tx.clone();
                let _list_token = mgr.SessionsChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = tx_list.send(Signal::SessionChanged);
                    Ok(())
                }));
                watcher.rebind(&mgr, &tx);
                let mut last_state: Option<MediaState> = None;
                let mut last_art_key = String::new();
                // Apps often publish the title first and the cover a moment
                // later (browsers, Yandex Music), sometimes still showing the
                // previous track's cover in between. So for a few seconds
                // after a track change every properties event re-reads the
                // cover, and a missing one is retried on the 1.5 s tick.
                let mut last_art: Option<String> = None;
                let mut art_since = Instant::now();
                let mut art_retries: u8 = 0;
                loop {
                    // Windows does not always say when another app starts
                    // playing (both sessions already existed): every 1.5 s
                    // check that the bound session is still the one to show.
                    let first = match rx.recv_timeout(Duration::from_millis(1500)) {
                        Ok(sig) => sig,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            let best = current_session(&mgr).and_then(|s| s.SourceAppUserModelId().ok()).map(|h| h.to_string());
                            let bound = watcher.bound.lock().as_ref().and_then(|b| b.session.SourceAppUserModelId().ok()).map(|h| h.to_string());
                            if best == bound {
                                if last_art.is_none() && !last_art_key.is_empty() && art_retries < 5 {
                                    art_retries += 1;
                                    Signal::Properties
                                } else {
                                    continue;
                                }
                            } else {
                                Signal::SessionChanged
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    // Debounce bursts (apps fire several events per change).
                    let mut session_changed = matches!(first, Signal::SessionChanged);
                    let mut props_changed = matches!(first, Signal::Properties);
                    while let Ok(sig) = rx.recv_timeout(Duration::from_millis(120)) {
                        match sig {
                            Signal::SessionChanged => session_changed = true,
                            Signal::Properties => props_changed = true,
                            Signal::Playback => {}
                        }
                    }
                    if session_changed {
                        watcher.rebind(&mgr, &tx);
                    }
                    let bound = watcher.bound.lock().as_ref().map(|b| b.session.clone());
                    let (state, thumb) = match bound.as_ref().map(session_state) {
                        Some(Ok((s, t))) => (s, t),
                        Some(Err(e)) => {
                            tracing::debug!("media snapshot failed: {e}");
                            continue;
                        }
                        None => (MediaState::default(), None),
                    };
                    if last_state.as_ref() != Some(&state) {
                        let _ = app.emit(EVT_MEDIA, &state);
                    }
                    // Artwork is only re-read when the track identity changes;
                    // timeline/playback ticks never touch it.
                    let new_track = state.track_key != last_art_key;
                    if new_track {
                        art_since = Instant::now();
                        art_retries = 0;
                    }
                    let settling = props_changed && art_since.elapsed() < Duration::from_secs(8);
                    if new_track || settling {
                        let data_url = thumb.as_ref().and_then(read_artwork);
                        if new_track || data_url != last_art {
                            let _ = app.emit(
                                EVT_ARTWORK,
                                ArtworkEvent { track_key: state.track_key.clone(), data_url: data_url.clone() },
                            );
                        }
                        last_art = data_url;
                        last_art_key = state.track_key.clone();
                    }
                    last_state = Some(state);
                }
            })
            .map_err(|e| AppError::internal(format!("media watcher thread: {e}")))?;
        Ok(())
    }

    fn rebind(&self, mgr: &SessionManager, tx: &mpsc::Sender<Signal>) {
        let mut bound = self.bound.lock();
        if let Some(old) = bound.take() {
            let _ = old.session.RemoveMediaPropertiesChanged(old.tokens[0]);
            let _ = old.session.RemovePlaybackInfoChanged(old.tokens[1]);
            let _ = old.session.RemoveTimelinePropertiesChanged(old.tokens[2]);
        }
        let Some(session) = current_session(mgr) else { return };
        let (t1, t2, t3) = (tx.clone(), tx.clone(), tx.clone());
        let tokens = (|| -> windows::core::Result<[i64; 3]> {
            Ok([
                session.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = t1.send(Signal::Properties);
                    Ok(())
                }))?,
                session.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = t2.send(Signal::Playback);
                    Ok(())
                }))?,
                session.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = t3.send(Signal::Playback);
                    Ok(())
                }))?,
            ])
        })();
        match tokens {
            Ok(tokens) => {
                *bound = Some(Bound { session, tokens });
                // Emit the initial state of the newly bound session.
                let _ = tx.send(Signal::Properties);
            }
            Err(e) => tracing::warn!("subscribing to media session failed: {e}"),
        }
    }
}

#[cfg(test)]
mod live {
    /// `cargo test --lib live_media_sessions -- --ignored --nocapture`: every
    /// media session Windows knows, and which one is «current».
    #[test]
    #[ignore]
    fn live_media_sessions() {
        crate::platform::com::ensure_mta();
        let mgr = super::manager().unwrap();
        let cur = mgr.GetCurrentSession().ok().and_then(|s| s.SourceAppUserModelId().ok()).map(|h| h.to_string());
        println!("current: {cur:?}");
        for s in mgr.GetSessions().unwrap() {
            let (st, _) = super::session_state(&s).unwrap();
            println!("session: app={} status={} title={:?} artist={:?}", st.source_app, st.status, st.title, st.artist);
        }
    }
}
