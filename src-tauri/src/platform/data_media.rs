use parking_lot::Mutex;
use serde::Deserialize;
use serde::Serialize;

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
    /// For a browser source: the site that plays, read from the browser's
    /// window title (`youtube`, `yandex`, `spotify`, `soundcloud`, `vk`,
    /// `twitch`), or empty when unknown.
    pub site: String,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlOutcome {
    pub state: MediaState,
    /// "session" when GSMTC accepted the command, "media_key" for the fallback.
    pub method: &'static str,
    /// Whether the resulting state confirms the action (e.g. status flipped).
    pub verified: bool,
}

/// The last player state the watcher saw (or a command produced). The voice
/// gate reads it for every phrase: a song or a film playing means the room's
/// sentences are not the user's.
static LAST: Mutex<Option<MediaState>> = Mutex::new(None);

pub(super) fn remember(state: &MediaState) {
    *LAST.lock() = Some(state.clone());
}

/// The player state as last seen, without asking the system.
pub fn cached() -> MediaState {
    LAST.lock().clone().unwrap_or_default()
}

/// Something plays right now.
pub fn is_playing() -> bool {
    LAST.lock().as_ref().is_some_and(|m| m.available && m.status == "playing")
}

/// A player is open (playing or paused), so «дальше» and «назад» are about it.
pub fn has_player() -> bool {
    LAST.lock().as_ref().is_some_and(|m| m.available)
}

/// Tests: pretend a player is open.
#[cfg(test)]
pub fn set_cached(state: MediaState) {
    remember(&state);
}
