//! `system_info` — real system telemetry for the model (read-only).

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::system::{Connectivity, ConnectivityMonitor, SystemMonitor, SystemSnapshot};

pub struct SystemInfoTool {
    spec: ToolSpec,
    monitor: Arc<Mutex<SystemMonitor>>,
    connectivity: ConnectivityMonitor,
}

impl SystemInfoTool {
    pub fn new(monitor: Arc<Mutex<SystemMonitor>>, connectivity: ConnectivityMonitor) -> Self {
        Self {
            spec: ToolSpec {
                name: "system_info",
                title: "System info",
                description: "Read the user's current system status from the operating system: CPU usage and model, memory, \
disks, network reachability and throughput, battery, temperatures, OS, hostname and uptime. Use it whenever the \
user asks about their computer's state or performance. Values are live measurements.",
                input_schema: json!({ "type": "object", "properties": {}, "required": [], "additionalProperties": false }),
                permission: PermissionLevel::Safe,
            },
            monitor,
            connectivity,
        }
    }
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1024f64.powi(3))
}

/// Compact, model-friendly rendering. Unavailable values are stated as such.
pub fn render(s: &SystemSnapshot) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "OS: {} {} ({}), host {}, uptime {} min",
        s.host.os_name.as_deref().unwrap_or("unknown"),
        s.host.os_version.as_deref().unwrap_or(""),
        s.host.arch,
        s.host.hostname.as_deref().unwrap_or("unknown"),
        s.uptime_secs / 60
    ));
    lines.push(format!(
        "CPU: {} — usage {}, {} logical cores{}",
        if s.cpu.brand.is_empty() { "unknown model" } else { &s.cpu.brand },
        s.cpu.usage_percent.map(|u| format!("{u:.0}%")).unwrap_or_else(|| "not yet measured".into()),
        s.cpu.logical_cores,
        s.cpu.physical_cores.map(|p| format!(" / {p} physical")).unwrap_or_default()
    ));
    let m = &s.memory;
    lines.push(format!(
        "Memory: {} used of {} ({:.0}%), {} available{}",
        gib(m.used_bytes),
        gib(m.total_bytes),
        if m.total_bytes > 0 { m.used_bytes as f64 / m.total_bytes as f64 * 100.0 } else { 0.0 },
        gib(m.available_bytes),
        if m.swap_total_bytes > 0 { format!(", swap {} of {}", gib(m.swap_used_bytes), gib(m.swap_total_bytes)) } else { String::new() }
    ));
    for d in &s.disks {
        let used = d.total_bytes.saturating_sub(d.available_bytes);
        lines.push(format!("Disk {}: {} free of {} ({} used)", d.mount_point, gib(d.available_bytes), gib(d.total_bytes), gib(used)));
    }
    let n = &s.network;
    lines.push(format!(
        "Network: internet {}, {}{}",
        match n.connectivity {
            Connectivity::Online => "reachable",
            Connectivity::Offline => "unreachable",
            Connectivity::Unknown => "not yet checked",
        },
        n.interface_count.map_or_else(|| "interfaces not visible to apps".to_string(), |c| format!("{c} interfaces")),
        match (n.rx_bytes_per_sec, n.tx_bytes_per_sec) {
            (Some(rx), Some(tx)) => format!(", down {:.0} KB/s, up {:.0} KB/s", rx / 1024.0, tx / 1024.0),
            _ => String::new(),
        }
    ));
    lines.push(match &s.battery {
        Some(b) => format!("Battery: {:.0}% ({})", b.percent, b.state),
        None => "Battery: none detected".into(),
    });
    if s.temperatures.is_empty() {
        lines.push("Temperatures: no sensors exposed".into());
    } else {
        let temps: Vec<String> = s.temperatures.iter().take(6).map(|t| format!("{} {:.0}°C", t.label, t.celsius)).collect();
        lines.push(format!("Temperatures: {}", temps.join(", ")));
    }
    lines.push("GPU: not supported yet".into());
    lines.join("\n")
}

#[async_trait::async_trait]
impl Tool for SystemInfoTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn describe(&self, _input: &Value) -> String {
        "Read system status".into()
    }

    async fn execute(&self, _input: &Value) -> ToolResultT {
        let snapshot = {
            let mut m = self.monitor.lock().map_err(|_| ToolError::failed("System monitor is unavailable."))?;
            m.snapshot(self.connectivity.current())
        };
        let cpu = snapshot.cpu.usage_percent.map(|u| format!("CPU {u:.0}%")).unwrap_or_else(|| "CPU —".into());
        let mem = if snapshot.memory.total_bytes > 0 {
            format!("RAM {:.0}%", snapshot.memory.used_bytes as f64 / snapshot.memory.total_bytes as f64 * 100.0)
        } else {
            "RAM —".into()
        };
        Ok(ToolOutput { content: render(&snapshot), summary: format!("{cpu}, {mem}"), sources: vec![], media: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_real_values() {
        let tool = SystemInfoTool::new(Arc::new(Mutex::new(SystemMonitor::new())), ConnectivityMonitor::fixed(Connectivity::Online));
        let out = tool.execute(&json!({})).await.unwrap();
        assert!(out.content.contains("Memory:"));
        assert!(out.content.contains("internet reachable"));
        assert!(out.content.contains("GPU: not supported yet"));
        assert!(out.summary.starts_with("CPU"));
    }
}
