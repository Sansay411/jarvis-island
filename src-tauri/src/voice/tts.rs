//! Speech synthesis with the voices installed in Windows (WinRT
//! `SpeechSynthesizer`: OneCore voices such as Microsoft Irina / Pavel for
//! ru-RU). Produces PCM16 mono at the playback rate of `AudioIo`.

use serde::Serialize;
use windows::core::HSTRING;
use windows::Media::SpeechSynthesis::{SpeechAppendedSilence, SpeechPunctuationSilence, SpeechSynthesizer};
use windows::Storage::Streams::DataReader;

use super::audio::Resampler;
use crate::error::{AppError, AppResult};

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

/// Installed voices, Russian first.
pub fn voices() -> AppResult<Vec<TtsVoice>> {
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

pub struct Speaker {
    synth: SpeechSynthesizer,
    pub voice: String,
}

impl Speaker {
    /// `voice_id` empty → the first Russian voice, else the system default.
    pub fn new(voice_id: &str, rate: f64) -> AppResult<Speaker> {
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
        Ok(Speaker { synth, voice })
    }

    /// Synthesise `text` to PCM16 mono at `out_rate` (`SPEAKER_RATE` for playback).
    pub fn synthesize(&self, text: &str, out_rate: u32) -> AppResult<Vec<i16>> {
        let stream = self.synth.SynthesizeTextToStreamAsync(&HSTRING::from(text)).and_then(|op| op.join()).map_err(platform)?;
        let size = stream.Size().map_err(platform)? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(platform)?).map_err(platform)?;
        reader.LoadAsync(size).and_then(|op| op.join()).map_err(platform)?;
        let mut wav = vec![0u8; size as usize];
        reader.ReadBytes(&mut wav).map_err(platform)?;
        let (rate, mono) = parse_wav(&wav)?;
        let mono = trim_leading_silence(&mono, rate);
        let mut out = Vec::with_capacity(mono.len() * out_rate as usize / rate as usize + 8);
        Resampler::new(rate, out_rate).process(mono, &mut out);
        Ok(out.iter().map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).collect())
    }
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
    fn trims_leading_silence() {
        let mut s = vec![0.0f32; 16_000];
        s.extend(std::iter::repeat_n(0.2, 100));
        assert_eq!(trim_leading_silence(&s, 16_000).len(), 480 + 100);
    }
}
