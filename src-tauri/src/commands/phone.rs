//! Phone settings: whether screen control (Android accessibility) is on, and
//! a way to Android's settings page where the user turns it on or off.

use crate::error::{AppError, AppResult};

#[tauri::command]
pub async fn phone_screen_access(#[allow(unused_variables)] app: tauri::AppHandle) -> AppResult<bool> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return tauri::async_runtime::spawn_blocking(move || app.igris_device().screen_access())
            .await
            .map_err(|e| AppError::internal(e.to_string()))?
            .map_err(|e| AppError::validation(e.to_string()));
    }
    #[cfg(not(mobile))]
    Err(AppError::validation("Screen control is for the phone app."))
}

/// Open Android's accessibility settings (IGRIS can't switch it on itself — only the user can).
#[tauri::command]
pub async fn phone_screen_settings(#[allow(unused_variables)] app: tauri::AppHandle) -> AppResult<()> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return tauri::async_runtime::spawn_blocking(move || app.igris_device().open_accessibility_settings())
            .await
            .map_err(|e| AppError::internal(e.to_string()))?
            .map_err(|e| AppError::validation(e.to_string()));
    }
    #[cfg(not(mobile))]
    Err(AppError::validation("Screen control is for the phone app."))
}
