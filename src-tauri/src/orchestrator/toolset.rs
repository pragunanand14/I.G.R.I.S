//! Which tools a request carries.
//!
//! Every tool belongs to a capability group. Groups decide two things:
//! whether calling the tool is an *action* on the user's computer (which
//! makes the turn a task, see `intent`), and whether the tool is *exposed*
//! to the model right now. Today only operator-session tools are held back
//! until operator mode is running (they can't do anything before that);
//! the groups are where more focused tool sets can be built later.
//!
//! A conversation's tool list is a snapshot taken when it was created. It is
//! re-evaluated against the registry and configuration when a task starts or
//! resumes ([`refreshed`]), so newly available tools appear and removed ones
//! disappear without starting a new conversation.

use crate::ai::ToolDef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Calculation, date and time.
    Reasoning,
    /// Read-only facts about the computer (system info, processes, app list).
    Information,
    Web,
    Memory,
    /// Tasks, reminders, timers and the local calendar (IGRIS's own data).
    Productivity,
    Projects,
    FilesRead,
    /// Creating, changing, moving or deleting files.
    FileChanges,
    /// Opening or closing applications, documents and websites.
    Apps,
    /// Developer commands.
    Terminal,
    /// Screenshots on request.
    Screen,
    /// Asking to start operator mode.
    Operator,
    /// Operator-mode actions; usable only while an operator task runs.
    OperatorSession,
    /// Declaring and revising a task plan.
    Tasks,
}

impl Capability {
    /// Using a tool of this group changes something on the user's computer.
    pub fn acts(self) -> bool {
        matches!(self, Capability::FileChanges | Capability::Apps | Capability::Terminal | Capability::Operator)
    }
}

/// The capability group of a tool. `None` for tools this table doesn't know
/// (treated as always exposed and not an action; a test keeps the table complete).
pub fn capability(name: &str) -> Option<Capability> {
    use Capability::*;
    Some(match name {
        "calculator" | "get_datetime" => Reasoning,
        "system_info" | "list_processes" | "list_applications" => Information,
        "web_search" | "fetch_url" => Web,
        "remember" | "search_memory" | "update_memory" | "forget_memory" => Memory,
        "add_task" | "list_tasks" | "update_task" | "delete_task" | "set_reminder" | "start_timer" | "list_reminders" | "cancel_reminder" | "add_event"
        | "list_events" | "delete_event" => Productivity,
        "list_projects" | "get_project_context" => Projects,
        "list_directory" | "search_files" | "read_file" => FilesRead,
        "create_file" | "create_folder" | "write_file" | "move_path" | "copy_path" | "trash_path" => FileChanges,
        "launch_application" | "close_application" | "open_url" | "open_path" => Apps,
        "run_command" => Terminal,
        "take_screenshot" => Screen,
        "operator_start" => Operator,
        "operator_update" | "operator_finish" => OperatorSession,
        n if n.starts_with("computer_") => OperatorSession,
        "task_plan" => Tasks,
        _ => return None,
    })
}

/// The tools of one generation: everything the conversation offers (the
/// executor's allow-list) and the subset the model sees this round.
#[derive(Debug, Clone)]
pub struct Toolset {
    offered: Vec<ToolDef>,
    operator_session: bool,
    exposed: Vec<ToolDef>,
}

impl Toolset {
    pub fn new(offered: Vec<ToolDef>) -> Self {
        let mut t = Self { offered: dedup(offered), operator_session: false, exposed: Vec::new() };
        t.rebuild();
        t
    }

    fn rebuild(&mut self) {
        let session = self.operator_session;
        self.exposed = self.offered.iter().filter(|d| session || capability(&d.name) != Some(Capability::OperatorSession)).cloned().collect();
    }

    pub fn offered(&self) -> &[ToolDef] {
        &self.offered
    }

    /// Names the executor accepts. Operator-session tools stay callable (they
    /// report that operator mode isn't running) so a call made in the same
    /// response as `operator_start` still works.
    pub fn allowed(&self) -> Vec<String> {
        self.offered.iter().map(|d| d.name.clone()).collect()
    }

    /// Definitions sent to the model this round.
    pub fn exposed(&self) -> &[ToolDef] {
        &self.exposed
    }

    /// Show or hide operator-session tools. Returns whether anything changed.
    pub fn set_operator_session(&mut self, active: bool) -> bool {
        if self.operator_session == active {
            return false;
        }
        self.operator_session = active;
        self.rebuild();
        true
    }

    /// Replace the offered tools (after [`refreshed`]).
    pub fn replace(&mut self, offered: Vec<ToolDef>) {
        self.offered = dedup(offered);
        self.rebuild();
    }
}

fn dedup(defs: Vec<ToolDef>) -> Vec<ToolDef> {
    let mut seen = std::collections::HashSet::new();
    defs.into_iter().filter(|d| seen.insert(d.name.clone())).collect()
}

/// A conversation's tools re-evaluated against what's available now.
/// `None` when nothing changed — and for conversations that never had tools
/// (created before tools existed; their prompt doesn't describe any).
pub fn refreshed(stored: &[ToolDef], current: Vec<ToolDef>) -> Option<Vec<ToolDef>> {
    if stored.is_empty() {
        return None;
    }
    let current = dedup(current);
    (current != stored).then_some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn def(name: &str) -> ToolDef {
        ToolDef { name: name.into(), description: format!("{name} tool"), input_schema: json!({"type": "object"}), server: None }
    }

    fn names(defs: &[ToolDef]) -> Vec<&str> {
        defs.iter().map(|d| d.name.as_str()).collect()
    }

    #[test]
    fn operator_session_tools_appear_only_while_operating() {
        let mut t = Toolset::new(vec![def("calculator"), def("operator_start"), def("computer_click"), def("operator_finish"), def("write_file")]);
        assert_eq!(names(t.exposed()), vec!["calculator", "operator_start", "write_file"]);
        assert_eq!(t.allowed().len(), 5, "still callable, so a same-response call reports properly");
        assert!(t.set_operator_session(true));
        assert_eq!(names(t.exposed()), vec!["calculator", "operator_start", "computer_click", "operator_finish", "write_file"]);
        assert!(!t.set_operator_session(true), "unchanged lists aren't rebuilt");
        assert!(t.set_operator_session(false));
        assert_eq!(t.exposed().len(), 3);
    }

    #[test]
    fn refresh_adds_new_tools_removes_gone_ones_and_never_duplicates() {
        let stored = vec![def("calculator"), def("web_search")];
        // Unchanged → no refresh.
        assert!(refreshed(&stored, stored.clone()).is_none());
        // A newly available tool (e.g. task_plan after an update) and a removed one (web search key deleted).
        let now = refreshed(&stored, vec![def("calculator"), def("task_plan"), def("task_plan")]).unwrap();
        assert_eq!(names(&now), vec!["calculator", "task_plan"]);
        // Conversations created without tools stay without tools.
        assert!(refreshed(&[], vec![def("calculator")]).is_none());
        let mut t = Toolset::new(vec![def("a"), def("a")]);
        assert_eq!(t.allowed(), vec!["a"]);
        t.replace(now);
        assert_eq!(t.allowed(), vec!["calculator", "task_plan"]);
    }

    #[test]
    fn every_registered_tool_has_a_capability_group() {
        let sources = [
            include_str!("../tools/apps.rs"),
            include_str!("../tools/calculator.rs"),
            include_str!("../tools/computer.rs"),
            include_str!("../tools/files.rs"),
            include_str!("../tools/memory.rs"),
            include_str!("../tools/processes.rs"),
            include_str!("../tools/productivity.rs"),
            include_str!("../tools/projects.rs"),
            include_str!("../tools/screen.rs"),
            include_str!("../tools/system_info.rs"),
            include_str!("../tools/task.rs"),
            include_str!("../tools/terminal.rs"),
            include_str!("../tools/web.rs"),
        ];
        let re = regex::Regex::new(r#"(?:name:\s*|spec\(\s*)"([a-z_]+)""#).unwrap();
        let mut found = 0;
        for src in sources {
            let code = src.split("#[cfg(test)]").next().unwrap();
            for c in re.captures_iter(code) {
                found += 1;
                assert!(capability(&c[1]).is_some(), "tool {} has no capability group", &c[1]);
            }
        }
        assert!(found >= 49, "found only {found} tools");
    }

    #[test]
    fn actions_are_the_tools_that_change_the_computer() {
        for name in ["write_file", "trash_path", "launch_application", "open_url", "run_command", "operator_start"] {
            assert!(capability(name).unwrap().acts(), "{name}");
        }
        for name in ["calculator", "read_file", "web_search", "remember", "add_task", "take_screenshot", "task_plan", "computer_click"] {
            assert!(!capability(name).unwrap().acts(), "{name}");
        }
    }
}
