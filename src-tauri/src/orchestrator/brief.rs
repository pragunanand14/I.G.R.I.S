//! The task state the model sees.
//!
//! Task state lives with the task, not in the conversation. Each round, a
//! short summary of it — plan, recent actions and their verification,
//! failures, and anything that changed (a pause, a restart) — is added to the
//! *request copy* only. It is never stored in the transcript.

use super::recovery::MAX_TASK_FAILURES;
use super::task::{StepStatus, Task, Verification};
use crate::ai::ChatTurn;

const RECENT_ACTIONS: usize = 4;

pub fn render(task: &Task, notes: &[String]) -> String {
    let mut out = format!(
        "<task_state source=\"IGRIS\">\nTask: {}\nState: {} · actions taken: {} · failed actions: {}/{}\n",
        task.objective,
        task.state.as_str(),
        task.steps,
        task.failures,
        MAX_TASK_FAILURES
    );
    if let Some(p) = &task.project {
        out.push_str(&format!("Project: {}\n", p.name));
    }
    if !task.plan.is_empty() {
        out.push_str("Plan:\n");
        for (i, s) in task.plan.iter().enumerate() {
            let mark = match s.status {
                StepStatus::Pending => "pending",
                StepStatus::Active => "in progress",
                StepStatus::Completed => "done",
                StepStatus::Failed => "failed",
                StepStatus::Skipped => "skipped",
            };
            out.push_str(&format!("{}. [{mark}] {}\n", i + 1, s.title));
        }
    }
    let recent: Vec<_> = task.context.actions.iter().rev().take(RECENT_ACTIONS).collect();
    if !recent.is_empty() {
        out.push_str("Recent actions (newest first):\n");
        for a in recent {
            let v = match (a.ok, a.verification) {
                (false, _) => "failed",
                (true, Verification::Passed) => "verified",
                (true, Verification::Failed) => "check failed",
                (true, Verification::Unverified) => "not verifiable",
                (true, Verification::Operator) => "done",
                (true, Verification::NotApplicable) => "done",
            };
            out.push_str(&format!("- {} {} — {v}{}\n", a.tool, a.target, if a.note.is_empty() { String::new() } else { format!(": {}", a.note) }));
        }
    }
    for n in notes {
        out.push_str(&format!("Note: {n}\n"));
    }
    out.push_str("The plan is a guide, not a fact: check the actual state and adapt it with task_plan when things differ.\n</task_state>");
    out
}

/// Add the brief to the newest turn of the request (after the latest tool
/// results, or after the user's message). Works with every adapter.
pub fn inject(turns: &mut [ChatTurn], brief: &str) {
    let Some(last) = turns.last_mut() else { return };
    if let Some(r) = last.tool_results.last_mut() {
        r.content.push_str("\n\n");
        r.content.push_str(brief);
    } else {
        last.text.push_str("\n\n");
        last.text.push_str(brief);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::ToolResult;
    use crate::orchestrator::task::{ActionRecord, PlanStep};

    #[test]
    fn brief_shows_plan_progress_evidence_and_notes() {
        let mut t = Task::new(Some("c"), "Create notes.txt and verify it");
        t.set_plan(vec![PlanStep { title: "Create the file".into(), status: StepStatus::Completed }, PlanStep::pending("Verify")]);
        t.steps = 1;
        t.context.record(ActionRecord {
            tool: "write_file".into(),
            target: "notes.txt".into(),
            ok: true,
            verification: Verification::Passed,
            note: "Confirmed".into(),
        });
        let b = render(&t, &["Resumed after a pause: check the current state first.".into()]);
        assert!(b.contains("1. [done] Create the file") && b.contains("2. [pending] Verify"));
        assert!(b.contains("write_file notes.txt — verified: Confirmed"));
        assert!(b.contains("Note: Resumed after a pause"));
        assert!(b.contains("failed actions: 0/5"));
    }

    #[test]
    fn brief_goes_into_the_request_copy_tail() {
        let mut turns = vec![ChatTurn::user("Do it")];
        inject(&mut turns, "<task_state/>");
        assert!(turns[0].text.ends_with("<task_state/>"));
        let mut turns = vec![
            ChatTurn::user("Do it"),
            ChatTurn { tool_results: vec![ToolResult { call_id: "1".into(), content: "ok".into(), is_error: false, media: vec![] }], ..ChatTurn::user("") },
        ];
        inject(&mut turns, "<task_state/>");
        assert_eq!(turns[1].tool_results[0].content, "ok\n\n<task_state/>");
        assert_eq!(turns[0].text, "Do it", "earlier turns untouched");
    }
}
