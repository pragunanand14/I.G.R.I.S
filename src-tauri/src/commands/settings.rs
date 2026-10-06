use tauri::State;

use crate::error::{AppError, AppResult};
use crate::settings::{self, Settings, SettingsPatch};
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    settings::load(&*state.db.conn()?)
}

#[tauri::command]
pub fn update_settings(state: State<'_, AppState>, overlay: State<'_, std::sync::Arc<crate::overlay::Overlay>>, patch: SettingsPatch) -> AppResult<Settings> {
    // The core checks the shortcut's shape; only the desktop knows which
    // combinations its global-shortcut backend can register.
    if let Some(v) = &patch.operator_stop_hotkey {
        if v.trim().parse::<tauri_plugin_global_shortcut::Shortcut>().is_err() {
            return Err(AppError::validation(settings::INVALID_SHORTCUT));
        }
    }
    let mut conn = state.db.conn()?;
    let hotkey_changed = patch.operator_stop_hotkey.is_some();
    let s = settings::update(&mut conn, patch)?;
    if hotkey_changed {
        overlay.set_stop_hotkey(&s.operator_stop_hotkey);
    }
    Ok(s)
}
