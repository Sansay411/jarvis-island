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

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaState {
    pub available: bool,
    /// AppUserModelID of the source app (e.g. `Spotify.exe`, `MSEdge`).
    pub source_app: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// playing | paused | stopped | changing | opened | closed
    pub status: String,
    pub position_ms: Option<i64>,
    pub duration_ms: Option<i64>,
    /// Unix ms at which `position_ms` was sampled by the source app.
    pub position_sampled_at: Option<i64>,
    pub can_toggle: bool,
    pub can_next: bool,
    pub can_previous: bool,
    /// Stable key for the current track; changes when artwork must reload.
    pub track_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtworkEvent {
    pub track_key: String,
    /// `data:` URL of a ≤160 px JPEG, or None when the app provides no art.
    pub data_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaAction {
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
    Stop,
}

/// WinRT DateTime ticks (100 ns since 1601) → Unix ms.
fn winrt_to_unix_ms(ticks: i64) -> i64 {
    (ticks - 116_444_736_000_000_000) / 10_000
}

pub fn manager() -> AppResult<SessionManager> {
    Ok(SessionManager::RequestAsync()?.join()?)
}

fn current_session(mgr: &SessionManager) -> Option<Session> {
    mgr.GetCurrentSession().ok()
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

    let state = MediaState {
        available: true,
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
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 8 * 1024 * 1024 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlOutcome {
    pub state: MediaState,
    /// "session" when GSMTC accepted the command, "media_key" for the fallback.
    pub method: &'static str,
    /// Whether the resulting state confirms the action (e.g. status flipped).
    pub verified: bool,
}

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
                watcher.rebind(&mgr, &tx);
                let mut last_state: Option<MediaState> = None;
                let mut last_art_key = String::new();
                loop {
                    let Ok(first) = rx.recv() else { break };
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
                    let _ = props_changed;
                    if state.track_key != last_art_key {
                        let data_url = thumb.as_ref().and_then(read_artwork);
                        let _ = app.emit(EVT_ARTWORK, ArtworkEvent { track_key: state.track_key.clone(), data_url });
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
