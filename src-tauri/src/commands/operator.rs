//! Operator mode and task controls for the UI (main window, overlay orb, task card).

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

/// Tasks of a conversation, newest first (the running one marked `live`).
#[tauri::command]
pub fn list_conversation_tasks(state: State<'_, AppState>, conversation_id: String, limit: Option<u32>) -> AppResult<Vec<crate::orchestrator::TaskInfo>> {
    state.orchestrator.for_conversation(&conversation_id, limit.unwrap_or(10).clamp(1, 50))
}

/// A task's activity log (oldest first): what it attempted and what happened.
#[tauri::command]
pub fn list_task_events(state: State<'_, AppState>, task_id: String, limit: Option<u32>) -> AppResult<Vec<crate::orchestrator::store::TaskEventRow>> {
    state.orchestrator.events(&task_id, limit.unwrap_or(40).clamp(1, 200))
}

/// Pause, resume or stop a running task; stop also dismisses an interrupted one.
#[tauri::command]
pub fn task_control(state: State<'_, AppState>, task_id: String, action: crate::orchestrator::Control) -> AppResult<crate::orchestrator::TaskInfo> {
    let task = state.orchestrator.control(&task_id, action)?;
    let live = state.orchestrator.is_live(&task.id);
    tracing::info!(event = "TASK_CONTROL", action = ?action);
    Ok(crate::orchestrator::TaskInfo { task, live })
}
