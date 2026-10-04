use serde::Serialize;
use tauri::State;

use crate::config::PublicConfig;
use crate::db::migrations;
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub platform: &'static str,
    pub arch: &'static str,
    pub debug: bool,
    pub data_dir: String,
    pub config_dir: String,
    pub log_dir: String,
    pub db_path: String,
    pub schema_version: u32,
}

#[tauri::command]
pub fn get_app_info(state: State<'_, AppState>) -> AppResult<AppInfo> {
    let schema_version = migrations::current_version(&*state.db.conn()?)?;
    Ok(AppInfo {
        name: "IGRIS",
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        debug: cfg!(debug_assertions),
        data_dir: state.paths.data_dir.display().to_string(),
        config_dir: state.paths.config_dir.display().to_string(),
        log_dir: state.paths.log_dir.display().to_string(),
        db_path: state.paths.db_path.display().to_string(),
        schema_version,
    })
}

/// Redacted configuration status — never includes secret values.
#[tauri::command]
pub fn get_config_status(state: State<'_, AppState>) -> AppResult<PublicConfig> {
    Ok(state.config.read().map_err(|_| crate::error::AppError::internal("config lock poisoned"))?.public())
}
