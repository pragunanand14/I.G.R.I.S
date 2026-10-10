//! Smart-home tools: see the user's devices and control the TV.
//!
//! Devices are set up by the user (Smart home → add a device; the TV asks for
//! permission on screen). The model can only use devices set up that way, only
//! through the fixed actions below, and every call goes through the tool
//! executor (permission policy, audit). Results say what the TV really did.

use std::sync::Arc;

use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::db::Database;
use crate::home::{self, DeviceKind, SmartHome, TvAction};

fn out(content: String, summary: impl Into<String>) -> ToolResultT {
    Ok(ToolOutput { content, summary: summary.into(), sources: vec![], media: Vec::new() })
}

pub struct HomeDevicesTool {
    spec: ToolSpec,
    home: SmartHome,
}

impl HomeDevicesTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "home_devices",
                title: "Smart-home devices",
                description: "List the user's smart-home devices set up in IGRIS (TVs today) and, for each, whether it's on, its \
volume and the app on screen. Use before controlling one, or when the user asks what's on.",
                input_schema: json!({"type": "object", "properties": {}, "required": [], "additionalProperties": false}),
                permission: PermissionLevel::Safe,
            },
            home: SmartHome::new(db),
        }
    }
}

#[async_trait::async_trait]
impl Tool for HomeDevicesTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, _i: &Value) -> String {
        "Check your smart-home devices".into()
    }
    async fn execute(&self, _i: &Value) -> ToolResultT {
        let devices = self.home.devices().map_err(|e| ToolError::failed(e.to_string()))?;
        if devices.is_empty() {
            return out("No smart-home devices are set up. The user can add their TV in IGRIS → Smart home.".into(), "None set up");
        }
        let mut lines = Vec::new();
        for d in &devices {
            let s = self.home.tv_state(&d.id).await.unwrap_or_default();
            let state = if !s.on {
                format!("off or unreachable{}", if d.can_power_on { " (can be switched on remotely)" } else { "" })
            } else {
                let mut parts = vec!["on".to_string()];
                if let Some(v) = s.volume {
                    parts.push(format!("volume {v}{}", if s.muted == Some(true) { ", muted" } else { "" }));
                }
                if let Some(app) = s.app.as_deref() {
                    let title = s.apps.iter().find(|a| a.id == app).map(|a| a.title.as_str()).unwrap_or(app);
                    parts.push(format!("showing {title}"));
                }
                parts.join(", ")
            };
            lines.push(format!("- \"{}\" ({}) — {state}", d.name, d.kind.label()));
        }
        out(lines.join("\n"), format!("{} device(s)", devices.len()))
    }
}

pub struct TvControlTool {
    spec: ToolSpec,
    home: SmartHome,
    db: Arc<Database>,
}

impl TvControlTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "tv_control",
                title: "Control the TV",
                description: "Control the user's TV (set up in IGRIS → Smart home). Actions: power_on, power_off, volume_up, \
volume_down, set_volume (value 0–100), mute, unmute, channel_up, channel_down, play, pause, stop, rewind, fast_forward, \
open_app (value: app name, e.g. \"Netflix\"), switch_input (value: e.g. \"HDMI 1\"), button (value: HOME, BACK, UP, DOWN, \
LEFT, RIGHT, ENTER, MENU, EXIT, INFO, 0–9), show_message (value: text shown on the TV). `device`: the TV's name, empty \
when there's only one. Report what the result says happened.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": {"type": "string", "enum": [
                            "power_on", "power_off", "volume_up", "volume_down", "set_volume", "mute", "unmute",
                            "channel_up", "channel_down", "play", "pause", "stop", "rewind", "fast_forward",
                            "open_app", "switch_input", "button", "show_message"
                        ]},
                        "value": {"type": "string", "description": "For set_volume, open_app, switch_input, button and show_message; otherwise empty."},
                        "device": {"type": "string", "description": "Which TV (its name); empty when there's only one."}
                    },
                    "required": ["action", "value", "device"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            home: SmartHome::new(db.clone()),
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for TvControlTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        let action = i["action"].as_str().unwrap_or("control").replace('_', " ");
        match i["value"].as_str().filter(|v| !v.trim().is_empty()) {
            Some(v) => format!("TV: {action} \"{v}\""),
            None => format!("TV: {action}"),
        }
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let action = TvAction::parse(i["action"].as_str().unwrap_or_default(), i.get("value").filter(|v| !v.as_str().is_some_and(|s| s.trim().is_empty())))
            .map_err(|e| ToolError::failed(e.to_string()))?;
        let device = {
            let c = self.db.conn().map_err(|e| ToolError::failed(e.to_string()))?;
            home::find(&c, i["device"].as_str(), DeviceKind::LgTv).map_err(|e| ToolError::failed(e.to_string()))?
        };
        let done = self.home.tv(&device.id, &action).await.map_err(|e| ToolError::failed(e.to_string()))?;
        out(done.clone(), done)
    }
}

/// All smart-home tools, for a platform registry.
pub fn all(db: &Arc<Database>) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(HomeDevicesTool::new(db.clone())), Arc::new(TvControlTool::new(db.clone()))]
}
