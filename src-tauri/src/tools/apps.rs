//! Application allowlist and launcher.
//!
//! The user registers applications (name + executable path) in the Tools
//! page. The model can only refer to them **by name**; it never supplies a
//! path, arguments or a command line. Paths are validated when added and
//! again before every launch.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::db::Database;
use crate::error::{AppError, AppResult};

pub const NAME_MAX_CHARS: usize = 60;
const PATH_MAX_CHARS: usize = 1024;
const SETTLE_TIME: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub created_at: String,
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<AppEntry> {
    Ok(AppEntry { id: r.get(0)?, name: r.get(1)?, path: r.get(2)?, created_at: r.get(3)? })
}

pub fn list(conn: &Connection) -> AppResult<Vec<AppEntry>> {
    let mut stmt = conn.prepare("SELECT id, name, path, created_at FROM applications ORDER BY name COLLATE NOCASE")?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Option<AppEntry>> {
    Ok(conn.query_row("SELECT id, name, path, created_at FROM applications WHERE id = ?1", [id], row).optional()?)
}

/// Validate that `path` is something IGRIS is willing to launch.
pub fn validate_path(path: &str) -> Result<PathBuf, String> {
    let path = path.trim();
    if path.is_empty() || path.chars().count() > PATH_MAX_CHARS {
        return Err("Enter the full path to the application.".into());
    }
    if path.chars().any(char::is_control) {
        return Err("The path contains invalid characters.".into());
    }
    let p = PathBuf::from(path);
    if !p.is_absolute() {
        return Err("The path must be absolute (e.g. C:\\Program Files\\App\\app.exe).".into());
    }
    let meta = std::fs::metadata(&p).map_err(|_| "No file exists at that path.".to_string())?;

    #[cfg(target_os = "macos")]
    if meta.is_dir() && p.extension().is_some_and(|e| e == "app") {
        return Ok(p);
    }
    if !meta.is_file() {
        return Err("The path must point to an application file, not a folder.".into());
    }

    #[cfg(windows)]
    {
        // Only real executables; scripts (.bat/.cmd/.ps1/...) would run through an interpreter.
        if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
            return Err("Only .exe applications can be added. Scripts and shortcuts aren't allowed.".into());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return Err("That file isn't executable.".into());
        }
    }
    Ok(p)
}

fn validate_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::validation("Give the application a name."));
    }
    if name.chars().count() > NAME_MAX_CHARS {
        return Err(AppError::validation(format!("Name must be at most {NAME_MAX_CHARS} characters.")));
    }
    if name.chars().any(char::is_control) {
        return Err(AppError::validation("Name cannot contain control characters."));
    }
    Ok(name.to_string())
}

pub fn add(conn: &Connection, name: &str, path: &str) -> AppResult<AppEntry> {
    let name = validate_name(name)?;
    let path = validate_path(path).map_err(AppError::Validation)?;
    let id = uuid::Uuid::new_v4().to_string();
    match conn.execute("INSERT INTO applications (id, name, path) VALUES (?1, ?2, ?3)", params![id, name, path.display().to_string()]) {
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
            return Err(AppError::validation(format!("An application named \"{name}\" already exists.")));
        }
        r => r?,
    };
    tracing::info!(event = "APP_ALLOWLIST_ADDED", name = %name);
    get(conn, &id)?.ok_or_else(|| AppError::internal("application vanished after insert"))
}

pub fn remove(conn: &Connection, id: &str) -> AppResult<()> {
    if conn.execute("DELETE FROM applications WHERE id = ?1", [id])? == 0 {
        return Err(AppError::validation("That application is no longer in the list."));
    }
    tracing::info!(event = "APP_ALLOWLIST_REMOVED");
    Ok(())
}

fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Resolve a user/model-supplied name against the allowlist.
pub fn resolve(apps: &[AppEntry], query: &str) -> Result<AppEntry, ToolError> {
    let q = normalize(query);
    if q.is_empty() {
        return Err(ToolError::invalid("Application name is empty."));
    }
    if let Some(a) = apps.iter().find(|a| normalize(&a.name) == q) {
        return Ok(a.clone());
    }
    let partial: Vec<&AppEntry> = apps
        .iter()
        .filter(|a| {
            let n = normalize(&a.name);
            n.contains(&q) || q.contains(&n)
        })
        .collect();
    match partial.as_slice() {
        [one] => Ok((*one).clone()),
        [] => {
            let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
            Err(ToolError::not_found(if names.is_empty() {
                format!("\"{query}\" isn't an allowed application. No applications have been added yet — the user can add them on the Tools page.")
            } else {
                format!("\"{query}\" isn't an allowed application. Allowed: {}. The user can add more on the Tools page.", names.join(", "))
            }))
        }
        many => Err(ToolError::invalid(format!(
            "\"{query}\" matches several applications: {}. Ask which one.",
            many.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
        ))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchOutcome {
    /// The process is still running after the settle time.
    Running { pid: u32 },
    /// The process exited successfully (typical for launchers / single-instance apps).
    HandedOff,
}

/// Launch an allowlisted application and verify the process started.
pub async fn launch(entry: &AppEntry) -> Result<LaunchOutcome, ToolError> {
    let path = validate_path(&entry.path).map_err(|e| ToolError::failed(format!("{} is no longer launchable: {e}", entry.name)))?;
    let mut cmd = command_for(&path);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(|e| ToolError::failed(format!("Couldn't start {}: {e}", entry.name)))?;
    let pid = child.id();
    tokio::time::sleep(SETTLE_TIME).await;
    match child.try_wait() {
        Ok(None) => {
            // Reap the process when it eventually exits so it never lingers as a zombie.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(LaunchOutcome::Running { pid })
        }
        Ok(Some(status)) if status.success() => Ok(LaunchOutcome::HandedOff),
        Ok(Some(status)) => Err(ToolError::failed(format!(
            "{} started but exited immediately ({}).",
            entry.name,
            status.code().map(|c| format!("exit code {c}")).unwrap_or_else(|| "terminated".into())
        ))),
        Err(e) => Err(ToolError::failed(format!("Couldn't check whether {} started: {e}", entry.name))),
    }
}

fn command_for(path: &Path) -> Command {
    #[cfg(target_os = "macos")]
    if path.extension().is_some_and(|e| e == "app") {
        let mut c = Command::new("/usr/bin/open");
        c.arg("-a").arg(path);
        return c;
    }
    #[allow(unused_mut)]
    let mut c = Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        c.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    c
}

/// Well-known applications that exist on this machine, offered as one-click
/// additions. Nothing is added without the user's action.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppCandidate {
    pub name: String,
    pub path: String,
}

/// Start-menu entries that aren't apps you'd ask to open (uninstallers, docs, links).
pub fn is_app_shortcut_name(name: &str) -> bool {
    let n = name.to_lowercase();
    !["uninstall", "uninst", "readme", "read me", "help", "documentation", "manual", "website", "release notes", "license", "setup", "repair"]
        .iter()
        .any(|w| n.contains(w))
}

pub fn detect() -> Vec<AppCandidate> {
    let mut out: Vec<AppCandidate> = Vec::new();
    let mut push = |name: &str, path: PathBuf| {
        if validate_path(&path.display().to_string()).is_ok() && !out.iter().any(|c| c.name == name) {
            out.push(AppCandidate { name: name.into(), path: path.display().to_string() });
        }
    };

    #[cfg(windows)]
    {
        let env = |k: &str| std::env::var(k).ok().map(PathBuf::from);
        let local = env("LOCALAPPDATA");
        let roaming = env("APPDATA");
        let pf = env("ProgramFiles");
        let pf86 = env("ProgramFiles(x86)");
        let win = env("SystemRoot").unwrap_or_else(|| PathBuf::from("C:\\Windows"));
        let cands: Vec<(&str, Option<PathBuf>)> = vec![
            ("VS Code", local.as_ref().map(|p| p.join("Programs\\Microsoft VS Code\\Code.exe"))),
            ("VS Code", pf.as_ref().map(|p| p.join("Microsoft VS Code\\Code.exe"))),
            ("Google Chrome", pf.as_ref().map(|p| p.join("Google\\Chrome\\Application\\chrome.exe"))),
            ("Google Chrome", pf86.as_ref().map(|p| p.join("Google\\Chrome\\Application\\chrome.exe"))),
            ("Microsoft Edge", pf86.as_ref().map(|p| p.join("Microsoft\\Edge\\Application\\msedge.exe"))),
            ("Firefox", pf.as_ref().map(|p| p.join("Mozilla Firefox\\firefox.exe"))),
            ("Spotify", roaming.as_ref().map(|p| p.join("Spotify\\Spotify.exe"))),
            ("Notepad", Some(win.join("System32\\notepad.exe"))),
            ("Calculator", Some(win.join("System32\\calc.exe"))),
            ("File Explorer", Some(win.join("explorer.exe"))),
            ("Paint", Some(win.join("System32\\mspaint.exe"))),
            ("Task Manager", Some(win.join("System32\\Taskmgr.exe"))),
            ("Windows Terminal", local.as_ref().map(|p| p.join("Microsoft\\WindowsApps\\wt.exe"))),
        ];
        for (name, path) in cands {
            if let Some(p) = path {
                push(name, p);
            }
        }
        // Everything else the Start menu lists: resolve each shortcut to its program.
        let start_menus = [env("ProgramData"), roaming.clone()].into_iter().flatten().map(|b| b.join("Microsoft\\Windows\\Start Menu\\Programs"));
        for dir in start_menus {
            for entry in walkdir::WalkDir::new(dir).max_depth(4).into_iter().flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("lnk")) != Some(true) {
                    continue;
                }
                let Some(name) = p.file_stem().and_then(|n| n.to_str()) else { continue };
                if !is_app_shortcut_name(name) {
                    continue;
                }
                let target = lnk::ShellLink::open(p, lnk::encoding::WINDOWS_1252)
                    .ok()
                    .and_then(|l| l.link_info().as_ref().and_then(|i| i.local_base_path().map(PathBuf::from)));
                if let Some(t) = target.filter(|t| t.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("exe")) == Some(true)) {
                    push(name, t);
                }
            }
        }
    }
    #[cfg(windows)]
    {
        out.sort_by_key(|c| c.name.to_lowercase());
        out.truncate(400);
    }
    #[cfg(target_os = "macos")]
    {
        for (name, p) in [
            ("VS Code", "/Applications/Visual Studio Code.app"),
            ("Safari", "/Applications/Safari.app"),
            ("Google Chrome", "/Applications/Google Chrome.app"),
            ("Firefox", "/Applications/Firefox.app"),
            ("Calculator", "/System/Applications/Calculator.app"),
            ("Terminal", "/System/Applications/Utilities/Terminal.app"),
            ("Notes", "/System/Applications/Notes.app"),
        ] {
            push(name, PathBuf::from(p));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let path_var = std::env::var_os("PATH").unwrap_or_default();
        for (name, bin) in [
            ("VS Code", "code"),
            ("Firefox", "firefox"),
            ("Google Chrome", "google-chrome"),
            ("Chromium", "chromium"),
            ("Calculator", "gnome-calculator"),
            ("Terminal", "gnome-terminal"),
            ("Files", "nautilus"),
            ("Text Editor", "gedit"),
            ("XTerm", "xterm"),
        ] {
            for dir in std::env::split_paths(&path_var) {
                push(name, dir.join(bin));
            }
        }
    }
    out
}

// ----- Tools -----

pub struct ListApplicationsTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl ListApplicationsTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "list_applications",
                title: "List applications",
                description: "List the applications the user has allowed IGRIS to open. Call this before launch_application \
if you're unsure of the exact name.",
                input_schema: json!({ "type": "object", "properties": {}, "required": [], "additionalProperties": false }),
                permission: PermissionLevel::Safe,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ListApplicationsTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn describe(&self, _input: &Value) -> String {
        "List allowed applications".into()
    }

    async fn execute(&self, _input: &Value) -> ToolResultT {
        let apps = list(&*self.db.conn().map_err(|e| ToolError::failed(e.to_string()))?).map_err(|e| ToolError::failed(e.to_string()))?;
        if apps.is_empty() {
            return Ok(ToolOutput {
                content: "No applications are allowed yet. The user can add them on the Tools page.".into(),
                summary: "No applications added".into(),
                sources: vec![],
                media: Vec::new(),
            });
        }
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        Ok(ToolOutput {
            content: format!("Allowed applications: {}", names.join(", ")),
            summary: format!("{} allowed", apps.len()),
            sources: vec![],
            media: Vec::new(),
        })
    }
}

pub struct LaunchApplicationTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl LaunchApplicationTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "launch_application",
                title: "Open application",
                description: "Open one of the user's allowed applications by name (see list_applications). Only applications \
the user has added on the Tools page can be opened; you cannot run arbitrary programs or commands.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "minLength": 1, "maxLength": NAME_MAX_CHARS, "description": "Application name, e.g. \"VS Code\"" }
                    },
                    "required": ["name"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for LaunchApplicationTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn describe(&self, input: &Value) -> String {
        format!("Open {}", input["name"].as_str().unwrap_or_default().trim())
    }

    async fn execute(&self, input: &Value) -> ToolResultT {
        let query = input["name"].as_str().unwrap_or_default();
        let apps = list(&*self.db.conn().map_err(|e| ToolError::failed(e.to_string()))?).map_err(|e| ToolError::failed(e.to_string()))?;
        let entry = resolve(&apps, query)?;
        tracing::info!(event = "APP_LAUNCH_REQUESTED", name = %entry.name);
        match launch(&entry).await? {
            LaunchOutcome::Running { pid } => Ok(ToolOutput {
                content: format!("{} is open (process {pid} is running).", entry.name),
                summary: format!("{} is running", entry.name),
                sources: vec![],
                media: Vec::new(),
            }),
            LaunchOutcome::HandedOff => Ok(ToolOutput {
                content: format!("{} was launched. Its launcher exited normally, which usually means it opened or was already running.", entry.name),
                summary: format!("{} launched", entry.name),
                sources: vec![],
                media: Vec::new(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> AppEntry {
        AppEntry { id: name.into(), name: name.into(), path: "/x".into(), created_at: String::new() }
    }

    #[test]
    fn resolves_names_forgivingly_but_unambiguously() {
        let apps = vec![entry("VS Code"), entry("Google Chrome"), entry("Chrome Canary")];
        assert_eq!(resolve(&apps, "vscode").unwrap().name, "VS Code");
        assert_eq!(resolve(&apps, "  VS-Code ").unwrap().name, "VS Code");
        assert_eq!(resolve(&apps, "google chrome").unwrap().name, "Google Chrome");
        assert_eq!(resolve(&apps, "chrome").unwrap_err().kind, super::super::ToolErrorKind::InvalidInput);
        let e = resolve(&apps, "photoshop").unwrap_err();
        assert!(e.message.contains("Allowed: VS Code"));
        assert!(resolve(&[], "x").unwrap_err().message.contains("No applications"));
    }

    #[test]
    fn rejects_bad_paths() {
        assert!(validate_path("").is_err());
        assert!(validate_path("relative/app").is_err());
        assert!(validate_path("/definitely/not/here").is_err());
        assert!(validate_path("/tmp").is_err(), "directories are not apps");
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("not-exec.sh");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        assert!(validate_path(&script.display().to_string()).is_err(), "non-executable file");
    }

    #[test]
    fn start_menu_noise_is_skipped() {
        assert!(is_app_shortcut_name("Visual Studio Code"));
        assert!(is_app_shortcut_name("Spotify"));
        for junk in ["Uninstall Zoom", "Python 3.12 Manuals", "Readme", "Steam Help", "Release Notes", "Repair Office"] {
            assert!(!is_app_shortcut_name(junk), "{junk}");
        }
    }

    #[test]
    fn allowlist_crud_and_duplicate_names() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn().unwrap();
        // Any real executable works; the test binary exists on every platform.
        let exe = std::env::current_exe().unwrap().display().to_string();
        let sh = exe.as_str();
        let a = add(&conn, " Shell ", sh).unwrap();
        assert_eq!(a.name, "Shell");
        assert!(add(&conn, "shell", sh).is_err(), "names are unique, case-insensitive");
        assert!(add(&conn, "", sh).is_err());
        assert!(add(&conn, "Nope", "/no/such/file").is_err());
        assert_eq!(list(&conn).unwrap().len(), 1);
        remove(&conn, &a.id).unwrap();
        assert!(remove(&conn, &a.id).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn launch_reports_running_handed_off_and_failure() {
        let dir = tempfile::tempdir().unwrap();
        let mk = |name: &str, body: &str| {
            use std::os::unix::fs::PermissionsExt;
            let p = dir.path().join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            AppEntry { id: name.into(), name: name.into(), path: p.display().to_string(), created_at: String::new() }
        };
        assert!(matches!(launch(&mk("long", "sleep 5")).await.unwrap(), LaunchOutcome::Running { .. }));
        assert_eq!(launch(&mk("quick", "exit 0")).await.unwrap(), LaunchOutcome::HandedOff);
        let err = launch(&mk("broken", "exit 3")).await.unwrap_err();
        assert!(err.message.contains("exit code 3"), "{}", err.message);
    }

    #[tokio::test]
    async fn launch_tool_refuses_unlisted_apps() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let tool = LaunchApplicationTool::new(db);
        let e = tool.execute(&json!({"name":"bash -c 'rm -rf /'"})).await.unwrap_err();
        assert_eq!(e.kind, super::super::ToolErrorKind::NotFound);
    }
}
