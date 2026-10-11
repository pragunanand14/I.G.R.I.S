//! Reminders before Google Calendar meetings.
//!
//! Every few minutes, upcoming timed events (not ones the user declined) get
//! an IGRIS reminder ten minutes before they start — so they ring like any
//! reminder: in-app, as a system notification, and on the phone even when
//! IGRIS is closed. Moved events move their reminder; removed ones drop it.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, OptionalExtension};

use super::{calendar, Google};
use crate::error::AppResult;
use crate::productivity::{reminders, time};

/// How far ahead to look, and how early to remind.
const LOOKAHEAD: i64 = 3 * 60;
const BEFORE: i64 = 10;
const EVERY: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Check every few minutes while IGRIS runs (the platform spawns this).
pub async fn run(g: Arc<Google>) {
    tokio::time::sleep(std::time::Duration::from_secs(20)).await;
    loop {
        if g.is_connected() {
            match tick(&g, Utc::now()).await {
                Ok(n) if n > 0 => tracing::info!(event = "MEETING_REMINDERS_SET", count = n),
                Ok(_) => {}
                Err(e) => tracing::warn!(event = "MEETING_REMINDERS_FAILED", error = %e),
            }
        }
        tokio::time::sleep(EVERY).await;
    }
}

/// One pass. Returns how many reminders were set or moved.
pub async fn tick(g: &Google, now: DateTime<Utc>) -> AppResult<usize> {
    let events = calendar::list(g, now, now + Duration::minutes(LOOKAHEAD)).await?;
    let conn = g.db().conn()?;
    let mut changed = 0;
    let mut seen = Vec::new();
    for e in events.iter().filter(|e| !e.all_day && !e.declined) {
        let Some(start) = e.start else { continue };
        if start <= now + Duration::minutes(1) {
            continue;
        }
        seen.push(e.id.clone());
        let known: Option<(String, i64)> = conn
            .query_row("SELECT starts_at, reminder_id FROM google_meeting_reminders WHERE event_id = ?1", [&e.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let start_s = time::to_db(start);
        match known {
            Some((s, _)) if s == start_s => continue,
            Some((_, rid)) => {
                let _ = reminders::cancel(&conn, rid);
            }
            None => {}
        }
        let due = (start - Duration::minutes(BEFORE)).max(now + Duration::seconds(30));
        let at = start.with_timezone(&chrono::Local).format("%H:%M");
        let place = if e.location.is_empty() { String::new() } else { format!(" · {}", e.location) };
        let r = reminders::add_at(&conn, &format!("{} at {at}{place}", e.title), due)?;
        conn.execute("INSERT OR REPLACE INTO google_meeting_reminders (event_id, starts_at, reminder_id) VALUES (?1, ?2, ?3)", params![e.id, start_s, r.id])?;
        changed += 1;
    }
    // Events in the window that are gone (deleted, declined, moved far away): drop their reminder.
    let horizon = time::to_db(now + Duration::minutes(LOOKAHEAD));
    let mut stmt = conn.prepare("SELECT event_id, reminder_id FROM google_meeting_reminders WHERE starts_at > ?1 AND starts_at <= ?2")?;
    let stale: Vec<(String, i64)> = stmt
        .query_map(params![time::to_db(now + Duration::minutes(1)), horizon], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(id, _)| !seen.contains(id))
        .collect();
    for (id, rid) in stale {
        let _ = reminders::cancel(&conn, rid);
        conn.execute("DELETE FROM google_meeting_reminders WHERE event_id = ?1", [&id])?;
    }
    conn.execute("DELETE FROM google_meeting_reminders WHERE starts_at < ?1", [time::to_db(now - Duration::days(1))])?;
    Ok(changed)
}
