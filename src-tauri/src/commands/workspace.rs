//! Shared folders and projects (Tools / Projects pages).

use serde::Serialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::files::{self, AllowedFolder};
use crate::projects::{self, Detected, Project, ProjectInput};
use crate::state::AppState;

#[tauri::command]
pub fn list_folders(state: State<'_, AppState>) -> AppResult<Vec<AllowedFolder>> {
    files::list(&*state.db.conn()?)
}

#[tauri::command]
pub fn add_folder(state: State<'_, AppState>, path: String, writable: bool) -> AppResult<AllowedFolder> {
    files::add(&*state.db.conn()?, &path, writable)
}

#[tauri::command]
pub fn set_folder_writable(state: State<'_, AppState>, id: i64, writable: bool) -> AppResult<()> {
    files::set_writable(&*state.db.conn()?, id, writable)
}

#[tauri::command]
pub fn remove_folder(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    files::remove(&*state.db.conn()?, id)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSuggestion {
    pub label: String,
    pub path: String,
}

/// Common user folders that exist on this machine and aren't shared yet.
#[tauri::command]
pub fn suggest_folders(state: State<'_, AppState>) -> AppResult<Vec<FolderSuggestion>> {
    let shared = files::list(&*state.db.conn()?)?;
    let candidates =
        [("Documents", dirs::document_dir()), ("Desktop", dirs::desktop_dir()), ("Downloads", dirs::download_dir()), ("Pictures", dirs::picture_dir())];
    Ok(candidates
        .into_iter()
        .filter_map(|(label, p)| {
            let p = p?.canonicalize().ok()?;
            let s = p.display().to_string();
            (p.is_dir() && !shared.iter().any(|f| f.path == s)).then(|| FolderSuggestion { label: label.into(), path: s })
        })
        .collect())
}

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> AppResult<Vec<Project>> {
    projects::list(&*state.db.conn()?)
}

#[tauri::command]
pub fn add_project(state: State<'_, AppState>, project: ProjectInput) -> AppResult<Project> {
    projects::add(&*state.db.conn()?, &project)
}

#[tauri::command]
pub fn update_project(state: State<'_, AppState>, id: i64, project: ProjectInput) -> AppResult<Project> {
    projects::update(&*state.db.conn()?, id, &project)
}

#[tauri::command]
pub fn remove_project(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    projects::remove(&*state.db.conn()?, id)
}

/// Inspect a folder to pre-fill a new project.
#[tauri::command]
pub fn detect_project(path: String) -> AppResult<Detected> {
    let p = std::path::PathBuf::from(path.trim());
    if !p.is_absolute() || !p.is_dir() {
        return Err(AppError::validation("Choose an existing folder."));
    }
    Ok(projects::detect(&p))
}
