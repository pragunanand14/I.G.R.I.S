//! What a device can do, as a typed list each device publishes about itself.
//!
//! Capabilities are derived from the tools that are actually registered on the
//! device (never claimed in advance), so a phone never advertises desktop
//! control and a missing capability is reported honestly to the user.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Answer questions and run tasks with its own tools (every IGRIS device).
    Tasks,
    /// Remembers things; can sync memories.
    Memory,
    /// Reminders, timers, to-dos, calendar.
    Productivity,
    /// Read and write files in the user's folders.
    Files,
    /// Open and close desktop applications.
    DesktopApps,
    /// Run allowlisted terminal commands.
    Terminal,
    /// Drive IGRIS's own browser.
    Browser,
    /// Operate the computer (mouse, keyboard, windows) in operator mode.
    ComputerControl,
    /// Take screenshots of the screen (only when asked, never synced).
    Screenshots,
    /// List and open apps installed on a phone.
    PhoneApps,
    /// Battery and connection status of a phone.
    PhoneStatus,
    /// Web search and reading pages.
    Web,
}

impl Capability {
    pub const ALL: [Capability; 12] = [
        Capability::Tasks,
        Capability::Memory,
        Capability::Productivity,
        Capability::Files,
        Capability::DesktopApps,
        Capability::Terminal,
        Capability::Browser,
        Capability::ComputerControl,
        Capability::Screenshots,
        Capability::PhoneApps,
        Capability::PhoneStatus,
        Capability::Web,
    ];

    /// Plain-language label for the UI and the model.
    pub fn describe(self) -> &'static str {
        match self {
            Capability::Tasks => "run tasks",
            Capability::Memory => "memory",
            Capability::Productivity => "reminders, timers, to-dos and calendar",
            Capability::Files => "files and folders",
            Capability::DesktopApps => "open and close desktop apps",
            Capability::Terminal => "run allowlisted terminal commands",
            Capability::Browser => "browse websites",
            Capability::ComputerControl => "operate the computer (mouse and keyboard)",
            Capability::Screenshots => "screenshots",
            Capability::PhoneApps => "open apps on the phone",
            Capability::PhoneStatus => "phone battery and connection",
            Capability::Web => "web search",
        }
    }

    /// The tool whose presence proves this capability (any of them).
    fn evidence(self) -> &'static [&'static str] {
        match self {
            Capability::Tasks => &[],
            Capability::Memory => &["remember", "search_memory"],
            Capability::Productivity => &["set_reminder", "start_timer", "add_task"],
            Capability::Files => &["read_file", "list_directory"],
            Capability::DesktopApps => &["launch_application"],
            Capability::Terminal => &["run_command"],
            Capability::Browser => &["browser_open"],
            Capability::ComputerControl => &["computer_observe", "operator_start"],
            Capability::Screenshots => &["take_screenshot"],
            Capability::PhoneApps => &["open_phone_app", "list_phone_apps"],
            Capability::PhoneStatus => &["device_status"],
            Capability::Web => &["web_search", "fetch_url"],
        }
    }

    /// Capabilities proven by a set of registered tool names.
    pub fn from_tools<'a>(tools: impl IntoIterator<Item = &'a str> + Clone) -> Vec<Capability> {
        let mut caps: Vec<Capability> = Capability::ALL
            .into_iter()
            .filter(|c| c.evidence().is_empty() || c.evidence().iter().any(|e| tools.clone().into_iter().any(|t| t == *e)))
            .collect();
        caps.sort();
        caps
    }
}

/// Parse a stored list, dropping names this version doesn't know.
pub fn parse_list(json: &str) -> Vec<Capability> {
    serde_json::from_str::<Vec<serde_json::Value>>(json).unwrap_or_default().into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_come_from_real_tools() {
        let phone = Capability::from_tools(["open_phone_app", "device_status", "remember", "web_search"]);
        assert!(phone.contains(&Capability::PhoneApps) && phone.contains(&Capability::Tasks));
        assert!(!phone.contains(&Capability::ComputerControl) && !phone.contains(&Capability::Files));
        let pc = Capability::from_tools(["launch_application", "read_file", "computer_observe"]);
        assert!(pc.contains(&Capability::DesktopApps) && pc.contains(&Capability::ComputerControl));
        assert!(!pc.contains(&Capability::PhoneApps));
    }

    #[test]
    fn unknown_names_from_newer_versions_are_ignored() {
        assert_eq!(parse_list(r#"["tasks","teleport","files"]"#), vec![Capability::Tasks, Capability::Files]);
        assert!(parse_list("garbage").is_empty());
    }
}
