//! Task persistence (the `operator_tasks` table, generalized in migration 10).
//!
//! Operator mode writes its own columns (state, steps, retries, result, error)
//! for the task it runs; the orchestrator writes the rest and, for general
//! tasks, the action count. Only bounded task state is stored — tool output
//! stays in the conversation.

use rusqlite::{params, Connection, OptionalExtension};

use super::task::{decode_plan, Task, TaskContext, TaskKind, TaskProject, TaskState};
use crate::error::AppResult;

const COLS: &str = "t.id, t.conversation_id, t.kind, t.objective, t.state, t.plan, t.current_step, t.steps, t.failures, t.result, t.error, \
t.pause_reason, t.context, t.project_id, p.name, t.created_at, COALESCE(t.updated_at, t.ended_at, t.created_at)";
const FROM: &str = "operator_tasks t LEFT JOIN projects p ON p.id = t.project_id";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let kind: String = r.get(2)?;
    let state: String = r.get(4)?;
    let plan: String = r.get(5)?;
    let context: String = r.get(12)?;
    let project = match (r.get::<_, Option<i64>>(13)?, r.get::<_, Option<String>>(14)?) {
        (Some(id), Some(name)) => Some(TaskProject { id, name }),
        _ => None,
    };
    Ok(Task {
        id: r.get(0)?,
        conversation_id: r.get(1)?,
        kind: if kind == "general" { TaskKind::General } else { TaskKind::Operator },
        objective: r.get(3)?,
        // Unknown states (a newer version's) read as failed rather than live.
        state: TaskState::parse(&state).unwrap_or(TaskState::Failed),
        plan: decode_plan(&plan),
        current_step: r.get::<_, Option<i64>>(6)?.map(|v| v as usize),
        activity: None,
        steps: r.get(7)?,
        failures: r.get(8)?,
        result: r.get(9)?,
        error: r.get(10)?,
        pause_reason: r.get(11)?,
        context: serde_json::from_str::<TaskContext>(&context).unwrap_or_default(),
        project,
        created_at: r.get(15)?,
        updated_at: r.get(16)?,
    })
}

/// Insert or update a task.
pub fn save(conn: &Connection, t: &Task) -> AppResult<()> {
    let plan = serde_json::to_string(&t.plan).unwrap_or_else(|_| "[]".into());
    let context = serde_json::to_string(&t.context).unwrap_or_else(|_| "{}".into());
    conn.execute(
        "INSERT INTO operator_tasks (id, conversation_id, objective, plan, state, steps, result, error, kind, current_step, failures, context,
                                     project_id, pause_reason, created_at, updated_at, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, CASE WHEN ?17 THEN ?16 END)
         ON CONFLICT(id) DO UPDATE SET objective = ?3, plan = ?4, state = ?5,
             steps = CASE WHEN ?9 = 'operator' THEN steps ELSE ?6 END,
             result = ?7, error = ?8, kind = ?9, current_step = ?10, failures = ?11, context = ?12, project_id = ?13,
             pause_reason = ?14, updated_at = ?16, ended_at = CASE WHEN ?17 THEN COALESCE(ended_at, ?16) END",
        params![
            t.id,
            t.conversation_id,
            t.objective,
            plan,
            t.state.as_str(),
            t.steps,
            t.result,
            t.error,
            t.kind.as_str(),
            t.current_step.map(|v| v as i64),
            t.failures,
            context,
            t.project.as_ref().map(|p| p.id),
            t.pause_reason,
            t.created_at,
            t.updated_at,
            t.state.is_final(),
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Option<Task>> {
    Ok(conn.query_row(&format!("SELECT {COLS} FROM {FROM} WHERE t.id = ?1"), [id], row).optional()?)
}

/// Tasks of a conversation, newest first.
pub fn for_conversation(conn: &Connection, conversation_id: &str, limit: u32) -> AppResult<Vec<Task>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM {FROM} WHERE t.conversation_id = ?1 ORDER BY t.created_at DESC, t.rowid DESC LIMIT ?2"))?;
    let rows = stmt.query_map(params![conversation_id, limit], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Recent tasks across conversations, newest first.
pub fn recent(conn: &Connection, limit: u32) -> AppResult<Vec<Task>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM {FROM} ORDER BY t.created_at DESC, t.rowid DESC LIMIT ?1"))?;
    let rows = stmt.query_map([limit], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// After a restart: tasks that were running when IGRIS closed. They are
/// paused and marked interrupted — never resumed automatically, because the
/// computer may have changed in the meantime.
pub fn recover_interrupted(conn: &Connection) -> AppResult<Vec<Task>> {
    let live: Vec<String> = TaskState::ALL.iter().filter(|s| !s.is_final()).map(|s| format!("'{}'", s.as_str())).collect();
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM {FROM} WHERE t.state IN ({}, 'waiting_for_permission')", live.join(", ")))?;
    let tasks: Vec<Task> = stmt.query_map([], row)?.collect::<Result<_, _>>()?;
    let mut recovered = Vec::new();
    for mut t in tasks {
        if t.state != TaskState::Paused && t.state.advance(TaskState::Paused).is_err() {
            // e.g. Created: nothing ran yet, so there is nothing to resume.
            t.state = TaskState::Cancelled;
            t.error = Some("IGRIS closed before this task started.".into());
        } else {
            t.pause_reason = Some("IGRIS was closed while this task was running. Check the current state before resuming.".into());
            t.context.interrupted = true;
        }
        t.updated_at = super::task::now();
        save(conn, &t)?;
        recovered.push(t);
    }
    Ok(recovered)
}

/// Activity entries kept per task (older ones are pruned).
pub const MAX_EVENTS_PER_TASK: i64 = 200;

/// One entry of a task's activity log.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEventRow {
    pub at: String,
    pub kind: String,
    pub tool: Option<String>,
    pub detail: Option<String>,
    pub state: String,
}

/// Append to a task's activity log (details are clipped and redacted).
pub fn record_event(conn: &Connection, task_id: &str, kind: &str, tool: Option<&str>, detail: Option<&str>, state: TaskState) -> AppResult<()> {
    let detail = detail.filter(|d| !d.trim().is_empty()).map(crate::tools::audit::redact);
    let id = conn.query_row(
        "INSERT INTO task_events (task_id, kind, tool, detail, state) VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
        params![task_id, kind, tool, detail, state.as_str()],
        |r| r.get::<_, i64>(0),
    )?;
    if id % 25 == 0 {
        conn.execute(
            "DELETE FROM task_events WHERE task_id = ?1 AND id <= (SELECT id FROM task_events WHERE task_id = ?1 ORDER BY id DESC LIMIT 1 OFFSET ?2)",
            params![task_id, MAX_EVENTS_PER_TASK],
        )?;
    }
    Ok(())
}

/// The newest `limit` entries of a task's activity, oldest first.
pub fn events(conn: &Connection, task_id: &str, limit: u32) -> AppResult<Vec<TaskEventRow>> {
    let mut stmt =
        conn.prepare("SELECT at, kind, tool, detail, state FROM (SELECT * FROM task_events WHERE task_id = ?1 ORDER BY id DESC LIMIT ?2) ORDER BY id")?;
    let rows =
        stmt.query_map(params![task_id, limit], |r| Ok(TaskEventRow { at: r.get(0)?, kind: r.get(1)?, tool: r.get(2)?, detail: r.get(3)?, state: r.get(4)? }))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// A tool that started and never reported back (IGRIS closed mid-action), with what it was doing.
pub fn unfinished_action(conn: &Connection, task_id: &str) -> AppResult<Option<(String, Option<String>)>> {
    let mut open: Option<(String, Option<String>)> = None;
    for e in events(conn, task_id, MAX_EVENTS_PER_TASK as u32)? {
        match (e.kind.as_str(), e.tool) {
            ("tool_started", Some(t)) => open = Some((t, e.detail)),
            ("tool_completed" | "interrupted", _) => open = None,
            _ => {}
        }
    }
    Ok(open)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::orchestrator::task::{ActionRecord, PlanStep, StepStatus, Verification};

    fn conn_with_conversation() -> (Database, String) {
        let db = Database::open_in_memory().unwrap();
        let id = crate::conversations::create(&db.conn().unwrap(), "t", "s", &[]).unwrap().id;
        (db, id)
    }

    #[test]
    fn tasks_round_trip_with_plan_context_and_project() {
        let (db, cid) = conn_with_conversation();
        let conn = db.conn().unwrap();
        let project = crate::projects::add(
            &conn,
            &crate::projects::ProjectInput { name: "IGRIS".into(), path: std::env::temp_dir().to_string_lossy().into_owned(), ..Default::default() },
        )
        .unwrap();
        let mut t = Task::new(Some(&cid), "Write notes");
        t.state = TaskState::Executing;
        t.set_plan(vec![PlanStep { title: "Write".into(), status: StepStatus::Active }, PlanStep::pending("Check")]);
        t.steps = 2;
        t.failures = 1;
        t.project = Some(TaskProject { id: project.id, name: String::new() });
        t.context.record(ActionRecord {
            tool: "write_file".into(),
            target: "notes.txt".into(),
            ok: true,
            verification: Verification::Passed,
            note: "ok".into(),
        });
        save(&conn, &t).unwrap();
        let back = get(&conn, &t.id).unwrap().unwrap();
        assert_eq!((back.state, back.kind, back.steps, back.failures, back.current_step), (TaskState::Executing, TaskKind::General, 2, 1, Some(0)));
        assert_eq!(back.plan, t.plan);
        assert_eq!(back.context, t.context);
        assert_eq!(back.project.as_ref().unwrap().name, "IGRIS");

        // Completed tasks get an end time; listing is per conversation, newest first.
        let mut done = back.clone();
        done.state = TaskState::Completed;
        save(&conn, &done).unwrap();
        let ended: Option<String> = conn.query_row("SELECT ended_at FROM operator_tasks WHERE id = ?1", [&t.id], |r| r.get(0)).unwrap();
        assert!(ended.is_some());
        let second = Task::new(Some(&cid), "Second");
        save(&conn, &second).unwrap();
        let list = for_conversation(&conn, &cid, 10).unwrap();
        assert_eq!(list.iter().map(|t| t.objective.as_str()).collect::<Vec<_>>().len(), 2);
        assert_eq!(recent(&conn, 1).unwrap().len(), 1);
    }

    #[test]
    fn interrupted_tasks_come_back_paused_and_never_running() {
        let (db, cid) = conn_with_conversation();
        let conn = db.conn().unwrap();
        let mk = |state: TaskState, objective: &str| {
            let mut t = Task::new(Some(&cid), objective);
            t.state = state;
            save(&conn, &t).unwrap();
            t.id
        };
        let running = mk(TaskState::Executing, "running");
        let waiting = mk(TaskState::WaitingForApproval, "waiting");
        let created = mk(TaskState::Created, "created");
        let done = mk(TaskState::Completed, "done");
        // A pre-Phase-3 operator row that was mid-task.
        conn.execute("INSERT INTO operator_tasks (id, objective, plan, state) VALUES ('legacy', 'old', '[\"a\"]', 'executing')", []).unwrap();

        let recovered = recover_interrupted(&conn).unwrap();
        assert_eq!(recovered.len(), 4);
        for id in [&running, &waiting, &"legacy".to_string()] {
            let t = get(&conn, id).unwrap().unwrap();
            assert_eq!(t.state, TaskState::Paused, "{id}");
            assert!(t.context.interrupted && t.pause_reason.unwrap().contains("closed"));
        }
        assert_eq!(get(&conn, "legacy").unwrap().unwrap().kind, TaskKind::Operator);
        assert_eq!(get(&conn, "legacy").unwrap().unwrap().plan, vec![PlanStep::pending("a")]);
        assert_eq!(get(&conn, &created).unwrap().unwrap().state, TaskState::Cancelled);
        assert_eq!(get(&conn, &done).unwrap().unwrap().state, TaskState::Completed);
        // Paused-and-interrupted tasks stay that way on the next start.
        assert_eq!(recover_interrupted(&conn).unwrap().len(), 3);
        assert_eq!(get(&conn, &running).unwrap().unwrap().state, TaskState::Paused);
    }

    #[test]
    fn activity_is_logged_bounded_redacted_and_shows_unfinished_actions() {
        let (db, cid) = conn_with_conversation();
        let conn = db.conn().unwrap();
        let t = Task::new(Some(&cid), "x");
        save(&conn, &t).unwrap();
        record_event(&conn, &t.id, "tool_started", Some("write_file"), Some("Write notes.txt"), TaskState::Executing).unwrap();
        assert_eq!(unfinished_action(&conn, &t.id).unwrap(), Some(("write_file".to_string(), Some("Write notes.txt".to_string()))));
        record_event(&conn, &t.id, "tool_completed", Some("write_file"), None, TaskState::Executing).unwrap();
        assert_eq!(unfinished_action(&conn, &t.id).unwrap(), None);
        record_event(&conn, &t.id, "tool_started", Some("computer_type"), Some("Type sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789"), TaskState::Executing)
            .unwrap();
        let ev = events(&conn, &t.id, 10).unwrap();
        assert_eq!(ev.len(), 3);
        assert!(!ev[2].detail.as_deref().unwrap().contains("sk-ant"), "secrets never reach the activity log: {:?}", ev[2].detail);
        for i in 0..(MAX_EVENTS_PER_TASK + 60) {
            record_event(&conn, &t.id, "observation_received", Some("read_file"), Some(&i.to_string()), TaskState::Executing).unwrap();
        }
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM task_events WHERE task_id = ?1", [&t.id], |r| r.get(0)).unwrap();
        assert!(n <= MAX_EVENTS_PER_TASK + 25, "bounded: {n}");
    }
}
