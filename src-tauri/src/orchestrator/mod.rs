//! The IGRIS orchestrator: actionable requests become tasks with a lifecycle.
//!
//! ```text
//! chat turn ─▶ agent loop (agent_loop.rs) ─▶ model (Phase 2 router, budget, compaction)
//!                  │  action requested → task (intent.rs) ─▶ Orchestrator (this file)
//!                  ▼
//!            tool executor (validation → permission → approval → run → audit)
//!                  │
//!            verification (verify.rs) → recovery (recovery.rs) → continue / finish
//! ```
//!
//! This file is the hub: it holds the tasks that are running now, applies
//! state transitions (persisting and broadcasting each one), and implements
//! pause / resume / stop. A task runs inside the chat turn that started it;
//! its state lives here and in the database, independent of the response
//! stream, and survives a restart as an interrupted task that the user can
//! resume.

pub mod agent_loop;
pub mod brief;
#[cfg(test)]
mod e2e;
pub mod intent;
pub mod recovery;
pub mod session;
pub mod store;
pub mod task;
pub mod toolset;
pub mod verify;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::operator::Operator;
use task::{IllegalTransition, Task, TaskKind, TaskState};

/// How long a paused task waits for the user before it is cancelled.
pub const PAUSE_LIMIT: Duration = crate::operator::PAUSE_LIMIT;

/// Lifecycle events (logged as `TASK_*` and sent to the UI with the task).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskEvent {
    Created,
    PlanningStarted,
    PlanUpdated,
    StepStarted { step: usize },
    ToolRequested { tool: String },
    ApprovalRequested { tool: String },
    ToolCompleted { tool: String, ok: bool },
    ObservationReceived { tool: String },
    VerificationStarted { tool: String },
    VerificationPassed { tool: String },
    VerificationFailed { tool: String },
    Retrying { tool: String },
    Recovering,
    Paused,
    Resumed,
    Cancelled,
    Completed,
    Failed,
    Ended,
    /// Any other state change (e.g. back to executing after a check).
    StateChanged,
}

impl TaskEvent {
    pub fn name(&self) -> &'static str {
        match self {
            TaskEvent::Created => "TASK_CREATED",
            TaskEvent::PlanningStarted => "TASK_PLANNING_STARTED",
            TaskEvent::PlanUpdated => "TASK_PLAN_UPDATED",
            TaskEvent::StepStarted { .. } => "TASK_STEP_STARTED",
            TaskEvent::ToolRequested { .. } => "TASK_TOOL_REQUESTED",
            TaskEvent::ApprovalRequested { .. } => "TASK_APPROVAL_REQUESTED",
            TaskEvent::ToolCompleted { .. } => "TASK_TOOL_COMPLETED",
            TaskEvent::ObservationReceived { .. } => "TASK_OBSERVATION_RECEIVED",
            TaskEvent::VerificationStarted { .. } => "TASK_VERIFICATION_STARTED",
            TaskEvent::VerificationPassed { .. } => "TASK_VERIFICATION_PASSED",
            TaskEvent::VerificationFailed { .. } => "TASK_VERIFICATION_FAILED",
            TaskEvent::Retrying { .. } => "TASK_RETRYING",
            TaskEvent::Recovering => "TASK_RECOVERING",
            TaskEvent::Paused => "TASK_PAUSED",
            TaskEvent::Resumed => "TASK_RESUMED",
            TaskEvent::Cancelled => "TASK_CANCELLED",
            TaskEvent::Completed => "TASK_COMPLETED",
            TaskEvent::Failed => "TASK_FAILED",
            TaskEvent::Ended => "TASK_ENDED",
            TaskEvent::StateChanged => "TASK_STATE_CHANGED",
        }
    }

    /// The event announcing that a task entered `state`, if there is one.
    pub fn for_state(state: TaskState) -> Option<TaskEvent> {
        Some(match state {
            TaskState::Planning => TaskEvent::PlanningStarted,
            TaskState::Recovering => TaskEvent::Recovering,
            TaskState::Paused => TaskEvent::Paused,
            TaskState::Cancelled => TaskEvent::Cancelled,
            TaskState::Completed => TaskEvent::Completed,
            TaskState::Failed => TaskEvent::Failed,
            TaskState::Ended => TaskEvent::Ended,
            _ => return None,
        })
    }
}

/// A task as the UI sees it: the task, and whether a turn is running it now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfo {
    #[serde(flatten)]
    pub task: Task,
    /// Being worked on right now (false for a task interrupted by a restart).
    pub live: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskUpdate {
    pub event: TaskEvent,
    pub task: TaskInfo,
}

pub type Listener = Arc<dyn Fn(&TaskUpdate) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Control {
    Pause,
    Resume,
    Stop,
}

struct Live {
    task: Task,
    cancel: CancellationToken,
    /// Resumed since the agent loop last looked (it must re-observe).
    resumed: bool,
}

pub struct Orchestrator {
    db: Arc<Database>,
    operator: Option<Arc<Operator>>,
    live: Mutex<HashMap<String, Live>>,
    changed: Notify,
    listeners: RwLock<Vec<Listener>>,
}

impl Orchestrator {
    pub fn new(db: Arc<Database>, operator: Option<Arc<Operator>>) -> Arc<Self> {
        let hub =
            Arc::new(Self { db, operator: operator.clone(), live: Mutex::new(HashMap::new()), changed: Notify::new(), listeners: RwLock::new(Vec::new()) });
        if let Some(op) = operator {
            let weak: Weak<Self> = Arc::downgrade(&hub);
            op.on_change(Arc::new(move |snap| {
                if let Some(hub) = weak.upgrade() {
                    hub.sync_operator(snap);
                }
            }));
        }
        hub
    }

    pub fn operator(&self) -> Option<&Arc<Operator>> {
        self.operator.as_ref()
    }

    pub fn on_event(&self, l: Listener) {
        if let Ok(mut g) = self.listeners.write() {
            g.push(l);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Live>> {
        self.live.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn persist(&self, t: &Task) {
        if let Err(e) = self.db.conn().and_then(|c| store::save(&c, t)) {
            tracing::warn!(event = "TASK_SAVE_FAILED", error = %e);
        }
    }

    fn emit(&self, event: TaskEvent, task: &Task, live: bool) {
        tracing::info!(event = event.name(), task_id = %task.id, state = task.state.as_str(), steps = task.steps, failures = task.failures);
        let update = TaskUpdate { event, task: TaskInfo { task: task.clone(), live } };
        let listeners = self.listeners.read().map(|g| g.clone()).unwrap_or_default();
        for l in listeners {
            l(&update);
        }
    }

    /// Startup: tasks that were running when IGRIS closed come back paused and
    /// marked interrupted. Nothing is resumed automatically.
    pub fn recover(&self) -> AppResult<Vec<Task>> {
        let tasks = store::recover_interrupted(&*self.db.conn()?)?;
        for t in &tasks {
            tracing::info!(event = "TASK_INTERRUPTED", task_id = %t.id, state = t.state.as_str());
        }
        Ok(tasks)
    }

    /// Start a task for the running turn of `conversation_id`.
    pub fn begin(&self, conversation_id: &str, objective: &str, cancel: &CancellationToken) -> AppResult<Task> {
        let mut task = Task::new(Some(conversation_id), objective);
        {
            let mut g = self.lock();
            if g.values().any(|l| l.task.conversation_id.as_deref() == Some(conversation_id)) {
                return Err(AppError::validation("A task is already running in this conversation."));
            }
            task.updated_at = task::now();
            g.insert(task.id.clone(), Live { task: task.clone(), cancel: cancel.clone(), resumed: false });
        }
        self.persist(&task);
        self.emit(TaskEvent::Created, &task, true);
        Ok(task)
    }

    /// Continue a paused (interrupted) or failed task in a new turn. The user
    /// asked for it; the loop re-observes before acting.
    pub fn attach(&self, task_id: &str, conversation_id: &str, cancel: &CancellationToken) -> AppResult<Task> {
        if self.lock().contains_key(task_id) {
            return Err(AppError::validation("That task is already running."));
        }
        let mut task = store::get(&*self.db.conn()?, task_id)?.ok_or_else(|| AppError::validation("That task no longer exists."))?;
        if task.conversation_id.as_deref() != Some(conversation_id) {
            return Err(AppError::validation("That task belongs to another conversation."));
        }
        let event = match task.state {
            TaskState::Paused => {
                task.state.advance(TaskState::Executing).map_err(|e| AppError::internal(e.to_string()))?;
                TaskEvent::Resumed
            }
            TaskState::Failed => {
                task.state.advance(TaskState::Recovering).map_err(|e| AppError::internal(e.to_string()))?;
                task.error = None;
                task.failures = 0;
                TaskEvent::Retrying { tool: String::new() }
            }
            s => return Err(AppError::validation(format!("A {} task can't be resumed.", s.as_str()))),
        };
        task.pause_reason = None;
        task.context.interrupted = false;
        task.context.resumes += 1;
        task.updated_at = task::now();
        {
            let mut g = self.lock();
            if g.values().any(|l| l.task.conversation_id.as_deref() == Some(conversation_id)) {
                return Err(AppError::validation("A task is already running in this conversation."));
            }
            g.insert(task.id.clone(), Live { task: task.clone(), cancel: cancel.clone(), resumed: true });
        }
        self.persist(&task);
        self.emit(event, &task, true);
        Ok(task)
    }

    /// The current state of a task (running or stored).
    pub fn task(&self, id: &str) -> Option<Task> {
        if let Some(l) = self.lock().get(id) {
            return Some(l.task.clone());
        }
        self.db.conn().ok().and_then(|c| store::get(&c, id).ok().flatten())
    }

    pub fn is_live(&self, id: &str) -> bool {
        self.lock().contains_key(id)
    }

    /// Change a running task (not its state), persist it and announce `event`.
    pub fn update(&self, id: &str, event: Option<TaskEvent>, f: impl FnOnce(&mut Task)) -> Option<Task> {
        let t = {
            let mut g = self.lock();
            let l = g.get_mut(id)?;
            f(&mut l.task);
            l.task.updated_at = task::now();
            l.task.clone()
        };
        self.persist(&t);
        if let Some(e) = event {
            self.emit(e, &t, true);
        }
        self.changed.notify_waiters();
        Some(t)
    }

    /// Move a running task to `to`; illegal transitions are refused and leave it unchanged.
    pub fn transition(&self, id: &str, to: TaskState, f: impl FnOnce(&mut Task)) -> Result<Task, IllegalTransition> {
        let (t, changed) = {
            let mut g = self.lock();
            let Some(l) = g.get_mut(id) else { return Err(IllegalTransition { from: to, to }) };
            let from = l.task.state;
            if from == to {
                return Ok(l.task.clone());
            }
            l.task.state.advance(to)?;
            f(&mut l.task);
            l.task.updated_at = task::now();
            (l.task.clone(), from != to)
        };
        if changed {
            self.persist(&t);
            self.emit(TaskEvent::for_state(to).unwrap_or(TaskEvent::StateChanged), &t, true);
            self.changed.notify_waiters();
        }
        Ok(t)
    }

    /// Between steps: wait while the task is paused. `Err` when the turn must
    /// stop now (the user stopped it, or it stayed paused too long).
    /// `Ok(true)` when it was resumed since the last checkpoint.
    pub async fn checkpoint(&self, id: &str) -> Result<bool, String> {
        let deadline = Instant::now() + PAUSE_LIMIT;
        loop {
            let notified = self.changed.notified();
            let (state, cancelled) = {
                let g = self.lock();
                let Some(l) = g.get(id) else { return Err("The task is no longer running.".into()) };
                (l.task.state, l.cancel.is_cancelled())
            };
            if cancelled {
                return Err("Stopped by the user.".into());
            }
            if state != TaskState::Paused {
                let mut g = self.lock();
                return Ok(g.get_mut(id).map(|l| std::mem::take(&mut l.resumed)).unwrap_or(false));
            }
            if tokio::time::timeout_at(deadline.into(), notified).await.is_err() {
                let _ = self.transition(id, TaskState::Cancelled, |t| t.error = Some("Paused for too long; the task was stopped.".into()));
                return Err("The task stayed paused for 5 minutes, so it was stopped.".into());
            }
        }
    }

    /// Pause, resume or stop a task (UI, voice). Operator tasks pause and stop
    /// operator mode too.
    pub fn control(&self, id: &str, action: Control) -> AppResult<Task> {
        let Some((kind, cancel)) = self.lock().get(id).map(|l| (l.task.kind, l.cancel.clone())) else {
            // Not running: an interrupted task can only be dismissed (or resumed from the chat).
            return match action {
                Control::Stop => self.dismiss(id),
                _ => Err(AppError::validation("This task isn't running. Resume it from its conversation.")),
            };
        };
        let op = self.operator.as_ref().filter(|_| kind == TaskKind::Operator);
        let result = match action {
            Control::Pause => {
                if let Some(op) = op {
                    op.pause("Paused by you.");
                }
                self.transition(id, TaskState::Paused, |t| t.pause_reason = Some("Paused by you.".into()))
            }
            Control::Resume => {
                if let Some(op) = op {
                    op.resume();
                }
                self.resume_live(id)
            }
            Control::Stop => {
                if let Some(op) = op {
                    op.stop("Stopped by you.");
                }
                cancel.cancel();
                self.transition(id, TaskState::Cancelled, |t| t.error = Some("Stopped by you.".into()))
            }
        };
        result.map_err(|e| AppError::validation(format!("Can't do that now ({e}).")))
    }

    fn resume_live(&self, id: &str) -> Result<Task, IllegalTransition> {
        match self.lock().get(id).map(|l| l.task.clone()) {
            Some(t) if t.state.is_final() => return Err(IllegalTransition { from: t.state, to: TaskState::Executing }),
            Some(t) if t.state != TaskState::Paused => return Ok(t),
            _ => {}
        }
        let t = self.transition(id, TaskState::Executing, |t| {
            t.pause_reason = None;
            t.context.resumes += 1;
        })?;
        if let Some(l) = self.lock().get_mut(id) {
            l.resumed = true;
        }
        self.emit(TaskEvent::Resumed, &t, true);
        Ok(t)
    }

    /// Give up on an interrupted task without resuming it.
    pub fn dismiss(&self, id: &str) -> AppResult<Task> {
        if self.is_live(id) {
            return Err(AppError::validation("Stop the task first."));
        }
        let conn = self.db.conn()?;
        let mut t = store::get(&conn, id)?.ok_or_else(|| AppError::validation("That task no longer exists."))?;
        if t.state.is_final() {
            return Err(AppError::validation("That task has already ended."));
        }
        t.state.advance(TaskState::Cancelled).map_err(|e| AppError::validation(format!("Can't dismiss this task ({e}).")))?;
        t.error = Some("Dismissed by you.".into());
        t.pause_reason = None;
        t.updated_at = task::now();
        store::save(&conn, &t)?;
        drop(conn);
        self.emit(TaskEvent::Cancelled, &t, false);
        Ok(t)
    }

    /// The turn running this task ended.
    pub fn release(&self, id: &str) -> Option<Task> {
        let t = self.lock().remove(id).map(|l| l.task);
        if let Some(t) = &t {
            self.persist(t);
        }
        self.changed.notify_waiters();
        t
    }

    /// Tasks of a conversation, newest first, with the running one marked.
    pub fn for_conversation(&self, conversation_id: &str, limit: u32) -> AppResult<Vec<TaskInfo>> {
        let stored = store::for_conversation(&*self.db.conn()?, conversation_id, limit)?;
        let g = self.lock();
        Ok(stored
            .into_iter()
            .map(|t| match g.get(&t.id) {
                Some(l) => TaskInfo { task: l.task.clone(), live: true },
                None => TaskInfo { task: t, live: false },
            })
            .collect())
    }

    /// Operator mode changed: mirror its pause/resume into the task it belongs to.
    fn sync_operator(&self, snap: &crate::operator::Snapshot) {
        let Some(op_task) = &snap.task else { return };
        let Some(state) = self.lock().get(&op_task.id).map(|l| l.task.state) else { return };
        if self.lock().get(&op_task.id).is_some_and(|l| l.task.kind != TaskKind::Operator) {
            self.update(&op_task.id, None, |t| t.kind = TaskKind::Operator);
        }
        match (op_task.state, state) {
            (TaskState::Paused, s) if s != TaskState::Paused => {
                let reason = op_task.pause_reason.clone();
                let _ = self.transition(&op_task.id, TaskState::Paused, |t| t.pause_reason = reason);
            }
            (TaskState::Executing | TaskState::Verifying, TaskState::Paused) => {
                let _ = self.resume_live(&op_task.id);
            }
            // Stopped with Esc, the orb or by voice: the task stops too, so no
            // other action runs; the model may still say what was done.
            (TaskState::Cancelled, s) if !s.is_final() => {
                let reason = op_task.error.clone().unwrap_or_else(|| "Stopped by you.".into());
                if self.transition(&op_task.id, TaskState::Cancelled, |t| t.error = Some(reason.clone())).is_err() {
                    let _ = self.transition(&op_task.id, TaskState::Recovering, |_| {});
                    let _ = self.transition(&op_task.id, TaskState::Cancelled, |t| t.error = Some(reason));
                }
            }
            _ => {
                let status = op_task.status.clone();
                if !status.is_empty() {
                    self.update(&op_task.id, None, |t| t.activity = Some(status));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer::fake::{window, FakeDriver};

    type Events = Arc<Mutex<Vec<String>>>;

    fn hub() -> (Arc<Orchestrator>, Arc<Operator>, String, Events) {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let cid = crate::conversations::create(&db.conn().unwrap(), "t", "s", &[]).unwrap().id;
        let op = Arc::new(Operator::new(db.clone(), Arc::new(FakeDriver::with(vec![window(1, "Notepad", "notepad.exe")], vec![]))));
        let hub = Orchestrator::new(db, Some(op.clone()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let e2 = events.clone();
        hub.on_event(Arc::new(move |u: &TaskUpdate| e2.lock().unwrap().push(u.event.name().to_string())));
        (hub, op, cid, events)
    }

    #[tokio::test]
    async fn pause_resume_and_stop_reach_the_running_turn() {
        let (hub, _op, cid, events) = hub();
        let cancel = CancellationToken::new();
        let t = hub.begin(&cid, "Write notes", &cancel).unwrap();
        assert!(hub.begin(&cid, "again", &cancel).is_err(), "one running task per conversation");
        hub.transition(&t.id, TaskState::Executing, |_| {}).unwrap();
        assert_eq!(hub.checkpoint(&t.id).await, Ok(false));

        hub.control(&t.id, Control::Pause).unwrap();
        let h2 = hub.clone();
        let id = t.id.clone();
        let waiter = tokio::spawn(async move { h2.checkpoint(&id).await });
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!waiter.is_finished(), "paused tasks wait");
        hub.control(&t.id, Control::Resume).unwrap();
        assert_eq!(waiter.await.unwrap(), Ok(true), "the loop learns it was resumed (and must re-observe)");
        assert_eq!(hub.checkpoint(&t.id).await, Ok(false), "only once");

        hub.control(&t.id, Control::Stop).unwrap();
        assert!(cancel.is_cancelled(), "stop cancels the running turn");
        assert!(hub.checkpoint(&t.id).await.is_err());
        assert_eq!(hub.task(&t.id).unwrap().state, TaskState::Cancelled);
        assert!(hub.control(&t.id, Control::Resume).is_err(), "cancelled is final");
        hub.release(&t.id);
        assert!(!hub.is_live(&t.id));
        let ev = events.lock().unwrap().clone();
        for name in ["TASK_CREATED", "TASK_PAUSED", "TASK_RESUMED", "TASK_CANCELLED"] {
            assert!(ev.contains(&name.to_string()), "{name} in {ev:?}");
        }
    }

    #[tokio::test]
    async fn interrupted_tasks_need_the_user_to_resume_them() {
        let (hub, _op, cid, _) = hub();
        let cancel = CancellationToken::new();
        let t = hub.begin(&cid, "Build it", &cancel).unwrap();
        hub.transition(&t.id, TaskState::Executing, |_| {}).unwrap();
        // The app closes mid-task (nothing released), then starts again.
        let hub2 = Orchestrator::new(hub.db.clone(), None);
        let recovered = hub2.recover().unwrap();
        assert_eq!(recovered.len(), 1);
        let stored = hub2.task(&t.id).unwrap();
        assert_eq!(stored.state, TaskState::Paused);
        assert!(stored.context.interrupted);
        assert!(!hub2.is_live(&t.id));
        assert!(hub2.control(&t.id, Control::Resume).is_err(), "no blind resume");
        assert!(hub2.attach(&t.id, "other-conversation", &cancel).is_err());
        let resumed = hub2.attach(&t.id, &cid, &cancel).unwrap();
        assert_eq!(resumed.state, TaskState::Executing);
        assert!(!resumed.context.interrupted);
        assert_eq!(hub2.checkpoint(&t.id).await, Ok(true), "the first round re-observes");
    }

    #[test]
    fn failed_tasks_can_be_retried_and_interrupted_ones_dismissed() {
        let (hub, _op, cid, _) = hub();
        let cancel = CancellationToken::new();
        let t = hub.begin(&cid, "x", &cancel).unwrap();
        hub.transition(&t.id, TaskState::Executing, |_| {}).unwrap();
        hub.transition(&t.id, TaskState::Failed, |t| t.error = Some("boom".into())).unwrap();
        hub.release(&t.id);
        let retry = hub.attach(&t.id, &cid, &cancel).unwrap();
        assert_eq!((retry.state, retry.error), (TaskState::Recovering, None));
        hub.transition(&t.id, TaskState::Paused, |_| {}).unwrap();
        hub.release(&t.id);
        let d = hub.dismiss(&t.id).unwrap();
        assert_eq!(d.state, TaskState::Cancelled);
        assert!(hub.dismiss(&t.id).is_err());
    }

    #[tokio::test]
    async fn operator_pause_and_takeover_pause_the_task() {
        let (hub, op, cid, _) = hub();
        let cancel = CancellationToken::new();
        let t = hub.begin(&cid, "Open Notepad", &cancel).unwrap();
        hub.transition(&t.id, TaskState::Executing, |_| {}).unwrap();
        op.start(Some(&t.id), Some(&cid), "Open Notepad", vec!["Open it".into()]).unwrap();
        assert_eq!(hub.task(&t.id).unwrap().kind, TaskKind::Operator);
        // The orb / Esc / voice / takeover pause the operator; the task follows.
        op.pause("You switched to another window, so I paused.");
        let paused = hub.task(&t.id).unwrap();
        assert_eq!(paused.state, TaskState::Paused);
        assert!(paused.pause_reason.unwrap().contains("another window"));
        op.resume();
        assert_eq!(hub.task(&t.id).unwrap().state, TaskState::Executing);
        // Pausing from the task card pauses operator mode too.
        hub.control(&t.id, Control::Pause).unwrap();
        assert_eq!(op.snapshot().task.unwrap().state, TaskState::Paused);
        hub.control(&t.id, Control::Stop).unwrap();
        assert_eq!(op.snapshot().task.unwrap().state, TaskState::Cancelled);
        // Esc / the orb / "IGRIS, stop" stop the operator; the task follows.
        let cancel2 = CancellationToken::new();
        hub.release(&t.id);
        let t2 = hub.begin(&cid, "Again", &cancel2).unwrap();
        hub.transition(&t2.id, TaskState::Executing, |_| {}).unwrap();
        op.start(Some(&t2.id), Some(&cid), "Again", vec![]).unwrap();
        op.stop("Stopped with Esc.");
        assert_eq!(hub.task(&t2.id).unwrap().state, TaskState::Cancelled);
        assert!(!cancel2.is_cancelled(), "the reply may still report what was done");
        // One row per task: the operator session and the task share it.
        let n: i64 = hub.db.conn().unwrap().query_row("SELECT COUNT(*) FROM operator_tasks", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }
}
