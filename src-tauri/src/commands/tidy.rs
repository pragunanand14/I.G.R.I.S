//! Tidy Downloads (computer): status, tidy now, undo, and "tidy by itself".

use serde::Serialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::files::tidy;
use crate::state::AppState;

/// The Downloads folder, when this device has one.
pub fn downloads_dir() -> Option<std::path::PathBuf> {
    #[cfg(desktop)]
    {
        dirs::download_dir()
    }
    #[cfg(mobile)]
    {
        None
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TidyStatus {
    pub dir: Option<String>,
    pub auto: bool,
    /// What tidying now would move ("3 files: 2 Documents, 1 Images"), or None.
    pub pending: Option<String>,
    /// How many files the last tidy moved (undo puts them back).
    pub last: usize,
}

#[tauri::command]
pub fn tidy_status(state: State<'_, AppState>) -> AppResult<TidyStatus> {
    let conn = state.db.conn()?;
    let dir = downloads_dir();
    let pending = dir.as_deref().and_then(|d| tidy::plan(d, std::time::SystemTime::now()).ok()).filter(|m| !m.is_empty()).map(|m| tidy::describe(&m));
    Ok(TidyStatus {
        dir: dir.map(|d| d.to_string_lossy().into_owned()),
        auto: crate::settings::get_extra::<bool>(&conn, tidy::AUTO_KEY)?.unwrap_or(false),
        pending,
        last: crate::settings::get_extra::<Vec<tidy::Move>>(&conn, tidy::LAST_KEY)?.map_or(0, |v| v.len()),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TidyResult {
    pub moved: usize,
    pub summary: String,
    pub errors: Vec<String>,
}

#[tauri::command]
pub fn tidy_now(state: State<'_, AppState>) -> AppResult<TidyResult> {
    let dir = downloads_dir().ok_or_else(|| AppError::validation("There's no Downloads folder on this device."))?;
    let (done, errors) = tidy::run(&*state.db.conn()?, &dir)?;
    Ok(TidyResult { moved: done.len(), summary: if done.is_empty() { String::new() } else { tidy::describe(&done) }, errors })
}

#[tauri::command]
pub fn tidy_undo(state: State<'_, AppState>) -> AppResult<usize> {
    tidy::undo_last(&*state.db.conn()?)
}

#[tauri::command]
pub fn tidy_set_auto(state: State<'_, AppState>, on: bool) -> AppResult<()> {
    crate::settings::set_extra(&*state.db.conn()?, tidy::AUTO_KEY, &on)
}

/// While IGRIS runs and the user turned it on: tidy Downloads every 10 minutes.
pub fn watch(db: std::sync::Arc<crate::db::Database>) {
    let Some(dir) = downloads_dir() else { return };
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            let Ok(conn) = db.conn() else { continue };
            if crate::settings::get_extra::<bool>(&conn, tidy::AUTO_KEY).ok().flatten().unwrap_or(false) {
                if let Err(e) = tidy::run(&conn, &dir) {
                    tracing::warn!(event = "DOWNLOADS_TIDY_FAILED", error = %e);
                }
            }
        }
    });
}
