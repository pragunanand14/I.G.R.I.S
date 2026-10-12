//! Home-screen widgets (Android): what a widget asked for when it opened
//! IGRIS, and the weather widget's place.

use crate::error::{AppError, AppResult};

pub use crate::weather::Place;

/// What a widget asked for when it opened IGRIS ("listen", "widgets"), once.
#[tauri::command]
pub fn widget_launch_action(#[allow(unused_variables)] app: tauri::AppHandle) -> Option<String> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return app.igris_device().take_launch_action().ok().flatten();
    }
    #[cfg(not(mobile))]
    None
}

/// The weather widget's place, if chosen.
#[tauri::command]
pub async fn widget_weather_place(#[allow(unused_variables)] app: tauri::AppHandle) -> AppResult<Option<String>> {
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        return tauri::async_runtime::spawn_blocking(move || app.igris_device().weather_place())
            .await
            .map_err(|e| AppError::internal(e.to_string()))?
            .map_err(|e| AppError::validation(e.to_string()));
    }
    #[cfg(not(mobile))]
    Ok(None)
}

/// Choose the weather widget's place by name; returns the place found.
#[tauri::command]
pub async fn widget_set_weather_place(#[allow(unused_variables)] app: tauri::AppHandle, #[allow(unused_variables)] query: String) -> AppResult<Place> {
    // Widgets exist only on the phone: say so before changing anything.
    #[cfg(not(mobile))]
    return Err(AppError::validation("Widgets are on the phone app only. Set your city in the daily briefing settings instead."));
    #[cfg(mobile)]
    let place = crate::weather::find_place(crate::weather::GEOCODING, &query).await?;
    // The same place is IGRIS's home place (weather questions, the briefing).
    #[cfg(mobile)]
    {
        use tauri::Manager;
        crate::weather::set_home(&*app.state::<crate::state::AppState>().db.conn()?, &place)?;
    }
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        let p = place.clone();
        tauri::async_runtime::spawn_blocking(move || app.igris_device().set_weather_place(&p.name, p.latitude, p.longitude))
            .await
            .map_err(|e| AppError::internal(e.to_string()))?
            .map_err(|e| AppError::validation(e.to_string()))?;
    }
    #[cfg(mobile)]
    Ok(place)
}
