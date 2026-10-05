//! Reminders and timers. Both are a title plus a due instant; the scheduler
//! fires them (OS notification + in-app alert) and the user dismisses or
//! snoozes them.

use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use super::{clean, time};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub id: i64,
    pub title: String,
    /// "reminder" or "timer".
    pub kind: String,
    pub due_at: String,
    /// Timers: the original length, for display.
    pub duration_secs: Option<i64>,
    /// "pending", "fired", "dismissed" or "cancelled".
    pub status: String,
    pub fired_at: Option<String>,
    pub created_at: String,
}

const COLS: &str = "id, title, kind, due_at, duration_secs, status, fired_at, created_at";
/// Upper bound on active (pending + ringing) reminders, so a runaway loop can't flood the scheduler.
const MAX_ACTIVE: i64 = 500;

fn row(r: &Row) -> rusqlite::Result<Reminder> {
    Ok(Reminder {
        id: r.get(0)?,
        title: r.get(1)?,
        kind: r.get(2)?,
        due_at: r.get(3)?,
        duration_secs: r.get(4)?,
        status: r.get(5)?,
        fired_at: r.get(6)?,
        created_at: r.get(7)?,
    })
}

pub fn get(conn: &Connection, id: i64) -> AppResult<Option<Reminder>> {
    Ok(conn.query_row(&format!("SELECT {COLS} FROM reminders WHERE id = ?1"), [id], row).optional()?)
}

fn must_get(conn: &Connection, id: i64) -> AppResult<Reminder> {
    get(conn, id)?.ok_or_else(|| AppError::validation(format!("There's no reminder or timer #{id}.")))
}

fn insert(conn: &Connection, title: &str, kind: &str, due: chrono::DateTime<Utc>, duration: Option<i64>) -> AppResult<Reminder> {
    let active: i64 = conn.query_row("SELECT COUNT(*) FROM reminders WHERE status IN ('pending', 'fired')", [], |r| r.get(0))?;
    if active >= MAX_ACTIVE {
        return Err(AppError::validation("Too many active reminders and timers — clear some first."));
    }
    conn.execute(
        "INSERT INTO reminders (title, kind, due_at, duration_secs) VALUES (?1, ?2, ?3, ?4)",
        params![title, kind, time::to_db(due), duration],
    )?;
    must_get(conn, conn.last_insert_rowid())
}

/// `when` is natural language or RFC 3339; it must be in the future.
pub fn add_reminder(conn: &Connection, title: &str, when: &str) -> AppResult<Reminder> {
    let title = clean(title, 200, "Reminder")?;
    if title.is_empty() {
        return Err(AppError::validation("Say what to remind you about."));
    }
    let due = time::parse_when(when).map_err(AppError::validation)?;
    insert(conn, &title, "reminder", due, None)
}

pub fn start_timer(conn: &Connection, label: &str, duration: &str) -> AppResult<Reminder> {
    let d = time::parse_duration(duration).map_err(AppError::validation)?;
    let label = clean(label, 120, "Label")?;
    let label = if label.is_empty() { "Timer".to_string() } else { label };
    insert(conn, &label, "timer", Utc::now() + d, Some(d.num_seconds()))
}

/// Pending and ringing items, soonest first.
pub fn active(conn: &Connection) -> AppResult<Vec<Reminder>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM reminders WHERE status IN ('pending', 'fired') ORDER BY status = 'pending', due_at, id"))?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Recently finished items (dismissed / cancelled), newest first.
pub fn history(conn: &Connection, limit: u32) -> AppResult<Vec<Reminder>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM reminders WHERE status IN ('dismissed', 'cancelled') ORDER BY due_at DESC, id DESC LIMIT ?1"))?;
    let rows = stmt.query_map([limit], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Atomically mark every pending item due at or before `now` as fired and return them.
pub fn take_due(conn: &Connection, now: chrono::DateTime<Utc>) -> AppResult<Vec<Reminder>> {
    let now = time::to_db(now);
    let mut stmt = conn.prepare(&format!(
        "UPDATE reminders SET status = 'fired', fired_at = ?1 WHERE status = 'pending' AND due_at <= ?1 RETURNING {COLS}"
    ))?;
    let rows = stmt.query_map([&now], row)?;
    let mut v: Vec<Reminder> = rows.collect::<Result<_, _>>()?;
    v.sort_by(|a, b| a.due_at.cmp(&b.due_at));
    Ok(v)
}

fn transition(conn: &Connection, id: i64, from: &[&str], to: &str, verb: &str) -> AppResult<Reminder> {
    let r = must_get(conn, id)?;
    if !from.contains(&r.status.as_str()) {
        return Err(AppError::validation(format!("That {} is already {} and can't be {verb}.", r.kind, r.status)));
    }
    conn.execute("UPDATE reminders SET status = ?1 WHERE id = ?2", params![to, id])?;
    must_get(conn, id)
}

pub fn cancel(conn: &Connection, id: i64) -> AppResult<Reminder> {
    transition(conn, id, &["pending"], "cancelled", "cancelled")
}

pub fn dismiss(conn: &Connection, id: i64) -> AppResult<Reminder> {
    transition(conn, id, &["fired", "pending"], "dismissed", "dismissed")
}

pub fn snooze(conn: &Connection, id: i64, minutes: i64) -> AppResult<Reminder> {
    if !(1..=24 * 60).contains(&minutes) {
        return Err(AppError::validation("Snooze for between 1 minute and 24 hours."));
    }
    let r = must_get(conn, id)?;
    if r.status != "fired" {
        return Err(AppError::validation("Only a reminder that has gone off can be snoozed."));
    }
    let due = Utc::now() + Duration::minutes(minutes);
    conn.execute("UPDATE reminders SET status = 'pending', due_at = ?1, fired_at = NULL WHERE id = ?2", params![time::to_db(due), id])?;
    must_get(conn, id)
}

pub fn clear_history(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute("DELETE FROM reminders WHERE status IN ('dismissed', 'cancelled')", [])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn lifecycle() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        let r = add_reminder(&c, "Call Riya", "in 10 minutes").unwrap();
        let t = start_timer(&c, "", "90 seconds").unwrap();
        assert_eq!((r.kind.as_str(), r.status.as_str()), ("reminder", "pending"));
        assert_eq!((t.title.as_str(), t.duration_secs), ("Timer", Some(90)));
        assert_eq!(active(&c).unwrap().iter().map(|r| r.id).collect::<Vec<_>>(), [t.id, r.id]);

        assert!(take_due(&c, Utc::now()).unwrap().is_empty());
        let fired = take_due(&c, Utc::now() + Duration::minutes(5)).unwrap();
        assert_eq!(fired.iter().map(|r| r.id).collect::<Vec<_>>(), [t.id]);
        assert_eq!(fired[0].status, "fired");
        // Fires exactly once.
        assert!(take_due(&c, Utc::now() + Duration::minutes(5)).unwrap().is_empty());
        // Ringing items sort first.
        assert_eq!(active(&c).unwrap()[0].id, t.id);

        let snoozed = snooze(&c, t.id, 5).unwrap();
        assert_eq!(snoozed.status, "pending");
        assert!(snooze(&c, t.id, 5).is_err());
        assert!(snooze(&c, r.id, 0).is_err());

        cancel(&c, r.id).unwrap();
        assert!(cancel(&c, r.id).unwrap_err().to_string().contains("already cancelled"));
        dismiss(&c, t.id).unwrap();
        assert!(active(&c).unwrap().is_empty());
        assert_eq!(history(&c, 10).unwrap().len(), 2);
        assert_eq!(clear_history(&c).unwrap(), 2);
    }

    #[test]
    fn validation() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        assert!(add_reminder(&c, "", "in 5 minutes").is_err());
        assert!(add_reminder(&c, "x", "whenever").unwrap_err().to_string().contains("couldn't understand"));
        assert!(add_reminder(&c, "x", "2020-01-01 10:00").unwrap_err().to_string().contains("in the past"));
        assert!(start_timer(&c, "", "ten").is_err());
        assert!(cancel(&c, 42).is_err());
    }
}
