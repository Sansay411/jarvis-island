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
