pub use super::data_audio::*;
use super::native;
use crate::error::{AppError, AppResult};

fn parse_state(output: &str) -> AppResult<AudioState> {
    let (volume, muted) = output
        .trim()
        .split_once('|')
        .ok_or_else(|| AppError::platform("Некорректный ответ настроек звука macOS"))?;
    if volume.trim() == "missing value" || muted.trim() == "missing value" {
        return Err(AppError::unsupported(
            "Звуковое устройство вывода не найдено",
        ));
    }
    let volume = volume
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|v| *v <= 100)
        .ok_or_else(|| AppError::platform("Некорректная громкость в ответе macOS"))?;
    let muted = match muted.trim() {
        "true" => true,
        "false" => false,
        _ => {
            return Err(AppError::platform(
                "Некорректное состояние звука в ответе macOS",
            ))
        }
    };
    Ok(AudioState {
        volume,
        muted,
        device: "Системный выход macOS".into(),
    })
}
pub fn get_state() -> AppResult<AudioState> {
    parse_state(&native::apple("set v to get volume settings\nreturn (output volume of v as text) & \"|\" & (output muted of v as text)")?)
}
pub fn set_volume(percent: u8) -> AppResult<AudioState> {
    let original = get_state()?;
    native::apple(&volume_script(percent, original.muted))?;
    get_state()
}
fn volume_script(percent: u8, muted: bool) -> String {
    format!(
        "set volume output volume {}\nset volume output muted {}",
        percent.min(100),
        muted && percent == 0
    )
}
pub fn adjust_volume(delta: i16) -> AppResult<AudioState> {
    let current = get_state()?;
    set_volume((i32::from(current.volume) + i32::from(delta)).clamp(0, 100) as u8)
}
pub fn set_mute(muted: bool) -> AppResult<AudioState> {
    get_state()?;
    native::apple(&format!("set volume output muted {muted}"))?;
    get_state()
}
pub fn list_output_devices() -> AppResult<Vec<OutputDevice>> {
    let state = get_state()?;
    Ok(vec![OutputDevice {
        id: "default".into(),
        name: state.device,
        is_default: true,
    }])
}
pub fn list_app_sessions() -> AppResult<Vec<AppAudioSession>> {
    Err(AppError::unsupported(
        "Громкость отдельных приложений пока не поддерживается на macOS",
    ))
}
pub fn set_app_volume(_process: &str, _percent: u8) -> AppResult<Vec<AppAudioSession>> {
    Err(AppError::unsupported(
        "Громкость отдельных приложений пока не поддерживается на macOS",
    ))
}

/// macOS has no per-app mixer, but Music and Spotify have a volume of their
/// own: those are the players made quieter while JARVIS listens and talks.
const DUCK: &str = r#"
const running=$.NSWorkspace.sharedWorkspace.runningApplications, out=[];
for(let i=0;i<running.count;i++) {
    const id=ObjC.unwrap(running.objectAtIndex(i).bundleIdentifier);
    if(id!=='com.apple.Music' && id!=='com.spotify.client') continue;
    try {
        const app=Application(id);
        if(String(app.playerState())!=='playing') continue;
        const original=Number(app.soundVolume());
        if(!(original>1)) continue;
        const ducked=Math.round(Math.min(original, Math.max(0, original*a.factor)));
        app.soundVolume=ducked;
        out.push({pid:0, process:id, original:original/100, ducked:ducked/100});
    } catch(e) {}
}
return out;
"#;

const UNDUCK: &str = r#"
for(const d of a.list) {
    try {
        const app=Application(d.process);
        if(!app.running()) continue;
        if(Math.abs(Number(app.soundVolume())/100-d.ducked)<0.02) app.soundVolume=Math.round(d.original*100);
    } catch(e) {}
}
return true;
"#;

pub fn duck(factor: f32) -> AppResult<Vec<Ducked>> {
    serde_json::from_value(native::jxa(DUCK, serde_json::json!({ "factor": factor }))?)
        .map_err(|_| AppError::platform("Некорректный ответ медиаплеера macOS"))
}

/// Put the players back if their level is still the one JARVIS set. They are
/// found by bundle id, so `by_name` changes nothing here.
pub fn unduck(list: &[Ducked], _by_name: bool) -> AppResult<()> {
    if list.is_empty() {
        return Ok(());
    }
    native::jxa(UNDUCK, serde_json::json!({ "list": list }))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn positive_volume_unmutes_but_zero_preserves_mute() {
        assert!(volume_script(40, true).ends_with("muted false"));
        assert!(volume_script(100, false).ends_with("muted false"));
        assert!(volume_script(0, true).ends_with("muted true"));
        assert!(volume_script(0, false).ends_with("muted false"));
    }
    #[test]
    fn parses_volume_and_missing_or_invalid_output() {
        let state = parse_state("75|false\n").unwrap();
        assert_eq!(state.volume, 75);
        assert!(!state.muted);
        assert!(parse_state("0|true").unwrap().muted);
        for bad in ["", "101|false", "text|false", "50|maybe"] {
            assert!(parse_state(bad).is_err());
        }
        assert!(matches!(
            parse_state("missing value|missing value"),
            Err(AppError::Unsupported(_))
        ));
    }
}
