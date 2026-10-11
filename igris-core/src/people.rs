//! The people in the user's life: who they are to the user ("mom", "my
//! manager"), their birthday and a few notes ("vegetarian", "likes jazz").
//! IGRIS looks them up when the user mentions someone, and the briefing
//! mentions birthdays coming up. Notes that look like credentials or ID
//! numbers are refused, like memories.

use chrono::{Datelike, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::productivity::time;

pub const MAX_PEOPLE: i64 = 500;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub relation: String,
    /// "MM-DD" or "YYYY-MM-DD"; "" when not known.
    pub birthday: String,
    pub notes: String,
}

/// What to change; `None` keeps the current value.
#[derive(Debug, Clone, Default)]
pub struct Change<'a> {
    pub relation: Option<&'a str>,
    pub birthday: Option<&'a str>,
    pub notes: Option<&'a str>,
}

fn row(r: &Row) -> rusqlite::Result<Person> {
    Ok(Person { id: r.get(0)?, name: r.get(1)?, relation: r.get(2)?, birthday: r.get(3)?, notes: r.get(4)? })
}

const COLS: &str = "id, name, relation, birthday, notes";

fn one_line(s: &str, max: usize, what: &str) -> AppResult<String> {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > max {
        return Err(AppError::validation(format!("The {what} must be at most {max} characters.")));
    }
    if let Some(kind) = crate::memory::sensitive::detect(&s) {
        return Err(AppError::validation(format!("This looks like {}. IGRIS doesn't store that.", kind.describe())));
    }
    Ok(s)
}

/// "MM-DD" or "YYYY-MM-DD" (also "DD/MM" is not guessed: ambiguous).
pub fn parse_birthday(s: &str) -> AppResult<String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(String::new());
    }
    let bad = || AppError::validation("Give the birthday as MM-DD or YYYY-MM-DD.");
    let parts: Vec<&str> = s.split('-').collect();
    let (y, m, d) = match parts.as_slice() {
        [m, d] => (None, m, d),
        [y, m, d] => (Some(y.parse::<i32>().map_err(|_| bad())?), m, d),
        _ => return Err(bad()),
    };
    let (m, d): (u32, u32) = (m.parse().map_err(|_| bad())?, d.parse().map_err(|_| bad())?);
    // Year 2000 is a leap year, so 02-29 is accepted without a year.
    NaiveDate::from_ymd_opt(y.unwrap_or(2000), m, d).ok_or_else(bad)?;
    if let Some(y) = y {
        if !(1900..=chrono::Local::now().year()).contains(&y) {
            return Err(bad());
        }
        return Ok(format!("{y:04}-{m:02}-{d:02}"));
    }
    Ok(format!("{m:02}-{d:02}"))
}

pub fn get(conn: &Connection, name: &str) -> AppResult<Option<Person>> {
    Ok(conn.query_row(&format!("SELECT {COLS} FROM people WHERE name = ?1"), [name.trim()], row).optional()?)
}

/// Add someone, or change what's known about them.
pub fn save(conn: &Connection, name: &str, c: Change) -> AppResult<Person> {
    let name = one_line(name, 60, "name")?;
    if name.is_empty() {
        return Err(AppError::validation("Give the person's name."));
    }
    let existing = get(conn, &name)?;
    let pick = |new: Option<&str>, old: Option<&String>, max, what| -> AppResult<String> {
        match new {
            Some(v) => one_line(v, max, what),
            None => Ok(old.cloned().unwrap_or_default()),
        }
    };
    let relation = pick(c.relation, existing.as_ref().map(|p| &p.relation), 60, "relation")?;
    let notes = pick(c.notes, existing.as_ref().map(|p| &p.notes), 500, "notes")?;
    let birthday = match c.birthday {
        Some(b) => parse_birthday(b)?,
        None => existing.as_ref().map(|p| p.birthday.clone()).unwrap_or_default(),
    };
    if existing.is_none() {
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM people", [], |r| r.get(0))?;
        if n >= MAX_PEOPLE {
            return Err(AppError::validation(format!("IGRIS already knows {MAX_PEOPLE} people. Remove some first.")));
        }
    }
    conn.execute(
        "INSERT INTO people (name, relation, birthday, notes, updated_at) VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT (name) DO UPDATE SET relation = excluded.relation, birthday = excluded.birthday, notes = excluded.notes, \
         updated_at = excluded.updated_at",
        params![name, relation, birthday, notes, time::now_db()],
    )?;
    get(conn, &name)?.ok_or_else(|| AppError::internal("person not saved"))
}

pub fn list(conn: &Connection) -> AppResult<Vec<Person>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM people ORDER BY name COLLATE NOCASE"))?;
    let rows = stmt.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// People whose name, relation or notes contain any word of `query`
/// ("mom", "Rahul", "manager"); everyone when it's empty.
pub fn find(conn: &Connection, query: &str) -> AppResult<Vec<Person>> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()).filter(|w| w.len() > 1).collect();
    let all = list(conn)?;
    if words.is_empty() {
        return Ok(all);
    }
    Ok(all
        .into_iter()
        .filter(|p| {
            let hay = format!("{} {} {}", p.name, p.relation, p.notes).to_lowercase();
            words.iter().any(|w| hay.contains(w.as_str()))
        })
        .collect())
}

pub fn forget(conn: &Connection, name: &str) -> AppResult<()> {
    if conn.execute("DELETE FROM people WHERE name = ?1", [name.trim()])? == 0 {
        return Err(AppError::validation(format!("IGRIS doesn't know anyone called \"{}\".", name.trim())));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Upcoming {
    pub person: Person,
    /// 0 = today.
    pub in_days: i64,
    /// The age they turn, when the birth year is known.
    pub turns: Option<i32>,
}

/// Birthdays within `days` of `today`, soonest first.
pub fn upcoming_birthdays(conn: &Connection, today: NaiveDate, days: i64) -> AppResult<Vec<Upcoming>> {
    let mut out = Vec::new();
    for p in list(conn)? {
        let parts: Vec<&str> = p.birthday.split('-').collect();
        let (year, m, d) = match parts.as_slice() {
            [m, d] => (None, m, d),
            [y, m, d] => (y.parse::<i32>().ok(), m, d),
            _ => continue,
        };
        let (Ok(m), Ok(d)) = (m.parse::<u32>(), d.parse::<u32>()) else { continue };
        let on = |y: i32| NaiveDate::from_ymd_opt(y, m, d).or_else(|| NaiveDate::from_ymd_opt(y, 2, 28));
        let Some(mut next) = on(today.year()) else { continue };
        if next < today {
            let Some(n) = on(today.year() + 1) else { continue };
            next = n;
        }
        let in_days = (next - today).num_days();
        if in_days <= days {
            out.push(Upcoming { turns: year.map(|y| next.year() - y), person: p, in_days });
        }
    }
    out.sort_by_key(|u| u.in_days);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn people_and_birthdays() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        save(&c, "Mom", Change { relation: Some("mother"), birthday: Some("1968-03-04"), notes: Some("Loves orchids") }).unwrap();
        save(&c, "Rahul", Change { relation: Some("manager"), ..Default::default() }).unwrap();
        // Changing one thing keeps the rest.
        let r = save(&c, "rahul", Change { birthday: Some("10-12"), ..Default::default() }).unwrap();
        assert_eq!((r.name.as_str(), r.relation.as_str(), r.birthday.as_str()), ("Rahul", "manager", "10-12"));
        assert_eq!(find(&c, "my manager").unwrap().len(), 1);
        assert_eq!(find(&c, "orchids").unwrap()[0].name, "Mom");
        assert_eq!(find(&c, "").unwrap().len(), 2);

        let today = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();
        let up = upcoming_birthdays(&c, today, 7).unwrap();
        assert_eq!((up.len(), up[0].in_days, up[0].turns), (1, 1, None));
        let mar = NaiveDate::from_ymd_opt(2027, 3, 1).unwrap();
        assert_eq!(upcoming_birthdays(&c, mar, 7).unwrap()[0].turns, Some(59));

        assert!(parse_birthday("31-12").is_err() && parse_birthday("02-30").is_err() && parse_birthday("tomorrow").is_err());
        assert_eq!(parse_birthday("02-29").unwrap(), "02-29");
        assert!(save(&c, "X", Change { notes: Some("password: hunter2 and card 4111 1111 1111 1111"), ..Default::default() }).is_err());
        forget(&c, "RAHUL").unwrap();
        assert!(forget(&c, "Rahul").is_err());
    }
}
