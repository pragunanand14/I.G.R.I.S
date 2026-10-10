//! Handoff: send a link or a bit of text to another of the user's devices
//! ("send this to my phone"). It travels end-to-end encrypted like every
//! device message; the receiving device keeps it in a small inbox (so nothing
//! is lost if IGRIS wasn't open) and shows it with Open / Copy. Nothing is
//! opened automatically: links open only when the user taps them.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Text longer than this doesn't fit one relay message comfortably.
pub const MAX_CHARS: usize = 2000;
/// Received items kept; older ones are dropped.
const KEEP: i64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffKind {
    Link,
    Text,
}

impl HandoffKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HandoffKind::Link => "link",
            HandoffKind::Text => "text",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "link" => Some(HandoffKind::Link),
            "text" => Some(HandoffKind::Text),
            _ => None,
        }
    }
}

/// Something another device sent here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Handoff {
    pub id: String,
    pub from_device: String,
    pub from_name: String,
    pub kind: HandoffKind,
    pub content: String,
    pub received_at: String,
}

/// Check (and tidy) what's about to be sent. Links must be http(s).
pub fn validate(kind: HandoffKind, content: &str) -> AppResult<String> {
    let c = content.trim();
    if c.is_empty() {
        return Err(AppError::validation("There's nothing to send."));
    }
    if c.chars().count() > MAX_CHARS {
        return Err(AppError::validation(format!("That's too long to send (at most {MAX_CHARS} characters).")));
    }
    if kind == HandoffKind::Link {
        let ok = reqwest::Url::parse(c).ok().is_some_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some());
        if !ok || c.chars().any(char::is_whitespace) {
            return Err(AppError::validation("Only web links (http or https) can be sent as links."));
        }
    }
    Ok(c.to_string())
}

/// Store something received (validated again: it came from another device).
pub fn receive(conn: &Connection, id: &str, from_device: &str, from_name: &str, kind: HandoffKind, content: &str) -> AppResult<Option<Handoff>> {
    let content = validate(kind, content)?;
    let now = chrono::Utc::now().to_rfc3339();
    let added = conn.execute(
        "INSERT OR IGNORE INTO handoffs (id, from_device, from_name, kind, content, received_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, from_device, from_name, kind.as_str(), content, now],
    )?;
    conn.execute("DELETE FROM handoffs WHERE id NOT IN (SELECT id FROM handoffs ORDER BY received_at DESC LIMIT ?1)", [KEEP])?;
    Ok((added > 0).then(|| Handoff { id: id.into(), from_device: from_device.into(), from_name: from_name.into(), kind, content, received_at: now }))
}

/// Received items, newest first.
pub fn list(conn: &Connection) -> AppResult<Vec<Handoff>> {
    let mut stmt = conn.prepare("SELECT id, from_device, from_name, kind, content, received_at FROM handoffs ORDER BY received_at DESC")?;
    let rows = stmt.query_map([], |r| {
        Ok(Handoff {
            id: r.get(0)?,
            from_device: r.get(1)?,
            from_name: r.get(2)?,
            kind: HandoffKind::parse(&r.get::<_, String>(3)?).unwrap_or(HandoffKind::Text),
            content: r.get(4)?,
            received_at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn dismiss(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM handoffs WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn links_must_be_web_links_and_text_has_a_limit() {
        assert_eq!(validate(HandoffKind::Link, " https://example.com/a?b=1 ").unwrap(), "https://example.com/a?b=1");
        for bad in ["javascript:alert(1)", "file:///C:/secret.txt", "example.com", "https://", "https://a.com/x y", ""] {
            assert!(validate(HandoffKind::Link, bad).is_err(), "{bad}");
        }
        assert!(validate(HandoffKind::Text, "hello there").is_ok());
        assert!(validate(HandoffKind::Text, &"x".repeat(MAX_CHARS + 1)).is_err());
    }

    #[test]
    fn received_items_are_kept_once_and_can_be_dismissed() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        assert!(receive(&c, "h1", "d1", "Phone", HandoffKind::Link, "https://example.com").unwrap().is_some());
        assert!(receive(&c, "h1", "d1", "Phone", HandoffKind::Link, "https://example.com").unwrap().is_none(), "a re-delivery isn't a second item");
        assert!(receive(&c, "h2", "d1", "Phone", HandoffKind::Link, "javascript:alert(1)").is_err());
        assert_eq!(list(&c).unwrap().len(), 1);
        dismiss(&c, "h1").unwrap();
        assert!(list(&c).unwrap().is_empty());
    }
}
