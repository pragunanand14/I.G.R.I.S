//! Real system telemetry.
//!
//! Every value comes from OS APIs (via `sysinfo` / `starship-battery`).
//! When a value is unavailable on the current platform it is reported as
//! `null` / absent — never estimated or fabricated.

mod battery;
pub mod connectivity;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sysinfo::{Components, Disks, Networks, System};

pub use battery::BatteryInfo;
pub use connectivity::{Connectivity, ConnectivityMonitor};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSnapshot {
    pub timestamp_ms: u64,
    pub host: HostInfo,
    pub uptime_secs: u64,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub disks: Vec<DiskInfo>,
    pub network: NetworkInfo,
    /// `None` when no battery is present (e.g. desktops) or it can't be read.
    pub battery: Option<BatteryInfo>,
    /// Empty when the platform exposes no temperature sensors.
    pub temperatures: Vec<TemperatureInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostInfo {
    pub hostname: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub kernel_version: Option<String>,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    /// `None` until two samples have been taken (usage is a delta).
    pub usage_percent: Option<f32>,
    pub brand: String,
    pub logical_cores: usize,
    pub physical_cores: Option<usize>,
    pub frequency_mhz: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub kind: String,
    pub removable: bool,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfo {
    pub connectivity: Connectivity,
    /// Aggregate throughput across non-loopback interfaces. `None` on the first sample.
    pub rx_bytes_per_sec: Option<f64>,
    pub tx_bytes_per_sec: Option<f64>,
    pub interface_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemperatureInfo {
    pub label: String,
    pub celsius: f32,
    pub critical_celsius: Option<f32>,
}

/// Holds sysinfo state between samples (CPU and network figures are deltas).
pub struct SystemMonitor {
    sys: System,
    networks: Networks,
    disks: Disks,
    components: Components,
    last_cpu_refresh: Option<Instant>,
    cpu_usage: Option<f32>,
    last_net_refresh: Instant,
    host: HostInfo,
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemMonitor {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_all();
        sys.refresh_memory();
        Self {
            sys,
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            components: Components::new_with_refreshed_list(),
            last_cpu_refresh: Some(Instant::now()),
            cpu_usage: None,
            last_net_refresh: Instant::now(),
            host: HostInfo {
                hostname: System::host_name(),
                os_name: System::name(),
                os_version: System::os_version(),
                kernel_version: System::kernel_version(),
                arch: System::cpu_arch(),
            },
        }
    }

    pub fn snapshot(&mut self, connectivity: Connectivity) -> SystemSnapshot {
        let now = Instant::now();

        // CPU usage is computed between refreshes; sysinfo needs a minimum gap.
        let cpu_ready = self
            .last_cpu_refresh
            .map(|t| now.duration_since(t) >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL)
            .unwrap_or(false);
        if cpu_ready {
            self.sys.refresh_cpu_all();
            self.cpu_usage = Some(self.sys.global_cpu_usage().clamp(0.0, 100.0));
            self.last_cpu_refresh = Some(now);
        }

        self.sys.refresh_memory();
        self.disks.refresh(true);
        self.components.refresh(true);

        let elapsed = now.duration_since(self.last_net_refresh);
        self.networks.refresh(true);
        self.last_net_refresh = now;
        let (rx, tx, count) = self
            .networks
            .list()
            .iter()
            .filter(|(name, _)| !is_loopback(name))
            .fold((0u64, 0u64, 0usize), |(rx, tx, n), (_, d)| (rx + d.received(), tx + d.transmitted(), n + 1));
        let rate = |bytes: u64| (elapsed >= Duration::from_millis(200)).then(|| bytes as f64 / elapsed.as_secs_f64());

        let cpus = self.sys.cpus();
        SystemSnapshot {
            timestamp_ms: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
            host: self.host.clone(),
            uptime_secs: System::uptime(),
            cpu: CpuInfo {
                usage_percent: self.cpu_usage,
                brand: cpus.first().map(|c| c.brand().trim().to_string()).unwrap_or_default(),
                logical_cores: cpus.len(),
                physical_cores: System::physical_core_count(),
                frequency_mhz: cpus.first().map(|c| c.frequency()).filter(|f| *f > 0),
            },
            memory: MemoryInfo {
                total_bytes: self.sys.total_memory(),
                used_bytes: self.sys.used_memory(),
                available_bytes: self.sys.available_memory(),
                swap_total_bytes: self.sys.total_swap(),
                swap_used_bytes: self.sys.used_swap(),
            },
            disks: self
                .disks
                .list()
                .iter()
                .filter(|d| d.total_space() > 0)
                .map(|d| DiskInfo {
                    name: d.name().to_string_lossy().into_owned(),
                    mount_point: d.mount_point().display().to_string(),
                    file_system: d.file_system().to_string_lossy().into_owned(),
                    kind: d.kind().to_string(),
                    removable: d.is_removable(),
                    total_bytes: d.total_space(),
                    available_bytes: d.available_space(),
                })
                .collect(),
            network: NetworkInfo {
                connectivity,
                rx_bytes_per_sec: rate(rx),
                tx_bytes_per_sec: rate(tx),
                interface_count: count,
            },
            battery: battery::read(),
            temperatures: self
                .components
                .list()
                .iter()
                .filter_map(|c| {
                    let t = c.temperature()?;
                    (t.is_finite() && t > 0.0).then(|| TemperatureInfo {
                        label: c.label().to_string(),
                        celsius: t,
                        critical_celsius: c.critical().filter(|v| v.is_finite() && *v > 0.0),
                    })
                })
                .collect(),
        }
    }
}

fn is_loopback(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "lo" || n.starts_with("lo0") || n.contains("loopback")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reports_real_values() {
        let mut m = SystemMonitor::new();
        let first = m.snapshot(Connectivity::Unknown);
        assert!(first.memory.total_bytes > 0, "total memory should be read from the OS");
        assert!(first.memory.used_bytes <= first.memory.total_bytes);
        assert!(first.cpu.logical_cores > 0);
        // First sample has no rate baseline yet.
        assert!(first.network.rx_bytes_per_sec.is_none());

        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(50));
        let second = m.snapshot(Connectivity::Online);
        let usage = second.cpu.usage_percent.expect("cpu usage after two samples");
        assert!((0.0..=100.0).contains(&usage));
        assert!(second.network.rx_bytes_per_sec.is_some());
        assert_eq!(second.network.connectivity, Connectivity::Online);
    }

    #[test]
    fn loopback_detection() {
        assert!(is_loopback("lo"));
        assert!(is_loopback("Loopback Pseudo-Interface 1"));
        assert!(!is_loopback("eth0"));
        assert!(!is_loopback("Wi-Fi"));
    }
}
