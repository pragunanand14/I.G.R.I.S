//! Forward-only schema migrations tracked with SQLite's `user_version`.
//!
//! To change the schema, append a new entry to [`MIGRATIONS`]. Never edit or
//! reorder an existing migration once it has shipped.

use rusqlite::Connection;

use crate::error::AppResult;

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "settings",
        sql: r#"
            CREATE TABLE settings (
                key        TEXT PRIMARY KEY NOT NULL,
                value      TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );
        "#,
    },
    Migration {
        version: 2,
        name: "conversations",
        sql: r#"
            CREATE TABLE conversations (
                id            TEXT PRIMARY KEY NOT NULL,
                title         TEXT NOT NULL,
                -- Frozen at creation: providers bind reasoning to the exact prefix.
                system_prompt TEXT NOT NULL,
                created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );
            CREATE INDEX idx_conversations_updated ON conversations(updated_at DESC);

            CREATE TABLE messages (
                id              TEXT PRIMARY KEY NOT NULL,
                conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
                seq             INTEGER NOT NULL,
                role            TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
                content         TEXT NOT NULL,
                status          TEXT NOT NULL CHECK (status IN ('complete', 'error', 'cancelled', 'refused', 'truncated')),
                error           TEXT,
                provider        TEXT,
                model           TEXT,
                -- Provider-native content blocks for unchanged replay (JSON).
                raw             TEXT,
                input_tokens    INTEGER,
                output_tokens   INTEGER,
                created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                UNIQUE (conversation_id, seq)
            );
        "#,
    },
    Migration {
        version: 3,
        name: "tools",
        sql: r#"
            -- Tool definitions offered to the model, frozen per conversation (JSON array).
            ALTER TABLE conversations ADD COLUMN tool_specs TEXT;
            -- Tool calls shown with an assistant message (JSON array).
            ALTER TABLE messages ADD COLUMN tool_activity TEXT;

            CREATE TABLE applications (
                id         TEXT PRIMARY KEY NOT NULL,
                name       TEXT NOT NULL UNIQUE COLLATE NOCASE,
                path       TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            -- Append-only; intentionally not tied to conversations so it survives deletion.
            CREATE TABLE tool_audit (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id TEXT,
                tool            TEXT NOT NULL,
                permission      TEXT NOT NULL,
                actor           TEXT NOT NULL CHECK (actor IN ('assistant', 'user')),
                description     TEXT NOT NULL,
                input           TEXT,
                status          TEXT NOT NULL,
                approval        TEXT NOT NULL,
                result          TEXT,
                duration_ms     INTEGER,
                created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );
        "#,
    },
];

pub fn current_version(conn: &Connection) -> AppResult<u32> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Apply all pending migrations, each in its own transaction.
pub fn run(conn: &mut Connection) -> AppResult<u32> {
    let start = current_version(conn)?;
    let mut version = start;
    for m in MIGRATIONS.iter().filter(|m| m.version > start) {
        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)?;
        tx.pragma_update(None, "user_version", m.version)?;
        tx.commit()?;
        version = m.version;
        tracing::info!(event = "DB_MIGRATION_APPLIED", version = m.version, name = m.name);
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_strictly_increasing() {
        let mut prev = 0;
        for m in MIGRATIONS {
            assert!(m.version > prev, "migration {} is out of order", m.name);
            prev = m.version;
        }
    }

    #[test]
    fn applies_all_and_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        let latest = MIGRATIONS.last().unwrap().version;
        assert_eq!(run(&mut conn).unwrap(), latest);
        assert_eq!(run(&mut conn).unwrap(), latest);
        assert_eq!(current_version(&conn).unwrap(), latest);
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("igris.db");
        {
            let mut conn = Connection::open(&path).unwrap();
            run(&mut conn).unwrap();
            conn.execute("INSERT INTO settings (key, value) VALUES ('k', '\"v\"')", []).unwrap();
        }
        let mut conn = Connection::open(&path).unwrap();
        run(&mut conn).unwrap();
        let v: String = conn.query_row("SELECT value FROM settings WHERE key = 'k'", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "\"v\"");
    }
}
