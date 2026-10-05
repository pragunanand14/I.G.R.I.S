use tauri::State;

use crate::error::AppResult;
use crate::settings::{self, Settings, SettingsPatch};
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    settings::load(&*state.db.conn()?)
}

#[tauri::command]
pub fn update_settings(state: State<'_, AppState>, overlay: State<'_, std::sync::Arc<crate::overlay::Overlay>>, patch: SettingsPatch) -> AppResult<Settings> {
    let mut conn = state.db.conn()?;
    let hotkey_changed = patch.operator_stop_hotkey.is_some();
    let s = settings::update(&mut conn, patch)?;
    if hotkey_changed {
        overlay.set_stop_hotkey(&s.operator_stop_hotkey);
    }
    Ok(s)
}
