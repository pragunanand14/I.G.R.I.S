//! Append-only audit log of every tool invocation.

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: i64,
    pub conversation_id: Option<String>,
    pub tool: String,
    pub permission: String,
    /// `assistant` (model-initiated) or `user` (clicked in the UI).
    pub actor: String,
    pub description: String,
    pub status: String,
    /// `auto`, `approved`, `denied`, `expired` or `cancelled`.
    pub approval: String,
    pub result: Option<String>,
    pub duration_ms: Option<i64>,
    pub created_at: String,
}

pub struct NewAuditEntry<'a> {
    pub conversation_id: Option<&'a str>,
    pub tool: &'a str,
    pub permission: &'a str,
    pub actor: &'a str,
    pub description: &'a str,
    pub input: &'a str,
    pub status: &'a str,
    pub approval: &'a str,
    pub result: Option<&'a str>,
    pub duration_ms: Option<i64>,
}

const MAX_FIELD: usize = 2000;

fn clip(s: &str) -> String {
    if s.chars().count() <= MAX_FIELD {
        s.to_string()
    } else {
        s.chars().take(MAX_FIELD).collect::<String>() + "…"
    }
}

pub fn record(conn: &Connection, e: &NewAuditEntry) -> AppResult<()> {
    conn.execute(
        "INSERT INTO tool_audit (conversation_id, tool, permission, actor, description, input, status, approval, result, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            e.conversation_id,
            e.tool,
            e.permission,
            e.actor,
            clip(e.description),
            clip(e.input),
            e.status,
            e.approval,
            e.result.map(clip),
            e.duration_ms
        ],
    )?;
    Ok(())
}

pub fn list(conn: &Connection, limit: u32) -> AppResult<Vec<AuditEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, tool, permission, actor, description, status, approval, result, duration_ms, created_at
         FROM tool_audit ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit.min(1000)], |r| {
        Ok(AuditEntry {
            id: r.get(0)?,
            conversation_id: r.get(1)?,
            tool: r.get(2)?,
            permission: r.get(3)?,
            actor: r.get(4)?,
            description: r.get(5)?,
            status: r.get(6)?,
            approval: r.get(7)?,
            result: r.get(8)?,
            duration_ms: r.get(9)?,
            created_at: r.get(10)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
