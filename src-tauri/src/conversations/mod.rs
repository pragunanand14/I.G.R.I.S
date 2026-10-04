//! Conversation and message persistence.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::ai::Role;
use crate::error::{AppError, AppResult};

pub const TITLE_MAX_CHARS: usize = 80;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Complete,
    Error,
    Cancelled,
    Refused,
    /// Stopped by the output token limit.
    Truncated,
}

impl MessageStatus {
    fn as_str(self) -> &'static str {
        match self {
            MessageStatus::Complete => "complete",
            MessageStatus::Error => "error",
            MessageStatus::Cancelled => "cancelled",
            MessageStatus::Refused => "refused",
            MessageStatus::Truncated => "truncated",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "error" => MessageStatus::Error,
            "cancelled" => MessageStatus::Cancelled,
            "refused" => MessageStatus::Refused,
            "truncated" => MessageStatus::Truncated,
            _ => MessageStatus::Complete,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub seq: i64,
    pub role: Role,
    pub content: String,
    pub status: MessageStatus,
    pub error: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    #[serde(skip)]
    pub raw: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub created_at: String,
}

/// Fields for a new message; ids and sequence numbers are assigned here.
#[derive(Debug, Clone)]
pub struct NewMessage {
    pub role: Role,
    pub content: String,
    pub status: MessageStatus,
    pub error: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub raw: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
}

impl NewMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            status: MessageStatus::Complete,
            error: None,
            provider: None,
            model: None,
            raw: None,
            input_tokens: None,
            output_tokens: None,
        }
    }
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Derive a title from the first user message (no model call needed).
pub fn title_from(content: &str) -> String {
    let line = content.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("New conversation");
    let mut title: String = line.chars().take(TITLE_MAX_CHARS).collect();
    if line.chars().count() > TITLE_MAX_CHARS {
        title = title.trim_end().to_string() + "…";
    }
    title
}

fn conversation_from_row(r: &Row) -> rusqlite::Result<Conversation> {
    Ok(Conversation { id: r.get(0)?, title: r.get(1)?, created_at: r.get(2)?, updated_at: r.get(3)?, message_count: r.get(4)? })
}

const CONVERSATION_COLS: &str =
    "c.id, c.title, c.created_at, c.updated_at, (SELECT COUNT(*) FROM messages m WHERE m.conversation_id = c.id)";

fn message_from_row(r: &Row) -> rusqlite::Result<Message> {
    let role: String = r.get(3)?;
    let status: String = r.get(5)?;
    Ok(Message {
        id: r.get(0)?,
        conversation_id: r.get(1)?,
        seq: r.get(2)?,
        role: if role == "assistant" { Role::Assistant } else { Role::User },
        content: r.get(4)?,
        status: MessageStatus::parse(&status),
        error: r.get(6)?,
        provider: r.get(7)?,
        model: r.get(8)?,
        raw: r.get(9)?,
        input_tokens: r.get(10)?,
        output_tokens: r.get(11)?,
        created_at: r.get(12)?,
    })
}

const MESSAGE_COLS: &str =
    "id, conversation_id, seq, role, content, status, error, provider, model, raw, input_tokens, output_tokens, created_at";

pub fn create(conn: &Connection, title: &str, system_prompt: &str) -> AppResult<Conversation> {
    let id = new_id();
    conn.execute("INSERT INTO conversations (id, title, system_prompt) VALUES (?1, ?2, ?3)", params![id, title, system_prompt])?;
    get(conn, &id)?.ok_or_else(|| AppError::internal("conversation vanished after insert"))
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Option<Conversation>> {
    Ok(conn
        .query_row(&format!("SELECT {CONVERSATION_COLS} FROM conversations c WHERE c.id = ?1"), [id], conversation_from_row)
        .optional()?)
}

pub fn require(conn: &Connection, id: &str) -> AppResult<Conversation> {
    get(conn, id)?.ok_or_else(|| AppError::validation("That conversation no longer exists."))
}

pub fn system_prompt(conn: &Connection, id: &str) -> AppResult<String> {
    Ok(conn.query_row("SELECT system_prompt FROM conversations WHERE id = ?1", [id], |r| r.get(0))?)
}

pub fn list(conn: &Connection) -> AppResult<Vec<Conversation>> {
    let mut stmt = conn.prepare(&format!("SELECT {CONVERSATION_COLS} FROM conversations c ORDER BY c.updated_at DESC, c.rowid DESC"))?;
    let rows = stmt.query_map([], conversation_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn rename(conn: &Connection, id: &str, title: &str) -> AppResult<Conversation> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::validation("Title can't be empty."));
    }
    if title.chars().count() > TITLE_MAX_CHARS {
        return Err(AppError::validation(format!("Title must be at most {TITLE_MAX_CHARS} characters.")));
    }
    if title.chars().any(char::is_control) {
        return Err(AppError::validation("Title cannot contain control characters."));
    }
    let n = conn.execute("UPDATE conversations SET title = ?2 WHERE id = ?1", params![id, title])?;
    if n == 0 {
        return Err(AppError::validation("That conversation no longer exists."));
    }
    require(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<bool> {
    Ok(conn.execute("DELETE FROM conversations WHERE id = ?1", [id])? > 0)
}

fn touch(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("UPDATE conversations SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1", [id])?;
    Ok(())
}

pub fn messages(conn: &Connection, conversation_id: &str) -> AppResult<Vec<Message>> {
    let mut stmt = conn.prepare(&format!("SELECT {MESSAGE_COLS} FROM messages WHERE conversation_id = ?1 ORDER BY seq"))?;
    let rows = stmt.query_map([conversation_id], message_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get_message(conn: &Connection, id: &str) -> AppResult<Option<Message>> {
    Ok(conn.query_row(&format!("SELECT {MESSAGE_COLS} FROM messages WHERE id = ?1"), [id], message_from_row).optional()?)
}

pub fn append(conn: &mut Connection, conversation_id: &str, m: NewMessage) -> AppResult<Message> {
    let id = new_id();
    let tx = conn.transaction()?;
    let seq: i64 = tx.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE conversation_id = ?1", [conversation_id], |r| r.get(0))?;
    tx.execute(
        &format!("INSERT INTO messages ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))", MESSAGE_COLS),
        params![
            id,
            conversation_id,
            seq,
            if m.role == Role::Assistant { "assistant" } else { "user" },
            m.content,
            m.status.as_str(),
            m.error,
            m.provider,
            m.model,
            m.raw,
            m.input_tokens,
            m.output_tokens,
        ],
    )?;
    touch(&tx, conversation_id)?;
    tx.commit()?;
    get_message(conn, &id)?.ok_or_else(|| AppError::internal("message vanished after insert"))
}

/// Delete every message after `seq` (exclusive). History is only ever cut
/// from the tail, so the remaining prefix stays byte-identical.
pub fn truncate_after(conn: &Connection, conversation_id: &str, seq: i64) -> AppResult<usize> {
    Ok(conn.execute("DELETE FROM messages WHERE conversation_id = ?1 AND seq > ?2", params![conversation_id, seq])?)
}

/// Replace a user message's text and drop everything after it.
pub fn edit_user_message(conn: &mut Connection, message_id: &str, content: &str) -> AppResult<Message> {
    let msg = get_message(conn, message_id)?.ok_or_else(|| AppError::validation("That message no longer exists."))?;
    if msg.role != Role::User {
        return Err(AppError::validation("Only your own messages can be edited."));
    }
    let tx = conn.transaction()?;
    tx.execute("UPDATE messages SET content = ?2 WHERE id = ?1", params![message_id, content])?;
    tx.execute("DELETE FROM messages WHERE conversation_id = ?1 AND seq > ?2", params![msg.conversation_id, msg.seq])?;
    touch(&tx, &msg.conversation_id)?;
    tx.commit()?;
    get_message(conn, message_id)?.ok_or_else(|| AppError::internal("message vanished after edit"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn assistant(text: &str) -> NewMessage {
        NewMessage { role: Role::Assistant, provider: Some("anthropic".into()), raw: Some("[]".into()), ..NewMessage::user(text) }
    }

    #[test]
    fn creates_appends_and_lists_in_order() {
        let db = Database::open_in_memory().unwrap();
        let mut conn = db.conn().unwrap();
        let c = create(&conn, "Hello", "sys").unwrap();
        append(&mut conn, &c.id, NewMessage::user("hi")).unwrap();
        append(&mut conn, &c.id, assistant("hello")).unwrap();
        let ms = messages(&conn, &c.id).unwrap();
        assert_eq!(ms.iter().map(|m| m.seq).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(ms[1].role, Role::Assistant);
        assert_eq!(ms[1].raw.as_deref(), Some("[]"));
        assert_eq!(list(&conn).unwrap()[0].message_count, 2);
        assert_eq!(system_prompt(&conn, &c.id).unwrap(), "sys");
    }

    #[test]
    fn raw_blocks_are_not_serialized_to_the_ui() {
        let db = Database::open_in_memory().unwrap();
        let mut conn = db.conn().unwrap();
        let c = create(&conn, "t", "s").unwrap();
        let m = append(&mut conn, &c.id, assistant("x")).unwrap();
        assert!(serde_json::to_value(&m).unwrap().get("raw").is_none());
    }

    #[test]
    fn edit_truncates_following_messages() {
        let db = Database::open_in_memory().unwrap();
        let mut conn = db.conn().unwrap();
        let c = create(&conn, "t", "s").unwrap();
        let u1 = append(&mut conn, &c.id, NewMessage::user("one")).unwrap();
        append(&mut conn, &c.id, assistant("a1")).unwrap();
        append(&mut conn, &c.id, NewMessage::user("two")).unwrap();
        let edited = edit_user_message(&mut conn, &u1.id, "uno").unwrap();
        assert_eq!(edited.content, "uno");
        assert_eq!(messages(&conn, &c.id).unwrap().len(), 1);
    }

    #[test]
    fn cannot_edit_assistant_messages() {
        let db = Database::open_in_memory().unwrap();
        let mut conn = db.conn().unwrap();
        let c = create(&conn, "t", "s").unwrap();
        let a = append(&mut conn, &c.id, assistant("a")).unwrap();
        assert!(edit_user_message(&mut conn, &a.id, "x").is_err());
    }

    #[test]
    fn delete_cascades_to_messages() {
        let db = Database::open_in_memory().unwrap();
        let mut conn = db.conn().unwrap();
        let c = create(&conn, "t", "s").unwrap();
        append(&mut conn, &c.id, NewMessage::user("hi")).unwrap();
        assert!(delete(&conn, &c.id).unwrap());
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        assert!(!delete(&conn, &c.id).unwrap());
    }

    #[test]
    fn rename_validates() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn().unwrap();
        let c = create(&conn, "t", "s").unwrap();
        assert!(rename(&conn, &c.id, "  ").is_err());
        assert!(rename(&conn, &c.id, &"x".repeat(TITLE_MAX_CHARS + 1)).is_err());
        assert_eq!(rename(&conn, &c.id, " Plans ").unwrap().title, "Plans");
        assert!(rename(&conn, "missing", "x").is_err());
    }

    #[test]
    fn titles_come_from_first_non_empty_line() {
        assert_eq!(title_from("\n  Plan my week  \nmore"), "Plan my week");
        let long = "a".repeat(200);
        let t = title_from(&long);
        assert!(t.ends_with('…'));
        assert_eq!(t.chars().count(), TITLE_MAX_CHARS + 1);
        assert_eq!(title_from("   "), "New conversation");
    }
}
