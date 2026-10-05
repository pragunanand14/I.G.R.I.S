//! IGRIS native backend.
//!
//! The backend owns everything privileged: configuration and secrets,
//! persistence, and OS access. The UI can only reach it through the
//! commands registered in [`run`].

pub mod ai;
pub mod commands;
pub mod config;
pub mod conversations;
pub mod core;
pub mod db;
pub mod error;
pub mod logging;
pub mod memory;
pub mod settings;
pub mod state;
pub mod system;
pub mod tools;
pub mod voice;

use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::ai::AiRuntime;
use crate::config::AppConfig;
use crate::db::Database;
use crate::state::{AppPaths, AppState, Generations, PendingApprovals};
use crate::tools::apps::{LaunchApplicationTool, ListApplicationsTool};
use crate::tools::calculator::CalculatorTool;
use crate::tools::memory::{ForgetMemoryTool, RememberTool, SearchMemoryTool, UpdateMemoryTool};
use crate::tools::system_info::SystemInfoTool;
use crate::tools::web::{FetchUrlTool, WebSearchTool};
use crate::tools::ToolRegistry;
use crate::system::{ConnectivityMonitor, SystemMonitor};

const CONNECTIVITY_INTERVAL: Duration = Duration::from_secs(15);

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
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
            let db = Arc::new(Database::open(&db_path).map_err(|e| {
                tracing::error!(event = "DB_OPEN_FAILED", path = %db_path.display(), error = %e);
                e
            })?);
            tracing::info!(event = "DB_READY", path = %db_path.display());

            let ai = AiRuntime::from_config(&config);
            match &ai.status.problem {
                None => tracing::info!(event = "AI_PROVIDER_READY", provider = ?ai.status.provider, model = ?ai.status.configured_model),
                Some(problem) => tracing::warn!(event = "AI_PROVIDER_UNAVAILABLE", reason = %problem),
            }

            let system = Arc::new(Mutex::new(SystemMonitor::new()));
            let connectivity = ConnectivityMonitor::start(CONNECTIVITY_INTERVAL);
            let config = Arc::new(RwLock::new(config));
            let mut tools = ToolRegistry::default();
            tools.register(Arc::new(WebSearchTool::new(config.clone())));
            tools.register(Arc::new(FetchUrlTool::default()));
            tools.register(Arc::new(CalculatorTool::default()));
            tools.register(Arc::new(SystemInfoTool::new(system.clone(), connectivity.clone())));
            tools.register(Arc::new(ListApplicationsTool::new(db.clone())));
            tools.register(Arc::new(LaunchApplicationTool::new(db.clone())));
            tools.register(Arc::new(RememberTool::new(db.clone())));
            tools.register(Arc::new(SearchMemoryTool::new(db.clone())));
            tools.register(Arc::new(UpdateMemoryTool::new(db.clone())));
            tools.register(Arc::new(ForgetMemoryTool::new(db.clone())));
            tracing::info!(event = "TOOLS_REGISTERED", count = tools.specs().len());

            app.manage(AppState {
                config,
                ai: RwLock::new(ai),
                generations: Generations::default(),
                db,
                system,
                connectivity,
                tools: Arc::new(tools),
                approvals: Arc::new(PendingApprovals::default()),
                paths: AppPaths { data_dir, config_dir, log_dir, db_path },
            });
            register_voice_hotkey(app.handle());
            tracing::info!(event = "APP_STARTED");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::get_app_info,
            commands::app::get_config_status,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::system::get_system_snapshot,
            commands::ai::get_ai_status,
            commands::ai::reload_config,
            commands::chat::chat_send,
            commands::chat::chat_regenerate,
            commands::chat::chat_edit,
            commands::chat::chat_cancel,
            commands::chat::list_conversations,
            commands::chat::get_conversation,
            commands::chat::rename_conversation,
            commands::chat::delete_conversation,
            commands::chat::respond_tool_approval,
            commands::tools::list_tools,
            commands::tools::list_tool_audit,
            commands::tools::list_applications,
            commands::tools::add_application,
            commands::tools::remove_application,
            commands::tools::detect_applications,
            commands::tools::launch_application,
            commands::tools::get_permission_policy,
            commands::memory::list_memories,
            commands::memory::add_memory,
            commands::memory::update_memory,
            commands::memory::delete_memory,
            commands::voice::get_voice_status,
            commands::voice::transcribe_audio,
            commands::voice::synthesize_speech,
        ])
        .run(tauri::generate_context!())
        .expect("error while running IGRIS");
}

/// Global push-to-talk hotkey. Failing to register (e.g. another app owns the
/// combination) is logged, not fatal — the in-app mic button still works.
pub const VOICE_HOTKEY: &str = "ctrl+shift+space";

fn register_voice_hotkey(app: &tauri::AppHandle) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let result = app.global_shortcut().on_shortcut(VOICE_HOTKEY, |app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
            let _ = app.emit("voice-toggle", ());
        }
    });
    match result {
        Ok(()) => tracing::info!(event = "VOICE_HOTKEY_REGISTERED", hotkey = VOICE_HOTKEY),
        Err(e) => tracing::warn!(event = "VOICE_HOTKEY_UNAVAILABLE", hotkey = VOICE_HOTKEY, error = %e),
    }
}
