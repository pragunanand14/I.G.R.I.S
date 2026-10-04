use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::system::SystemSnapshot;

#[tauri::command]
pub fn get_system_snapshot(state: State<'_, AppState>) -> AppResult<SystemSnapshot> {
    let connectivity = state.connectivity.current();
    let mut monitor = state.system.lock().map_err(|_| AppError::internal("system monitor lock poisoned"))?;
    Ok(monitor.snapshot(connectivity))
}
