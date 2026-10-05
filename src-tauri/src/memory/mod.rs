//! Persistent memory.
//!
//! * **Long-term** memories are durable facts about the user ("My main project
//!   is SkillTrack").
//! * **Knowledge** memories are information intentionally stored for later
//!   retrieval (notes, reference facts).
//!
//! Conversation memory is the conversation history itself (`conversations`).
//! Everything here is inspectable, editable and deletable from the Memory
//! page, and content that looks like a credential or ID number is refused.

pub mod retrieval;
pub mod sensitive;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const CONTENT_MAX_CHARS: usize = 500;
pub const MAX_MEMORIES: i64 = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    LongTerm,
    Knowledge,
}

impl MemoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryKind::LongTerm => "long_term",
            MemoryKind::Knowledge => "knowledge",
        }
    }

    fn parse(s: &str) -> Self {
        if s == "knowledge" {
            MemoryKind::Knowledge
        } else {
            MemoryKind::LongTerm
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemorySource {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: i64,
    pub kind: MemoryKind,
    pub content: String,
    pub source: MemorySource,
    pub conversation_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
    pub use_count: i64,
}

const COLS: &str = "m.id, m.kind, m.content, m.source, m.conversation_id, m.created_at, m.updated_at, m.last_used_at, m.use_count";

fn row(r: &Row) -> rusqlite::Result<Memory> {
    let kind: String = r.get(1)?;
    let source: String = r.get(3)?;
    Ok(Memory {
        id: r.get(0)?,
        kind: MemoryKind::parse(&kind),
        content: r.get(2)?,
        source: if source == "assistant" { MemorySource::Assistant } else { MemorySource::User },
        conversation_id: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
        last_used_at: r.get(7)?,
        use_count: r.get(8)?,
    })
}

/// Validate and normalise memory text.
pub fn validate_content(content: &str) -> AppResult<String> {
    let content = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if content.is_empty() {
        return Err(AppError::validation("Memory can't be empty."));
    }
    if content.chars().count() > CONTENT_MAX_CHARS {
        return Err(AppError::validation(format!("Memory must be at most {CONTENT_MAX_CHARS} characters.")));
    }
    if let Some(kind) = sensitive::detect(&content) {
        return Err(AppError::validation(format!(
            "This looks like {}. IGRIS doesn't store passwords, keys, card numbers or ID numbers in memory.",
            kind.describe()
        )));
    }
    Ok(content)
}

fn normalized(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

pub fn get(conn: &Connection, id: i64) -> AppResult<Option<Memory>> {
    Ok(conn.query_row(&format!("SELECT {COLS} FROM memories m WHERE m.id = ?1"), [id], row).optional()?)
}

pub fn require(conn: &Connection, id: i64) -> AppResult<Memory> {
    get(conn, id)?.ok_or_else(|| AppError::validation(format!("Memory #{id} doesn't exist.")))
}

#[derive(Debug, Clone, PartialEq)]
pub enum AddOutcome {
    Added(Memory),
    /// An equivalent memory already exists; nothing was stored.
    Duplicate(Memory),
}

pub fn add(conn: &Connection, kind: MemoryKind, content: &str, source: MemorySource, conversation_id: Option<&str>) -> AppResult<AddOutcome> {
    let content = validate_content(content)?;
    let key = normalized(&content);
    if let Some(existing) = all(conn)?.into_iter().find(|m| normalized(&m.content) == key) {
        return Ok(AddOutcome::Duplicate(existing));
    }
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))?;
    if count >= MAX_MEMORIES {
        return Err(AppError::validation(format!("Memory is full ({MAX_MEMORIES} items). Delete some on the Memory page.")));
    }
    conn.execute(
        "INSERT INTO memories (kind, content, source, conversation_id) VALUES (?1, ?2, ?3, ?4)",
        params![kind.as_str(), content, if source == MemorySource::Assistant { "assistant" } else { "user" }, conversation_id],
    )?;
    let m = require(conn, conn.last_insert_rowid())?;
    tracing::info!(event = "MEMORY_ADDED", id = m.id, kind = kind.as_str(), source = ?source);
    Ok(AddOutcome::Added(m))
}

pub fn update(conn: &Connection, id: i64, content: &str, kind: Option<MemoryKind>) -> AppResult<Memory> {
    let content = validate_content(content)?;
    let current = require(conn, id)?;
    conn.execute(
        "UPDATE memories SET content = ?2, kind = ?3, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
        params![id, content, kind.unwrap_or(current.kind).as_str()],
    )?;
    tracing::info!(event = "MEMORY_UPDATED", id);
    require(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> AppResult<Memory> {
    let m = require(conn, id)?;
    conn.execute("DELETE FROM memories WHERE id = ?1", [id])?;
    tracing::info!(event = "MEMORY_DELETED", id);
    Ok(m)
}

pub fn all(conn: &Connection) -> AppResult<Vec<Memory>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM memories m ORDER BY m.updated_at DESC, m.id DESC"))?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "your", "with", "this", "that", "what", "whats", "was", "were", "have", "has", "had", "how", "who", "why",
    "when", "where", "which", "can", "could", "would", "should", "about", "from", "into", "they", "them", "there", "their", "then", "than", "will", "just",
    "its", "it's", "is", "am", "be", "me", "my", "mine", "our", "we", "do", "does", "did", "a", "an", "of", "to", "in", "on", "at", "or", "if", "so", "as",
    "by", "up", "please", "tell", "know", "remember", "igris", "hey", "hi", "hello", "thanks",
];

/// Build an FTS5 query from free text: significant words, prefix-matched, OR-ed.
pub fn fts_query(text: &str) -> Option<String> {
    let mut terms: Vec<String> = Vec::new();
    for w in text.split(|c: char| !c.is_alphanumeric()) {
        let w = w.to_lowercase();
        if w.chars().count() < 2 || STOPWORDS.contains(&w.as_str()) || terms.contains(&w) {
            continue;
        }
        terms.push(w);
        if terms.len() >= 12 {
            break;
        }
    }
    (!terms.is_empty()).then(|| terms.iter().map(|t| format!("\"{t}\"*")).collect::<Vec<_>>().join(" OR "))
}

/// Full-text search, best matches first.
pub fn search(conn: &Connection, text: &str, kind: Option<MemoryKind>, limit: u32) -> AppResult<Vec<Memory>> {
    let Some(q) = fts_query(text) else { return Ok(Vec::new()) };
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM memories_fts f JOIN memories m ON m.id = f.rowid
         WHERE memories_fts MATCH ?1 AND (?2 IS NULL OR m.kind = ?2)
         ORDER BY bm25(memories_fts) LIMIT ?3"
    ))?;
    let rows = stmt.query_map(params![q, kind.map(MemoryKind::as_str), limit.min(200)], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// List for the Memory page: optional kind filter and text search.
pub fn list(conn: &Connection, kind: Option<MemoryKind>, query: Option<&str>) -> AppResult<Vec<Memory>> {
    match query.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => search(conn, q, kind, 200),
        None => Ok(all(conn)?
            .into_iter()
            .filter(|m| match kind {
                Some(k) => m.kind == k,
                None => true,
            })
            .collect()),
    }
}

pub fn mark_used(conn: &Connection, ids: &[i64]) -> AppResult<()> {
    for id in ids {
        conn.execute("UPDATE memories SET use_count = use_count + 1, last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1", [id])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn added(o: AddOutcome) -> Memory {
        match o {
            AddOutcome::Added(m) => m,
            AddOutcome::Duplicate(m) => panic!("unexpected duplicate #{}", m.id),
        }
    }

    #[test]
    fn crud_and_search() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn().unwrap();
        let a = added(add(&conn, MemoryKind::LongTerm, "My main project is called  SkillTrack.", MemorySource::User, None).unwrap());
        assert_eq!(a.content, "My main project is called SkillTrack.");
        added(add(&conn, MemoryKind::Knowledge, "SkillTrack uses Java and MySQL", MemorySource::Assistant, Some("c1")).unwrap());
        added(add(&conn, MemoryKind::LongTerm, "I prefer dark themes", MemorySource::User, None).unwrap());

        let hits = search(&conn, "What's my main project?", None, 10).unwrap();
        assert_eq!(hits[0].id, a.id);
        assert_eq!(search(&conn, "projects", None, 10).unwrap().len(), 1, "porter stemming matches plural");
        assert_eq!(search(&conn, "skilltrack", Some(MemoryKind::Knowledge), 10).unwrap().len(), 1);
        assert!(search(&conn, "the and of", None, 10).unwrap().is_empty(), "stopword-only queries match nothing");

        let u = update(&conn, a.id, "My main project is SkillTrack v2", None).unwrap();
        assert_eq!(u.kind, MemoryKind::LongTerm);
        assert_eq!(search(&conn, "v2", None, 10).unwrap()[0].id, a.id, "index follows updates");
        delete(&conn, a.id).unwrap();
        assert!(search(&conn, "main project", None, 10).unwrap().is_empty(), "index follows deletes");
        assert!(delete(&conn, a.id).is_err());
        assert_eq!(list(&conn, Some(MemoryKind::LongTerm), None).unwrap().len(), 1);
    }

    #[test]
    fn deduplicates_equivalent_memories() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn().unwrap();
        let a = added(add(&conn, MemoryKind::LongTerm, "I live in Gurugram", MemorySource::User, None).unwrap());
        match add(&conn, MemoryKind::LongTerm, "i live in gurugram.", MemorySource::Assistant, None).unwrap() {
            AddOutcome::Duplicate(m) => assert_eq!(m.id, a.id),
            other => panic!("expected duplicate, got {other:?}"),
        }
        assert_eq!(all(&conn).unwrap().len(), 1);
    }

    #[test]
    fn refuses_sensitive_and_invalid_content() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn().unwrap();
        for bad in ["my password is hunter2", "card 4111 1111 1111 1111", "   ", &"x".repeat(CONTENT_MAX_CHARS + 1)] {
            assert!(add(&conn, MemoryKind::LongTerm, bad, MemorySource::User, None).is_err(), "{bad}");
        }
        let a = added(add(&conn, MemoryKind::LongTerm, "fine", MemorySource::User, None).unwrap());
        assert!(update(&conn, a.id, "my api key is abcdefgh12345678", None).is_err());
        assert_eq!(require(&conn, a.id).unwrap().content, "fine");
    }

    #[test]
    fn fts_query_escapes_input() {
        assert_eq!(fts_query("main \"project\" OR NEAR(x)").unwrap(), "\"main\"* OR \"project\"* OR \"near\"*");
        assert!(fts_query("?? !!").is_none());
    }

    #[test]
    fn usage_is_tracked() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn().unwrap();
        let a = added(add(&conn, MemoryKind::LongTerm, "fact", MemorySource::User, None).unwrap());
        mark_used(&conn, &[a.id, a.id]).unwrap();
        let m = require(&conn, a.id).unwrap();
        assert_eq!(m.use_count, 2);
        assert!(m.last_used_at.is_some());
    }
}
