//! Windows Core Audio: master endpoint volume (IAudioEndpointVolume) and
//! per-application session volume (ISimpleAudioVolume). Every setter reads
//! the value back so callers can report the verified state.

use windows::core::{Interface, GUID};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eConsole, eRender, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2, IMMDevice,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};

use super::process::process_name;
use crate::error::{AppError, AppResult};

pub use super::data_audio::AudioState;

pub use super::data_audio::OutputDevice;

pub use super::data_audio::AppAudioSession;

pub use super::data_audio::{ducked_level, still_ducked, Ducked};

/// PKEY_Device_FriendlyName (functiondiscoverykeys_devpkey.h); declared here to
/// avoid pulling the whole FunctionDiscovery feature into the build.
const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
    pid: 14,
};

fn enumerator() -> AppResult<IMMDeviceEnumerator> {
    unsafe { Ok(CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?) }
}

fn default_device() -> AppResult<IMMDevice> {
    unsafe {
        enumerator()?
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|_| AppError::not_found("Нет активного устройства вывода звука"))
    }
}

fn device_name(dev: &IMMDevice) -> String {
    unsafe {
        dev.OpenPropertyStore(STGM_READ)
            .and_then(|store| store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME))
            .map(|v| v.to_string())
            .unwrap_or_else(|_| "Устройство вывода".to_string())
    }
}

fn endpoint_volume(dev: &IMMDevice) -> AppResult<IAudioEndpointVolume> {
    unsafe { Ok(dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)?) }
}

fn to_percent(scalar: f32) -> u8 {
    (scalar.clamp(0.0, 1.0) * 100.0).round() as u8
}

pub fn get_state() -> AppResult<AudioState> {
    let dev = default_device()?;
    let vol = endpoint_volume(&dev)?;
    unsafe {
        Ok(AudioState {
            volume: to_percent(vol.GetMasterVolumeLevelScalar()?),
            muted: vol.GetMute()?.as_bool(),
            device: device_name(&dev),
        })
    }
}

pub fn set_volume(percent: u8) -> AppResult<AudioState> {
    if percent > 100 {
        return Err(AppError::invalid("Громкость должна быть от 0 до 100%"));
    }
    let dev = default_device()?;
    let vol = endpoint_volume(&dev)?;
    unsafe {
        vol.SetMasterVolumeLevelScalar(percent as f32 / 100.0, &GUID::zeroed())?;
        // Setting a non-zero level is an explicit request to hear sound.
        if percent > 0 && vol.GetMute()?.as_bool() {
            vol.SetMute(false, &GUID::zeroed())?;
        }
    }
    get_state()
}

pub fn adjust_volume(delta: i16) -> AppResult<AudioState> {
    let current = get_state()?.volume as i16;
    set_volume((current + delta).clamp(0, 100) as u8)
}

pub fn set_mute(muted: bool) -> AppResult<AudioState> {
    let dev = default_device()?;
    let vol = endpoint_volume(&dev)?;
    unsafe { vol.SetMute(muted, &GUID::zeroed())? };
    get_state()
}

pub fn list_output_devices() -> AppResult<Vec<OutputDevice>> {
    unsafe {
        let en = enumerator()?;
        let default_id = en
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .ok()
            .and_then(|d| d.GetId().ok())
            .map(|p| p.to_string().unwrap_or_default())
            .unwrap_or_default();
        let coll = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let mut out = Vec::new();
        for i in 0..coll.GetCount()? {
            let dev = coll.Item(i)?;
            let id = dev.GetId()?.to_string().unwrap_or_default();
            out.push(OutputDevice {
                is_default: id == default_id,
                name: device_name(&dev),
                id,
            });
        }
        Ok(out)
    }
}

fn sessions() -> AppResult<Vec<(u32, ISimpleAudioVolume)>> {
    let dev = default_device()?;
    unsafe {
        let mgr = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)?;
        let en = mgr.GetSessionEnumerator()?;
        let mut out = Vec::new();
        for i in 0..en.GetCount()? {
            let ctl = en.GetSession(i)?;
            let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
            // The system-sounds session has PID 0 and is not an application.
            if ctl2.IsSystemSoundsSession().is_ok() {
                continue;
            }
            let pid = ctl2.GetProcessId().unwrap_or(0);
            if pid == 0 {
                continue;
            }
            if let Ok(simple) = ctl.cast::<ISimpleAudioVolume>() {
                out.push((pid, simple));
            }
        }
        Ok(out)
    }
}

pub fn list_app_sessions() -> AppResult<Vec<AppAudioSession>> {
    let mut out = Vec::new();
    for (pid, simple) in sessions()? {
        unsafe {
            out.push(AppAudioSession {
                pid,
                process: process_name(pid).unwrap_or_else(|| format!("pid {pid}")),
                volume: to_percent(simple.GetMasterVolume()?),
                muted: simple.GetMute()?.as_bool(),
            });
        }
    }
    Ok(out)
}

/// Set the volume of every audio session whose process name matches
/// `process` (case-insensitive, `.exe` optional). Returns verified sessions.
pub fn set_app_volume(process: &str, percent: u8) -> AppResult<Vec<AppAudioSession>> {
    if percent > 100 {
        return Err(AppError::invalid("Громкость должна быть от 0 до 100%"));
    }
    let wanted = process.trim().to_lowercase();
    let wanted = wanted.strip_suffix(".exe").unwrap_or(&wanted).to_string();
    let mut touched = false;
    for (pid, simple) in sessions()? {
        let name = process_name(pid).unwrap_or_default().to_lowercase();
        let base = name.strip_suffix(".exe").unwrap_or(&name);
        if base == wanted {
            unsafe {
                simple.SetMasterVolume(percent as f32 / 100.0, &GUID::zeroed())?;
                if percent > 0 {
                    simple.SetMute(false, &GUID::zeroed())?;
                }
            }
            touched = true;
        }
    }
    if !touched {
        return Err(AppError::not_found(format!(
            "У приложения «{process}» нет активного аудиосеанса"
        )));
    }
    Ok(list_app_sessions()?
        .into_iter()
        .filter(|s| {
            let n = s.process.to_lowercase();
            n.strip_suffix(".exe").unwrap_or(&n) == wanted
        })
        .collect())
}

/// Make every app that is playing sound right now (but JARVIS itself)
/// quieter by `factor`, the way a phone does for a call: music under the
/// voice is what the microphone hears instead of the command.
pub fn duck(factor: f32) -> AppResult<Vec<Ducked>> {
    let own = std::process::id();
    let dev = default_device()?;
    let mut out = Vec::new();
    unsafe {
        let mgr = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)?;
        let en = mgr.GetSessionEnumerator()?;
        for i in 0..en.GetCount()? {
            let ctl = en.GetSession(i)?;
            if ctl.GetState().ok() != Some(AudioSessionStateActive) {
                continue;
            }
            let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
            let pid = ctl2.GetProcessId().unwrap_or(0);
            if pid == 0 || pid == own || ctl2.IsSystemSoundsSession().is_ok() {
                continue;
            }
            let Ok(simple) = ctl.cast::<ISimpleAudioVolume>() else { continue };
            let original = simple.GetMasterVolume()?;
            let ducked = ducked_level(original, factor);
            if original <= 0.01 || simple.GetMute()?.as_bool() {
                continue;
            }
            simple.SetMasterVolume(ducked, &GUID::zeroed())?;
            out.push(Ducked { pid, process: process_name(pid).unwrap_or_default(), original, ducked });
        }
    }
    Ok(out)
}

/// Put ducked apps back. `by_name`: match by process name, not pid — after a
/// crash the apps (and their pids) may have restarted, and Windows remembers
/// an app's level, so the ducked one would stay for good.
pub fn unduck(list: &[Ducked], by_name: bool) -> AppResult<()> {
    if list.is_empty() {
        return Ok(());
    }
    for (pid, simple) in sessions()? {
        let name = if by_name { process_name(pid).unwrap_or_default().to_lowercase() } else { String::new() };
        let Some(d) = list.iter().find(|d| if by_name { !name.is_empty() && d.process.to_lowercase() == name } else { d.pid == pid }) else {
            continue;
        };
        unsafe {
            if still_ducked(simple.GetMasterVolume()?, d) {
                simple.SetMasterVolume(d.original, &GUID::zeroed())?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod live {
    /// `cargo test --lib live_audio_apps -- --ignored --nocapture`: apps with an audio session now.
    #[test]
    #[ignore]
    fn live_audio_apps() {
        crate::platform::com::ensure_mta();
        for s in super::list_app_sessions().unwrap() {
            println!("audio: {s:?}");
        }
    }
}
