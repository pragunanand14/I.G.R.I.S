//! The music bar on Home: what's playing, and play/pause/skip. The AI uses
//! the same device through the media tools (with their permission checks).

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::tools::media::{MediaAction, NowPlaying, SharedMedia};

pub struct MediaState(pub SharedMedia);

#[tauri::command]
pub async fn media_now_playing(media: State<'_, MediaState>) -> AppResult<NowPlaying> {
    let m = media.0.clone();
    tauri::async_runtime::spawn_blocking(move || m.now_playing()).await.map_err(|e| AppError::internal(e.to_string()))?.map_err(AppError::validation)
}

/// A button the user pressed on the music bar.
#[tauri::command]
pub async fn media_control(media: State<'_, MediaState>, action: String) -> AppResult<NowPlaying> {
    let action = MediaAction::parse(&action).ok_or_else(|| AppError::validation("Unknown media action."))?;
    let m = media.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        m.control(action)?;
        // Give the player a moment to update before reporting.
        std::thread::sleep(std::time::Duration::from_millis(250));
        m.now_playing()
    })
    .await
    .map_err(|e| AppError::internal(e.to_string()))?
    .map_err(AppError::validation)
}
