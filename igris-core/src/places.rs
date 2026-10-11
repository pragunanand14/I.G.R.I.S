//! Places the user named ("home", "office", "gym") and reminders for arriving
//! at or leaving one. The phone saves a place where it is and watches for the
//! reminders (Android proximity alerts); this is the record of both.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::productivity::time;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceReminder {
    pub id: i64,
    pub place: String,
    /// true: on arriving; false: on leaving.
    pub on_arrive: bool,
    pub text: String,
    /// active, done, cancelled
    pub status: String,
}

fn name_ok(name: &str) -> AppResult<String> {
    let n: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_lowercase();
    if n.is_empty() || n.chars().count() > 40 {
        return Err(AppError::validation("Give the place a short name, like \"office\"."));
    }
    Ok(n)
}

pub fn save(conn: &Connection, name: &str, latitude: f64, longitude: f64) -> AppResult<Place> {
    let name = name_ok(name)?;
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return Err(AppError::validation("That location isn't valid."));
    }
    conn.execute(
        "INSERT INTO places (name, latitude, longitude, created_at) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT (name) DO UPDATE SET latitude = excluded.latitude, longitude = excluded.longitude",
        params![name, latitude, longitude, time::now_db()],
    )?;
    Ok(Place { name, latitude, longitude })
}

pub fn get(conn: &Connection, name: &str) -> AppResult<Option<Place>> {
    let n = name_ok(name)?;
    Ok(conn
        .query_row("SELECT name, latitude, longitude FROM places WHERE name = ?1", [n], |r| {
            Ok(Place { name: r.get(0)?, latitude: r.get(1)?, longitude: r.get(2)? })
        })
        .optional()?)
}

pub fn list(conn: &Connection) -> AppResult<Vec<Place>> {
    let mut stmt = conn.prepare("SELECT name, latitude, longitude FROM places ORDER BY name")?;
    let rows = stmt.query_map([], |r| Ok(Place { name: r.get(0)?, latitude: r.get(1)?, longitude: r.get(2)? }))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn forget(conn: &Connection, name: &str) -> AppResult<()> {
    if conn.execute("DELETE FROM places WHERE name = ?1", [name_ok(name)?])? == 0 {
        return Err(AppError::validation("There's no saved place with that name."));
    }
    Ok(())
}

/// Record a reminder for a saved place (the phone then watches for it).
pub fn add_reminder(conn: &Connection, place: &str, on_arrive: bool, text: &str) -> AppResult<PlaceReminder> {
    let p = get(conn, place)?
        .ok_or_else(|| AppError::validation(format!("\"{place}\" isn't a saved place. Save it first (be there and say \"save this place as {place}\").")))?;
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 200 {
        return Err(AppError::validation("Say what to remind you about (up to 200 characters)."));
    }
    conn.execute(
        "INSERT INTO place_reminders (place, on_arrive, text, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![p.name, on_arrive as i64, text, time::now_db()],
    )?;
    Ok(PlaceReminder { id: conn.last_insert_rowid(), place: p.name, on_arrive, text: text.into(), status: "active".into() })
}

pub fn reminders(conn: &Connection, active_only: bool) -> AppResult<Vec<PlaceReminder>> {
    let mut stmt =
        conn.prepare("SELECT id, place, on_arrive, text, status FROM place_reminders WHERE (?1 = 0 OR status = 'active') ORDER BY id DESC LIMIT 50")?;
    let rows = stmt
        .query_map([active_only as i64], |r| {
            Ok(PlaceReminder { id: r.get(0)?, place: r.get(1)?, on_arrive: r.get::<_, i64>(2)? != 0, text: r.get(3)?, status: r.get(4)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn set_status(conn: &Connection, id: i64, status: &str) -> AppResult<()> {
    if conn.execute("UPDATE place_reminders SET status = ?1 WHERE id = ?2", params![status, id])? == 0 {
        return Err(AppError::validation(format!("There's no location reminder #{id}.")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn places_and_reminders() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        save(&c, " Office ", 19.1, 72.9).unwrap();
        save(&c, "office", 19.2, 72.8).unwrap(); // moved
        assert_eq!(list(&c).unwrap(), vec![Place { name: "office".into(), latitude: 19.2, longitude: 72.8 }]);
        assert!(add_reminder(&c, "gym", true, "stretch").unwrap_err().to_string().contains("isn't a saved place"));
        let r = add_reminder(&c, "OFFICE", false, "Buy milk").unwrap();
        assert_eq!((r.place.as_str(), r.on_arrive), ("office", false));
        set_status(&c, r.id, "done").unwrap();
        assert!(reminders(&c, true).unwrap().is_empty());
        assert_eq!(reminders(&c, false).unwrap().len(), 1);
        forget(&c, "office").unwrap();
        assert!(save(&c, "", 0.0, 0.0).is_err() && save(&c, "x", 200.0, 0.0).is_err());
    }
}
