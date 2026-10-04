use std::path::PathBuf;
use std::sync::Mutex;

use crate::config::AppConfig;
use crate::db::Database;
use crate::system::{ConnectivityMonitor, SystemMonitor};

/// Shared application state, managed by Tauri and injected into commands.
pub struct AppState {
    pub config: AppConfig,
    pub db: Database,
    pub system: Mutex<SystemMonitor>,
    pub connectivity: ConnectivityMonitor,
    pub paths: AppPaths,
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub log_dir: PathBuf,
    pub db_path: PathBuf,
}
