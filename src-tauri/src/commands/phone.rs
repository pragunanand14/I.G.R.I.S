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

/// Whether IGRIS has Notification access (to read message notifications).
#[tauri::command]
pub async fn phone_messages_access(#[allow(unused_variables)] app: tauri::AppHandle) -> AppResult<bool> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return tauri::async_runtime::spawn_blocking(move || app.igris_device().messages_access())
            .await
            .map_err(|e| AppError::internal(e.to_string()))?
            .map_err(|e| AppError::validation(e.to_string()));
    }
    #[cfg(not(mobile))]
    Err(AppError::validation("Messages are for the phone app."))
}

/// Open Android's Notification access page (only the user can switch it on).
#[tauri::command]
pub async fn phone_messages_settings(#[allow(unused_variables)] app: tauri::AppHandle) -> AppResult<()> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return tauri::async_runtime::spawn_blocking(move || app.igris_device().open_notification_access_settings())
            .await
            .map_err(|e| AppError::internal(e.to_string()))?
            .map_err(|e| AppError::validation(e.to_string()));
    }
    #[cfg(not(mobile))]
    Err(AppError::validation("Messages are for the phone app."))
}

/// Read notifications from every app (default: messaging apps only).
#[tauri::command]
pub fn phone_messages_all_apps(state: tauri::State<'_, crate::state::AppState>) -> AppResult<bool> {
    Ok(crate::settings::get_extra::<bool>(&*state.db.conn()?, "messagesAllApps")?.unwrap_or(false))
}

#[tauri::command]
pub fn phone_set_messages_all_apps(state: tauri::State<'_, crate::state::AppState>, on: bool) -> AppResult<bool> {
    crate::settings::set_extra(&*state.db.conn()?, "messagesAllApps", &on)?;
    Ok(on)
}
