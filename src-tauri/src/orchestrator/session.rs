//! A task inside one agent-loop run: the lifecycle around each tool call.
//!
//! before the call → state Executing, event ToolRequested
//! during          → WaitingForApproval while the existing approval flow asks
//! after           → action: Verifying → verdict → Executing / Recovering
//!                   read-only call: an observation
//!                   failure: classify → retry (safe reads) / adjust / stop
//! end of the run  → Completed only when the evidence supports it.

use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use super::recovery::{self, Decision, Failure, RepeatGuard};
use super::task::{clip, ActionRecord, Task, TaskKind, TaskState, Verification};
use super::toolset::{capability, Capability};
use super::verify::{self, Check, Verdict};
use super::{intent::Trigger, Orchestrator, TaskEvent};
use crate::ai::{ToolCall, ToolResult};
use crate::error::AppResult;
use crate::tools::executor::{self, ActivityStatus, ExecContext, ToolActivity};
use std::sync::Arc;

/// Wall-clock limit for one run of a task.
pub const TASK_TIME_LIMIT: Duration = Duration::from_secs(30 * 60);

/// How a run ended, as far as the task is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnd {
    /// The model finished its reply.
    Answered,
    Cancelled,
    /// The AI request failed.
    Error(String),
    /// The round limit was reached.
    StepLimit(usize),
}

/// What the loop should do after a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Continue,
    /// Run the same call once more (read-only, transient failure).
    RetryNow,
}

pub struct TaskSession {
    hub: Arc<Orchestrator>,
    pub id: String,
    conversation_id: String,
    guard: RepeatGuard,
    /// One-shot notes for the next brief (a pause, a restart, a re-check).
    notes: Vec<String>,
    started: Instant,
}

/// Key for matching attempts at the same thing (a path, an app, a command).
pub fn target_of(call: &ToolCall) -> String {
    let i = &call.input;
    let s = |k: &str| i[k].as_str().map(str::to_string);
    if call.name == "run_command" {
        let args: Vec<&str> = i["args"].as_array().into_iter().flatten().filter_map(|a| a.as_str()).collect();
        return format!("{} {} (in {})", s("program").unwrap_or_default(), args.join(" "), s("cwd").unwrap_or_default());
    }
    s("to").or_else(|| s("path")).or_else(|| s("name")).or_else(|| s("url")).or_else(|| s("objective")).unwrap_or_else(|| call.name.clone())
}

impl TaskSession {
    /// Turn the running conversation turn into a task.
    pub fn begin(hub: &Arc<Orchestrator>, conversation_id: &str, objective: &str, trigger: Trigger, cancel: &CancellationToken) -> AppResult<Self> {
        let t = hub.begin(conversation_id, objective, cancel)?;
        let to = if trigger == Trigger::Plan { TaskState::Planning } else { TaskState::Executing };
        let _ = hub.transition(&t.id, to, |_| {});
        Ok(Self::new(hub, t.id, conversation_id))
    }

    /// Continue a stored task (paused by a restart, or failed and retried).
    pub fn resume(hub: &Arc<Orchestrator>, task_id: &str, conversation_id: &str, cancel: &CancellationToken) -> AppResult<Self> {
        let t = hub.attach(task_id, conversation_id, cancel)?;
        let mut s = Self::new(hub, t.id.clone(), conversation_id);
        if t.kind == TaskKind::Operator {
            s.notes.push(
                "This task operated the computer, and operator mode is off now. To continue on screen, start operator mode again (the user will be asked)."
                    .into(),
            );
        }
        Ok(s)
    }

    fn new(hub: &Arc<Orchestrator>, id: String, conversation_id: &str) -> Self {
        Self { hub: hub.clone(), id, conversation_id: conversation_id.to_string(), guard: RepeatGuard::default(), notes: Vec::new(), started: Instant::now() }
    }

    pub fn task(&self) -> Option<Task> {
        self.hub.task(&self.id)
    }

    fn state(&self) -> TaskState {
        self.task().map(|t| t.state).unwrap_or(TaskState::Cancelled)
    }

    /// Wait out a pause; after a resume, re-observe. `Ok(true)` when the task
    /// was resumed (the caller must not act on decisions made before the pause).
    pub async fn checkpoint(&mut self, ctx: &ExecContext<'_>) -> Result<bool, String> {
        let resumed = self.hub.checkpoint(&self.id).await?;
        if resumed {
            let observed = self.reobserve(ctx).await;
            let mut note = "The task was paused and has been resumed (or IGRIS was restarted). Things may have changed in the meantime: check the current state before your next action".to_string();
            if observed.is_empty() {
                note.push('.');
            } else {
                note.push_str(&format!(". IGRIS re-checked earlier work: {}", observed.join(" ")));
            }
            self.notes.push(note);
        }
        Ok(resumed)
    }

    /// Re-run the checks of the last few verified actions (read-only).
    async fn reobserve(&self, ctx: &ExecContext<'_>) -> Vec<String> {
        let Some(t) = self.task() else { return Vec::new() };
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for a in t.context.actions.iter().rev().filter(|a| a.ok && a.verification == Verification::Passed) {
            if out.len() >= 3 || !seen.insert(a.target.clone()) {
                continue;
            }
            // Only probe-based checks can be repeated; rebuild them from the record.
            let probe = match a.tool.as_str() {
                "write_file" | "create_file" | "create_folder" => Some(ToolCall {
                    id: format!("verify-{}", uuid::Uuid::new_v4()),
                    name: if a.tool == "create_folder" { "list_directory".into() } else { "read_file".into() },
                    input: serde_json::json!({ "path": a.target }),
                    invalid_input: None,
                    extras: None,
                }),
                _ => None,
            };
            if let Some(call) = probe {
                let (r, _) = executor::execute(&call, ctx, &mut |_| {}).await;
                out.push(if r.is_error {
                    format!("{} — no longer found ({}).", a.target, clip(&r.content, 120))
                } else {
                    format!("{} — still there.", a.target)
                });
            }
        }
        if !out.is_empty() {
            self.hub.update(&self.id, Some(TaskEvent::ObservationReceived { tool: "reobserve".into() }), |_| {});
        }
        out
    }

    /// The task-state brief for this round's request (one-shot notes included).
    pub fn brief(&mut self) -> Option<String> {
        let t = self.task()?;
        let notes = std::mem::take(&mut self.notes);
        Some(super::brief::render(&t, &notes))
    }

    /// A reason not to run `call` at all (the result tells the model why).
    pub fn refuse(&self, call: &ToolCall, cancel: &CancellationToken) -> Option<String> {
        if cancel.is_cancelled() {
            return Some("Cancelled by the user. Nothing more was done.".into());
        }
        let acts = capability(&call.name).is_some_and(|c| c.acts() || c == Capability::OperatorSession);
        let state = self.state();
        if state.is_final() && acts {
            let why = self.task().and_then(|t| t.error.or(t.result)).unwrap_or_default();
            return Some(format!(
                "The task has ended ({}{}), so no further actions are taken. Tell the user what was and wasn't done.",
                state.as_str(),
                if why.is_empty() { String::new() } else { format!(": {why}") }
            ));
        }
        if self.guard.blocked(call) {
            return Some(format!(
                "This exact call has already failed {} times, so it wasn't run again. Change the approach or explain to the user what's blocking you.",
                recovery::MAX_SAME_FAILURES
            ));
        }
        if acts && self.started.elapsed() > TASK_TIME_LIMIT {
            return Some("The task has been running for 30 minutes, the limit for one run. Stop and tell the user where things stand.".into());
        }
        None
    }

    /// A call is about to run.
    pub fn before_call(&self, call: &ToolCall) {
        if call.name == "task_plan" {
            return;
        }
        let state = self.state();
        if matches!(state, TaskState::Planning | TaskState::Recovering | TaskState::Verifying | TaskState::Created) {
            let _ = self.hub.transition(&self.id, TaskState::Executing, |_| {});
        }
        let step = self.task().and_then(|t| t.current_step);
        let label = clip(&target_of(call), 80);
        self.hub.update(&self.id, Some(TaskEvent::ToolRequested { tool: call.name.clone() }), |t| t.activity = Some(format!("{}: {label}", call.name)));
        if let Some(step) = step {
            tracing::debug!(event = "TASK_STEP", step);
        }
    }

    /// Progress from the executor (approval requested / granted).
    pub fn on_activity(&self, a: &ToolActivity) {
        match a.status {
            ActivityStatus::AwaitingApproval => {
                if self
                    .hub
                    .transition(&self.id, TaskState::WaitingForApproval, |t| t.activity = Some(format!("Waiting for your approval: {}", a.description)))
                    .is_ok()
                {
                    self.hub.update(&self.id, Some(TaskEvent::ApprovalRequested { tool: a.tool.clone() }), |_| {});
                }
            }
            ActivityStatus::Running if self.state() == TaskState::WaitingForApproval => {
                let _ = self.hub.transition(&self.id, TaskState::Executing, |t| t.activity = Some(a.description.clone()));
            }
            _ => {}
        }
    }

    /// The model declared or revised its plan (the task_plan tool ran).
    pub fn plan_changed(&self) {
        if self.state() == TaskState::Planning {
            let _ = self.hub.transition(&self.id, TaskState::Executing, |_| {});
        }
    }

    /// After a call: record, verify, recover. May append verification or
    /// recovery guidance to the result the model sees.
    pub async fn after_call(&mut self, call: &ToolCall, result: &mut ToolResult, activity: &ToolActivity, attempt: u32, ctx: &ExecContext<'_>) -> After {
        if call.name == "task_plan" {
            return After::Continue;
        }
        let cap = capability(&call.name);
        let acts = cap.is_some_and(|c| c.acts() || c == Capability::OperatorSession);
        let target = target_of(call);
        match recovery::classify(activity) {
            None => {
                self.guard.succeeded(call);
                if !acts {
                    self.hub.update(&self.id, Some(TaskEvent::ObservationReceived { tool: call.name.clone() }), |_| {});
                    return After::Continue;
                }
                let project = self.project_for(call);
                self.hub.update(&self.id, Some(TaskEvent::ToolCompleted { tool: call.name.clone(), ok: true }), |t| {
                    t.steps += 1;
                    if t.project.is_none() {
                        t.project = project;
                    }
                });
                let verdict = self.verify(call, result, ctx).await;
                let failed = verdict.verification == Verification::Failed;
                if matches!(verdict.verification, Verification::Passed | Verification::Failed | Verification::Unverified) {
                    result.content.push_str(&format!("\n[IGRIS check: {}]", verdict.note));
                }
                let record = ActionRecord { tool: call.name.clone(), target, ok: true, verification: verdict.verification, note: verdict.note.clone() };
                self.hub.update(&self.id, None, |t| t.context.record(record));
                if failed {
                    self.guard.failed(call);
                    return self.fail(Failure::Unconfirmed, activity, attempt, result);
                }
                if self.state() == TaskState::Verifying {
                    let _ = self.hub.transition(&self.id, TaskState::Executing, |t| t.activity = None);
                }
                After::Continue
            }
            Some(failure) => {
                self.guard.failed(call);
                // A failed read is an observation ("that file doesn't exist"), not a failed action —
                // unless it was invalid or the user stopped it.
                if !acts && !matches!(failure, Failure::Invalid | Failure::Cancelled | Failure::Denied) {
                    if failure == Failure::Transient && attempt == 0 && activity.permission == Some(crate::tools::PermissionLevel::Safe) {
                        self.hub.update(&self.id, Some(TaskEvent::Retrying { tool: call.name.clone() }), |_| {});
                        return After::RetryNow;
                    }
                    self.hub.update(&self.id, Some(TaskEvent::ObservationReceived { tool: call.name.clone() }), |_| {});
                    return After::Continue;
                }
                self.hub.update(&self.id, Some(TaskEvent::ToolCompleted { tool: call.name.clone(), ok: false }), |_| {});
                let note = clip(&result.content, 200);
                let record = ActionRecord { tool: call.name.clone(), target, ok: false, verification: Verification::NotApplicable, note };
                self.hub.update(&self.id, None, |t| t.context.record(record));
                self.fail(failure, activity, attempt, result)
            }
        }
    }

    /// The registered project whose folder this action works in, if any.
    fn project_for(&self, call: &ToolCall) -> Option<super::task::TaskProject> {
        if self.task().is_some_and(|t| t.project.is_some()) {
            return None;
        }
        let path = ["cwd", "path", "to"].iter().find_map(|k| call.input[*k].as_str())?;
        let conn = self.hub.db.conn().ok()?;
        let projects = crate::projects::list(&conn).ok()?;
        let target = comparable(std::path::Path::new(path));
        projects
            .into_iter()
            .filter(|p| !p.path.is_empty() && target.starts_with(comparable(std::path::Path::new(&p.path))))
            .max_by_key(|p| p.path.len())
            .map(|p| super::task::TaskProject { id: p.id, name: p.name })
    }

    fn fail(&mut self, failure: Failure, activity: &ToolActivity, attempt: u32, result: &mut ToolResult) -> After {
        let failures = self.hub.update(&self.id, None, |t| t.failures += 1).map(|t| t.failures).unwrap_or(u32::MAX);
        match recovery::decide(failure, activity.permission, attempt, failures, &activity.description) {
            Decision::RetryNow => {
                self.hub.update(&self.id, Some(TaskEvent::Retrying { tool: activity.tool.clone() }), |_| {});
                After::RetryNow
            }
            Decision::Adjust => {
                let _ = self.hub.transition(&self.id, TaskState::Recovering, |t| t.activity = Some(format!("Recovering: {}", clip(&activity.description, 80))));
                result.content.push_str(&format!("\n{}", recovery::guidance(failure, failures)));
                After::Continue
            }
            Decision::Stop { cancelled, reason } => {
                let to = if cancelled { TaskState::Cancelled } else { TaskState::Failed };
                let r = reason.clone();
                if self.hub.transition(&self.id, to, |t| t.error = Some(r)).is_err() {
                    // e.g. Verifying → Failed is legal; anything else goes through Recovering.
                    let _ = self.hub.transition(&self.id, TaskState::Recovering, |_| {});
                    let r = reason.clone();
                    let _ = self.hub.transition(&self.id, to, |t| t.error = Some(r));
                }
                result.content.push_str(&format!("\n[IGRIS task stopped: {reason} Don't take further actions; tell the user what happened.]"));
                After::Continue
            }
        }
    }

    async fn verify(&self, call: &ToolCall, result: &ToolResult, ctx: &ExecContext<'_>) -> Verdict {
        let check = verify::check_for(call, result);
        if !matches!(check, Check::Done(Verdict { verification: Verification::NotApplicable | Verification::Operator, .. })) {
            let _ = self.hub.transition(&self.id, TaskState::Verifying, |t| t.activity = Some(format!("Checking: {}", clip(&target_of(call), 80))));
            self.hub.update(&self.id, Some(TaskEvent::VerificationStarted { tool: call.name.clone() }), |_| {});
        }
        let verdict = match check {
            Check::Done(v) => v,
            Check::Probe { call: probe, expect, what } => {
                let (r, _) = executor::execute(&probe, ctx, &mut |_| {}).await;
                verify::judge(&expect, &what, &r)
            }
            Check::Window { app } => verify::window_check(self.hub.operator().map(|o| o.driver.clone()), &app).await,
        };
        let event = match verdict.verification {
            Verification::Passed => Some(TaskEvent::VerificationPassed { tool: call.name.clone() }),
            Verification::Failed => Some(TaskEvent::VerificationFailed { tool: call.name.clone() }),
            _ => None,
        };
        if let Some(e) = event {
            self.hub.update(&self.id, Some(e), |_| {});
        }
        verdict
    }

    /// The run is over: decide the outcome from the evidence, then release the task.
    pub fn finish(self, end: &RunEnd) -> Option<Task> {
        let task = self.task()?;
        if !task.state.is_final() {
            let (to, result, error) = match end {
                RunEnd::Cancelled => (TaskState::Cancelled, None, Some("Stopped by the user.".to_string())),
                RunEnd::Error(e) => (TaskState::Failed, None, Some(format!("The AI request failed: {e}"))),
                RunEnd::StepLimit(n) => (TaskState::Failed, None, Some(format!("Ran out of steps ({n}) before finishing."))),
                RunEnd::Answered => self.judge_outcome(&task),
            };
            if to == TaskState::Completed && task.state != TaskState::Verifying {
                let _ = self.hub.transition(&self.id, TaskState::Verifying, |_| {});
            }
            if matches!(to, TaskState::Failed | TaskState::Ended) && matches!(task.state, TaskState::Created) {
                let _ = self.hub.transition(&self.id, TaskState::Executing, |_| {});
            }
            if self
                .hub
                .transition(&self.id, to, |t| {
                    t.result = result.clone();
                    t.error = error.clone();
                    t.activity = None;
                    t.pause_reason = None;
                })
                .is_err()
            {
                // e.g. still waiting for approval when the turn ended.
                let _ = self.hub.transition(&self.id, TaskState::Failed, |t| t.error = error.or(Some("The reply ended before the task finished.".into())));
            }
        }
        self.hub.update(&self.id, None, |t| t.activity = None);
        let t = self.hub.release(&self.id);
        if let Some(t) = &t {
            tracing::info!(event = "TASK_RUN_ENDED", task_id = %t.id, state = t.state.as_str(), steps = t.steps, failures = t.failures);
        }
        t
    }

    /// The model finished answering. Completed only with evidence; failures
    /// and unverifiable work are reported as such.
    fn judge_outcome(&self, task: &Task) -> (TaskState, Option<String>, Option<String>) {
        if task.kind == TaskKind::Operator {
            // Operator mode decides for its part: completion needs the observation after the last action.
            if let Some(op) = self.hub.operator() {
                op.end_turn(&self.conversation_id);
                if let Some(o) = op.snapshot().task.filter(|o| o.id == task.id) {
                    return match o.state {
                        TaskState::Completed if self.unresolved(task).is_empty() => (TaskState::Completed, o.result, None),
                        TaskState::Completed => (TaskState::Failed, None, Some(self.unresolved(task).join(" "))),
                        TaskState::Ended => (TaskState::Ended, o.result.or(Some("Handed back to you.".into())), None),
                        TaskState::Cancelled => (TaskState::Cancelled, None, o.error),
                        _ => (TaskState::Failed, None, o.error.or(Some("Operator mode ended without a verified result.".into()))),
                    };
                }
            }
        }
        let unresolved = self.unresolved(task);
        if !unresolved.is_empty() {
            return (TaskState::Failed, None, Some(unresolved.join(" ")));
        }
        let actions: Vec<&ActionRecord> = Self::latest(task).into_iter().filter(|a| a.ok).collect();
        if task.steps == 0 {
            return (TaskState::Ended, Some("No action was taken.".into()), None);
        }
        let open: Vec<String> = task.open_steps().iter().map(|s| s.title.clone()).collect();
        if !open.is_empty() {
            return (TaskState::Ended, Some(format!("Stopped before finishing: {}.", open.join("; "))), None);
        }
        let unverified: Vec<&ActionRecord> = actions
            .iter()
            .copied()
            .filter(|a| !matches!(a.verification, Verification::Passed | Verification::Operator | Verification::NotApplicable))
            .collect();
        if unverified.is_empty() {
            (TaskState::Completed, Some(format!("Done — {} action(s), each checked.", task.steps)), None)
        } else {
            let what: Vec<String> = unverified.iter().map(|a| format!("{} {}", a.tool, a.target)).collect();
            (TaskState::Ended, Some(format!("Done, but this couldn't be verified: {}.", what.join("; "))), None)
        }
    }

    /// The latest attempt at each target (a later success supersedes a failure).
    fn latest(task: &Task) -> Vec<&ActionRecord> {
        let mut latest: Vec<&ActionRecord> = Vec::new();
        for a in &task.context.actions {
            latest.retain(|b| !(b.tool == a.tool && b.target == a.target));
            latest.push(a);
        }
        latest
    }

    /// Latest attempt at each target that failed or failed its check.
    fn unresolved(&self, task: &Task) -> Vec<String> {
        Self::latest(task)
            .into_iter()
            .filter(|a| !a.ok || a.verification == Verification::Failed)
            .map(|a| format!("{} {}: {}", a.tool, clip(&a.target, 80), if a.note.is_empty() { "failed" } else { &a.note }))
            .collect()
    }
}

/// A path in a form that compares reliably: resolved through its nearest
/// existing ancestor (the file may not exist yet), without Windows' `\\?\`
/// prefix (which `canonicalize` adds), and case-insensitive on Windows.
fn comparable(path: &std::path::Path) -> std::path::PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    let resolved = loop {
        if let Ok(c) = existing.canonicalize() {
            break c;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                existing = parent;
            }
            _ => break path.to_path_buf(),
        }
    };
    let mut full = resolved;
    for part in rest.into_iter().rev() {
        full.push(part);
    }
    let s = full.to_string_lossy().into_owned();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    std::path::PathBuf::from(if cfg!(windows) { s.to_lowercase() } else { s })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_compare_through_canonical_forms_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let new_file = dir.path().join("sub").join("new.txt");
        assert!(comparable(&new_file).starts_with(comparable(&canonical)), "a file that doesn't exist yet is still inside");
        assert!(!comparable(std::path::Path::new("/elsewhere/x")).starts_with(comparable(&canonical)));
        #[cfg(windows)]
        assert_eq!(comparable(std::path::Path::new(r"\\?\C:\Windows")), comparable(std::path::Path::new(r"c:\windows")));
    }
}
