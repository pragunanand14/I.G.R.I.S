//! Chat or task?
//!
//! Every turn starts as a plain conversational turn: no task, no extra model
//! call, no planning overhead ("What's 12 × 7?" stays a calculator call).
//! It becomes a task at the first point where it is actually one:
//! * the model declares a plan (`task_plan`), or
//! * the model asks for an action that changes something on the computer
//!   (files, apps, commands, operator mode — see `toolset::Capability::acts`).
//!
//! Resuming an interrupted or paused task is a task turn from the start.

use super::toolset::{capability, Capability};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// The model declared a plan.
    Plan,
    /// The model asked for an action of this kind.
    Action(Capability),
}

/// Whether calling `tool` turns the current turn into a task.
pub fn trigger(tool: &str) -> Option<Trigger> {
    match capability(tool)? {
        Capability::Tasks => Some(Trigger::Plan),
        c if c.acts() => Some(Trigger::Action(c)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_stays_chat_and_actions_become_tasks() {
        // Likely chat: answers, lookups, IGRIS's own records.
        for tool in [
            "calculator",
            "get_datetime",
            "web_search",
            "fetch_url",
            "read_file",
            "list_directory",
            "remember",
            "add_task",
            "set_reminder",
            "system_info",
            "take_screenshot",
            "unknown_tool",
        ] {
            assert_eq!(trigger(tool), None, "{tool}");
        }
        // Likely tasks: "Open VS Code", "Create a project", "Fix the failing tests", "Write it to a document".
        assert_eq!(trigger("launch_application"), Some(Trigger::Action(Capability::Apps)));
        assert_eq!(trigger("create_folder"), Some(Trigger::Action(Capability::FileChanges)));
        assert_eq!(trigger("run_command"), Some(Trigger::Action(Capability::Terminal)));
        assert_eq!(trigger("write_file"), Some(Trigger::Action(Capability::FileChanges)));
        assert_eq!(trigger("operator_start"), Some(Trigger::Action(Capability::Operator)));
        assert_eq!(trigger("task_plan"), Some(Trigger::Plan));
    }
}
