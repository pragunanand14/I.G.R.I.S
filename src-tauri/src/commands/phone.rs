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

/// The assistant panel ("Hey IGRIS" over other apps): `on` makes IGRIS see-through and shown over the lock screen.
#[tauri::command]
pub async fn phone_sheet_mode(#[allow(unused_variables)] app: tauri::AppHandle, #[allow(unused_variables)] on: bool) -> AppResult<()> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return on_phone(move || app.igris_device().sheet_mode(on)).await;
    }
    #[cfg(not(mobile))]
    Ok(())
}

/// Close the panel: back to the app that was on screen.
#[tauri::command]
pub async fn phone_sheet_hide(#[allow(unused_variables)] app: tauri::AppHandle) -> AppResult<()> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return on_phone(move || app.igris_device().sheet_hide()).await;
    }
    #[cfg(not(mobile))]
    Ok(())
}

/// Before acting on something asked on the lock screen: the user unlocks the phone. `true` when unlocked.
#[tauri::command]
pub async fn phone_unlock(#[allow(unused_variables)] app: tauri::AppHandle) -> AppResult<bool> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return on_phone(move || app.igris_device().unlock()).await;
    }
    #[cfg(not(mobile))]
    Ok(true)
}

/// Record a spoken request with the phone's own voice-recognition microphone (no call mode, music keeps playing).
#[tauri::command]
pub async fn phone_record_start(#[allow(unused_variables)] app: tauri::AppHandle) -> AppResult<()> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return on_phone(move || app.igris_device().record_start()).await;
    }
    #[cfg(not(mobile))]
    Err(AppError::validation("This is for the phone app."))
}

#[tauri::command]
pub async fn phone_record_poll(#[allow(unused_variables)] app: tauri::AppHandle) -> AppResult<tauri_plugin_igris_device::RecordPoll> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return on_phone(move || app.igris_device().record_poll()).await;
    }
    #[cfg(not(mobile))]
    Err(AppError::validation("This is for the phone app."))
}

#[tauri::command]
pub async fn phone_record_stop(#[allow(unused_variables)] app: tauri::AppHandle, #[allow(unused_variables)] cancel: bool) -> AppResult<()> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return on_phone(move || app.igris_device().record_stop(cancel)).await;
    }
    #[cfg(not(mobile))]
    Ok(())
}

/// The finished recording as base64 WAV (null until it's done).
#[tauri::command]
pub async fn phone_record_take(#[allow(unused_variables)] app: tauri::AppHandle) -> AppResult<Option<String>> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return on_phone(move || app.igris_device().record_take()).await;
    }
    #[cfg(not(mobile))]
    Ok(None)
}
