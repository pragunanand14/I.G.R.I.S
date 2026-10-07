//! Cross-device tools: see the user's devices and hand a task to one of them.
//!
//! `send_to_device` doesn't run anything here or remotely by itself: it asks
//! the other device to do the task, which that device plans and runs with its
//! own tools, its own permission policy and its own approvals (approvals can
//! come back to this device as signed answers). The result is reported as it
//! really is: sent, queued for an offline device, accepted, refused, done.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::device::hub::{DeviceHub, TaskView};
use crate::device::protocol::{ControlAction, RemoteStatus};
use crate::device::remote::Direction;
use crate::device::target::{self, Target};

/// How long `send_to_device` waits for the other device to pick the task up.
pub const PICKUP_WAIT: Duration = Duration::from_secs(20);

/// The device hub, filled in once this device's identity is unlocked (on
/// Android that needs the Keystore, which can't be asked during startup).
pub type HubSlot = Arc<std::sync::OnceLock<Arc<DeviceHub>>>;

fn ready(slot: &HubSlot) -> Result<&Arc<DeviceHub>, ToolError> {
    slot.get().ok_or_else(|| ToolError::failed("Cross-device features are still starting up (or couldn't start: see Settings → Devices)."))
}

fn out(content: String, summary: impl Into<String>) -> ToolResultT {
    Ok(ToolOutput { content, summary: summary.into(), sources: vec![], media: Vec::new() })
}

/// One line per task, in plain words.
pub fn describe_task(t: &TaskView) -> String {
    let who = &t.peer_name;
    let what = &t.task.objective;
    let state = match (t.task.direction, t.task.status) {
        (Direction::Outgoing, RemoteStatus::Pending) => format!("not sent yet ({})", t.task.detail.clone().unwrap_or_default()),
        (Direction::Outgoing, RemoteStatus::Sent) => format!("sent to {who}, waiting for it to accept"),
        (Direction::Outgoing, RemoteStatus::Queued) => format!("{who} is offline; it will get the task if it comes online within 10 minutes"),
        (_, RemoteStatus::Accepted) => format!("{who} accepted it"),
        (_, RemoteStatus::Running) => format!("running on {who}"),
        (_, RemoteStatus::WaitingForApproval) => "waiting for your approval".to_string(),
        (_, RemoteStatus::Paused) => format!("paused on {who}"),
        (_, RemoteStatus::Completed) => format!("done — completed on {who}"),
        (_, RemoteStatus::Ended) => format!("{who} stopped without a verified result; check it there"),
        (_, RemoteStatus::Failed) => format!("failed on {who}"),
        (_, RemoteStatus::Cancelled) => "stopped".to_string(),
        (_, RemoteStatus::Rejected) => format!("{who} didn't take it"),
        (_, RemoteStatus::TimedOut) => format!("{who} didn't respond in time, so it wasn't done"),
        (Direction::Incoming, s) => s.as_str().replace('_', " "),
    };
    let detail = t.task.detail.as_deref().filter(|d| !d.is_empty()).map(|d| format!("\n  {d}")).unwrap_or_default();
    format!("[{}] \"{what}\" — {state}{detail}", t.task.request_id)
}

pub struct ListDevicesTool {
    spec: ToolSpec,
    hub: HubSlot,
}

impl ListDevicesTool {
    pub fn new(hub: HubSlot) -> Self {
        Self {
            spec: ToolSpec {
                name: "list_devices",
                title: "List devices",
                description: "List the user's devices connected to this IGRIS (this one and the paired ones): name, kind, \
online or not, and what each can do. Use before sending a task to another device, or when the user asks about their devices.",
                input_schema: json!({"type": "object", "properties": {}, "required": [], "additionalProperties": false}),
                permission: PermissionLevel::Safe,
            },
            hub,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ListDevicesTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, _i: &Value) -> String {
        "List your devices".into()
    }
    async fn execute(&self, _i: &Value) -> ToolResultT {
        let hub = ready(&self.hub)?;
        let me = hub.this_device();
        let caps = |c: &[crate::device::Capability]| c.iter().map(|c| c.describe()).collect::<Vec<_>>().join(", ");
        let mut lines = vec![format!("- \"{}\" ({}) — this device. Can: {}.", me.name, me.platform.as_str(), caps(&me.capabilities))];
        let devices = hub.devices().map_err(|e| ToolError::failed(e.to_string()))?;
        for d in devices.iter().filter(|d| d.device.active()) {
            let state = if d.online { "online" } else { "offline" };
            let can = if d.device.capabilities.is_empty() { "not reported yet".to_string() } else { caps(&d.device.capabilities) };
            lines.push(format!("- \"{}\" ({}) — {state}. Can: {can}.", d.device.name, d.device.platform.as_str()));
        }
        let link = hub.status();
        if devices.iter().all(|d| !d.device.active()) {
            lines.push("No other devices are paired. The user can pair one in Settings → Devices.".into());
        } else if link.state != "connected" {
            lines.push(format!("Cross-device connection: {} — other devices can't be reached right now.", link.state));
        }
        out(lines.join("\n"), format!("{} device(s)", 1 + devices.iter().filter(|d| d.device.active()).count()))
    }
}

pub struct SendToDeviceTool {
    spec: ToolSpec,
    hub: HubSlot,
}

impl SendToDeviceTool {
    pub fn new(hub: HubSlot) -> Self {
        Self {
            spec: ToolSpec {
                name: "send_to_device",
                title: "Send to device",
                description: "Ask another of the user's devices to do a task (\"open Notepad on my laptop\", \"on my PC, find \
yesterday's report\"). `device` is how the user named it (\"my laptop\", \"PC\", \"phone\", or its name). `task` is the \
request in plain words, complete on its own (the other device doesn't see this conversation). The other device runs it with \
its own tools and asks for approval as usual. Returns whether it was accepted; it doesn't wait for the task to finish (use \
device_task_status). If more than one device fits, ask the user which one. Never use it for this device.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "device": { "type": "string", "minLength": 1, "maxLength": 80 },
                        "task": { "type": "string", "minLength": 1, "maxLength": 2000 }
                    },
                    "required": ["device", "task"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            hub,
        }
    }
}

#[async_trait::async_trait]
impl Tool for SendToDeviceTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("Ask {} to: {}", i["device"].as_str().unwrap_or("?"), crate::orchestrator::task::clip(i["task"].as_str().unwrap_or(""), 120))
    }
    fn timeout(&self, _i: &Value) -> Duration {
        PICKUP_WAIT + Duration::from_secs(10)
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let hub = ready(&self.hub)?;
        let phrase = i["device"].as_str().unwrap_or_default();
        let task = i["task"].as_str().unwrap_or_default();
        let devices: Vec<_> = hub.devices().map_err(|e| ToolError::failed(e.to_string()))?.into_iter().map(|d| d.device).collect();
        let dev = match target::resolve(phrase, &hub.this_device(), &devices) {
            Ok(Target::Here) => return Err(ToolError::invalid("That's this device. Do it here with your own tools instead.")),
            Ok(Target::Remote(d)) => *d,
            Err(msg) => return Err(ToolError::invalid(msg)),
        };
        if !dev.capabilities.is_empty() && !dev.capabilities.contains(&crate::device::Capability::Tasks) {
            return Err(ToolError::refused(format!("{} can't run tasks.", dev.name)));
        }
        let sent = hub.send_task(&dev.device_id, task).map_err(|e| ToolError::failed(e.to_string()))?;
        let t = hub.wait_for_pickup(&sent.task.request_id, PICKUP_WAIT).await.map_err(|e| ToolError::failed(e.to_string()))?;
        let line = describe_task(&t);
        let summary = match t.task.status {
            RemoteStatus::Accepted | RemoteStatus::Running | RemoteStatus::WaitingForApproval => format!("{} accepted the task", dev.name),
            RemoteStatus::Queued => format!("{} is offline — queued", dev.name),
            RemoteStatus::Sent => format!("Sent to {}", dev.name),
            RemoteStatus::Pending => "Not sent yet".to_string(),
            s if s.is_final() => format!("{}: {}", dev.name, s.as_str().replace('_', " ")),
            _ => format!("Sent to {}", dev.name),
        };
        let note = match t.task.status {
            RemoteStatus::Accepted | RemoteStatus::Running | RemoteStatus::WaitingForApproval => {
                "\nIt is not finished yet. Tell the user it was accepted; they'll be notified when it's done (or check with device_task_status)."
            }
            RemoteStatus::Queued | RemoteStatus::Sent | RemoteStatus::Pending => "\nIt has not started. Don't say it's done.",
            _ => "",
        };
        out(format!("{line}{note}"), summary)
    }
}

pub struct DeviceTaskStatusTool {
    spec: ToolSpec,
    hub: HubSlot,
}

impl DeviceTaskStatusTool {
    pub fn new(hub: HubSlot) -> Self {
        Self {
            spec: ToolSpec {
                name: "device_task_status",
                title: "Device task status",
                description: "Where tasks sent to (or received from) the user's other devices stand. `request_id` is the id \
send_to_device returned, or \"\" for the recent ones.",
                input_schema: json!({
                    "type": "object",
                    "properties": { "request_id": { "type": "string", "maxLength": 32 } },
                    "required": ["request_id"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Safe,
            },
            hub,
        }
    }
}

#[async_trait::async_trait]
impl Tool for DeviceTaskStatusTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, _i: &Value) -> String {
        "Check tasks on other devices".into()
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let hub = ready(&self.hub)?;
        let id = i["request_id"].as_str().unwrap_or_default().trim();
        let tasks = if id.is_empty() {
            hub.tasks(10).map_err(|e| ToolError::failed(e.to_string()))?
        } else {
            vec![hub.task(id).map_err(|e| ToolError::failed(e.to_string()))?.ok_or_else(|| ToolError::not_found("No task with that id."))?]
        };
        if tasks.is_empty() {
            return out("No tasks have been sent between devices yet.".into(), "No tasks");
        }
        out(tasks.iter().map(describe_task).collect::<Vec<_>>().join("\n"), format!("{} task(s)", tasks.len()))
    }
}

pub struct DeviceTaskControlTool {
    spec: ToolSpec,
    hub: HubSlot,
}

impl DeviceTaskControlTool {
    pub fn new(hub: HubSlot) -> Self {
        Self {
            spec: ToolSpec {
                name: "device_task_control",
                title: "Control device task",
                description: "Pause, resume or stop a task this device sent to another device (by its request_id). A resumed \
task re-checks the current state before acting.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "request_id": { "type": "string", "minLength": 32, "maxLength": 32 },
                        "action": { "type": "string", "enum": ["pause", "resume", "stop"] }
                    },
                    "required": ["request_id", "action"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            hub,
        }
    }
}

#[async_trait::async_trait]
impl Tool for DeviceTaskControlTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!(
            "{} a task on another device",
            match i["action"].as_str() {
                Some("pause") => "Pause",
                Some("resume") => "Resume",
                _ => "Stop",
            }
        )
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let hub = ready(&self.hub)?;
        let action = match i["action"].as_str() {
            Some("pause") => ControlAction::Pause,
            Some("resume") => ControlAction::Resume,
            _ => ControlAction::Stop,
        };
        let t = hub.control_task(i["request_id"].as_str().unwrap_or_default(), action).map_err(|e| ToolError::failed(e.to_string()))?;
        out(
            format!(
                "Asked {} to {}. Its answer will update the task (device_task_status); it hasn't been confirmed yet.",
                t.peer_name,
                i["action"].as_str().unwrap_or("stop")
            ),
            format!("Asked {}", t.peer_name),
        )
    }
}

/// All cross-device tools, for a platform registry.
pub fn all(hub: &HubSlot) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ListDevicesTool::new(hub.clone())),
        Arc::new(SendToDeviceTool::new(hub.clone())),
        Arc::new(DeviceTaskStatusTool::new(hub.clone())),
        Arc::new(DeviceTaskControlTool::new(hub.clone())),
    ]
}
