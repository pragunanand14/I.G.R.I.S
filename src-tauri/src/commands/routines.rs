//! The Routines page: list, pause/resume, run now, delete, and recent runs.
//! Routines are created by asking IGRIS (the create_routine tool).

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::routines::{self, Routine, Run};
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineView {
    #[serde(flatten)]
    pub routine: Routine,
    pub last_run: Option<Run>,
}

#[tauri::command]
pub fn routines_list(state: State<'_, AppState>) -> AppResult<Vec<RoutineView>> {
    let conn = state.db.conn()?;
    routines::list(&conn)?.into_iter().map(|r| Ok(RoutineView { last_run: routines::runs(&conn, Some(r.id), 1)?.into_iter().next(), routine: r })).collect()
}

#[tauri::command]
pub fn routine_runs(state: State<'_, AppState>, id: Option<i64>) -> AppResult<Vec<Run>> {
    routines::runs(&*state.db.conn()?, id, 30)
}

#[tauri::command]
pub fn routine_set_enabled(state: State<'_, AppState>, id: i64, enabled: bool) -> AppResult<Routine> {
    routines::set_enabled(&*state.db.conn()?, id, enabled)
}

#[tauri::command]
pub fn routine_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    routines::delete(&*state.db.conn()?, id).map(|_| ())
}

/// Run a routine now, in the background; the page updates when it finishes.
#[tauri::command]
pub fn routine_run_now(app: AppHandle, state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let r = routines::get(&*state.db.conn()?, id)?.ok_or_else(|| AppError::validation("That routine no longer exists."))?;
    let db = state.db.clone();
    tauri::async_runtime::spawn(async move {
        let runner = Arc::new(crate::routine_runner::AppRoutines(app.clone()));
        if let Err(e) = routines::run_one(&db, runner.as_ref(), &r, "started by the user (Run now)").await {
            tracing::warn!(event = "ROUTINE_RUN_FAILED", error = %e);
        }
        let _ = app.emit("routines-changed", ());
    });
    Ok(())
}
