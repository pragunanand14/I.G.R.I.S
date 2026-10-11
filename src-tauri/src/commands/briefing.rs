//! The daily briefing (Routines page) and the home place used for weather.

use tauri::State;

use crate::briefing::{self, Briefing};
use crate::error::AppResult;
use crate::routines::Run;
use crate::state::AppState;
use crate::weather::{self, Place};

#[tauri::command]
pub fn briefing_get(state: State<'_, AppState>) -> AppResult<Briefing> {
    briefing::get(&*state.db.conn()?)
}

#[tauri::command]
pub fn briefing_set(state: State<'_, AppState>, briefing: Briefing) -> AppResult<Briefing> {
    briefing::set(&*state.db.conn()?, briefing)
}

/// Today's briefing, once it has run.
#[tauri::command]
pub fn briefing_today(state: State<'_, AppState>) -> AppResult<Option<Run>> {
    briefing::today(&*state.db.conn()?)
}

#[tauri::command]
pub fn weather_home(state: State<'_, AppState>) -> AppResult<Option<Place>> {
    weather::home(&*state.db.conn()?)
}

/// Set the home place by name ("Mumbai"). On the phone the weather widget follows it.
#[tauri::command]
pub async fn weather_set_home(app: tauri::AppHandle, state: State<'_, AppState>, query: String) -> AppResult<Place> {
    let place = weather::find_place(weather::GEOCODING, &query).await?;
    weather::set_home(&*state.db.conn()?, &place)?;
    #[cfg(mobile)]
    {
        use tauri_plugin_igris_device::IgrisDeviceExt;
        let p = place.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || app.igris_device().set_weather_place(&p.name, p.latitude, p.longitude)).await;
    }
    #[cfg(not(mobile))]
    let _ = app;
    Ok(place)
}
