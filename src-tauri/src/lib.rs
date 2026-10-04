//! IGRIS native backend.
//!
//! The backend owns everything privileged: configuration and secrets,
//! persistence, and OS access. The UI can only reach it through the
//! commands registered in [`run`].

pub mod commands;
pub mod config;
pub mod db;
pub mod error;
pub mod logging;
pub mod settings;
pub mod state;
pub mod system;

use std::sync::Mutex;
use std::time::Duration;

use tauri::Manager;

use crate::config::AppConfig;
use crate::db::Database;
use crate::state::{AppPaths, AppState};
use crate::system::{ConnectivityMonitor, SystemMonitor};

const CONNECTIVITY_INTERVAL: Duration = Duration::from_secs(15);

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let resolver = app.path();
            let data_dir = resolver.app_data_dir()?;
            let config_dir = resolver.app_config_dir()?;
            let log_dir = resolver.app_log_dir()?;

            let config = AppConfig::load(Some(&config_dir));
            let guard = logging::init(Some(&log_dir), config.log_filter.as_deref());
            if let Some(guard) = guard {
                // Keep the log writer alive for the lifetime of the app.
                app.manage(guard);
            }
            tracing::info!(event = "APP_STARTING", version = env!("CARGO_PKG_VERSION"), os = std::env::consts::OS);
            tracing::info!(event = "CONFIG_LOADED", env_files = config.env_files.len(), ai_provider = ?config.ai_provider);

            let db_path = config.database_url.clone().unwrap_or_else(|| data_dir.join("igris.db"));
            let db = Database::open(&db_path).map_err(|e| {
                tracing::error!(event = "DB_OPEN_FAILED", path = %db_path.display(), error = %e);
                e
            })?;
            tracing::info!(event = "DB_READY", path = %db_path.display());

            app.manage(AppState {
                config,
                db,
                system: Mutex::new(SystemMonitor::new()),
                connectivity: ConnectivityMonitor::start(CONNECTIVITY_INTERVAL),
                paths: AppPaths { data_dir, config_dir, log_dir, db_path },
            });
            tracing::info!(event = "APP_STARTED");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::get_app_info,
            commands::app::get_config_status,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::system::get_system_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running IGRIS");
}
