//! Classic SAPI 5 voices — the way third-party engines plug into Windows.
//! Silero TTS ships its natural Russian voices this way
//! (`SileroTTS_Ru_Setup`, github.com/snakers4/silero-models releases);
//! WinRT `SpeechSynthesizer` cannot see them, so they are driven here
//! through `ISpVoice`, rendering into memory as 24 kHz PCM16 mono — the
//! playback format — without touching the speakers or the disk.

use windows::core::{GUID, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::Media::Audio::{WAVEFORMATEX, WAVE_FORMAT_PCM};
use windows::Win32::Media::Speech::{
    IEnumSpObjectTokens, ISpObjectToken, ISpObjectTokenCategory, ISpStream, ISpVoice, SpObjectTokenCategory, SpStream, SpVoice, SPCAT_VOICES,
    SPF_IS_NOT_XML,
};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IStream, CLSCTX_ALL, STREAM_SEEK_CUR};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};

use crate::error::{AppError, AppResult};

/// Voice ids in settings: `sapi:<token id>`.
pub const PREFIX: &str = "sapi:";
pub const RATE: u32 = 24_000;

/// `SPDFID_WaveFormatEx` from sapi.h: the stream carries a WAVEFORMATEX.
const SPDFID_WAVEFORMATEX: GUID = GUID::from_u128(0xc31adbae_527f_4ff5_a230_f62bb61ff70c);

fn err(what: &str, e: windows::core::Error) -> AppError {
    AppError::platform(format!("SAPI, {what}: {}", e.message()))
}

unsafe fn take(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as *const _));
    s
}

pub struct SapiVoiceInfo {
    pub id: String,
    pub name: String,
    /// Hex LCIDs from the token's «Language» attribute («419» = Russian).
    pub languages: String,
}

impl SapiVoiceInfo {
    pub fn russian(&self) -> bool {
        self.languages.split(';').any(|l| l.trim().eq_ignore_ascii_case("419"))
    }
}

fn tokens() -> AppResult<Vec<ISpObjectToken>> {
    crate::platform::com::ensure_mta();
    unsafe {
        let cat: ISpObjectTokenCategory = CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL).map_err(|e| err("категория голосов", e))?;
        cat.SetId(SPCAT_VOICES, false).map_err(|e| err("категория голосов", e))?;
        let en: IEnumSpObjectTokens = cat.EnumTokens(PCWSTR::null(), PCWSTR::null()).map_err(|e| err("список голосов", e))?;
        let mut n = 0u32;
        en.GetCount(&mut n).map_err(|e| err("список голосов", e))?;
        Ok((0..n).filter_map(|i| en.Item(i).ok()).collect())
    }
}

/// Every SAPI 5 voice registered for 64-bit programs.
pub fn voices() -> Vec<SapiVoiceInfo> {
    let Ok(list) = tokens() else { return Vec::new() };
    list.iter()
        .filter_map(|t| unsafe {
            let id = take(t.GetId().ok()?);
            let name = t.GetStringValue(PCWSTR::null()).map(|p| take(p)).unwrap_or_default();
            let languages = t.OpenKey(windows::core::w!("Attributes")).and_then(|k| k.GetStringValue(windows::core::w!("Language"))).map(|p| take(p)).unwrap_or_default();
            Some(SapiVoiceInfo { name: if name.is_empty() { id.rsplit('\\').next().unwrap_or(&id).to_string() } else { name }, id, languages })
        })
        .collect()
}

pub struct SapiVoice {
    voice: ISpVoice,
    pub name: String,
}

// ISpVoice is free-threaded in the SAPI runtime; calls are serialised by the
// speaker cache (one phrase at a time).
unsafe impl Send for SapiVoice {}
unsafe impl Sync for SapiVoice {}

impl SapiVoice {
    pub fn new(token_id: &str, rate: f64) -> AppResult<SapiVoice> {
        let list = tokens()?;
        let (token, name) = list
            .iter()
            .find_map(|t| unsafe {
                let id = take(t.GetId().ok()?);
                (id == token_id).then(|| (t.clone(), t.GetStringValue(PCWSTR::null()).map(|p| take(p)).unwrap_or(id)))
            })
            .ok_or_else(|| AppError::not_found("Голос SAPI не найден — возможно, его удалили"))?;
        unsafe {
            let voice: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL).map_err(|e| err("голос", e))?;
            voice.SetVoice(&token).map_err(|e| err("выбор голоса", e))?;
            // SAPI rate: -10…10, roughly ×3 per 10 steps.
            let steps = ((rate.clamp(0.5, 2.0)).ln() / 3f64.ln() * 10.0).round() as i32;
            voice.SetRate(steps.clamp(-10, 10)).map_err(|e| err("скорость", e))?;
            Ok(SapiVoice { voice, name })
        }
    }

    /// PCM16 mono at 24 kHz.
    pub fn synthesize(&self, text: &str) -> AppResult<Vec<i16>> {
        crate::platform::com::ensure_mta();
        unsafe {
            let mem: IStream = CreateStreamOnHGlobal(HGLOBAL::default(), true).map_err(|e| err("поток", e))?;
            let fmt = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: 1,
                nSamplesPerSec: RATE,
                nAvgBytesPerSec: RATE * 2,
                nBlockAlign: 2,
                wBitsPerSample: 16,
                cbSize: 0,
            };
            let stream: ISpStream = CoCreateInstance(&SpStream, None, CLSCTX_ALL).map_err(|e| err("поток", e))?;
            stream.SetBaseStream(&mem, &SPDFID_WAVEFORMATEX, &fmt).map_err(|e| err("поток", e))?;
            self.voice.SetOutput(&stream, true).map_err(|e| err("вывод", e))?;
            let res = self.voice.Speak(&HSTRING::from(text), SPF_IS_NOT_XML.0 as u32, None);
            // Detach the voice from the stream before reading it.
            let _ = self.voice.SetOutput(None::<&windows::core::IUnknown>, false);
            res.map_err(|e| err("синтез", e))?;
            let mut len = 0u64;
            mem.Seek(0, STREAM_SEEK_CUR, Some(&mut len)).map_err(|e| err("поток", e))?;
            let hg = GetHGlobalFromStream(&mem).map_err(|e| err("поток", e))?;
            let p = GlobalLock(hg) as *const u8;
            if p.is_null() {
                return Err(AppError::platform("SAPI: пустой результат"));
            }
            let bytes = std::slice::from_raw_parts(p, len as usize);
            let pcm: Vec<i16> = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
            let _ = GlobalUnlock(hg);
            if pcm.is_empty() {
                return Err(AppError::platform("Голос SAPI ничего не произнёс"));
            }
            Ok(pcm)
        }
    }
}
