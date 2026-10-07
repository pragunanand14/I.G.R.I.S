//! Memory sync between the user's devices.
//!
//! Only memories are synced (long-term facts and knowledge notes the user can
//! see on the Memory page). Never synced: the SQLite file, conversations,
//! settings, API keys, device keys, screenshots, attachments, tasks.
//!
//! Model: every memory has a stable `sync_id` and a `revision`; a local edit
//! bumps the revision, a delete leaves a tombstone with the next revision.
//! Each device keeps a local change clock and, per peer, how far that peer
//! has acknowledged; it sends changes in clock order in small batches.
//!
//! Conflicts: the higher revision wins; on equal revisions the change made by
//! the device with the greater device id wins. Both devices apply the same
//! rule, so they converge. A deletion is a revision like any other, so an
//! older edit can't resurrect a deleted memory and a newer edit after the
//! delete (made while offline) does bring it back.
//!
//! Incoming content is untrusted: it goes through the same validation as a
//! memory typed locally (length, credential detection).

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::memory::{self, MemoryKind};

pub const BATCH: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncItem {
    pub sync_id: String,
    pub revision: i64,
    /// Device that made this revision.
    pub origin: String,
    /// `None` for a deletion.
    pub content: Option<String>,
    pub kind: Option<MemoryKind>,
}

/// Changes after `after` on this device's clock, oldest first; returns the
/// items and the clock value they reach.
pub fn changes_since(conn: &Connection, me: &str, after: i64, limit: usize) -> AppResult<(Vec<SyncItem>, i64, bool)> {
    let mut stmt = conn.prepare(
        "SELECT sync_id, revision, origin, content, kind, change_seq FROM memories WHERE change_seq > ?1 AND sync_id IS NOT NULL
         UNION ALL
         SELECT sync_id, revision, origin, NULL, NULL, change_seq FROM memory_tombstones WHERE change_seq > ?1
         ORDER BY change_seq LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![after, limit as i64 + 1], |r| {
        let kind: Option<String> = r.get(4)?;
        Ok((
            SyncItem {
                sync_id: r.get(0)?,
                revision: r.get(1)?,
                origin: r.get::<_, Option<String>>(2)?.unwrap_or_else(|| me.to_string()),
                content: r.get(3)?,
                kind: kind.map(|k| if k == "knowledge" { MemoryKind::Knowledge } else { MemoryKind::LongTerm }),
            },
            r.get::<_, i64>(5)?,
        ))
    })?;
    let mut all: Vec<(SyncItem, i64)> = rows.collect::<Result<_, _>>()?;
    let more = all.len() > limit;
    all.truncate(limit);
    let upto = all.last().map(|(_, s)| *s).unwrap_or(after);
    Ok((all.into_iter().map(|(i, _)| i).collect(), upto, more))
}

/// `(revision, origin)` of `a` beats `b`.
fn wins(a_rev: i64, a_origin: &str, b_rev: i64, b_origin: &str) -> bool {
    (a_rev, a_origin) > (b_rev, b_origin)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Applied {
    pub added: usize,
    pub updated: usize,
    pub deleted: usize,
    pub ignored: usize,
    pub refused: usize,
}

/// Apply a peer's changes. `me` is this device's id (the origin of local
/// revisions stored as NULL).
pub fn apply(conn: &Connection, me: &str, items: &[SyncItem]) -> AppResult<Applied> {
    let mut out = Applied::default();
    for it in items {
        if it.sync_id.len() != 32 || !it.sync_id.bytes().all(|b| b.is_ascii_hexdigit()) || it.revision < 1 || it.origin.len() > 64 {
            out.refused += 1;
            continue;
        }
        let local: Option<(i64, i64, Option<String>)> = conn
            .query_row("SELECT id, revision, origin FROM memories WHERE sync_id = ?1", [&it.sync_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        let tomb: Option<(i64, Option<String>)> =
            conn.query_row("SELECT revision, origin FROM memory_tombstones WHERE sync_id = ?1", [&it.sync_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        let current = local.as_ref().map(|(_, r, o)| (*r, o.clone())).or_else(|| tomb.clone());
        if let Some((rev, origin)) = &current {
            let origin = origin.clone().unwrap_or_else(|| me.to_string());
            if !wins(it.revision, &it.origin, *rev, &origin) {
                out.ignored += 1;
                continue;
            }
        }
        let origin = (it.origin != me).then_some(it.origin.as_str());
        match (&it.content, local) {
            (None, Some((id, _, _))) => {
                let seq = memory::next_change_seq(conn)?;
                conn.execute("DELETE FROM memories WHERE id = ?1", [id])?;
                conn.execute(
                    "INSERT OR REPLACE INTO memory_tombstones (sync_id, revision, origin, change_seq) VALUES (?1, ?2, ?3, ?4)",
                    params![it.sync_id, it.revision, origin, seq],
                )?;
                out.deleted += 1;
            }
            (None, None) => {
                let seq = memory::next_change_seq(conn)?;
                conn.execute(
                    "INSERT OR REPLACE INTO memory_tombstones (sync_id, revision, origin, change_seq) VALUES (?1, ?2, ?3, ?4)",
                    params![it.sync_id, it.revision, origin, seq],
                )?;
                out.ignored += 1;
            }
            (Some(content), local) => {
                let Ok(content) = memory::validate_content(content) else {
                    out.refused += 1;
                    continue;
                };
                let kind = it.kind.unwrap_or(MemoryKind::LongTerm).as_str();
                let seq = memory::next_change_seq(conn)?;
                if let Some((id, _, _)) = local {
                    conn.execute(
                        "UPDATE memories SET content = ?2, kind = ?3, revision = ?4, origin = ?5, change_seq = ?6,
                                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                        params![id, content, kind, it.revision, origin, seq],
                    )?;
                    out.updated += 1;
                } else {
                    let count: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))?;
                    if count >= memory::MAX_MEMORIES {
                        out.refused += 1;
                        continue;
                    }
                    conn.execute("DELETE FROM memory_tombstones WHERE sync_id = ?1", [&it.sync_id])?;
                    conn.execute(
                        "INSERT INTO memories (kind, content, source, sync_id, revision, origin, change_seq) VALUES (?1, ?2, 'user', ?3, ?4, ?5, ?6)",
                        params![kind, content, it.sync_id, it.revision, origin, seq],
                    )?;
                    out.added += 1;
                }
            }
        }
    }
    if out != Applied::default() {
        tracing::info!(
            event = "MEMORY_SYNC_APPLIED",
            added = out.added,
            updated = out.updated,
            deleted = out.deleted,
            ignored = out.ignored,
            refused = out.refused
        );
    }
    Ok(out)
}

pub fn acked(conn: &Connection, peer: &str) -> AppResult<i64> {
    Ok(conn.query_row("SELECT acked_seq FROM sync_peers WHERE device_id = ?1", [peer], |r| r.get(0)).optional()?.unwrap_or(0))
}

pub fn set_acked(conn: &Connection, peer: &str, upto: i64) -> AppResult<()> {
    conn.execute(
        "INSERT INTO sync_peers (device_id, acked_seq) VALUES (?1, ?2)
         ON CONFLICT(device_id) DO UPDATE SET acked_seq = MAX(acked_seq, ?2)",
        params![peer, upto],
    )?;
    Ok(())
}

/// Highest local clock value (to tell whether a peer is behind).
pub fn clock(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT seq FROM sync_clock WHERE id = 1", [], |r| r.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::memory::{add, delete, update, AddOutcome, MemorySource};

    const A: &str = "dev_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "dev_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn added(conn: &Connection, text: &str) -> i64 {
        match add(conn, MemoryKind::LongTerm, text, MemorySource::User, None).unwrap() {
            AddOutcome::Added(m) => m.id,
            AddOutcome::Duplicate(m) => m.id,
        }
    }

    /// Send everything `from` has that `to` hasn't acknowledged.
    fn sync(from: &Connection, from_id: &str, to: &Connection, to_id: &str) -> Applied {
        let after = acked(from, to_id).unwrap();
        let (items, upto, _) = changes_since(from, from_id, after, 500).unwrap();
        let r = apply(to, to_id, &items).unwrap();
        set_acked(from, to_id, upto).unwrap();
        r
    }

    fn contents(conn: &Connection) -> Vec<String> {
        let mut v: Vec<String> = memory::all(conn).unwrap().into_iter().map(|m| m.content).collect();
        v.sort();
        v
    }

    #[test]
    fn adds_edits_and_deletes_propagate_and_converge() {
        let (da, db_) = (Database::open_in_memory().unwrap(), Database::open_in_memory().unwrap());
        let (a, b) = (da.conn().unwrap(), db_.conn().unwrap());
        let id = added(&a, "My main project is SkillTrack");
        added(&a, "I prefer dark mode");
        assert_eq!(sync(&a, A, &b, B).added, 2);
        assert_eq!(contents(&a), contents(&b));
        // Nothing new: nothing sent twice (idempotent).
        assert_eq!(sync(&a, A, &b, B), Applied::default());
        // B's copies are B's changes too (relayed on), but A ignores its own revisions coming back.
        let back = sync(&b, B, &a, A);
        assert_eq!((back.added, back.updated), (0, 0));

        update(&a, id, "My main project is SkillTrack v2", None).unwrap();
        assert_eq!(sync(&a, A, &b, B).updated, 1);
        assert!(contents(&b).contains(&"My main project is SkillTrack v2".to_string()));

        delete(&a, id).unwrap();
        assert_eq!(sync(&a, A, &b, B).deleted, 1);
        assert_eq!(contents(&a), contents(&b));
        assert_eq!(contents(&b), vec!["I prefer dark mode".to_string()]);
    }

    #[test]
    fn concurrent_edits_resolve_the_same_way_on_both_devices() {
        let (da, db_) = (Database::open_in_memory().unwrap(), Database::open_in_memory().unwrap());
        let (a, b) = (da.conn().unwrap(), db_.conn().unwrap());
        let id_a = added(&a, "Meeting room is 4B");
        sync(&a, A, &b, B);
        let id_b = memory::all(&b).unwrap()[0].id;
        // Both edit while offline (same revision): B has the greater device id, so B wins everywhere.
        update(&a, id_a, "Meeting room is 5C", None).unwrap();
        update(&b, id_b, "Meeting room is 6D", None).unwrap();
        sync(&a, A, &b, B);
        sync(&b, B, &a, A);
        assert_eq!(contents(&a), vec!["Meeting room is 6D".to_string()]);
        assert_eq!(contents(&a), contents(&b));
    }

    #[test]
    fn an_old_edit_cant_resurrect_a_delete() {
        let (da, db_) = (Database::open_in_memory().unwrap(), Database::open_in_memory().unwrap());
        let (a, b) = (da.conn().unwrap(), db_.conn().unwrap());
        let id_a = added(&a, "Temporary note");
        sync(&a, A, &b, B);
        let id_b = memory::all(&b).unwrap()[0].id;
        delete(&a, id_a).unwrap(); // revision 2 (tombstone)
        update(&b, id_b, "Temporary note edited", None).unwrap(); // revision 2 on B
        sync(&a, A, &b, B);
        sync(&b, B, &a, A);
        // Same revision: B (greater id) wins, on both — the edit survives on both.
        assert_eq!(contents(&a), contents(&b));
        // A later delete on A (revision 3) removes it everywhere.
        let id = memory::all(&a).unwrap()[0].id;
        delete(&a, id).unwrap();
        sync(&a, A, &b, B);
        assert!(contents(&b).is_empty());
        // And B's stale revision-2 edit replayed later is ignored.
        let stale = SyncItem { sync_id: memory_sync_id_of_tomb(&b), revision: 2, origin: B.into(), content: Some("zombie".into()), kind: None };
        assert_eq!(apply(&b, B, &[stale]).unwrap().ignored, 1);
        assert!(contents(&b).is_empty());
    }

    fn memory_sync_id_of_tomb(conn: &Connection) -> String {
        conn.query_row("SELECT sync_id FROM memory_tombstones LIMIT 1", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn untrusted_content_is_validated_like_local_input() {
        let d = Database::open_in_memory().unwrap();
        let c = d.conn().unwrap();
        let items = vec![
            SyncItem { sync_id: "1".repeat(32), revision: 1, origin: A.into(), content: Some("my password is hunter2-Secret!".into()), kind: None },
            SyncItem { sync_id: "2".repeat(32), revision: 1, origin: A.into(), content: Some("x".repeat(5000)), kind: None },
            SyncItem { sync_id: "../etc".into(), revision: 1, origin: A.into(), content: Some("ok".into()), kind: None },
            SyncItem { sync_id: "3".repeat(32), revision: 1, origin: A.into(), content: Some("Likes green tea".into()), kind: Some(MemoryKind::LongTerm) },
        ];
        let r = apply(&c, B, &items).unwrap();
        assert_eq!((r.added, r.refused), (1, 3));
        assert_eq!(contents(&c), vec!["Likes green tea".to_string()]);
    }

    #[test]
    fn batches_are_bounded_and_resume() {
        let (da, db_) = (Database::open_in_memory().unwrap(), Database::open_in_memory().unwrap());
        let (a, b) = (da.conn().unwrap(), db_.conn().unwrap());
        for i in 0..7 {
            added(&a, &format!("Fact number {i} about things"));
        }
        let (items, upto, more) = changes_since(&a, A, 0, 3).unwrap();
        assert_eq!((items.len(), more), (3, true));
        apply(&b, B, &items).unwrap();
        let (rest, _, more) = changes_since(&a, A, upto, 10).unwrap();
        assert_eq!((rest.len(), more), (4, false));
        apply(&b, B, &rest).unwrap();
        assert_eq!(contents(&a), contents(&b));
    }
}
