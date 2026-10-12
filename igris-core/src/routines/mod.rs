//! Routines: things IGRIS does by itself — on a schedule ("every morning at
//! 8, tell me the weather and my meetings"), or when an email from someone
//! arrives. A routine is a plain-words instruction plus a trigger; each run is
//! an ordinary conversation ("Routine: Morning"), so it uses the same tools,
//! the same permission checks and the same "Always allow" rules as a chat.
//! Anything that needs an OK the user hasn't given ahead of time waits for
//! them (they get a notification). Routines run while IGRIS is running on
//! that device.

pub mod schedule;
mod scheduler;

pub use schedule::Schedule;
pub use scheduler::{email_tick, run, run_one, tick, Runner};

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::productivity::time;

/// At most this many routines (a runaway loop can't flood the scheduler).
const MAX_ROUTINES: i64 = 100;
const MAX_RUNS_KEPT: i64 = 50;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    pub id: i64,
    pub name: String,
    pub instruction: String,
    pub schedule: Schedule,
    /// The schedule in plain words ("Every weekday at 08:00").
    pub when: String,
    pub enabled: bool,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: i64,
    pub routine_id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// running, done, failed, skipped
    pub status: String,
    pub summary: Option<String>,
    pub conversation_id: Option<String>,
}

const COLS: &str = "id, name, instruction, schedule, enabled, next_run_at, last_run_at, created_at";

fn row(r: &Row) -> rusqlite::Result<Routine> {
    let raw: String = r.get(3)?;
    let schedule: Schedule = serde_json::from_str(&raw).unwrap_or_default();
    Ok(Routine {
        id: r.get(0)?,
        name: r.get(1)?,
        instruction: r.get(2)?,
        when: schedule.describe(),
        schedule,
        enabled: r.get::<_, i64>(4)? != 0,
        next_run_at: r.get(5)?,
        last_run_at: r.get(6)?,
        created_at: r.get(7)?,
    })
}

fn clean(s: &str, max: usize, what: &str) -> AppResult<String> {
    let s: String = s.chars().filter(|c| !c.is_control() || *c == '\n').collect::<String>().trim().to_string();
    if s.is_empty() {
        return Err(AppError::validation(format!("The routine needs a {what}.")));
    }
    if s.chars().count() > max {
        return Err(AppError::validation(format!("The {what} can be at most {max} characters.")));
    }
    Ok(s)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<Option<Routine>> {
    Ok(conn.query_row(&format!("SELECT {COLS} FROM routines WHERE id = ?1"), [id], row).optional()?)
}

fn must_get(conn: &Connection, id: i64) -> AppResult<Routine> {
    get(conn, id)?.ok_or_else(|| AppError::validation(format!("There's no routine #{id}.")))
}

pub fn list(conn: &Connection) -> AppResult<Vec<Routine>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM routines ORDER BY created_at"))?;
    let rows = stmt.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn create(conn: &Connection, name: &str, instruction: &str, schedule: Schedule) -> AppResult<Routine> {
    let name = clean(name, 60, "name")?;
    let instruction = clean(instruction, 2000, "instruction")?;
    schedule.validate().map_err(AppError::validation)?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM routines", [], |r| r.get(0))?;
    if count >= MAX_ROUTINES {
        return Err(AppError::validation("That's a lot of routines — delete some first."));
    }
    let next = schedule.next_after(chrono::Utc::now()).map(time::to_db);
    if next.is_none() && !schedule.is_event() {
        return Err(AppError::validation("That time has already passed."));
    }
    conn.execute(
        "INSERT INTO routines (name, instruction, schedule, next_run_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![name, instruction, serde_json::to_string(&schedule)?, next, time::now_db()],
    )?;
    tracing::info!(event = "ROUTINE_CREATED", kind = schedule.kind.as_str());
    must_get(conn, conn.last_insert_rowid())
}

/// Turn on or off; turning on schedules the next run from now.
pub fn set_enabled(conn: &Connection, id: i64, enabled: bool) -> AppResult<Routine> {
    let r = must_get(conn, id)?;
    let next = if enabled { r.schedule.next_after(chrono::Utc::now()).map(time::to_db) } else { None };
    if enabled && next.is_none() && !r.schedule.is_event() {
        return Err(AppError::validation("That one-off time has already passed — give it a new time to turn it back on."));
    }
    conn.execute("UPDATE routines SET enabled = ?1, next_run_at = ?2 WHERE id = ?3", params![enabled as i64, next, id])?;
    must_get(conn, id)
}

pub fn update(conn: &Connection, id: i64, name: Option<&str>, instruction: Option<&str>, schedule: Option<Schedule>) -> AppResult<Routine> {
    let r = must_get(conn, id)?;
    let name = name.map(|n| clean(n, 60, "name")).transpose()?.unwrap_or(r.name);
    let instruction = instruction.map(|i| clean(i, 2000, "instruction")).transpose()?.unwrap_or(r.instruction);
    let schedule = match schedule {
        Some(s) => {
            s.validate().map_err(AppError::validation)?;
            s
        }
        None => r.schedule,
    };
    let next = if r.enabled { schedule.next_after(chrono::Utc::now()).map(time::to_db) } else { None };
    if r.enabled && next.is_none() && !schedule.is_event() {
        return Err(AppError::validation("That time has already passed."));
    }
    conn.execute(
        "UPDATE routines SET name = ?1, instruction = ?2, schedule = ?3, next_run_at = ?4, email_seen = NULL WHERE id = ?5",
        params![name, instruction, serde_json::to_string(&schedule)?, next, id],
    )?;
    must_get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> AppResult<Routine> {
    let r = must_get(conn, id)?;
    conn.execute("DELETE FROM routines WHERE id = ?1", [id])?;
    Ok(r)
}

/// Recent runs, newest first (all routines when `routine` is None).
pub fn runs(conn: &Connection, routine: Option<i64>, limit: u32) -> AppResult<Vec<Run>> {
    let mut stmt = conn.prepare(
        "SELECT id, routine_id, started_at, finished_at, status, summary, conversation_id FROM routine_runs \
         WHERE ?1 IS NULL OR routine_id = ?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![routine, limit], |r| {
            Ok(Run {
                id: r.get(0)?,
                routine_id: r.get(1)?,
                started_at: r.get(2)?,
                finished_at: r.get(3)?,
                status: r.get(4)?,
                summary: r.get(5)?,
                conversation_id: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn start_run(conn: &Connection, routine: i64) -> AppResult<i64> {
    conn.execute("INSERT INTO routine_runs (routine_id, started_at, status) VALUES (?1, ?2, 'running')", params![routine, time::now_db()])?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "DELETE FROM routine_runs WHERE routine_id = ?1 AND id NOT IN (SELECT id FROM routine_runs WHERE routine_id = ?1 ORDER BY id DESC LIMIT ?2)",
        params![routine, MAX_RUNS_KEPT],
    )?;
    Ok(id)
}

fn finish_run(conn: &Connection, run: i64, status: &str, summary: &str, conversation: Option<&str>) -> AppResult<()> {
    // Long enough for a whole spoken briefing (the Home card and Listen read it).
    let summary: String = summary.chars().take(4000).collect();
    conn.execute(
        "UPDATE routine_runs SET finished_at = ?1, status = ?2, summary = ?3, conversation_id = COALESCE(?4, conversation_id) WHERE id = ?5",
        params![time::now_db(), status, summary, conversation, run],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
