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

/// Run a phone call off the main thread; on a computer, say it's for the phone.
#[cfg(mobile)]
async fn on_phone<T: Send + 'static>(f: impl FnOnce() -> Result<T, tauri_plugin_igris_device::Error> + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| AppError::internal(e.to_string()))?.map_err(|e| AppError::validation(e.to_string()))
}

/// "Hey IGRIS" on the phone, and whether IGRIS is the phone's assistant.
#[tauri::command]
pub async fn phone_wake_status(#[allow(unused_variables)] app: tauri::AppHandle) -> AppResult<tauri_plugin_igris_device::WakeStatus> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return on_phone(move || app.igris_device().wake_status()).await;
    }
    #[cfg(not(mobile))]
    Err(AppError::validation("This is for the phone app."))
}

/// Turn "Hey IGRIS" on or off (Android asks for the microphone the first time).
#[tauri::command]
pub async fn phone_wake_set(
    #[allow(unused_variables)] app: tauri::AppHandle,
    #[allow(unused_variables)] on: bool,
    #[allow(unused_variables)] charging_only: bool,
) -> AppResult<()> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return on_phone(move || app.igris_device().wake_set(on, charging_only)).await;
    }
    #[cfg(not(mobile))]
    Err(AppError::validation("This is for the phone app."))
}

/// IGRIS is recording or speaking: the wake listener lets go of the microphone meanwhile.
#[tauri::command]
pub async fn phone_wake_hold(#[allow(unused_variables)] app: tauri::AppHandle, #[allow(unused_variables)] on: bool) -> AppResult<()> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return on_phone(move || app.igris_device().wake_hold(on)).await;
    }
    #[cfg(not(mobile))]
    Ok(())
}

/// Open an Android settings page: "overlay" (Display over other apps), "battery", "assistant".
#[tauri::command]
pub async fn phone_open_settings(#[allow(unused_variables)] app: tauri::AppHandle, #[allow(unused_variables)] page: String) -> AppResult<()> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return on_phone(move || app.igris_device().open_settings_page(&page)).await;
    }
    #[cfg(not(mobile))]
    Err(AppError::validation("This is for the phone app."))
}

/// While IGRIS is open on the phone, "Hey IGRIS" starts listening right there: check for it a few times a second.
#[cfg(mobile)]
pub fn follow_wake_word(app: tauri::AppHandle) {
    use tauri::Emitter;
    use tauri_plugin_igris_device::IgrisDeviceExt;
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let a = app.clone();
            let heard = tauri::async_runtime::spawn_blocking(move || a.igris_device().wake_take_heard().unwrap_or(false)).await.unwrap_or(false);
            if heard {
                let _ = app.emit("wake-heard", ());
            }
        }
    });
}
