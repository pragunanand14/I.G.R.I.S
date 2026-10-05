//! To-do tasks.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use super::{clean, time};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub notes: String,
    /// UTC RFC 3339, or None.
    pub due_at: Option<String>,
    pub priority: String,
    pub done: bool,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskInput {
    pub title: String,
    #[serde(default)]
    pub notes: String,
    /// UTC RFC 3339 or empty for no due date.
    #[serde(default)]
    pub due_at: String,
    #[serde(default)]
    pub priority: String,
}

pub const PRIORITIES: [&str; 3] = ["low", "normal", "high"];
const COLS: &str = "id, title, notes, due_at, priority, done, completed_at, created_at, updated_at";

fn row(r: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        title: r.get(1)?,
        notes: r.get(2)?,
        due_at: r.get(3)?,
        priority: r.get(4)?,
        done: r.get(5)?,
        completed_at: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

struct Valid {
    title: String,
    notes: String,
    due_at: Option<String>,
    priority: String,
}

fn validate(input: &TaskInput) -> AppResult<Valid> {
    let title = clean(&input.title, 200, "Title")?;
    if title.is_empty() {
        return Err(AppError::validation("Give the task a title."));
    }
    let due_at = match input.due_at.trim() {
        "" => None,
        s => Some(time::to_db(time::from_db(s).ok_or_else(|| AppError::validation("The due date isn't a valid time."))?)),
    };
    let priority = match input.priority.trim() {
        "" => "normal".to_string(),
        p if PRIORITIES.contains(&p) => p.to_string(),
        _ => return Err(AppError::validation("Priority must be low, normal or high.")),
    };
    Ok(Valid { title, notes: clean(&input.notes, 4000, "Notes")?, due_at, priority })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Open,
    Done,
    All,
}

/// Open tasks first by due date (undated last) then priority; done tasks newest first.
pub fn list(conn: &Connection, filter: Filter) -> AppResult<Vec<Task>> {
    let cond = match filter {
        Filter::Open => "WHERE done = 0",
        Filter::Done => "WHERE done = 1",
        Filter::All => "",
    };
    let sql = format!(
        "SELECT {COLS} FROM tasks {cond} ORDER BY done, \
         CASE WHEN done = 1 THEN completed_at END DESC, \
         due_at IS NULL, due_at, \
         CASE priority WHEN 'high' THEN 0 WHEN 'normal' THEN 1 ELSE 2 END, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<Option<Task>> {
    Ok(conn.query_row(&format!("SELECT {COLS} FROM tasks WHERE id = ?1"), [id], row).optional()?)
}

fn must_get(conn: &Connection, id: i64) -> AppResult<Task> {
    get(conn, id)?.ok_or_else(|| AppError::validation(format!("There's no task #{id}.")))
}

pub fn add(conn: &Connection, input: &TaskInput) -> AppResult<Task> {
    let v = validate(input)?;
    conn.execute("INSERT INTO tasks (title, notes, due_at, priority) VALUES (?1, ?2, ?3, ?4)", params![v.title, v.notes, v.due_at, v.priority])?;
    must_get(conn, conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, input: &TaskInput) -> AppResult<Task> {
    let v = validate(input)?;
    let n = conn.execute(
        "UPDATE tasks SET title = ?1, notes = ?2, due_at = ?3, priority = ?4, updated_at = ?5 WHERE id = ?6",
        params![v.title, v.notes, v.due_at, v.priority, time::now_db(), id],
    )?;
    if n == 0 {
        return Err(AppError::validation(format!("There's no task #{id}.")));
    }
    must_get(conn, id)
}

pub fn set_done(conn: &Connection, id: i64, done: bool) -> AppResult<Task> {
    let now = time::now_db();
    let n = conn.execute("UPDATE tasks SET done = ?1, completed_at = CASE WHEN ?1 THEN ?2 END, updated_at = ?2 WHERE id = ?3", params![done, now, id])?;
    if n == 0 {
        return Err(AppError::validation(format!("There's no task #{id}.")));
    }
    must_get(conn, id)
}

pub fn remove(conn: &Connection, id: i64) -> AppResult<Task> {
    let t = must_get(conn, id)?;
    conn.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
    Ok(t)
}

pub fn clear_done(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute("DELETE FROM tasks WHERE done = 1", [])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn input(title: &str, due: &str, priority: &str) -> TaskInput {
        TaskInput { title: title.into(), notes: String::new(), due_at: due.into(), priority: priority.into() }
    }

    #[test]
    fn crud_and_ordering() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        let undated = add(&c, &input("Undated", "", "high")).unwrap();
        let later = add(&c, &input("Later", "2026-10-09T10:00:00Z", "")).unwrap();
        let sooner = add(&c, &input("Sooner", "2026-10-06T08:00:00+02:00", "low")).unwrap();
        assert_eq!(later.priority, "normal");
        assert_eq!(sooner.due_at.as_deref(), Some("2026-10-06T06:00:00Z"));
        let order: Vec<_> = list(&c, Filter::Open).unwrap().into_iter().map(|t| t.title).collect();
        assert_eq!(order, ["Sooner", "Later", "Undated"]);

        let done = set_done(&c, later.id, true).unwrap();
        assert!(done.done && done.completed_at.is_some());
        assert_eq!(list(&c, Filter::Open).unwrap().len(), 2);
        assert_eq!(list(&c, Filter::Done).unwrap()[0].id, later.id);
        let reopened = set_done(&c, later.id, false).unwrap();
        assert!(!reopened.done && reopened.completed_at.is_none());

        let edited = update(&c, undated.id, &input("Renamed", "", "low")).unwrap();
        assert_eq!((edited.title.as_str(), edited.priority.as_str()), ("Renamed", "low"));
        remove(&c, undated.id).unwrap();
        assert!(get(&c, undated.id).unwrap().is_none());
        set_done(&c, sooner.id, true).unwrap();
        assert_eq!(clear_done(&c).unwrap(), 1);
        assert_eq!(list(&c, Filter::All).unwrap().len(), 1);
    }

    #[test]
    fn validation() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        assert!(add(&c, &input("  ", "", "")).is_err());
        assert!(add(&c, &input("x", "tomorrow", "")).is_err());
        assert!(add(&c, &input("x", "", "urgent")).is_err());
        assert!(update(&c, 99, &input("x", "", "")).is_err());
        assert!(set_done(&c, 99, true).is_err());
    }
}
