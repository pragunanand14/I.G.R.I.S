//! Operator mode controls for the UI (main window and the overlay orb).

use serde::Deserialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::operator::{Snapshot, TaskView};
use crate::state::AppState;

#[tauri::command]
pub fn get_operator_state(state: State<'_, AppState>) -> Snapshot {
    state.operator.snapshot()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperatorAction {
    Pause,
    Resume,
    Stop,
}

/// Pause, resume or stop the running task. Always available to the user.
#[tauri::command]
pub fn operator_control(state: State<'_, AppState>, action: OperatorAction) -> AppResult<Snapshot> {
    let op = &state.operator;
    if !op.is_active() {
        return Err(AppError::validation("IGRIS isn't operating the computer right now."));
    }
    match action {
        OperatorAction::Pause => op.pause("Paused by you."),
        OperatorAction::Resume => {
            op.resume();
        }
        OperatorAction::Stop => {
            op.stop("Stopped by you.");
        }
    }
    Ok(op.snapshot())
}

/// Recent operator tasks, newest first.
#[tauri::command]
pub fn list_operator_tasks(state: State<'_, AppState>, limit: Option<u32>) -> Vec<TaskView> {
    state.operator.history(limit.unwrap_or(20).clamp(1, 100))
}
