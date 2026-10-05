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
    Migration {
        version: 4,
        name: "memory",
        sql: r#"
            CREATE TABLE memories (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                kind            TEXT NOT NULL CHECK (kind IN ('long_term', 'knowledge')),
                content         TEXT NOT NULL,
                source          TEXT NOT NULL CHECK (source IN ('user', 'assistant')),
                conversation_id TEXT,
                created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                last_used_at    TEXT,
                use_count       INTEGER NOT NULL DEFAULT 0
            );

            -- Full-text index kept in sync by triggers (external content table).
            CREATE VIRTUAL TABLE memories_fts USING fts5(
                content, content='memories', content_rowid='id', tokenize='porter unicode61'
            );
            CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, content) VALUES (new.id, new.content);
            END;
            CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, content) VALUES ('delete', old.id, old.content);
            END;
            CREATE TRIGGER memories_au AFTER UPDATE OF content ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, content) VALUES ('delete', old.id, old.content);
                INSERT INTO memories_fts(rowid, content) VALUES (new.id, new.content);
            END;

            -- Memories attached to a user message (JSON), replayed unchanged on later turns.
            ALTER TABLE messages ADD COLUMN memory_context TEXT;
        "#,
    },
    Migration {
        version: 5,
        name: "computer_control",
        sql: r#"
            -- Folders IGRIS may access; nothing outside these is reachable by file tools.
            CREATE TABLE allowed_folders (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                path       TEXT NOT NULL UNIQUE,
                writable   INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE projects (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
                path        TEXT NOT NULL,
                repository  TEXT NOT NULL DEFAULT '',
                language    TEXT NOT NULL DEFAULT '',
                framework   TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                notes       TEXT NOT NULL DEFAULT '',
                created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );
        "#,
    },
    Migration {
        version: 6,
        name: "productivity",
        sql: r#"
            -- Times are UTC 'YYYY-MM-DDTHH:MM:SSZ' so they compare as strings.
            CREATE TABLE tasks (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                title        TEXT NOT NULL,
                notes        TEXT NOT NULL DEFAULT '',
                due_at       TEXT,
                priority     TEXT NOT NULL DEFAULT 'normal' CHECK (priority IN ('low', 'normal', 'high')),
                done         INTEGER NOT NULL DEFAULT 0,
                completed_at TEXT,
                created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );
            CREATE INDEX tasks_open_due ON tasks (done, due_at);

            -- Reminders and timers; the scheduler fires pending rows whose due_at has passed.
            CREATE TABLE reminders (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                title         TEXT NOT NULL,
                kind          TEXT NOT NULL CHECK (kind IN ('reminder', 'timer')),
                due_at        TEXT NOT NULL,
                duration_secs INTEGER,
                status        TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'fired', 'dismissed', 'cancelled')),
                fired_at      TEXT,
                created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );
            CREATE INDEX reminders_status_due ON reminders (status, due_at);

            -- Local calendar (no external sync).
            CREATE TABLE events (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                title      TEXT NOT NULL,
                starts_at  TEXT NOT NULL,
                ends_at    TEXT,
                all_day    INTEGER NOT NULL DEFAULT 0,
                location   TEXT NOT NULL DEFAULT '',
                notes      TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );
            CREATE INDEX events_starts ON events (starts_at);
        "#,
    },
    Migration {
        version: 7,
        name: "attachments",
        sql: r#"
            -- Images/PDFs on disk under <data>/attachments; uploads are staged (no
            -- conversation) until sent. Rows cascade with their conversation/message.
            CREATE TABLE attachments (
                id              TEXT PRIMARY KEY,
                kind            TEXT NOT NULL CHECK (kind IN ('image', 'pdf')),
                mime            TEXT NOT NULL,
                name            TEXT NOT NULL,
                size            INTEGER NOT NULL,
                width           INTEGER,
                height          INTEGER,
                source          TEXT NOT NULL CHECK (source IN ('upload', 'screenshot')),
                conversation_id TEXT REFERENCES conversations(id) ON DELETE CASCADE,
                message_id      TEXT REFERENCES messages(id) ON DELETE CASCADE,
                created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );
            CREATE INDEX attachments_conversation ON attachments (conversation_id, message_id);
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
