//! The IGRIS task: what the user asked for, the plan, the state, and the
//! evidence gathered while doing it.
//!
//! One task model for all actionable work. Operator mode (IGRIS operating the
//! computer) is a task of kind [`TaskKind::Operator`] and shares the state
//! machine below.

use serde::{Deserialize, Serialize};

/// Lifecycle of a task. Every change goes through [`TaskState::advance`];
/// illegal transitions are refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Created,
    Planning,
    /// An action is waiting for the user's approval (the existing approval flow).
    WaitingForApproval,
    Executing,
    Paused,
    /// Checking that an action had the intended effect.
    Verifying,
    /// An action failed; deciding whether to retry, try something else or stop.
    Recovering,
    Completed,
    /// Can't continue. Final, unless the user explicitly retries it.
    Failed,
    Cancelled,
    /// Control handed back without a verified result: the user has to act
    /// (log in, decide), or the work couldn't be verified.
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("illegal task transition {from:?} → {to:?}")]
pub struct IllegalTransition {
    pub from: TaskState,
    pub to: TaskState,
}

impl TaskState {
    pub const ALL: [TaskState; 11] = [
        TaskState::Created,
        TaskState::Planning,
        TaskState::WaitingForApproval,
        TaskState::Executing,
        TaskState::Paused,
        TaskState::Verifying,
        TaskState::Recovering,
        TaskState::Completed,
        TaskState::Failed,
        TaskState::Cancelled,
        TaskState::Ended,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskState::Created => "created",
            TaskState::Planning => "planning",
            TaskState::WaitingForApproval => "waiting_for_approval",
            TaskState::Executing => "executing",
            TaskState::Paused => "paused",
            TaskState::Verifying => "verifying",
            TaskState::Recovering => "recovering",
            TaskState::Completed => "completed",
            TaskState::Failed => "failed",
            TaskState::Cancelled => "cancelled",
            TaskState::Ended => "ended",
        }
    }

    pub fn parse(s: &str) -> Option<TaskState> {
        // "waiting_for_permission" was the operator's name for the same state.
        if s == "waiting_for_permission" {
            return Some(TaskState::WaitingForApproval);
        }
        TaskState::ALL.into_iter().find(|t| t.as_str() == s)
    }

    /// No further work happens in this state.
    pub fn is_final(self) -> bool {
        matches!(self, TaskState::Completed | TaskState::Failed | TaskState::Cancelled | TaskState::Ended)
    }

    /// The legal transitions.
    pub fn can_transition(self, to: TaskState) -> bool {
        use TaskState::*;
        match self {
            Created => matches!(to, Planning | Executing | WaitingForApproval | Cancelled | Failed),
            Planning => matches!(to, Executing | WaitingForApproval | Paused | Cancelled | Failed | Ended),
            WaitingForApproval => matches!(to, Executing | Paused | Failed | Cancelled),
            Executing => matches!(to, Planning | WaitingForApproval | Verifying | Paused | Recovering | Failed | Cancelled | Ended),
            Verifying => matches!(to, Executing | Completed | Recovering | Paused | Failed | Cancelled | Ended),
            Recovering => matches!(to, Executing | Planning | WaitingForApproval | Paused | Failed | Cancelled | Ended),
            Paused => matches!(to, Executing | Cancelled | Failed | Ended),
            // Only an explicit retry by the user reopens a failed task.
            Failed => matches!(to, Recovering),
            Completed | Cancelled | Ended => false,
        }
    }

    /// Move to `to` if that's a legal transition.
    pub fn advance(&mut self, to: TaskState) -> Result<(), IllegalTransition> {
        if *self == to {
            return Ok(());
        }
        if !self.can_transition(to) {
            return Err(IllegalTransition { from: *self, to });
        }
        *self = to;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// Work done with IGRIS's tools (files, apps, commands…).
    General,
    /// IGRIS operating the computer (operator mode).
    Operator,
}

impl TaskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskKind::General => "general",
            TaskKind::Operator => "operator",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Active,
    Completed,
    Failed,
    Skipped,
}

/// One step of a plan. Plans are working hypotheses: the model revises them as
/// it observes what's actually there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub title: String,
    pub status: StepStatus,
}

impl PlanStep {
    pub fn pending(title: impl Into<String>) -> Self {
        Self { title: title.into(), status: StepStatus::Pending }
    }
}

/// Plans are stored as JSON; operator tasks before this version stored plain strings.
pub fn decode_plan(raw: &str) -> Vec<PlanStep> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Step(PlanStep),
        Title(String),
    }
    serde_json::from_str::<Vec<Stored>>(raw)
        .unwrap_or_default()
        .into_iter()
        .map(|s| match s {
            Stored::Step(p) => p,
            Stored::Title(t) => PlanStep::pending(t),
        })
        .collect()
}

/// How an action's effect was checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    /// An independent check confirmed the intended effect.
    Passed,
    /// The check found the effect missing or wrong.
    Failed,
    /// No reliable check exists for this action.
    Unverified,
    /// Checked by operator mode's own observe → verify loop.
    Operator,
    /// Nothing to verify (the action failed, or it only reads).
    NotApplicable,
}

/// What happened when the task did something. Short by design: the full
/// tool output lives in the conversation, not in the task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub tool: String,
    /// What the action was about (path, app, command), for matching retries.
    pub target: String,
    pub ok: bool,
    pub verification: Verification,
    /// Result or verification note (clipped).
    pub note: String,
}

/// Execution state that lives with the task, separate from the conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskContext {
    /// Most recent actions (bounded).
    pub actions: Vec<ActionRecord>,
    /// IGRIS was closed while the task ran; resuming needs the user's go-ahead.
    pub interrupted: bool,
    /// Times the task was resumed after a pause or interruption.
    pub resumes: u32,
}

pub const MAX_RECORDED_ACTIONS: usize = 24;
pub const MAX_NOTE_CHARS: usize = 240;

impl TaskContext {
    pub fn record(&mut self, mut a: ActionRecord) {
        a.note = clip(&a.note, MAX_NOTE_CHARS);
        a.target = clip(&a.target, MAX_NOTE_CHARS);
        self.actions.push(a);
        if self.actions.len() > MAX_RECORDED_ACTIONS {
            let extra = self.actions.len() - MAX_RECORDED_ACTIONS;
            self.actions.drain(..extra);
        }
    }
}

pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProject {
    pub id: i64,
    pub name: String,
}

/// A task, as persisted and as shown to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub conversation_id: Option<String>,
    pub kind: TaskKind,
    /// The user's request, in their words.
    pub objective: String,
    pub state: TaskState,
    pub plan: Vec<PlanStep>,
    /// Index into `plan` of the step being worked on.
    pub current_step: Option<usize>,
    /// What IGRIS is doing right now (UI), e.g. "Write notes.txt".
    pub activity: Option<String>,
    /// Actions taken.
    pub steps: u32,
    /// Failed actions (the failure budget).
    pub failures: u32,
    pub result: Option<String>,
    pub error: Option<String>,
    pub pause_reason: Option<String>,
    pub context: TaskContext,
    pub project: Option<TaskProject>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

impl Task {
    pub fn new(conversation_id: Option<&str>, objective: &str) -> Self {
        let now = now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id: conversation_id.map(str::to_string),
            kind: TaskKind::General,
            objective: clip(objective.trim(), 300),
            state: TaskState::Created,
            plan: Vec::new(),
            current_step: None,
            activity: None,
            steps: 0,
            failures: 0,
            result: None,
            error: None,
            pause_reason: None,
            context: TaskContext::default(),
            project: None,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Replace the plan (the model revises it as it learns more).
    pub fn set_plan(&mut self, plan: Vec<PlanStep>) {
        self.plan = plan;
        self.current_step =
            self.plan.iter().position(|s| s.status == StepStatus::Active).or_else(|| self.plan.iter().position(|s| s.status == StepStatus::Pending));
    }

    /// Steps not yet done or deliberately skipped.
    pub fn open_steps(&self) -> Vec<&PlanStep> {
        self.plan.iter().filter(|s| matches!(s.status, StepStatus::Pending | StepStatus::Active)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TaskState::*;

    #[test]
    fn the_documented_paths_are_legal() {
        let paths: &[&[TaskState]] = &[
            &[Created, Planning, Executing, Verifying, Completed],
            &[Created, Executing, Recovering, Executing, Verifying, Completed],
            &[Created, Executing, Failed, Recovering, Executing],
            &[Created, Executing, Paused, Executing, Cancelled],
            &[Created, Executing, WaitingForApproval, Executing, Verifying, Executing, Ended],
            &[Created, Executing, WaitingForApproval, Failed],
            &[Created, Planning, WaitingForApproval, Cancelled],
            &[Created, Executing, Verifying, Recovering, Failed],
            &[Created, Executing, Paused, Ended],
        ];
        for path in paths {
            let mut s = path[0];
            for &next in &path[1..] {
                s.advance(next).unwrap_or_else(|e| panic!("{e} in {path:?}"));
            }
        }
    }

    #[test]
    fn illegal_transitions_are_refused() {
        let illegal = [
            (Created, Completed),
            (Planning, Completed),
            (Executing, Completed), // completion needs verification first
            (Paused, Completed),
            (Paused, Verifying),
            (WaitingForApproval, Completed),
            (Completed, Executing),
            (Cancelled, Executing),
            (Ended, Executing),
            (Failed, Executing),
            (Failed, Completed),
            (Recovering, Completed),
        ];
        for (from, to) in illegal {
            let mut s = from;
            assert_eq!(s.advance(to), Err(IllegalTransition { from, to }), "{from:?} → {to:?}");
            assert_eq!(s, from, "state unchanged after a refused transition");
        }
        // Final states go nowhere (Failed only to an explicit retry).
        for from in [Completed, Cancelled, Ended] {
            for to in TaskState::ALL {
                assert!(from == to || !from.can_transition(to), "{from:?} → {to:?}");
            }
        }
        assert_eq!(TaskState::ALL.iter().filter(|t| Failed.can_transition(**t)).collect::<Vec<_>>(), vec![&Recovering]);
    }

    #[test]
    fn states_round_trip_and_legacy_names_parse() {
        for s in TaskState::ALL {
            assert_eq!(TaskState::parse(s.as_str()), Some(s));
            assert_eq!(serde_json::to_value(s).unwrap(), s.as_str());
        }
        assert_eq!(TaskState::parse("waiting_for_permission"), Some(WaitingForApproval));
        assert_eq!(TaskState::parse("bogus"), None);
    }

    #[test]
    fn plans_decode_old_and_new_formats() {
        assert_eq!(decode_plan(r#"["Open Notepad","Type"]"#), vec![PlanStep::pending("Open Notepad"), PlanStep::pending("Type")]);
        let steps = vec![PlanStep { title: "a".into(), status: StepStatus::Completed }, PlanStep::pending("b")];
        assert_eq!(decode_plan(&serde_json::to_string(&steps).unwrap()), steps);
        assert!(decode_plan("not json").is_empty());
    }

    #[test]
    fn current_step_follows_the_plan() {
        let mut t = Task::new(Some("c"), "  Build it  ");
        assert_eq!(t.objective, "Build it");
        t.set_plan(vec![PlanStep { title: "a".into(), status: StepStatus::Completed }, PlanStep::pending("b"), PlanStep::pending("c")]);
        assert_eq!(t.current_step, Some(1));
        t.set_plan(vec![PlanStep::pending("a"), PlanStep { title: "b".into(), status: StepStatus::Active }]);
        assert_eq!(t.current_step, Some(1));
        assert_eq!(t.open_steps().len(), 2);
    }

    #[test]
    fn recorded_actions_are_bounded_and_clipped() {
        let mut c = TaskContext::default();
        for i in 0..MAX_RECORDED_ACTIONS + 5 {
            c.record(ActionRecord { tool: "t".into(), target: i.to_string(), ok: true, verification: Verification::Passed, note: "x".repeat(1000) });
        }
        assert_eq!(c.actions.len(), MAX_RECORDED_ACTIONS);
        assert_eq!(c.actions[0].target, "5");
        assert!(c.actions[0].note.chars().count() <= MAX_NOTE_CHARS + 1);
    }
}
