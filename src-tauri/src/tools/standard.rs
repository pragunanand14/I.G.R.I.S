//! The standard tool set, shared by the app and the live test harness.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use super::apps::{LaunchApplicationTool, ListApplicationsTool};
use super::browser::{BrowserClickTool, BrowserConfirmedClickTool, BrowserOpenTool, BrowserSnapshotTool, BrowserTools, BrowserTypeTool};
use super::calculator::CalculatorTool;
use super::computer::*;
use super::files::{
    CopyPathTool, CreateFileTool, CreateFolderTool, ListDirectoryTool, MovePathTool, OpenPathTool, ReadFileTool, SearchFilesTool, TrashPathTool, WriteFileTool,
};
use super::memory::{ForgetMemoryTool, RememberTool, SearchMemoryTool, UpdateMemoryTool};
use super::processes::{CloseApplicationTool, ListProcessesTool, OpenUrlTool};
use super::productivity::{
    AddEventTool, AddTaskTool, CancelReminderTool, DateTimeTool, DeleteEventTool, DeleteTaskTool, ListEventsTool, ListRemindersTool, ListTasksTool,
    SetReminderTool, StartTimerTool, UpdateTaskTool,
};
use super::projects::{ListProjectsTool, ProjectContextTool};
use super::system_info::SystemInfoTool;
use super::task::{RequestToolsTool, TaskPlanTool};
use super::terminal::RunCommandTool;
use super::web::{FetchUrlTool, WebSearchTool};
use super::ToolRegistry;
use crate::attachments::AttachmentStore;
use crate::computer::browser::Browser;
use crate::config::AppConfig;
use crate::db::Database;
use crate::operator::Operator;
use crate::orchestrator::Orchestrator;
use crate::system::{ConnectivityMonitor, SystemMonitor};

pub type PathOpener = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;
pub type UrlOpener = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// What the tools need from the app.
pub struct Deps {
    pub db: Arc<Database>,
    pub config: Arc<RwLock<AppConfig>>,
    pub system: Arc<Mutex<SystemMonitor>>,
    pub connectivity: ConnectivityMonitor,
    pub attachments: Arc<AttachmentStore>,
    pub operator: Arc<Operator>,
    pub orchestrator: Arc<Orchestrator>,
    /// IGRIS's own browser profile (DevTools browser control).
    pub browser_profile: PathBuf,
    pub open_path: PathOpener,
    pub open_url: UrlOpener,
}

/// Every tool IGRIS offers.
pub fn registry(d: Deps) -> ToolRegistry {
    let Deps { db, config, system, connectivity, attachments, operator, orchestrator, browser_profile, open_path, open_url } = d;
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
    tools.register(Arc::new(ListDirectoryTool::new(db.clone())));
    tools.register(Arc::new(SearchFilesTool::new(db.clone())));
    tools.register(Arc::new(ReadFileTool::new(db.clone())));
    tools.register(Arc::new(CreateFileTool::new(db.clone())));
    tools.register(Arc::new(CreateFolderTool::new(db.clone())));
    tools.register(Arc::new(WriteFileTool::new(db.clone())));
    tools.register(Arc::new(MovePathTool::new(db.clone())));
    tools.register(Arc::new(CopyPathTool::new(db.clone())));
    tools.register(Arc::new(TrashPathTool::new(db.clone())));
    tools.register(Arc::new(OpenPathTool::new(db.clone(), open_path)));
    tools.register(Arc::new(ListProcessesTool::default()));
    tools.register(Arc::new(CloseApplicationTool::new(db.clone())));
    tools.register(Arc::new(OpenUrlTool::new(open_url)));
    tools.register(Arc::new(ListProjectsTool::new(db.clone())));
    tools.register(Arc::new(ProjectContextTool::new(db.clone())));
    tools.register(Arc::new(DateTimeTool::default()));
    tools.register(Arc::new(AddTaskTool::new(db.clone())));
    tools.register(Arc::new(ListTasksTool::new(db.clone())));
    tools.register(Arc::new(UpdateTaskTool::new(db.clone())));
    tools.register(Arc::new(DeleteTaskTool::new(db.clone())));
    tools.register(Arc::new(SetReminderTool::new(db.clone())));
    tools.register(Arc::new(StartTimerTool::new(db.clone())));
    tools.register(Arc::new(ListRemindersTool::new(db.clone())));
    tools.register(Arc::new(CancelReminderTool::new(db.clone())));
    tools.register(Arc::new(AddEventTool::new(db.clone())));
    tools.register(Arc::new(ListEventsTool::new(db.clone())));
    tools.register(Arc::new(DeleteEventTool::new(db.clone())));
    tools.register(Arc::new(super::screen::screenshot_tool(db.clone(), attachments.clone())));

    {
        tools.register(Arc::new(OperatorStartTool::new(operator.clone())));
        tools.register(Arc::new(OperatorUpdateTool::new(operator.clone())));
        tools.register(Arc::new(OperatorFinishTool::new(operator.clone())));
        tools.register(Arc::new(ComputerObserveTool::new(operator.clone())));
        tools.register(Arc::new(ComputerClickTool::new(operator.clone())));
        tools.register(Arc::new(ComputerTypeTool::new(operator.clone())));
        tools.register(Arc::new(ComputerKeyTool::new(operator.clone())));
        tools.register(Arc::new(ComputerScrollTool::new(operator.clone())));
        tools.register(Arc::new(ComputerDragTool::new(operator.clone())));
        tools.register(Arc::new(ComputerFocusWindowTool::new(operator.clone())));
        tools.register(Arc::new(ComputerConfirmedActionTool::new(operator.clone())));
        // IGRIS's own Chrome/Edge, driven through the DevTools protocol (operator mode).
        let b = BrowserTools::new(Browser::new(browser_profile), operator.clone());
        tools.register(Arc::new(BrowserOpenTool::new(b.clone())));
        tools.register(Arc::new(BrowserSnapshotTool::new(b.clone())));
        tools.register(Arc::new(BrowserClickTool::new(b.clone())));
        tools.register(Arc::new(BrowserTypeTool::new(b.clone())));
        tools.register(Arc::new(BrowserConfirmedClickTool::new(b)));
    }
    tools.register(Arc::new(RunCommandTool::new(db.clone(), Some(operator.clone()))));

    tools.register(Arc::new(TaskPlanTool::new(orchestrator.clone())));
    tools.register(Arc::new(RequestToolsTool::default()));
    tools
}
