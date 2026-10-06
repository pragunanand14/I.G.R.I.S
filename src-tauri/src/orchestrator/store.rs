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
}
