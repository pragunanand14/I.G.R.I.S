use base64::Engine;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::voice::{self, Backend, VoiceStatus};

fn config(state: &AppState) -> AppResult<crate::config::AppConfig> {
    Ok(state.config.read().map_err(|_| AppError::internal("config lock poisoned"))?.clone())
}

#[tauri::command]
pub fn get_voice_status(state: State<'_, AppState>) -> AppResult<VoiceStatus> {
    Ok(voice::voice_status(&config(&state)?))
}

/// Transcribe base64-encoded audio with the configured speech-to-text service.
#[tauri::command]
pub async fn transcribe_audio(state: State<'_, AppState>, audio_base64: String, mime_type: String) -> AppResult<String> {
    let backend = voice::stt_backend(&config(&state)?);
    let h = match backend {
        Backend::Http(h) => h,
        Backend::Browser => return Err(AppError::validation("Speech recognition is set to the browser; nothing to send.")),
        Backend::Misconfigured(p) => return Err(AppError::AiUnavailable(p)),
    };
    if audio_base64.len() > voice::MAX_AUDIO_BYTES * 4 / 3 + 4 {
        return Err(AppError::validation("The recording is too long."));
    }
    let audio = base64::engine::general_purpose::STANDARD.decode(audio_base64.as_bytes()).map_err(|_| AppError::validation("Invalid audio data."))?;
    let started = std::time::Instant::now();
    let text = voice::transcribe(&h, audio, &mime_type).await.map_err(|e| AppError::AiUnavailable(e.message))?;
    tracing::info!(event = "VOICE_TRANSCRIBED", backend = h.label, chars = text.chars().count(), ms = started.elapsed().as_millis() as u64);
    Ok(text)
}

/// Synthesize speech; returns base64 MP3.
#[tauri::command]
pub async fn synthesize_speech(state: State<'_, AppState>, text: String) -> AppResult<String> {
    let h = match voice::tts_backend(&config(&state)?) {
        Backend::Http(h) => h,
        Backend::Browser => return Err(AppError::validation("Speech output is set to the browser voice.")),
        Backend::Misconfigured(p) => return Err(AppError::AiUnavailable(p)),
    };
    let bytes = voice::synthesize(&h, &text).await.map_err(|e| AppError::AiUnavailable(e.message))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}
