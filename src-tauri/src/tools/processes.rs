//! Process tools: list running processes (read-only) and close allowlisted apps.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use super::apps;
use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::db::Database;

pub struct ListProcessesTool {
    spec: ToolSpec,
}

impl Default for ListProcessesTool {
    fn default() -> Self {
        Self {
            spec: ToolSpec {
                name: "list_processes",
                title: "List processes",
                description: "List the processes using the most CPU or memory right now (name, PID, CPU %, memory). Read-only.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "sort": { "type": "string", "enum": ["cpu", "memory"] },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 30 }
                    },
                    "required": ["sort", "limit"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Safe,
            },
        }
    }
}

#[async_trait::async_trait]
impl Tool for ListProcessesTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("List top processes by {}", input["sort"].as_str().unwrap_or("cpu"))
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let by_mem = input["sort"] == "memory";
        let limit = input["limit"].as_u64().unwrap_or(10) as usize;
        let rows = tokio::task::spawn_blocking(move || {
            let mut sys = System::new();
            let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
            sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(Duration::from_millis(250)));
            sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
            let cores = sys.cpus().len().max(1) as f32;
            let mut v: Vec<(String, u32, f32, u64)> = sys
                .processes()
                .values()
                // On Linux, sysinfo also lists threads as tasks; only real processes count.
                .filter(|p| p.thread_kind().is_none())
                .map(|p| (p.name().to_string_lossy().into_owned(), p.pid().as_u32(), p.cpu_usage() / cores, p.memory()))
                .collect();
            if by_mem {
                v.sort_by_key(|p| std::cmp::Reverse(p.3));
            } else {
                v.sort_by(|a, b| b.2.total_cmp(&a.2));
            }
            (v.len(), v.into_iter().take(limit).collect::<Vec<_>>())
        })
        .await
        .map_err(|e| ToolError::failed(e.to_string()))?;
        let lines: Vec<String> = rows
            .1
            .iter()
            .map(|(n, pid, cpu, mem)| format!("{n} (PID {pid}) — CPU {cpu:.1}%, memory {:.0} MB", *mem as f64 / 1_048_576.0))
            .collect();
        Ok(ToolOutput { content: format!("{} processes running. Top {} by {}:\n{}", rows.0, lines.len(), if by_mem { "memory" } else { "CPU" }, lines.join("\n")), summary: format!("{} processes", rows.0), sources: vec![] })
    }
}

pub struct CloseApplicationTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl CloseApplicationTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "close_application",
                title: "Close application",
                description: "Ask one of the user's allowed applications (Tools page) to close, by name. The app gets a normal \
close request, so it may prompt to save unsaved work. Always requires the user's approval. Other processes can't be closed.",
                input_schema: json!({
                    "type": "object",
                    "properties": { "name": { "type": "string", "minLength": 1, "maxLength": apps::NAME_MAX_CHARS } },
                    "required": ["name"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Sensitive,
            },
            db,
        }
    }
}

fn canonical(p: &std::path::Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Send a polite close request (SIGTERM / WM_CLOSE via taskkill without /F).
fn request_close(pid: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err(format!("taskkill exited with {status}")) }
    }
    #[cfg(not(windows))]
    {
        let sys = System::new_with_specifics(sysinfo::RefreshKind::nothing().with_processes(ProcessRefreshKind::nothing()));
        match sys.process(sysinfo::Pid::from_u32(pid)) {
            Some(p) => match p.kill_with(sysinfo::Signal::Term) {
                Some(true) => Ok(()),
                _ => Err("the close signal was refused".into()),
            },
            None => Ok(()),
        }
    }
}

#[async_trait::async_trait]
impl Tool for CloseApplicationTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("Close {}", input["name"].as_str().unwrap_or_default())
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let list = apps::list(&*self.db.conn().map_err(|e| ToolError::failed(e.to_string()))?).map_err(|e| ToolError::failed(e.to_string()))?;
        let entry = apps::resolve(&list, input["name"].as_str().unwrap_or_default())?;
        let target = canonical(std::path::Path::new(&entry.path));
        let find = move || -> Vec<u32> {
            let mut sys = System::new();
            sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::Always));
            sys.processes().values().filter(|p| p.thread_kind().is_none() && p.exe().map(canonical).as_deref() == Some(target.as_path())).map(|p| p.pid().as_u32()).collect()
        };
        let pids = tokio::task::spawn_blocking(find.clone()).await.map_err(|e| ToolError::failed(e.to_string()))?;
        if pids.is_empty() {
            return Ok(ToolOutput { content: format!("{} isn't running.", entry.name), summary: "Not running".into(), sources: vec![] });
        }
        let mut errors = Vec::new();
        for pid in &pids {
            if let Err(e) = request_close(*pid) {
                errors.push(format!("PID {pid}: {e}"));
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        let remaining = tokio::task::spawn_blocking(find).await.map_err(|e| ToolError::failed(e.to_string()))?;
        if remaining.is_empty() {
            Ok(ToolOutput { content: format!("Closed {}.", entry.name), summary: "Closed".into(), sources: vec![] })
        } else if errors.len() == pids.len() {
            Err(ToolError::failed(format!("Couldn't ask {} to close ({}).", entry.name, errors.join("; "))))
        } else {
            Ok(ToolOutput {
                content: format!("Asked {} to close, but it's still running — it may be waiting for the user to save or confirm.", entry.name),
                summary: "Still running".into(),
                sources: vec![],
            })
        }
    }
}

// ----- open_url -----

pub type UrlOpener = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

pub struct OpenUrlTool {
    spec: ToolSpec,
    opener: UrlOpener,
}

impl OpenUrlTool {
    pub fn new(opener: UrlOpener) -> Self {
        Self {
            spec: ToolSpec {
                name: "open_url",
                title: "Open website",
                description: "Open an http(s) web address in the user's default browser.",
                input_schema: json!({
                    "type": "object",
                    "properties": { "url": { "type": "string", "minLength": 8, "maxLength": 2000 } },
                    "required": ["url"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            opener,
        }
    }
}

#[async_trait::async_trait]
impl Tool for OpenUrlTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("Open {}", input["url"].as_str().unwrap_or_default())
    }
    fn validate(&self, input: &Value) -> Result<(), ToolError> {
        let url = reqwest::Url::parse(input["url"].as_str().unwrap_or_default().trim()).map_err(|_| ToolError::invalid("That isn't a valid URL."))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ToolError::invalid("Only http and https links can be opened."));
        }
        Ok(())
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let url = input["url"].as_str().unwrap_or_default().trim();
        (self.opener)(url).map_err(|e| ToolError::failed(format!("Couldn't open the browser: {e}")))?;
        Ok(ToolOutput { content: format!("Opened {url} in the default browser."), summary: "Opened".into(), sources: vec![] })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[tokio::test]
    async fn lists_real_processes() {
        let out = ListProcessesTool::default().execute(&json!({"sort":"memory","limit":5})).await.unwrap();
        assert!(out.content.contains("PID"));
        assert!(out.content.lines().count() <= 6);
    }

    #[test]
    fn open_url_only_allows_web_links() {
        let t = OpenUrlTool::new(Arc::new(|_: &str| Ok(())));
        assert!(t.validate(&json!({"url":"https://example.com"})).is_ok());
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "ms-settings:privacy", "not a url"] {
            assert!(t.validate(&json!({ "url": bad })).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn open_url_calls_the_opener() {
        let seen = Arc::new(Mutex::new(String::new()));
        let s = seen.clone();
        let t = OpenUrlTool::new(Arc::new(move |u: &str| {
            *s.lock().unwrap() = u.to_string();
            Ok(())
        }));
        t.execute(&json!({"url":"https://tauri.app"})).await.unwrap();
        assert_eq!(*seen.lock().unwrap(), "https://tauri.app");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn closes_only_allowlisted_apps() {
        use std::os::unix::fs::PermissionsExt;
        // A private copy of `sleep`, so only this test's process matches.
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("igris-test-sleeper");
        std::fs::copy(which_sleep(), &app).unwrap();
        std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
        let db = Arc::new(Database::open_in_memory().unwrap());
        apps::add(&db.conn().unwrap(), "Sleeper", &app.display().to_string()).unwrap();
        let mut child = std::process::Command::new(&app).arg("30").spawn().unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;

        let tool = CloseApplicationTool::new(db.clone());
        assert_eq!(tool.execute(&json!({"name":"Unknown"})).await.unwrap_err().kind, super::super::ToolErrorKind::NotFound);
        let out = tool.execute(&json!({"name":"sleeper"})).await.unwrap();
        assert_eq!(out.content, "Closed Sleeper.");
        let status = child.wait().unwrap();
        assert!(!status.success(), "terminated by signal");
    }

    #[cfg(unix)]
    fn which_sleep() -> String {
        for p in ["/usr/bin/sleep", "/bin/sleep"] {
            if std::path::Path::new(p).exists() {
                return std::fs::canonicalize(p).unwrap().display().to_string();
            }
        }
        panic!("sleep not found");
    }
}
