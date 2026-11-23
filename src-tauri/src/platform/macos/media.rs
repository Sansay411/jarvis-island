pub use super::data_media::*;
use super::native;
use crate::error::{AppError, AppResult};
use serde::Deserialize;
use serde_json::json;
use std::{io::Read, time::Duration};
use tauri::{AppHandle, Emitter};
pub const EVT_MEDIA: &str = "media:changed";
pub const EVT_ARTWORK: &str = "media:artwork";
pub struct SessionManager;
pub struct MediaWatcher;

const SNAPSHOT: &str = r#"
const running=$.NSWorkspace.sharedWorkspace.runningApplications;
const ids=[];
for(let i=0;i<running.count;i++) {
    const id=ObjC.unwrap(running.objectAtIndex(i).bundleIdentifier);
    if(id==='com.apple.Music' || id==='com.spotify.client') ids.push(id);
}
let result=null, failure=null;
for(const id of ids) {
    try {
        const app=Application(id), status=String(app.playerState());
        let item={source:id,title:'',artist:'',album:'',status:status,position:0,duration:0,key:'',artwork_url:''};
        try {
        const t=app.currentTrack();
        let key='', artwork='';
        try { key=id==='com.spotify.client' ? t.id() : t.persistentID(); } catch(e) {}
        if(id==='com.spotify.client') { try { artwork=t.artworkUrl(); } catch(e) {} }
        item={source:id,title:t.name(),artist:t.artist(),album:t.album(),status:status,
            position:Number(app.playerPosition()),duration:Number(t.duration())/(id==='com.spotify.client'?1000:1),
            key:String(key),artwork_url:String(artwork)};
        } catch(e) { if(status==='playing') throw e; }
        const rank=s => s==='playing' ? 2 : s==='paused' ? 1 : 0;
        if(!result || rank(status)>rank(result.status)) result=item;
        if(status==='playing') break;
    } catch(e) { failure=e; }
}
if(!result && failure) throw failure;
return result;
"#;

#[derive(Deserialize)]
struct Track {
    source: String,
    title: String,
    artist: String,
    album: String,
    status: String,
    position: f64,
    duration: f64,
    key: String,
    #[serde(default)]
    artwork_url: String,
}
fn track() -> AppResult<Option<Track>> {
    serde_json::from_value(native::jxa(SNAPSHOT, json!(null))?)
        .map_err(|_| AppError::platform("Некорректный ответ медиаплеера macOS"))
}
fn state(t: Option<Track>) -> MediaState {
    let Some(t) = t else {
        return MediaState::default();
    };
    let millis = |v: f64| v.is_finite().then(|| (v.max(0.0) * 1000.0) as i64);
    let key = if t.key.is_empty() {
        format!("{}:{}:{}", t.source, t.artist, t.title)
    } else {
        format!("{}:{}", t.source, t.key)
    };
    MediaState {
        available: true,
        source_app: t.source,
        title: t.title,
        artist: t.artist,
        album: t.album,
        status: match t.status.as_str() {
            "playing" => "playing",
            "paused" => "paused",
            _ => "stopped",
        }
        .into(),
        position_ms: millis(t.position),
        duration_ms: millis(t.duration),
        position_sampled_at: Some(chrono::Utc::now().timestamp_millis()),
        can_toggle: true,
        can_next: true,
        can_previous: true,
        track_key: key,
        site: String::new(),
    }
}
pub fn manager() -> AppResult<SessionManager> {
    Ok(SessionManager)
}
pub fn snapshot() -> AppResult<MediaState> {
    let state = state(track()?);
    remember(&state);
    Ok(state)
}

fn art_url(url: &str) -> Option<String> {
    use base64::Engine;
    let url = reqwest::Url::parse(url).ok()?;
    if url.scheme() != "https" || !matches!(url.host_str(), Some("i.scdn.co" | "mosaic.scdn.co")) {
        return None;
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    let response = client.get(url).send().ok()?.error_for_status().ok()?;
    let mut bytes = Vec::new();
    response
        .take(3 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 3 * 1024 * 1024 {
        return None;
    }
    let image = image::load_from_memory(&bytes)
        .ok()?
        .thumbnail(160, 160)
        .to_rgba8();
    let rgba = crate::filekit::wic::Rgba {
        width: image.width(),
        height: image.height(),
        pixels: image.into_raw(),
    };
    let jpeg = crate::filekit::wic::encode_jpeg(&rgba, 80).ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    ))
}
pub fn artwork() -> AppResult<ArtworkEvent> {
    let t = track()?;
    let data_url = t.as_ref().and_then(|t| art_url(&t.artwork_url));
    Ok(ArtworkEvent {
        track_key: state(t).track_key,
        data_url,
    })
}

const CONTROL: &str = r#"
const app=Application(a.source);
if(!app.running()) throw Error('Медиаплеер уже закрыт');
switch(a.action) {
case 'play_pause': app.playpause(); break;
case 'play': app.play(); break;
case 'pause': app.pause(); break;
case 'next': app.nextTrack(); break;
case 'previous': app.previousTrack(); break;
case 'stop': if(a.source==='com.apple.Music') app.stop(); else app.pause(); break;
case 'seek': app.playerPosition=a.position; break;
}
return true;
"#;
fn current() -> AppResult<MediaState> {
    let state = snapshot()?;
    if !state.available {
        return Err(AppError::not_found(
            "Нет активного сеанса Music или Spotify",
        ));
    }
    Ok(state)
}
pub fn control(action: MediaAction) -> AppResult<ControlOutcome> {
    let before = current()?;
    let command = match action {
        MediaAction::PlayPause => "play_pause",
        MediaAction::Play => "play",
        MediaAction::Pause => "pause",
        MediaAction::Next => "next",
        MediaAction::Previous => "previous",
        MediaAction::Stop => "stop",
    };
    native::jxa(
        CONTROL,
        json!({"source":before.source_app,"action":command}),
    )?;
    std::thread::sleep(Duration::from_millis(200));
    let state = snapshot()?;
    let verified = match action {
        MediaAction::PlayPause => state.status != before.status,
        MediaAction::Play => state.status == "playing",
        MediaAction::Pause => state.status == "paused",
        MediaAction::Stop => {
            !state.available || matches!(state.status.as_str(), "stopped" | "paused")
        }
        MediaAction::Next | MediaAction::Previous => state.track_key != before.track_key,
    };
    Ok(ControlOutcome {
        state,
        method: "applescript",
        verified,
    })
}
pub fn seek(position_ms: i64) -> AppResult<()> {
    if position_ms < 0 {
        return Err(AppError::invalid(
            "Позиция воспроизведения не может быть отрицательной",
        ));
    }
    let state = current()?;
    let position = position_ms.min(state.duration_ms.unwrap_or(i64::MAX));
    native::jxa(
        CONTROL,
        json!({"source":state.source_app,"action":"seek","position":position as f64/1000.0}),
    )?;
    Ok(())
}
impl MediaWatcher {
    pub fn spawn(app: AppHandle) -> AppResult<()> {
        std::thread::Builder::new()
            .name("macos-media".into())
            .spawn(move || {
                let mut last = None;
                let mut art_key = None;
                loop {
                    match snapshot() {
                        Ok(state) => {
                            let mut comparable = state.clone();
                            comparable.position_sampled_at = None;
                            if last.as_ref() != Some(&comparable) {
                                let _ = app.emit(EVT_MEDIA, &state);
                                last = Some(comparable);
                            }
                            if art_key.as_ref() != Some(&state.track_key) {
                                let art = artwork().unwrap_or(ArtworkEvent {
                                    track_key: state.track_key.clone(),
                                    data_url: None,
                                });
                                let _ = app.emit(EVT_ARTWORK, art);
                                art_key = Some(state.track_key);
                            }
                        }
                        Err(e) => tracing::debug!("Не удалось обновить медиаплеер: {e}"),
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            })
            .map_err(|e| {
                AppError::internal(format!("Не удалось запустить наблюдение за музыкой: {e}"))
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stopped_and_paused_players_remain_controllable_without_metadata() {
        for status in ["stopped", "paused"] {
            let track: Track = serde_json::from_value(json!({
                "source":"com.apple.Music","title":"","artist":"","album":"","status":status,
                "position":0,"duration":0,"key":""
            })).unwrap();
            let state = state(Some(track));
            assert!(state.available && state.can_toggle);
            assert_eq!(state.source_app, "com.apple.Music");
            assert_eq!(state.status, status);
        }
    }
    #[test]
    fn parses_player_payload_and_normalizes_timing() {
        let t:Track=serde_json::from_str(r#"{"source":"com.spotify.client","title":"Песня","artist":"Автор","album":"Альбом","status":"playing","position":1.5,"duration":120,"key":"id"}"#).unwrap();
        let state = state(Some(t));
        assert_eq!(state.position_ms, Some(1500));
        assert_eq!(state.duration_ms, Some(120000));
        assert_eq!(state.track_key, "com.spotify.client:id");
        assert!(state.available);
        assert!(!super::state(None).available);
        assert!(serde_json::from_str::<Track>("{}").is_err());
        assert!(art_url("file:///etc/hosts").is_none());
        assert!(art_url("https://localhost/image").is_none());
    }
}
