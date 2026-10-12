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

/// Runs `f` off the UI thread (listing and moving files can take a while on a big Downloads folder).
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn tidy_status(state: State<'_, AppState>) -> AppResult<TidyStatus> {
    let db = state.db.clone();
    blocking(move || {
        let dir = downloads_dir();
        let pending = dir.as_deref().and_then(|d| tidy::plan(d, std::time::SystemTime::now()).ok()).filter(|m| !m.is_empty()).map(|m| tidy::describe(&m));
        let conn = db.conn()?;
        Ok(TidyStatus {
            dir: dir.map(|d| d.to_string_lossy().into_owned()),
            auto: crate::settings::get_extra::<bool>(&conn, tidy::AUTO_KEY)?.unwrap_or(false),
            pending,
            last: tidy::undoable(&conn)?,
        })
    })
    .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TidyResult {
    pub moved: usize,
    pub summary: String,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn tidy_now(state: State<'_, AppState>) -> AppResult<TidyResult> {
    let dir = downloads_dir().ok_or_else(|| AppError::validation("There's no Downloads folder on this device."))?;
    let db = state.db.clone();
    let (done, errors) = blocking(move || tidy::run(&db, &dir)).await?;
    Ok(TidyResult { moved: done.len(), summary: if done.is_empty() { String::new() } else { tidy::describe(&done) }, errors })
}

#[tauri::command]
pub async fn tidy_undo(state: State<'_, AppState>) -> AppResult<usize> {
    let db = state.db.clone();
    blocking(move || tidy::undo_last(&db)).await
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
            let on = db.conn().ok().and_then(|c| crate::settings::get_extra::<bool>(&c, tidy::AUTO_KEY).ok().flatten()).unwrap_or(false);
            if on {
                let (db, dir) = (db.clone(), dir.clone());
                if let Ok(Err(e)) = tauri::async_runtime::spawn_blocking(move || tidy::run(&db, &dir)).await {
                    tracing::warn!(event = "DOWNLOADS_TIDY_FAILED", error = %e);
                }
            }
        }
    });
}
