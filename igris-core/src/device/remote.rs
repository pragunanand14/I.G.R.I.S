//! Persistence of cross-device tasks (`remote_tasks`): what this device asked
//! others to do (outgoing) and what others asked of it (incoming).

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use super::protocol::RemoteStatus;
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Outgoing,
    Incoming,
}

impl Direction {
    fn as_str(self) -> &'static str {
        match self {
            Direction::Outgoing => "outgoing",
            Direction::Incoming => "incoming",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTask {
    pub request_id: String,
    pub direction: Direction,
    pub peer_device_id: String,
    pub objective: String,
    pub conversation_id: Option<String>,
    pub task_id: Option<String>,
    pub status: RemoteStatus,
    /// Current step, reason or result, in plain words.
    pub detail: Option<String>,
    pub result: Option<String>,
    pub last_seq: i64,
    pub created_at: String,
    pub updated_at: String,
}

const COLS: &str = "request_id, direction, peer_device_id, objective, conversation_id, task_id, status, detail, result, last_seq, created_at, updated_at";

fn row(r: &Row) -> rusqlite::Result<RemoteTask> {
    let dir: String = r.get(1)?;
    let status: String = r.get(6)?;
    Ok(RemoteTask {
        request_id: r.get(0)?,
        direction: if dir == "incoming" { Direction::Incoming } else { Direction::Outgoing },
        peer_device_id: r.get(2)?,
        objective: r.get(3)?,
        conversation_id: r.get(4)?,
        task_id: r.get(5)?,
        status: RemoteStatus::parse(&status),
        detail: r.get(7)?,
        result: r.get(8)?,
        last_seq: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

pub fn insert(
    conn: &Connection,
    request_id: &str,
    dir: Direction,
    peer: &str,
    objective: &str,
    conversation_id: Option<&str>,
    status: RemoteStatus,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO remote_tasks (request_id, direction, peer_device_id, objective, conversation_id, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![request_id, dir.as_str(), peer, objective, conversation_id, status.as_str()],
    )?;
    // Bounded history.
    conn.execute("DELETE FROM remote_tasks WHERE request_id IN (SELECT request_id FROM remote_tasks ORDER BY updated_at DESC LIMIT -1 OFFSET 500)", [])?;
    Ok(())
}

pub fn get(conn: &Connection, request_id: &str) -> AppResult<Option<RemoteTask>> {
    Ok(conn.query_row(&format!("SELECT {COLS} FROM remote_tasks WHERE request_id = ?1"), [request_id], row).optional()?)
}

pub fn recent(conn: &Connection, limit: u32) -> AppResult<Vec<RemoteTask>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM remote_tasks ORDER BY created_at DESC LIMIT ?1"))?;
    let rows = stmt.query_map([limit.min(200)], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn unfinished(conn: &Connection, dir: Direction) -> AppResult<Vec<RemoteTask>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM remote_tasks WHERE direction = ?1 ORDER BY created_at"))?;
    let rows = stmt.query_map([dir.as_str()], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?.into_iter().filter(|t| !t.status.is_final()).collect())
}

/// Update status/detail (and optionally result, seq, task id). Never moves a
/// finished task back to an unfinished state.
pub fn set_status(
    conn: &Connection,
    request_id: &str,
    status: RemoteStatus,
    detail: Option<&str>,
    result: Option<&str>,
    seq: Option<i64>,
) -> AppResult<Option<RemoteTask>> {
    let Some(cur) = get(conn, request_id)? else { return Ok(None) };
    if cur.status.is_final() && cur.status != status {
        return Ok(Some(cur));
    }
    let detail = detail.map(|d| crate::orchestrator::task::clip(d, super::protocol::MAX_DETAIL_CHARS));
    let result = result.map(|d| crate::orchestrator::task::clip(d, super::protocol::MAX_DETAIL_CHARS));
    conn.execute(
        "UPDATE remote_tasks SET status = ?2, detail = COALESCE(?3, detail), result = COALESCE(?4, result), last_seq = COALESCE(?5, last_seq),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE request_id = ?1",
        params![request_id, status.as_str(), detail, result, seq],
    )?;
    get(conn, request_id)
}

pub fn set_task_id(conn: &Connection, request_id: &str, task_id: &str) -> AppResult<()> {
    conn.execute("UPDATE remote_tasks SET task_id = ?2 WHERE request_id = ?1 AND task_id IS NULL", params![request_id, task_id])?;
    Ok(())
}

/// Next update sequence number for an incoming task (persisted, so it keeps
/// increasing across restarts).
pub fn next_seq(conn: &Connection, request_id: &str) -> AppResult<i64> {
    Ok(conn.query_row("UPDATE remote_tasks SET last_seq = last_seq + 1 WHERE request_id = ?1 RETURNING last_seq", [request_id], |r| r.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn finished_tasks_dont_reopen() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        insert(&c, "r1", Direction::Outgoing, "dev_x", "open notepad", None, RemoteStatus::Pending).unwrap();
        set_status(&c, "r1", RemoteStatus::Completed, Some("Done"), None, None).unwrap();
        let t = set_status(&c, "r1", RemoteStatus::Running, Some("late update"), None, None).unwrap().unwrap();
        assert_eq!(t.status, RemoteStatus::Completed);
        assert_eq!(t.detail.as_deref(), Some("Done"));
        assert!(unfinished(&c, Direction::Outgoing).unwrap().is_empty());
    }

    #[test]
    fn sequence_numbers_increase() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        insert(&c, "r1", Direction::Incoming, "dev_x", "x", Some("c"), RemoteStatus::Accepted).unwrap();
        assert_eq!(next_seq(&c, "r1").unwrap(), 1);
        assert_eq!(next_seq(&c, "r1").unwrap(), 2);
    }
}
