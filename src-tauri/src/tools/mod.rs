//! Desktop tools on top of the core tool framework: the core's registry,
//! executor, permissions and platform-neutral tools, plus the tools that need
//! this desktop (applications, processes, terminal, files, screenshots,
//! system info, computer and browser control), the Android phone tools, and
//! the app's registry for each platform.

pub use igris_core::tools::*;

pub mod apps;
pub mod browser;
pub mod computer;
pub mod files;
pub mod phone;
pub mod processes;
pub mod screen;
pub mod standard;
pub mod system_info;
pub mod terminal;

#[cfg(test)]
mod tests {
    use crate::orchestrator::toolset::{capability, tool_names_in};

    #[test]
    fn every_platform_tool_has_a_capability_group() {
        let sources = [
            include_str!("apps.rs"),
            include_str!("browser.rs"),
            include_str!("computer.rs"),
            include_str!("files.rs"),
            include_str!("phone.rs"),
            include_str!("processes.rs"),
            include_str!("system_info.rs"),
            include_str!("terminal.rs"),
        ];
        let names: Vec<String> = sources.iter().flat_map(|s| tool_names_in(s)).collect();
        for n in &names {
            assert!(capability(n).is_some(), "tool {n} has no capability group");
        }
        assert!(names.len() >= 36, "found only {} tools", names.len());
    }
}
