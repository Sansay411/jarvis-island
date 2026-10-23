//! Speech synthesis for the local voice engine. Neural voices (sherpa-onnx:
//! Supertonic 3, Piper) by default; the voices installed in Windows (WinRT
//! `SpeechSynthesizer`, OneCore Irina / Pavel) remain as a fallback that
//! needs no download. Produces PCM16 mono at the playback rate of `AudioIo`.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;
use windows::core::HSTRING;
use windows::Media::SpeechSynthesis::{SpeechAppendedSilence, SpeechPunctuationSilence, SpeechSynthesizer};
use windows::Storage::Streams::DataReader;

use super::audio::Resampler;
use super::models::{self, Kind};
use super::neural::NeuralVoice;
use crate::error::{AppError, AppResult};

#[derive(Default)]
pub struct PcmCache(Mutex<HashMap<String,Vec<i16>>>);

impl PcmCache {
    pub fn render(&self, text: &str, synth: impl FnOnce()->AppResult<Vec<i16>>) -> AppResult<Vec<i16>> {
        if text.chars().count()>64 { return synth(); }
        if let Some(pcm)=self.0.lock().get(text).cloned() { return Ok(pcm); }
        let pcm=synth()?;
        let mut cache=self.0.lock();
        if cache.len()<16 { cache.insert(text.into(),pcm.clone()); }
        Ok(pcm)
    }
}

pub struct CachedSpeaker { pub speaker:Speaker, cache:PcmCache }
impl CachedSpeaker {
    pub fn synthesize(&self,text:&str)->AppResult<Vec<i16>> {
        self.cache.render(text,||self.speaker.synthesize(text,super::audio::SPEAKER_RATE))
    }
}

#[derive(Default)]
pub struct SpeechEngine(Mutex<Option<(String,u32,Arc<CachedSpeaker>)>>);
impl SpeechEngine {
    pub fn get(&self,voice:&str,rate:f32)->AppResult<Arc<CachedSpeaker>> {
        let mut slot=self.0.lock();
        if let Some((id,r,speaker))=slot.as_ref() {
            if id==voice && *r==rate.to_bits() { return Ok(speaker.clone()); }
        }
        let speaker=Speaker::new(voice,rate as f64).or_else(|e| {
            tracing::warn!("selected voice unavailable, using Windows voice: {e}");
            Speaker::new(WINDOWS_PREFIX,rate as f64)
        })?;
        let cached=Arc::new(CachedSpeaker {speaker,cache:PcmCache::default()});
        *slot=Some((voice.into(),rate.to_bits(),cached.clone()));
        Ok(cached)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsVoice {
    pub id: String,
    pub name: String,
    pub language: String,
}

fn platform(e: windows::core::Error) -> AppError {
    AppError::platform(format!("Синтез речи: {}", e.message()))
}

/// Prefix of Windows voice ids in settings (`windows:<WinRT voice id>`).
pub const WINDOWS_PREFIX: &str = "windows:";

/// Supertonic 3 speakers (sid → name), default first. Male/female by median
/// pitch of a test phrase (sids 5–9: 94–128 Hz; 0–4: 170–200 Hz); all ten
/// were read back word for word by GigaAM.
pub const SUPERTONIC_SPEAKERS: &[(i32, &str)] = &[
    (6, "Мужской 7, низкий"),
    (9, "Мужской 10, низкий"),
    (5, "Мужской 6"),
    (7, "Мужской 8"),
    (8, "Мужской 9"),
    (0, "Женский 1"),
    (1, "Женский 2"),
    (2, "Женский 3"),
    (3, "Женский 4"),
    (4, "Женский 5"),
];

/// Voices for Settings: installed neural voices first, then Windows voices.
pub fn voices() -> AppResult<Vec<TtsVoice>> {
    let mut out = Vec::new();
    for a in models::ASSETS.iter().filter(|a| a.kind == Kind::Tts && a.installed()) {
        if a.id.starts_with("supertonic") {
            for (sid, name) in SUPERTONIC_SPEAKERS {
                out.push(TtsVoice { id: format!("{}:{sid}", a.id), name: format!("{name} (Supertonic)"), language: "ru-RU".into() });
            }
        } else {
            out.push(TtsVoice { id: a.id.into(), name: format!("{} (Piper)", a.label.trim_start_matches("Piper ")), language: "ru-RU".into() });
        }
    }
    out.extend(windows_voices()?.into_iter().map(|v| TtsVoice { id: format!("{WINDOWS_PREFIX}{}", v.id), name: format!("{} (Windows)", v.name), ..v }));
    Ok(out)
}

/// The best voice available without a choice: a downloaded neural voice,
/// else the first Russian Windows voice.
pub fn default_voice() -> String {
    if models::asset("supertonic-3").is_some_and(|a| a.installed()) {
        if let Some((sid, _)) = SUPERTONIC_SPEAKERS.first() {
            return format!("supertonic-3:{sid}");
        }
    }
    models::ASSETS
        .iter()
        .find(|a| a.kind == Kind::Tts && a.id.starts_with("piper") && a.installed())
        .map(|a| a.id.to_string())
        .unwrap_or_else(|| WINDOWS_PREFIX.to_string())
}

pub enum Speaker {
    Neural(NeuralVoice),
    Windows(WindowsSpeaker),
}

impl Speaker {
    /// `voice`: an id from `voices()`; empty = `default_voice()`. Legacy
    /// settings stored a bare WinRT id.
    pub fn new(voice: &str, rate: f64) -> AppResult<Speaker> {
        let voice = if voice.trim().is_empty() { default_voice() } else { voice.trim().to_string() };
        if let Some(id) = voice.strip_prefix(WINDOWS_PREFIX) {
            return Ok(Speaker::Windows(WindowsSpeaker::new(id, rate)?));
        }
        if voice.starts_with("HKEY_") {
            return Ok(Speaker::Windows(WindowsSpeaker::new(&voice, rate)?));
        }
        Ok(Speaker::Neural(NeuralVoice::load(&voice, rate as f32)?))
    }

    pub fn name(&self) -> String {
        match self {
            Speaker::Neural(v) => v.name.clone(),
            Speaker::Windows(w) => format!("{} (Windows)", w.voice),
        }
    }

    /// Synthesise `text` to PCM16 mono at `out_rate` (`SPEAKER_RATE` for playback).
    /// Russian text is normalised first (numbers, names, symbols in words).
    pub fn synthesize(&self, text: &str, out_rate: u32) -> AppResult<Vec<i16>> {
        let text = super::ru_speech::normalize(text);
        let text = text.as_str();
        let (rate, mono) = match self {
            Speaker::Neural(v) => v.synthesize(text)?,
            Speaker::Windows(w) => w.synthesize_raw(text)?,
        };
        let mono = trim_leading_silence(&mono, rate);
        let mut out = Vec::with_capacity(mono.len() * out_rate as usize / rate as usize + 8);
        Resampler::new(rate, out_rate).process(mono, &mut out);
        Ok(out.iter().map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).collect())
    }
}

/// Voices installed in Windows, Russian first.
pub fn windows_voices() -> AppResult<Vec<TtsVoice>> {
    crate::platform::com::ensure_mta();
    let mut out: Vec<TtsVoice> = SpeechSynthesizer::AllVoices()
        .map_err(platform)?
        .into_iter()
        .filter_map(|v| {
            Some(TtsVoice {
                id: v.Id().ok()?.to_string(),
                name: v.DisplayName().ok()?.to_string(),
                language: v.Language().ok()?.to_string(),
            })
        })
        .collect();
    out.sort_by_key(|v| (!v.language.eq_ignore_ascii_case("ru-RU"), v.name.clone()));
    Ok(out)
}

pub struct WindowsSpeaker {
    synth: SpeechSynthesizer,
    pub voice: String,
}

impl WindowsSpeaker {
    /// `voice_id` empty → the first Russian voice, else the system default.
    pub fn new(voice_id: &str, rate: f64) -> AppResult<WindowsSpeaker> {
        crate::platform::com::ensure_mta();
        let synth = SpeechSynthesizer::new().map_err(platform)?;
        let all: Vec<_> = SpeechSynthesizer::AllVoices().map_err(platform)?.into_iter().collect();
        let chosen = all
            .iter()
            .find(|v| !voice_id.is_empty() && v.Id().map(|id| id.to_string() == voice_id).unwrap_or(false))
            .or_else(|| all.iter().find(|v| v.Language().map(|l| l.to_string().eq_ignore_ascii_case("ru-RU")).unwrap_or(false)));
        let voice = match chosen {
            Some(v) => {
                synth.SetVoice(v).map_err(platform)?;
                v.DisplayName().map(|n| n.to_string()).unwrap_or_default()
            }
            None => SpeechSynthesizer::DefaultVoice().and_then(|v| v.DisplayName()).map(|n| n.to_string()).unwrap_or_default(),
        };
        let opts = synth.Options().map_err(platform)?;
        opts.SetSpeakingRate(rate.clamp(0.5, 2.0)).map_err(platform)?;
        // Short pauses: sentences are synthesised one by one and queued.
        let _ = opts.SetAppendedSilence(SpeechAppendedSilence::Min);
        let _ = opts.SetPunctuationSilence(SpeechPunctuationSilence::Min);
        Ok(WindowsSpeaker { synth, voice })
    }

    /// Mono samples and their rate.
    pub fn synthesize_raw(&self, text: &str) -> AppResult<(u32, Vec<f32>)> {
        let stream = self.synth.SynthesizeTextToStreamAsync(&HSTRING::from(text)).and_then(|op| op.join()).map_err(platform)?;
        let size = stream.Size().map_err(platform)? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(platform)?).map_err(platform)?;
        reader.LoadAsync(size).and_then(|op| op.join()).map_err(platform)?;
        let mut wav = vec![0u8; size as usize];
        reader.ReadBytes(&mut wav).map_err(platform)?;
        parse_wav(&wav)
    }
}

/// A PCM16 mono RIFF/WAVE file.
pub fn wav_bytes(pcm: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    w.extend(pcm.iter().flat_map(|s| s.to_le_bytes()));
    w
}

/// Parse a PCM16 RIFF/WAVE file into (sample rate, mono samples).
pub fn parse_wav(b: &[u8]) -> AppResult<(u32, Vec<f32>)> {
    let bad = || AppError::platform("Синтез речи вернул неожиданный формат звука");
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(bad());
    }
    let (mut pos, mut fmt) = (12usize, None::<(u16, u16, u32, u16)>);
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let len = u32::from_le_bytes([b[pos + 4], b[pos + 5], b[pos + 6], b[pos + 7]]) as usize;
        let body = &b[pos + 8..(pos + 8 + len).min(b.len())];
        match id {
            b"fmt " if body.len() >= 16 => {
                fmt = Some((
                    u16::from_le_bytes([body[0], body[1]]),
                    u16::from_le_bytes([body[2], body[3]]),
                    u32::from_le_bytes([body[4], body[5], body[6], body[7]]),
                    u16::from_le_bytes([body[14], body[15]]),
                ));
            }
            b"data" => {
                let (format, channels, rate, bits) = fmt.ok_or_else(bad)?;
                if format != 1 || bits != 16 || channels == 0 || rate == 0 {
                    return Err(bad());
                }
                let ch = channels as usize;
                let mono = body
                    .chunks_exact(2 * ch)
                    .map(|frame| {
                        frame.chunks_exact(2).map(|s| i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0).sum::<f32>() / ch as f32
                    })
                    .collect();
                return Ok((rate, mono));
            }
            _ => {}
        }
        pos += 8 + len + (len & 1);
    }
    Err(bad())
}

/// Drop leading silence (keeping 30 ms) so speech starts sooner.
fn trim_leading_silence(s: &[f32], rate: u32) -> &[f32] {
    let first = s.iter().position(|v| v.abs() > 0.004).unwrap_or(s.len());
    let keep = (rate as usize * 30) / 1000;
    &s[first.saturating_sub(keep)..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_short_reply_uses_audio_cache_but_long_answers_are_not_retained() {
        let cache=PcmCache::default();
        let calls=std::cell::Cell::new(0);
        for _ in 0..2 {
            assert_eq!(cache.render("Слушаю.",|| {calls.set(calls.get()+1);Ok(vec![7,8])}).unwrap(),vec![7,8]);
        }
        assert_eq!(calls.get(),1);
        for _ in 0..2 { cache.render(&"x".repeat(100),|| {calls.set(calls.get()+1);Ok(vec![1])}).unwrap(); }
        assert_eq!(calls.get(),3);
    }

    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        b.extend_from_slice(&(channels * 2).to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(&data);
        b
    }

    #[test]
    fn parses_mono_and_stereo() {
        let (rate, s) = parse_wav(&wav(22_050, 1, &[0, 16384, -16384])).unwrap();
        assert_eq!(rate, 22_050);
        assert_eq!(s, vec![0.0, 0.5, -0.5]);
        let (_, st) = parse_wav(&wav(16_000, 2, &[16384, 0, -16384, -16384])).unwrap();
        assert_eq!(st, vec![0.25, -0.5]);
        assert!(parse_wav(b"not a wav file").is_err());
    }

    #[test]
    fn wav_round_trip() {
        let (rate, s) = parse_wav(&wav_bytes(&[0, 16384, -16384], 24_000)).unwrap();
        assert_eq!((rate, s), (24_000, vec![0.0, 0.5, -0.5]));
    }

    #[test]
    fn trims_leading_silence() {
        let mut s = vec![0.0f32; 16_000];
        s.extend(std::iter::repeat_n(0.2, 100));
        assert_eq!(trim_leading_silence(&s, 16_000).len(), 480 + 100);
    }
}
