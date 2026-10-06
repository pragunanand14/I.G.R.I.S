//! The IGRIS app (Tauri), for Windows/desktop and Android.
//!
//! The shared IGRIS brain lives in the `igris-core` crate; this crate is the
//! platform around it: the Tauri app and its commands (the only way the UI
//! reaches the backend, registered in [`run`]), system monitoring and
//! logging, and per platform — on desktops the overlay and global hotkeys,
//! the Windows computer driver, browser control and desktop tools; on Android
//! the phone tools (through `tauri-plugin-igris-device`). Core modules are
//! re-exported under their usual paths.

pub use igris_core::{ai, attachments, config, conversations, core, db, error, files, memory, operator, orchestrator, productivity, projects, settings, voice};

pub mod commands;
pub mod computer;
pub mod logging;
#[cfg(desktop)]
pub mod overlay;
pub mod state;
pub mod system;
pub mod tools;

#[cfg(test)]
mod e2e;

use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::ai::AiRuntime;
use crate::config::AppConfig;
use crate::db::Database;
use crate::state::{AppPaths, AppState, Generations, PendingApprovals};
use crate::system::{ConnectivityMonitor, SystemMonitor};

const CONNECTIVITY_INTERVAL: Duration = Duration::from_secs(15);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init()).plugin(tauri_plugin_dialog::init());
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_igris_device::init());
    builder
        .plugin(tauri_plugin_notification::init())
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

            let attachments = Arc::new(attachments::AttachmentStore::new(data_dir.join("attachments"))?);
            match attachments.gc(&*db.conn()?) {
                Ok(n) => tracing::info!(event = "ATTACHMENT_GC", removed = n),
                Err(e) => tracing::warn!(event = "ATTACHMENT_GC_FAILED", error = %e),
            }

            let ai = AiRuntime::from_config(&config);
            match &ai.status.problem {
                None => tracing::info!(event = "AI_PROVIDER_READY", provider = ?ai.status.provider, model = ?ai.status.configured_model),
                Some(problem) => tracing::warn!(event = "AI_PROVIDER_UNAVAILABLE", reason = %problem),
            }

            let system = Arc::new(Mutex::new(SystemMonitor::new()));
            let connectivity = ConnectivityMonitor::start(CONNECTIVITY_INTERVAL);
            let config = Arc::new(RwLock::new(config));
            // Operator mode: IGRIS operating the computer (see operator/ and tools/computer.rs).
            let operator = Arc::new(operator::Operator::new(db.clone(), computer::native()));
            // The task orchestrator: actionable requests become tasks (see orchestrator/).
            let orchestrator = orchestrator::Orchestrator::new(db.clone(), Some(operator.clone()));
            match orchestrator.recover() {
                Ok(t) if !t.is_empty() => tracing::info!(event = "TASKS_RECOVERED", count = t.len()),
                Ok(_) => {}
                Err(e) => tracing::warn!(event = "TASKS_RECOVER_FAILED", error = %e),
            }
            {
                let app = app.handle().clone();
                orchestrator.on_event(Arc::new(move |u: &orchestrator::TaskUpdate| {
                    let _ = app.emit("task-update", u);
                }));
            }
            let tools = crate::tools::standard::registry(crate::tools::standard::Deps {
                db: db.clone(),
                config: config.clone(),
                system: system.clone(),
                connectivity: connectivity.clone(),
                attachments: attachments.clone(),
                operator: operator.clone(),
                orchestrator: orchestrator.clone(),
                browser_profile: data_dir.join("browser-profile"),
                open_path: platform::path_opener(app.handle()),
                open_url: platform::url_opener(app.handle()),
                #[cfg(mobile)]
                phone: platform::phone(app.handle()),
            });
            tracing::info!(event = "TOOLS_REGISTERED", count = tools.specs().len());
            #[cfg(desktop)]
            {
                let stop_hotkey = settings::load(&*db.conn()?).map(|s| s.operator_stop_hotkey).unwrap_or_else(|_| settings::DEFAULT_STOP_HOTKEY.into());
                app.manage(overlay::Overlay::install(app.handle(), operator.clone(), &stop_hotkey));
            }
            #[cfg(mobile)]
            {
                let phone = platform::phone(app.handle());
                let source = phone.clone();
                system.lock().map_err(|_| "system monitor lock poisoned")?.set_battery_source(Box::new(move || platform::phone_battery(source.as_ref())));
                // One check that the phone answers (off the main thread, which serves the plugin). Counts and kinds only.
                tauri::async_runtime::spawn_blocking(move || match (phone.apps(), phone.status()) {
                    (Ok(apps), Ok(s)) => tracing::info!(
                        event = "PHONE_READY",
                        apps = apps.len(),
                        network = s.network.as_deref().unwrap_or("unknown"),
                        battery_known = s.battery_percent.is_some(),
                        sdk = s.sdk_int.unwrap_or(0)
                    ),
                    (a, s) => tracing::warn!(event = "PHONE_UNAVAILABLE", apps_error = ?a.err(), status_error = ?s.err()),
                });
            }

            tauri::async_runtime::spawn(productivity::run_scheduler(db.clone(), reminder_notifier(app.handle().clone())));

            app.manage(AppState {
                config,
                ai: RwLock::new(ai),
                generations: Generations::default(),
                db,
                system,
                connectivity,
                tools: Arc::new(tools),
                approvals: Arc::new(PendingApprovals::default()),
                trust: Arc::new(crate::tools::executor::Trust::default()),
                paths: AppPaths { data_dir, config_dir, log_dir, db_path },
                attachments,
                operator,
                orchestrator,
            });
            #[cfg(desktop)]
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
            commands::attachments::attach_file,
            commands::attachments::discard_attachment,
            commands::attachments::read_attachment,
            commands::chat::chat_regenerate,
            commands::chat::chat_edit,
            commands::chat::chat_cancel,
            commands::chat::list_conversations,
            commands::chat::get_conversation,
            commands::chat::rename_conversation,
            commands::chat::delete_conversation,
            commands::chat::respond_tool_approval,
            commands::chat::trust_conversation,
            commands::operator::get_operator_state,
            commands::operator::operator_control,
            commands::operator::list_operator_tasks,
            commands::operator::list_conversation_tasks,
            commands::operator::task_control,
            commands::operator::list_task_events,
            commands::chat::task_resume,
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
            commands::workspace::list_folders,
            commands::workspace::add_folder,
            commands::workspace::set_folder_writable,
            commands::workspace::remove_folder,
            commands::workspace::suggest_folders,
            commands::workspace::list_projects,
            commands::workspace::add_project,
            commands::workspace::update_project,
            commands::workspace::remove_project,
            commands::workspace::detect_project,
            commands::productivity::list_tasks,
            commands::productivity::add_task,
            commands::productivity::update_task,
            commands::productivity::set_task_done,
            commands::productivity::delete_task,
            commands::productivity::clear_done_tasks,
            commands::productivity::parse_time,
            commands::productivity::list_reminders,
            commands::productivity::reminder_history,
            commands::productivity::add_reminder,
            commands::productivity::start_timer,
            commands::productivity::cancel_reminder,
            commands::productivity::dismiss_reminder,
            commands::productivity::snooze_reminder,
            commands::productivity::clear_reminder_history,
            commands::productivity::list_events,
            commands::productivity::add_event,
            commands::productivity::update_event,
            commands::productivity::delete_event,
        ])
        .run(tauri::generate_context!())
        .expect("error while running IGRIS");
}

/// A fired reminder becomes an OS notification plus a `reminder-fired` event
/// for the in-app alert. Notification failures (e.g. no notification daemon)
/// are logged; the in-app alert still shows.
fn reminder_notifier(app: tauri::AppHandle) -> productivity::OnFire {
    use tauri_plugin_notification::NotificationExt;
    Arc::new(move |f: &productivity::FiredReminder| {
        let heading = match (f.reminder.kind.as_str(), f.late) {
            ("timer", _) => "Timer finished".to_string(),
            (_, true) => "Missed reminder".to_string(),
            _ => "Reminder".to_string(),
        };
        let body = if f.late { format!("{} (was due while IGRIS was closed)", f.reminder.title) } else { f.reminder.title.clone() };
        if let Err(e) = app.notification().builder().title(format!("IGRIS · {heading}")).body(body).show() {
            tracing::warn!(event = "NOTIFICATION_FAILED", error = %e);
        }
        let _ = app.emit("reminder-fired", f);
    })
}

/// Global push-to-talk hotkey. Failing to register (e.g. another app owns the
/// combination) is logged, not fatal — the in-app mic button still works.
pub const VOICE_HOTKEY: &str = "ctrl+shift+space";

#[cfg(desktop)]
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

/// How this platform opens things and reaches the device.
mod platform {
    use std::sync::Arc;

    use crate::tools::standard::{PathOpener, UrlOpener};

    #[cfg(desktop)]
    pub fn path_opener(_app: &tauri::AppHandle) -> PathOpener {
        Arc::new(|p: &std::path::Path| tauri_plugin_opener::open_path(p, None::<&str>).map_err(|e| e.to_string()))
    }

    #[cfg(desktop)]
    pub fn url_opener(_app: &tauri::AppHandle) -> UrlOpener {
        Arc::new(|u: &str| tauri_plugin_opener::open_url(u, None::<&str>).map_err(|e| e.to_string()))
    }

    #[cfg(mobile)]
    pub fn path_opener(app: &tauri::AppHandle) -> PathOpener {
        use tauri_plugin_opener::OpenerExt;
        let app = app.clone();
        Arc::new(move |p: &std::path::Path| app.opener().open_path(p.display().to_string(), None::<&str>).map_err(|e| e.to_string()))
    }

    #[cfg(mobile)]
    pub fn url_opener(app: &tauri::AppHandle) -> UrlOpener {
        use tauri_plugin_opener::OpenerExt;
        let app = app.clone();
        Arc::new(move |u: &str| app.opener().open_url(u, None::<&str>).map_err(|e| e.to_string()))
    }

    /// The phone, through the IGRIS device plugin.
    #[cfg(mobile)]
    pub fn phone(app: &tauri::AppHandle) -> crate::tools::phone::SharedPhone {
        Arc::new(PluginPhone(app.clone()))
    }

    #[cfg(mobile)]
    struct PluginPhone(tauri::AppHandle);

    #[cfg(mobile)]
    impl crate::tools::phone::Phone for PluginPhone {
        fn apps(&self) -> Result<Vec<crate::tools::phone::PhoneApp>, String> {
            use tauri_plugin_igris_device::IgrisDeviceExt;
            self.0.igris_device().list_apps().map_err(|e| e.to_string())
        }
        fn open(&self, package: &str) -> Result<(), String> {
            use tauri_plugin_igris_device::IgrisDeviceExt;
            self.0.igris_device().launch_app(package).map_err(|e| e.to_string())
        }
        fn status(&self) -> Result<crate::tools::phone::PhoneStatus, String> {
            use tauri_plugin_igris_device::IgrisDeviceExt;
            self.0.igris_device().status().map_err(|e| e.to_string())
        }
    }

    /// The phone's battery, for the system monitor.
    #[cfg(mobile)]
    pub fn phone_battery(phone: &dyn crate::tools::phone::Phone) -> Option<crate::system::BatteryInfo> {
        let s = phone.status().ok()?;
        Some(crate::system::BatteryInfo {
            percent: s.battery_percent?.clamp(0.0, 100.0),
            state: match s.charging {
                Some(true) => "charging",
                Some(false) => "discharging",
                None => "unknown",
            },
            time_to_empty_secs: None,
            time_to_full_secs: None,
        })
    }
}
