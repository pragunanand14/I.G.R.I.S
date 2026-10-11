//! The daily briefing: a routine IGRIS sets up from a few choices (time,
//! what to include, news topics). Each morning it runs like any routine; its
//! text is the notification, and Home shows the latest one with "Listen".

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::routines::{self, schedule::Kind, Run, Schedule};

pub const NAME: &str = "Daily briefing";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Briefing {
    pub enabled: bool,
    /// "HH:MM", local.
    pub time: String,
    pub weekdays_only: bool,
    pub weather: bool,
    pub calendar: bool,
    pub email: bool,
    pub tasks: bool,
    /// News on these topics (needs web search).
    pub news: Vec<String>,
    /// The routine it runs as.
    pub routine_id: Option<i64>,
}

impl Default for Briefing {
    fn default() -> Self {
        Self {
            enabled: false,
            time: "07:30".into(),
            weekdays_only: false,
            weather: true,
            calendar: true,
            email: true,
            tasks: true,
            news: Vec::new(),
            routine_id: None,
        }
    }
}

impl Briefing {
    /// What the routine is told to do, from the choices.
    pub fn instruction(&self) -> String {
        let mut parts = Vec::new();
        if self.weather {
            parts.push("the weather for my home place (get_weather)".to_string());
        }
        if self.calendar {
            parts.push("today's meetings and events (Google Calendar with calendar_events if connected, otherwise list_events)".to_string());
        }
        if self.email {
            parts.push(
                "important unread emails from the last day (gmail_search \"is:unread newer_than:1d\", skip newsletters and promotions) — who, \
and what they need from me"
                    .to_string(),
            );
        }
        if self.tasks {
            parts.push("my open to-dos due soon, today's reminders, and birthdays in the next few days (find_people)".to_string());
        }
        if !self.news.is_empty() {
            parts.push(format!("the top news on {} (web_search; a line or two each)", self.news.join(", ")));
        }
        format!(
            "Give me my daily briefing: {}. Make it short and easy to listen to — a few spoken-style sentences, under 150 words, most \
important first. Skip any part that isn't available (e.g. Google not connected) without explaining tools.",
            parts.join("; ")
        )
    }

    fn validate(&mut self) -> AppResult<()> {
        self.news = self.news.iter().map(|t| t.trim().chars().take(60).collect::<String>()).filter(|t| !t.is_empty()).take(5).collect();
        if !(self.weather || self.calendar || self.email || self.tasks || !self.news.is_empty()) {
            return Err(AppError::validation("Choose at least one thing for the briefing."));
        }
        Ok(())
    }
}

pub fn get(conn: &Connection) -> AppResult<Briefing> {
    let mut b: Briefing = crate::settings::get_extra(conn, "briefing")?.unwrap_or_default();
    // The routine may have been deleted on the Routines page.
    if let Some(id) = b.routine_id {
        match routines::get(conn, id)? {
            Some(r) => b.enabled = r.enabled,
            None => {
                b.routine_id = None;
                b.enabled = false;
            }
        }
    }
    Ok(b)
}

/// Save the choices and create / update / pause the routine to match.
pub fn set(conn: &Connection, mut b: Briefing) -> AppResult<Briefing> {
    b.validate()?;
    let schedule = Schedule { kind: if b.weekdays_only { Kind::Weekdays } else { Kind::Daily }, time: b.time.trim().into(), ..Default::default() };
    schedule.validate().map_err(AppError::validation)?;
    let existing = get(conn)?.routine_id.and_then(|id| routines::get(conn, id).ok().flatten());
    b.routine_id = match (existing, b.enabled) {
        (Some(r), true) => {
            routines::update(conn, r.id, Some(NAME), Some(&b.instruction()), Some(schedule))?;
            routines::set_enabled(conn, r.id, true)?;
            Some(r.id)
        }
        (Some(r), false) => {
            routines::update(conn, r.id, Some(NAME), Some(&b.instruction()), Some(schedule))?;
            routines::set_enabled(conn, r.id, false)?;
            Some(r.id)
        }
        (None, true) => Some(routines::create(conn, NAME, &b.instruction(), schedule)?.id),
        (None, false) => None,
    };
    crate::settings::set_extra(conn, "briefing", &b)?;
    Ok(b)
}

/// Today's briefing, if it has run today.
pub fn today(conn: &Connection) -> AppResult<Option<Run>> {
    let Some(id) = get(conn)?.routine_id else { return Ok(None) };
    let today = chrono::Local::now().date_naive();
    Ok(routines::runs(conn, Some(id), 5)?.into_iter().find(|r| {
        r.status == "done" && crate::productivity::time::from_db(&r.started_at).is_some_and(|t| t.with_timezone(&chrono::Local).date_naive() == today)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn choices_become_a_routine_and_follow_it() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        assert!(!get(&c).unwrap().enabled);
        let b = set(&c, Briefing { enabled: true, time: "07:15".into(), news: vec!["cricket".into(), " AI ".into()], ..Default::default() }).unwrap();
        let r = routines::get(&c, b.routine_id.unwrap()).unwrap().unwrap();
        assert_eq!((r.name.as_str(), r.when.as_str(), r.enabled), (NAME, "Every day at 07:15", true));
        assert!(r.instruction.contains("get_weather") && r.instruction.contains("cricket, AI"));

        // Weekdays only, no email: the same routine is updated.
        let b = set(&c, Briefing { email: false, weekdays_only: true, ..b }).unwrap();
        let r2 = routines::get(&c, b.routine_id.unwrap()).unwrap().unwrap();
        assert_eq!((r2.id, r2.when.as_str()), (r.id, "Every weekday at 07:15"));
        assert!(!r2.instruction.contains("gmail_search"));
        assert_eq!(routines::list(&c).unwrap().len(), 1);

        // Off pauses it; deleting the routine elsewhere turns the briefing off.
        set(&c, Briefing { enabled: false, ..b.clone() }).unwrap();
        assert!(!routines::get(&c, r.id).unwrap().unwrap().enabled);
        routines::delete(&c, r.id).unwrap();
        assert!(!get(&c).unwrap().enabled && get(&c).unwrap().routine_id.is_none());

        assert!(set(&c, Briefing { enabled: true, weather: false, calendar: false, email: false, tasks: false, ..Default::default() }).is_err());
        assert!(set(&c, Briefing { enabled: true, time: "7".into(), ..Default::default() }).is_err());
    }
}
