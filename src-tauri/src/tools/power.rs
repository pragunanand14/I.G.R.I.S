//! `pc_power` — lock, sleep, shut down or restart this computer (also when the
//! user asks from their paired phone: the task arrives here and runs with this
//! computer's tools and rules). Locking runs right away; sleep, shutdown and
//! restart always ask first (unsaved work), and shutdown/restart give a minute
//! to cancel.

use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};

pub struct PcPowerTool {
    spec: ToolSpec,
}

impl Default for PcPowerTool {
    fn default() -> Self {
        Self {
            spec: ToolSpec {
                name: "pc_power",
                title: "Lock, sleep, shut down or restart",
                description: "Lock this computer, put it to sleep, shut it down or restart it, or cancel a pending shutdown. Use it \
when the user asks (also from their phone: \"lock my PC\"). Shutdown and restart happen after one minute (cancel_shutdown \
stops them); sleep and shutdown lose unsaved work, so they ask the user first.",
                input_schema: json!({
                    "type": "object",
                    "properties": { "action": { "type": "string", "enum": ["lock", "sleep", "shutdown", "restart", "cancel_shutdown"] } },
                    "required": ["action"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
        }
    }
}

/// Run a system command without flashing a console window.
fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("Couldn't run {program}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("{program} failed ({})", out.status) } else { err })
    }
}

/// The command for `action` on this operating system.
fn command(action: &str) -> Option<(&'static str, Vec<&'static str>)> {
    if cfg!(windows) {
        Some(match action {
            "lock" => ("rundll32.exe", vec!["user32.dll,LockWorkStation"]),
            "sleep" => ("rundll32.exe", vec!["powrprof.dll,SetSuspendState", "0,1,0"]),
            "shutdown" => ("shutdown.exe", vec!["/s", "/t", "60", "/c", "IGRIS: shutting down in 1 minute."]),
            "restart" => ("shutdown.exe", vec!["/r", "/t", "60", "/c", "IGRIS: restarting in 1 minute."]),
            "cancel_shutdown" => ("shutdown.exe", vec!["/a"]),
            _ => return None,
        })
    } else if cfg!(target_os = "macos") {
        Some(match action {
            "lock" => ("pmset", vec!["displaysleepnow"]),
            "sleep" => ("pmset", vec!["sleepnow"]),
            "shutdown" => ("osascript", vec!["-e", "tell app \"System Events\" to shut down"]),
            "restart" => ("osascript", vec!["-e", "tell app \"System Events\" to restart"]),
            _ => return None,
        })
    } else {
        Some(match action {
            "lock" => ("loginctl", vec!["lock-session"]),
            "sleep" => ("systemctl", vec!["suspend"]),
            "shutdown" => ("shutdown", vec!["-h", "+1"]),
            "restart" => ("shutdown", vec!["-r", "+1"]),
            "cancel_shutdown" => ("shutdown", vec!["-c"]),
            _ => return None,
        })
    }
}

#[async_trait::async_trait]
impl Tool for PcPowerTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    /// Sleep, shutdown and restart can lose unsaved work.
    fn always_ask(&self, i: &Value) -> bool {
        matches!(i["action"].as_str(), Some("sleep" | "shutdown" | "restart"))
    }
    fn describe(&self, i: &Value) -> String {
        match i["action"].as_str().unwrap_or_default() {
            "lock" => "Lock the computer".into(),
            "sleep" => "Put the computer to sleep (unsaved work stays open)".into(),
            "shutdown" => "Shut down the computer in 1 minute (unsaved work will be lost)".into(),
            "restart" => "Restart the computer in 1 minute (unsaved work will be lost)".into(),
            _ => "Cancel the pending shutdown".into(),
        }
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let action = i["action"].as_str().unwrap_or_default().to_string();
        let (program, args) = command(&action).ok_or_else(|| ToolError::invalid("This computer can't do that."))?;
        tokio::task::spawn_blocking(move || run(program, &args)).await.map_err(|e| ToolError::failed(e.to_string()))?.map_err(ToolError::failed)?;
        let said = match action.as_str() {
            "lock" => "Locked the computer.",
            "sleep" => "The computer is going to sleep.",
            "shutdown" => "The computer shuts down in 1 minute (say \"cancel the shutdown\" to stop it).",
            "restart" => "The computer restarts in 1 minute (say \"cancel the shutdown\" to stop it).",
            _ => "Cancelled the shutdown.",
        };
        tracing::info!(event = "PC_POWER", action = %action);
        Ok(ToolOutput { content: said.into(), summary: said.into(), sources: vec![], media: vec![] })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutting_down_asks_locking_doesnt() {
        let t = PcPowerTool::default();
        assert!(!t.always_ask(&json!({"action": "lock"})) && !t.always_ask(&json!({"action": "cancel_shutdown"})));
        for a in ["sleep", "shutdown", "restart"] {
            assert!(t.always_ask(&json!({"action": a})), "{a}");
        }
        assert!(command("lock").is_some() && command("nope").is_none());
        if cfg!(windows) {
            assert_eq!(command("shutdown").unwrap().1[..3], ["/s", "/t", "60"]);
        }
    }
}
