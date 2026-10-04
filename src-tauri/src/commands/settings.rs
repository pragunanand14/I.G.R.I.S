use tauri::State;

use crate::error::AppResult;
use crate::settings::{self, Settings, SettingsPatch};
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    settings::load(&*state.db.conn()?)
}

#[tauri::command]
pub fn update_settings(state: State<'_, AppState>, patch: SettingsPatch) -> AppResult<Settings> {
    let mut conn = state.db.conn()?;
    settings::update(&mut conn, patch)
}
