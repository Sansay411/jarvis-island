//! Universal TTS Tauri IPC commands (`tts_speak`, `tts_stop`, `tts_status`, `tts_benchmark`).
//!
//! Thin adapters over `VoiceManager` so any frontend surface (Dynamic Island,
//! Command Workspace per-message readout, auto-speak replies, Settings HD Voice
//! Studio preview & speed benchmark) can drive speech synthesis cleanly.

use std::sync::Arc;

use tauri::State;

use crate::error::AppResult;
use crate::voice::{TtsBenchResult, TtsSpeakOutcome, TtsStatusPayload, VoiceManager};

#[tauri::command]
pub fn tts_speak(
    voice_mgr: State<'_, Arc<VoiceManager>>,
    text: String,
    voice: Option<String>,
    rate: Option<f32>,
    interrupt: Option<bool>,
) -> AppResult<TtsSpeakOutcome> {
    voice_mgr.speak(text, voice, rate, interrupt.unwrap_or(true))
}

#[tauri::command]
pub fn tts_stop(voice_mgr: State<'_, Arc<VoiceManager>>) -> AppResult<()> {
    voice_mgr.stop_speaking();
    Ok(())
}

#[tauri::command]
pub fn tts_status(voice_mgr: State<'_, Arc<VoiceManager>>) -> AppResult<TtsStatusPayload> {
    Ok(voice_mgr.tts_status())
}

#[tauri::command]
pub async fn tts_benchmark(
    voice_mgr: State<'_, Arc<VoiceManager>>,
    voice: Option<String>,
) -> AppResult<TtsBenchResult> {
    voice_mgr.tts_benchmark(voice).await
}
