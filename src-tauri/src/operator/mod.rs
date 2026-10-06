//! Operator mode: the state in which IGRIS is actively operating the computer.
//!
//! One task at a time. A task starts only after the user approves it (the
//! `operator_start` tool is SENSITIVE), runs while the chat turn that started it
//! runs, and ends when IGRIS reports the outcome, the user stops it, or the turn
//! ends. Every computer action passes [`Operator::checkpoint`] first, which
//! enforces stop and pause and detects the user taking over the mouse or
//! keyboard (→ automatic pause). Task history is kept in `operator_tasks`
//! (shared with the orchestrator, which owns the task an operator session
//! belongs to; see `orchestrator/`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use rusqlite::params;
use serde::Serialize;
use tokio::sync::Notify;

use crate::computer::{Display, SharedDriver, UiElement, WindowInfo};
use crate::db::Database;
use crate::orchestrator::task::{decode_plan, PlanStep};

/// Failed actions allowed per task before IGRIS gives up (no endless retry loops).
pub const MAX_FAILURES: u32 = 8;
/// Hard cap on computer actions per task.
pub const MAX_ACTIONS: u32 = 150;
/// How long a paused task waits for the user before ending.
pub const PAUSE_LIMIT: Duration = Duration::from_secs(300);
/// Input this long after IGRIS's own last input counts as the user's.
const INPUT_SLACK_MS: u64 = 400;

/// The task state machine is shared with the orchestrator: an operator
/// session is a task of kind `operator`.
pub use crate::orchestrator::task::TaskState;

/// What the overlay orb shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Planning,
    Executing,
    Waiting,
    Verifying,
    Paused,
    Success,
    Error,
    Stopped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: String,
    pub conversation_id: Option<String>,
    pub objective: String,
    pub plan: Vec<String>,
    pub state: TaskState,
    pub phase: Phase,
    /// Short status line, e.g. "OPENING VS CODE".
    pub status: String,
    pub steps: u32,
    pub retries: u32,
    pub result: Option<String>,
    pub error: Option<String>,
    pub pause_reason: Option<String>,
    /// Display the task is working on (overlay placement).
    pub display: Option<Display>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// IGRIS is controlling (or holding paused control of) the computer.
    pub active: bool,
    pub task: Option<TaskView>,
}

/// The last look at the screen; actions target elements and coordinates from it.
#[derive(Debug, Clone)]
pub struct Observation {
    pub window: Option<WindowInfo>,
    pub display: Display,
    /// Size of the screenshot the model saw, if one was taken.
    pub image: Option<(u32, u32)>,
    pub elements: Vec<UiElement>,
}

struct Inner {
    task: Option<TaskView>,
    last_input: Instant,
    observation: Option<Observation>,
    actions_since_observe: u32,
    /// Window IGRIS was working in, re-focused on resume.
    target_window: Option<u64>,
    /// The user touched the computer since the last observation: look again before acting.
    stale: bool,
}

pub type Listener = Arc<dyn Fn(&Snapshot) + Send + Sync>;
/// Called with a screen point before IGRIS clicks there (to move the orb out of the way).
pub type PointHook = Arc<dyn Fn(i32, i32) + Send + Sync>;
/// Suspend (`false`) / restore (`true`) the stop hotkey while IGRIS itself presses Esc.
pub type HotkeyHook = Arc<dyn Fn(bool) + Send + Sync>;

pub struct Operator {
    db: Arc<Database>,
    pub driver: SharedDriver,
    inner: Mutex<Inner>,
    changed: Notify,
    listeners: RwLock<Vec<Listener>>,
    point_hook: RwLock<Option<PointHook>>,
    hotkey_hook: RwLock<Option<HotkeyHook>>,
    /// Set while IGRIS is injecting input (so its own Esc doesn't stop it).
    pub injecting: AtomicBool,
}

impl Operator {
    pub fn new(db: Arc<Database>, driver: SharedDriver) -> Self {
        Self {
            db,
            driver,
            inner: Mutex::new(Inner { task: None, last_input: Instant::now(), observation: None, actions_since_observe: 0, target_window: None, stale: false }),
            changed: Notify::new(),
            listeners: RwLock::new(Vec::new()),
            point_hook: RwLock::new(None),
            hotkey_hook: RwLock::new(None),
            injecting: AtomicBool::new(false),
        }
    }

    /// Add a listener for state changes (the overlay, the orchestrator).
    pub fn on_change(&self, l: Listener) {
        if let Ok(mut g) = self.listeners.write() {
            g.push(l);
        }
    }
    pub fn on_point(&self, h: PointHook) {
        if let Ok(mut g) = self.point_hook.write() {
            *g = Some(h);
        }
    }
    pub fn on_hotkey(&self, h: HotkeyHook) {
        if let Ok(mut g) = self.hotkey_hook.write() {
            *g = Some(h);
        }
    }
    pub fn before_point(&self, x: i32, y: i32) {
        if let Some(h) = self.point_hook.read().ok().and_then(|g| g.clone()) {
            h(x, y);
        }
    }
    pub fn hotkey_enabled(&self, on: bool) {
        if let Some(h) = self.hotkey_hook.read().ok().and_then(|g| g.clone()) {
            h(on);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn snapshot(&self) -> Snapshot {
        let g = self.lock();
        Snapshot { active: g.task.as_ref().is_some_and(|t| !t.state.is_final()), task: g.task.clone() }
    }

    fn notify(&self) {
        self.changed.notify_waiters();
        let snap = self.snapshot();
        let listeners = self.listeners.read().map(|g| g.clone()).unwrap_or_default();
        for l in listeners {
            l(&snap);
        }
    }

    fn persist(&self, t: &TaskView) {
        let Ok(conn) = self.db.conn() else { return };
        let plan: Vec<PlanStep> = t.plan.iter().map(PlanStep::pending).collect();
        let plan = serde_json::to_string(&plan).unwrap_or_else(|_| "[]".into());
        // The orchestrator's task row (same id) may exist already: operator mode
        // owns only its own columns.
        let r = conn.execute(
            "INSERT INTO operator_tasks (id, conversation_id, objective, plan, state, steps, retries, result, error, kind, pause_reason, updated_at, ended_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'operator', ?11, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
                     CASE WHEN ?10 THEN strftime('%Y-%m-%dT%H:%M:%SZ', 'now') END)
             ON CONFLICT(id) DO UPDATE SET state = ?5, steps = ?6, retries = ?7, result = ?8, error = ?9, kind = 'operator',
               pause_reason = ?11, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
               ended_at = COALESCE(ended_at, CASE WHEN ?10 THEN strftime('%Y-%m-%dT%H:%M:%SZ', 'now') END)",
            params![t.id, t.conversation_id, t.objective, plan, t.state.as_str(), t.steps, t.retries, t.result, t.error, t.state.is_final(), t.pause_reason],
        );
        if let Err(e) = r {
            tracing::warn!(event = "OPERATOR_TASK_SAVE_FAILED", error = %e);
        }
    }

    /// Mutate the active task, persist and broadcast.
    fn update(&self, f: impl FnOnce(&mut TaskView, &mut Inner)) -> Option<TaskView> {
        let t = {
            let mut g = self.lock();
            let mut task = g.task.take()?;
            f(&mut task, &mut g);
            g.task = Some(task.clone());
            task
        };
        self.persist(&t);
        self.notify();
        Some(t)
    }

    /// Whether `conversation_id` owns the running task (its tool calls skip per-action approval).
    pub fn covers(&self, conversation_id: Option<&str>) -> bool {
        let g = self.lock();
        match (&g.task, conversation_id) {
            (Some(t), Some(c)) => !t.state.is_final() && t.conversation_id.as_deref() == Some(c),
            _ => false,
        }
    }

    pub fn is_active(&self) -> bool {
        self.snapshot().active
    }

    /// Begin a task (after the user approved it). `task_id` is the
    /// orchestrator task this session belongs to (its row is shared).
    pub fn start(&self, task_id: Option<&str>, conversation_id: Option<&str>, objective: &str, plan: Vec<String>) -> Result<TaskView, String> {
        let display = self.driver.displays().ok().and_then(|d| d.into_iter().find(|d| d.primary));
        let task = {
            let mut g = self.lock();
            if let Some(t) = g.task.as_ref().filter(|t| !t.state.is_final()) {
                return Err(if t.conversation_id.as_deref() == conversation_id {
                    "Operator mode is already running for this task.".into()
                } else {
                    "IGRIS is already operating the computer for another chat.".into()
                });
            }
            let mut state = TaskState::Created;
            state.advance(TaskState::Executing).map_err(|e| e.to_string())?;
            let t = TaskView {
                id: task_id.map(str::to_string).unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                conversation_id: conversation_id.map(str::to_string),
                objective: objective.trim().to_string(),
                plan,
                state,
                phase: Phase::Planning,
                status: "PLANNING".into(),
                steps: 0,
                retries: 0,
                result: None,
                error: None,
                pause_reason: None,
                display,
            };
            g.task = Some(t.clone());
            g.last_input = Instant::now();
            g.observation = None;
            g.actions_since_observe = 0;
            g.target_window = None;
            g.stale = false;
            t
        };
        tracing::info!(event = "OPERATOR_STARTED", task_id = %task.id);
        self.persist(&task);
        self.notify();
        Ok(task)
    }

    /// Short status for the overlay, e.g. ("OPENING VS CODE", Executing).
    pub fn set_status(&self, status: &str, phase: Phase) {
        let status: String = status.trim().chars().take(40).collect::<String>().to_uppercase();
        self.update(|t, _| {
            if t.state.is_final() {
                return;
            }
            t.status = status;
            t.phase = phase;
            if t.state != TaskState::Paused {
                let _ = t.state.advance(if phase == Phase::Verifying { TaskState::Verifying } else { TaskState::Executing });
            }
        });
    }

    /// Before every computer action: honour stop and pause, and notice the
    /// user using the computer. If they switched away from the window IGRIS was
    /// working in, the task pauses; otherwise IGRIS must look again before acting.
    pub async fn checkpoint(&self) -> Result<(), String> {
        let deadline = Instant::now() + PAUSE_LIMIT;
        loop {
            let notified = self.changed.notified();
            let (state, user_input, target) = {
                let g = self.lock();
                let Some(t) = &g.task else { return Err("Operator mode isn't running. Start it with operator_start first.".into()) };
                let since_ours = g.last_input.elapsed().as_millis() as u64;
                let input = t.state != TaskState::Paused && self.driver.idle_ms().is_some_and(|idle| idle + INPUT_SLACK_MS < since_ours);
                (t.state, input, g.target_window)
            };
            match state {
                TaskState::Cancelled => return Err("The user stopped operator mode. Don't continue; tell them what was and wasn't done.".into()),
                s if s.is_final() => return Err("This task has already ended.".into()),
                TaskState::Paused => {
                    if tokio::time::timeout_at(deadline.into(), notified).await.is_err() {
                        self.finish_with(TaskState::Cancelled, None, Some("Paused for too long; control returned to the user.".into()));
                        return Err("The task stayed paused for 5 minutes, so it was ended. Tell the user what was done.".into());
                    }
                }
                _ if user_input => {
                    let fg = self.driver.foreground().ok().flatten().map(|w| w.id);
                    let mut g = self.lock();
                    g.last_input = Instant::now();
                    g.stale = true;
                    drop(g);
                    if target.is_some() && fg != target {
                        self.pause("You switched to another window, so I paused.");
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// Whether the user has interacted since IGRIS last looked at the screen.
    pub fn is_stale(&self) -> bool {
        self.lock().stale
    }

    /// Record IGRIS's own input (so it isn't mistaken for the user's).
    pub fn mark_input(&self) {
        self.lock().last_input = Instant::now();
    }

    /// Count an action; ends the task at the action cap.
    pub fn action_done(&self, label: &str) -> Result<(), String> {
        let over = self
            .update(|t, g| {
                t.steps += 1;
                g.actions_since_observe += 1;
                if !label.is_empty() {
                    t.status = label.chars().take(40).collect::<String>().to_uppercase();
                }
                t.phase = Phase::Executing;
            })
            .is_some_and(|t| t.steps >= MAX_ACTIONS);
        self.mark_input();
        if over {
            self.finish_with(TaskState::Failed, None, Some(format!("Stopped after {MAX_ACTIONS} actions.")));
            return Err(format!("Reached the limit of {MAX_ACTIONS} actions for one task; stopping."));
        }
        Ok(())
    }

    /// Count a failed action; ends the task after too many.
    pub fn action_failed(&self, why: &str) -> Option<String> {
        let t = self.update(|t, _| t.retries += 1)?;
        if t.retries >= MAX_FAILURES {
            self.finish_with(TaskState::Failed, None, Some(format!("Too many failed actions (last: {why}).")));
            return Some(format!("{MAX_FAILURES} actions have failed in this task, so it was stopped. Explain to the user what went wrong."));
        }
        None
    }

    pub fn set_observation(&self, o: Observation) {
        let mut g = self.lock();
        g.target_window = o.window.as_ref().map(|w| w.id).or(g.target_window);
        g.observation = Some(o);
        g.actions_since_observe = 0;
        g.stale = false;
        let d = g.observation.as_ref().map(|o| o.display.clone());
        if let (Some(t), Some(d)) = (g.task.as_mut(), d) {
            t.display = Some(d);
        }
        drop(g);
        self.notify();
    }

    pub fn observation(&self) -> Option<Observation> {
        self.lock().observation.clone()
    }

    pub fn set_target_window(&self, id: u64) {
        self.lock().target_window = Some(id);
    }

    pub fn pause(&self, reason: &str) {
        let paused = self.update(|t, _| {
            if t.state.advance(TaskState::Paused).is_err() {
                return;
            }
            t.phase = Phase::Paused;
            t.pause_reason = Some(reason.to_string());
            t.status = "PAUSED".into();
        });
        if paused.is_some() {
            tracing::info!(event = "OPERATOR_PAUSED");
        }
    }

    /// Continue a paused task; IGRIS re-focuses the window it was working in.
    pub fn resume(&self) -> bool {
        let mut target = None;
        let resumed = self.update(|t, g| {
            if t.state != TaskState::Paused || t.state.advance(TaskState::Executing).is_err() {
                return;
            }
            t.phase = Phase::Executing;
            t.pause_reason = None;
            t.status = "RESUMING".into();
            g.last_input = Instant::now();
            target = g.target_window;
        });
        if let Some(id) = target {
            let _ = self.driver.focus_window(id);
            self.mark_input();
        }
        resumed.is_some_and(|t| t.state == TaskState::Executing)
    }

    /// Stop immediately and return control to the user.
    pub fn stop(&self, reason: &str) -> bool {
        let active = self.is_active();
        if active {
            self.finish_with(TaskState::Cancelled, None, Some(reason.to_string()));
            tracing::info!(event = "OPERATOR_STOPPED");
        }
        active
    }

    /// IGRIS reports the outcome. "Completed" requires a look at the screen
    /// after the last action, so success is never claimed blind.
    pub fn finish(&self, outcome: TaskState, summary: &str) -> Result<TaskView, String> {
        let completed = outcome == TaskState::Completed;
        {
            let g = self.lock();
            let t = g.task.as_ref().filter(|t| !t.state.is_final()).ok_or("No operator task is running.")?;
            if completed && g.actions_since_observe > 0 && t.steps > 0 {
                return Err("Verify the result first: call computer_observe after your last action, check it shows the expected outcome, then finish.".into());
            }
            if completed && t.state == TaskState::Paused {
                return Err("The task is paused. Wait for the user to resume it, observe again, then finish.".into());
            }
        }
        let (state, result, error) = match outcome {
            TaskState::Completed => (TaskState::Completed, Some(summary.to_string()), None),
            TaskState::Ended => (TaskState::Ended, Some(summary.to_string()), None),
            _ => (TaskState::Failed, None, Some(summary.to_string())),
        };
        self.finish_with(state, result, error).ok_or_else(|| "No operator task is running.".to_string())
    }

    fn finish_with(&self, state: TaskState, result: Option<String>, error: Option<String>) -> Option<TaskView> {
        let t = self.update(|t, g| {
            if t.state.is_final() {
                return;
            }
            // Completion is reached through verification (the observation after the last action).
            if state == TaskState::Completed {
                let _ = t.state.advance(TaskState::Verifying);
            }
            if let Err(e) = t.state.advance(state) {
                tracing::warn!(event = "TASK_TRANSITION_REFUSED", error = %e);
                return;
            }
            t.phase = match state {
                TaskState::Completed => Phase::Success,
                TaskState::Cancelled => Phase::Stopped,
                TaskState::Ended => Phase::Waiting,
                _ => Phase::Error,
            };
            t.status = match state {
                TaskState::Completed => "COMPLETE",
                TaskState::Cancelled => "STOPPED",
                TaskState::Ended => "OVER TO YOU",
                _ => "FAILED",
            }
            .into();
            t.result = result;
            t.error = error;
            t.pause_reason = None;
            g.observation = None;
        })?;
        if !t.state.is_final() {
            return None;
        }
        tracing::info!(event = "OPERATOR_ENDED", state = t.state.as_str(), steps = t.steps, retries = t.retries);
        Some(t)
    }

    /// The chat turn that owned the task ended: control always returns to the
    /// user. A task IGRIS didn't report on counts as not completed.
    pub fn end_turn(&self, conversation_id: &str) {
        if self.covers(Some(conversation_id)) {
            // Not a claimed success; a failure only if the last action was never checked.
            let unchecked = self.lock().actions_since_observe > 0;
            if unchecked {
                self.finish_with(TaskState::Failed, None, Some("The reply ended before IGRIS confirmed the result.".into()));
            } else {
                self.finish_with(TaskState::Ended, Some("The reply ended without a final report.".into()), None);
            }
        }
    }

    /// Recent operator tasks, newest first (for recall and the UI). General tasks share the table.
    pub fn history(&self, limit: u32) -> Vec<TaskView> {
        let Ok(conn) = self.db.conn() else { return Vec::new() };
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, conversation_id, objective, plan, state, steps, retries, result, error FROM operator_tasks WHERE kind = 'operator' ORDER BY created_at DESC, rowid DESC LIMIT ?1",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map([limit], |r| {
            let state: String = r.get(4)?;
            let plan: String = r.get(3)?;
            let state = TaskState::parse(&state).unwrap_or(TaskState::Failed);
            Ok(TaskView {
                id: r.get(0)?,
                conversation_id: r.get(1)?,
                objective: r.get(2)?,
                plan: decode_plan(&plan).into_iter().map(|p| p.title).collect(),
                state,
                phase: Phase::Executing,
                status: String::new(),
                steps: r.get(5)?,
                retries: r.get(6)?,
                result: r.get(7)?,
                error: r.get(8)?,
                pause_reason: None,
                display: None,
            })
        });
        rows.map(|r| r.filter_map(Result::ok).collect()).unwrap_or_default()
    }

    /// Whether IGRIS is injecting input right now.
    pub fn is_injecting(&self) -> bool {
        self.injecting.load(Ordering::SeqCst)
    }
}

/// Marks IGRIS's own input for the duration of a scope.
pub struct Injecting<'a>(&'a Operator);

impl<'a> Injecting<'a> {
    pub fn new(op: &'a Operator) -> Self {
        op.injecting.store(true, Ordering::SeqCst);
        Self(op)
    }
}

impl Drop for Injecting<'_> {
    fn drop(&mut self) {
        self.0.mark_input();
        self.0.injecting.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer::fake::{window, FakeDriver};

    fn op() -> (Arc<Operator>, Arc<FakeDriver>) {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let driver = Arc::new(FakeDriver::with(vec![window(1, "Notepad", "notepad.exe")], vec![]));
        (Arc::new(Operator::new(db, driver.clone())), driver)
    }

    #[tokio::test]
    async fn lifecycle_is_recorded_and_broadcast() {
        let (op, _) = op();
        let c1 = crate::conversations::create(&op.db.conn().unwrap(), "t", "s", &[]).unwrap().id;
        let seen = Arc::new(Mutex::new(Vec::<(bool, Option<TaskState>)>::new()));
        let s2 = seen.clone();
        op.on_change(Arc::new(move |s: &Snapshot| s2.lock().unwrap().push((s.active, s.task.as_ref().map(|t| t.state)))));
        assert!(op.checkpoint().await.is_err(), "no task, no actions");
        let t = op.start(None, Some(c1.as_str()), "Open Notepad", vec!["open".into()]).unwrap();
        assert!(op.covers(Some(c1.as_str())));
        assert!(!op.covers(Some("c2")));
        assert!(op.start(None, Some("c2"), "x", vec![]).unwrap_err().contains("another chat"));
        op.checkpoint().await.unwrap();
        op.action_done("Opening Notepad").unwrap();
        assert_eq!(op.snapshot().task.unwrap().status, "OPENING NOTEPAD");
        assert!(op.finish(TaskState::Completed, "done").unwrap_err().contains("Verify"), "can't claim success without looking");
        op.set_observation(Observation { window: None, display: op.driver.displays().unwrap()[0].clone(), image: None, elements: vec![] });
        let done = op.finish(TaskState::Completed, "Notepad is open").unwrap();
        assert_eq!(done.state, TaskState::Completed);
        assert!(!op.is_active());
        assert!(op.checkpoint().await.is_err());
        let h = op.history(5);
        assert_eq!((h[0].id.as_str(), h[0].state, h[0].steps), (t.id.as_str(), TaskState::Completed, 1));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.first().unwrap(), &(true, Some(TaskState::Executing)));
        assert_eq!(seen.last().unwrap(), &(false, Some(TaskState::Completed)));
    }

    #[tokio::test]
    async fn stop_pause_resume_and_user_takeover() {
        let (op, driver) = op();
        op.start(None, Some("c1"), "task", vec![]).unwrap();
        // The user used the computer in the same window → keep going, but look again first.
        op.set_target_window(1);
        op.mark_input();
        tokio::time::sleep(Duration::from_millis(500)).await;
        driver.0.lock().unwrap().idle_ms = Some(0);
        op.checkpoint().await.unwrap();
        assert!(op.is_stale());
        assert_eq!(op.snapshot().task.unwrap().state, TaskState::Executing);
        // …and switched to another window → paused.
        driver.0.lock().unwrap().windows.push(window(2, "Mail", "mail.exe"));
        driver.0.lock().unwrap().foreground = Some(2);
        tokio::time::sleep(Duration::from_millis(500)).await;
        let op2 = op.clone();
        let waiter = tokio::spawn(async move { op2.checkpoint().await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let t = op.snapshot().task.unwrap();
        assert_eq!(t.state, TaskState::Paused);
        assert!(t.pause_reason.unwrap().contains("another window"));
        assert!(!waiter.is_finished(), "actions wait while paused");
        driver.0.lock().unwrap().idle_ms = Some(10_000);
        assert!(op.resume());
        assert!(waiter.await.unwrap().is_ok(), "resumes after the user says so");

        // Stop while an action is waiting on a pause.
        op.pause("manual");
        let op2 = op.clone();
        let waiter = tokio::spawn(async move { op2.checkpoint().await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(op.stop("Stopped with Esc"));
        assert!(waiter.await.unwrap().unwrap_err().contains("stopped"));
        let t = op.snapshot().task.unwrap();
        assert_eq!((t.state, t.phase), (TaskState::Cancelled, Phase::Stopped));
        assert!(!op.stop("again"), "nothing left to stop");
    }

    #[test]
    fn failures_and_turn_end_are_not_success() {
        let (op, _) = op();
        op.start(None, Some("c1"), "task", vec![]).unwrap();
        for i in 1..MAX_FAILURES {
            assert!(op.action_failed("click missed").is_none(), "{i}");
        }
        assert!(op.action_failed("click missed").unwrap().contains("stopped"));
        assert_eq!(op.snapshot().task.unwrap().state, TaskState::Failed);

        // The reply ended right after an unchecked action: not a success.
        op.start(None, Some("c1"), "task 2", vec![]).unwrap();
        op.action_done("Clicking").unwrap();
        op.end_turn("c1");
        let t = op.snapshot().task.unwrap();
        assert_eq!(t.state, TaskState::Failed);
        assert!(t.error.unwrap().contains("before IGRIS confirmed"));

        // Ended after looking, just without a report: handed back, neither success nor failure.
        op.start(None, Some("c1"), "task 3", vec![]).unwrap();
        op.end_turn("c1");
        let t = op.snapshot().task.unwrap();
        assert_eq!((t.state, t.phase, t.status.as_str()), (TaskState::Ended, Phase::Waiting, "OVER TO YOU"));
        assert!(!op.is_active());
    }
}
