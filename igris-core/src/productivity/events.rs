//! Local calendar events. Sync with external calendars (Google, Outlook) is
//! not implemented — it needs OAuth app registration.
// TODO(phase-8+): calendar sync via OAuth (Google Calendar / Microsoft Graph).

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use super::{clean, time};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: i64,
    pub title: String,
    pub starts_at: String,
    pub ends_at: Option<String>,
    pub all_day: bool,
    pub location: String,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventInput {
    pub title: String,
    /// UTC RFC 3339. For all-day events, any instant on the local day.
    pub starts_at: String,
    #[serde(default)]
    pub ends_at: String,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub notes: String,
}

const COLS: &str = "id, title, starts_at, ends_at, all_day, location, notes, created_at, updated_at";

fn row(r: &Row) -> rusqlite::Result<CalendarEvent> {
    Ok(CalendarEvent {
        id: r.get(0)?,
        title: r.get(1)?,
        starts_at: r.get(2)?,
        ends_at: r.get(3)?,
        all_day: r.get(4)?,
        location: r.get(5)?,
        notes: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

struct Valid {
    title: String,
    starts_at: String,
    ends_at: Option<String>,
    all_day: bool,
    location: String,
    notes: String,
}

/// Start of the local day containing `t`, as UTC.
pub fn local_day_start(t: DateTime<Utc>) -> DateTime<Utc> {
    let d = t.with_timezone(&Local).date_naive();
    time::localize(d.and_hms_opt(0, 0, 0).unwrap()).with_timezone(&Utc)
}

fn validate(input: &EventInput) -> AppResult<Valid> {
    let title = clean(&input.title, 200, "Title")?;
    if title.is_empty() {
        return Err(AppError::validation("Give the event a title."));
    }
    let bad = |what: &str| AppError::validation(format!("The {what} time isn't valid."));
    let mut start = time::from_db(input.starts_at.trim()).ok_or_else(|| bad("start"))?;
    let mut end = match input.ends_at.trim() {
        "" => None,
        s => Some(time::from_db(s).ok_or_else(|| bad("end"))?),
    };
    if input.all_day {
        start = local_day_start(start);
        end = end.map(|e| local_day_start(e).max(start));
    }
    if let Some(e) = end {
        if e < start {
            return Err(AppError::validation("The event can't end before it starts."));
        }
    }
    Ok(Valid {
        title,
        starts_at: time::to_db(start),
        ends_at: end.map(time::to_db),
        all_day: input.all_day,
        location: clean(&input.location, 200, "Location")?,
        notes: clean(&input.notes, 4000, "Notes")?,
    })
}

pub fn get(conn: &Connection, id: i64) -> AppResult<Option<CalendarEvent>> {
    Ok(conn.query_row(&format!("SELECT {COLS} FROM events WHERE id = ?1"), [id], row).optional()?)
}

fn must_get(conn: &Connection, id: i64) -> AppResult<CalendarEvent> {
    get(conn, id)?.ok_or_else(|| AppError::validation(format!("There's no event #{id}.")))
}

/// Events overlapping [from, to), by start time.
pub fn range(conn: &Connection, from: DateTime<Utc>, to: DateTime<Utc>) -> AppResult<Vec<CalendarEvent>> {
    // All-day events without an end occupy their whole day.
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM events WHERE starts_at < ?2 AND \
         COALESCE(ends_at, CASE WHEN all_day THEN strftime('%Y-%m-%dT%H:%M:%SZ', starts_at, '+1 day') ELSE starts_at END) >= ?1 \
         ORDER BY starts_at, id"
    ))?;
    let rows = stmt.query_map([time::to_db(from), time::to_db(to)], row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn add(conn: &Connection, input: &EventInput) -> AppResult<CalendarEvent> {
    let v = validate(input)?;
    conn.execute(
        "INSERT INTO events (title, starts_at, ends_at, all_day, location, notes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![v.title, v.starts_at, v.ends_at, v.all_day, v.location, v.notes],
    )?;
    must_get(conn, conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, input: &EventInput) -> AppResult<CalendarEvent> {
    let v = validate(input)?;
    let n = conn.execute(
        "UPDATE events SET title = ?1, starts_at = ?2, ends_at = ?3, all_day = ?4, location = ?5, notes = ?6, updated_at = ?7 WHERE id = ?8",
        params![v.title, v.starts_at, v.ends_at, v.all_day, v.location, v.notes, time::now_db(), id],
    )?;
    if n == 0 {
        return Err(AppError::validation(format!("There's no event #{id}.")));
    }
    must_get(conn, id)
}

pub fn remove(conn: &Connection, id: i64) -> AppResult<CalendarEvent> {
    let e = must_get(conn, id)?;
    conn.execute("DELETE FROM events WHERE id = ?1", [id])?;
    Ok(e)
}

/// "today", "tomorrow", "this week" (next 7 days), "YYYY-MM-DD" → local [from, to).
pub fn parse_range(spec: &str) -> Result<(DateTime<Utc>, DateTime<Utc>), String> {
    let today = Local::now().date_naive();
    let day = |d: NaiveDate| time::localize(d.and_hms_opt(0, 0, 0).unwrap()).with_timezone(&Utc);
    let (from, days) = match spec.trim().to_lowercase().as_str() {
        "" | "today" => (today, 1),
        "tomorrow" => (today + Duration::days(1), 1),
        "week" | "this week" | "next 7 days" => (today, 7),
        "month" | "next 30 days" => (today, 30),
        s => (
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| format!("Use today, tomorrow, this week, next 30 days or a date like 2026-10-06 (got \"{s}\")."))?,
            1,
        ),
    };
    Ok((day(from), day(from + Duration::days(days))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn ev(title: &str, start: &str, end: &str, all_day: bool) -> EventInput {
        EventInput { title: title.into(), starts_at: start.into(), ends_at: end.into(), all_day, ..Default::default() }
    }

    fn utc(s: &str) -> DateTime<Utc> {
        time::from_db(s).unwrap()
    }

    #[test]
    fn crud_and_range() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        let a = add(&c, &ev("Standup", "2026-10-06T09:00:00Z", "2026-10-06T09:15:00Z", false)).unwrap();
        let b = add(&c, &ev("Long", "2026-10-05T22:00:00Z", "2026-10-07T02:00:00Z", false)).unwrap();
        let p = add(&c, &ev("Point", "2026-10-08T12:00:00Z", "", false)).unwrap();

        let ids = |from: &str, to: &str| range(&c, utc(from), utc(to)).unwrap().into_iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(ids("2026-10-06T00:00:00Z", "2026-10-07T00:00:00Z"), [b.id, a.id]);
        assert_eq!(ids("2026-10-07T01:00:00Z", "2026-10-08T00:00:00Z"), [b.id]); // still running
        assert_eq!(ids("2026-10-08T00:00:00Z", "2026-10-09T00:00:00Z"), [p.id]);
        assert!(ids("2026-10-09T00:00:00Z", "2026-10-10T00:00:00Z").is_empty());

        let moved = update(&c, a.id, &ev("Standup (moved)", "2026-10-09T09:00:00Z", "", false)).unwrap();
        assert_eq!(moved.title, "Standup (moved)");
        remove(&c, p.id).unwrap();
        assert!(get(&c, p.id).unwrap().is_none());
    }

    #[test]
    fn all_day_events_cover_their_day() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        let e = add(&c, &ev("Holiday", "2026-10-06T15:00:00Z", "", true)).unwrap();
        let start = time::from_db(&e.starts_at).unwrap();
        assert_eq!(start, local_day_start(utc("2026-10-06T15:00:00Z")));
        assert_eq!(range(&c, start + Duration::hours(20), start + Duration::hours(21)).unwrap().len(), 1);
        assert!(range(&c, start + Duration::hours(25), start + Duration::hours(26)).unwrap().is_empty());
    }

    #[test]
    fn validation_and_ranges() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        assert!(add(&c, &ev("", "2026-10-06T09:00:00Z", "", false)).is_err());
        assert!(add(&c, &ev("x", "tomorrow", "", false)).is_err());
        assert!(add(&c, &ev("x", "2026-10-06T09:00:00Z", "2026-10-06T08:00:00Z", false)).unwrap_err().to_string().contains("end before"));
        let (f, t) = parse_range("this week").unwrap();
        assert!((167..=169).contains(&(t - f).num_hours())); // DST-safe
        assert!(parse_range("2026-10-06").is_ok());
        assert!(parse_range("someday").is_err());
    }
}
