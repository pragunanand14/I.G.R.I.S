//! Google Calendar (the user's primary calendar): list, add, change, delete.

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};

use super::{seg, Google};
use crate::error::{AppError, AppResult};
use crate::productivity::time;

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub id: String,
    pub title: String,
    /// RFC 3339 for timed events; `None` with `all_day`.
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub all_day: bool,
    /// For all-day events: YYYY-MM-DD.
    pub date: Option<String>,
    pub location: String,
    pub attendees: Vec<String>,
    /// The user declined it.
    pub declined: bool,
}

fn when(v: &Value) -> Option<DateTime<Utc>> {
    v["dateTime"].as_str().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc))
}

fn event(v: &Value) -> Event {
    let attendees: Vec<&Value> = v["attendees"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    Event {
        id: v["id"].as_str().unwrap_or_default().into(),
        title: v["summary"].as_str().unwrap_or("(no title)").into(),
        start: when(&v["start"]),
        end: when(&v["end"]),
        all_day: v["start"]["date"].is_string(),
        date: v["start"]["date"].as_str().map(str::to_string),
        location: v["location"].as_str().unwrap_or_default().into(),
        attendees: attendees.iter().filter(|a| a["self"] != true).filter_map(|a| a["email"].as_str().map(str::to_string)).collect(),
        declined: attendees.iter().any(|a| a["self"] == true && a["responseStatus"] == "declined"),
    }
}

/// One line for the model / the user, in local time.
pub fn describe(e: &Event) -> String {
    let when = match (e.all_day, e.start, e.end) {
        (true, _, _) => format!("{} (all day)", e.date.clone().unwrap_or_default()),
        (false, Some(s), Some(end)) => format!("{} → {}", time::describe(s), end.with_timezone(&chrono::Local).format("%H:%M")),
        (false, Some(s), None) => time::describe(s),
        _ => "time unknown".into(),
    };
    let mut line = format!("[{}] {} — {when}", e.id, e.title);
    if !e.location.is_empty() {
        line.push_str(&format!(" @ {}", e.location));
    }
    if !e.attendees.is_empty() {
        line.push_str(&format!(" (with {})", e.attendees.join(", ")));
    }
    if e.declined {
        line.push_str(" [you declined]");
    }
    line
}

/// Events between `from` and `to`, in start order.
pub async fn list(g: &Google, from: DateTime<Utc>, to: DateTime<Utc>) -> AppResult<Vec<Event>> {
    let (a, b) = (from.to_rfc3339(), to.to_rfc3339());
    let v = g
        .get_json(
            &format!("{}/calendars/primary/events", g.ep.calendar),
            &[("timeMin", &a), ("timeMax", &b), ("singleEvents", "true"), ("orderBy", "startTime"), ("maxResults", "100")],
        )
        .await?;
    Ok(v["items"].as_array().into_iter().flatten().filter(|e| e["status"] != "cancelled").map(event).collect())
}

pub struct NewEvent<'a> {
    pub title: &'a str,
    /// Natural language or RFC 3339 ("tomorrow at 5pm").
    pub start: &'a str,
    pub minutes: i64,
    pub location: &'a str,
    pub description: &'a str,
    /// Invitees' emails: Google emails them an invitation.
    pub attendees: &'a [String],
}

fn times(start: &str, minutes: i64) -> AppResult<(DateTime<Utc>, DateTime<Utc>)> {
    let s = time::parse_when(start).map_err(AppError::validation)?;
    let minutes = if minutes <= 0 { 60 } else { minutes.min(24 * 60 * 14) };
    Ok((s, s + Duration::minutes(minutes)))
}

pub async fn add(g: &Google, e: NewEvent<'_>) -> AppResult<Event> {
    let title = e.title.trim();
    if title.is_empty() {
        return Err(AppError::validation("Say what the event is."));
    }
    let (s, end) = times(e.start, e.minutes)?;
    let mut body = json!({
        "summary": title,
        "start": { "dateTime": s.to_rfc3339() },
        "end": { "dateTime": end.to_rfc3339() },
    });
    if !e.location.trim().is_empty() {
        body["location"] = json!(e.location.trim());
    }
    if !e.description.trim().is_empty() {
        body["description"] = json!(e.description.trim());
    }
    let invite = !e.attendees.is_empty();
    if invite {
        body["attendees"] = json!(e.attendees.iter().map(|a| json!({ "email": a.trim() })).collect::<Vec<_>>());
    }
    let q: &[(&str, &str)] = if invite { &[("sendUpdates", "all")] } else { &[] };
    let v = g.post_json(&format!("{}/calendars/primary/events", g.ep.calendar), q, &body).await?;
    tracing::info!(event = "GCAL_EVENT_ADDED", invite);
    Ok(event(&v))
}

pub struct Change<'a> {
    pub title: Option<&'a str>,
    pub start: Option<&'a str>,
    pub minutes: Option<i64>,
    pub location: Option<&'a str>,
}

/// Change an event; attendees (if any) are told by Google.
pub async fn update(g: &Google, id: &str, c: Change<'_>) -> AppResult<Event> {
    let url = format!("{}/calendars/primary/events/{}", g.ep.calendar, seg(id));
    let mut body = json!({});
    if let Some(t) = c.title.map(str::trim).filter(|t| !t.is_empty()) {
        body["summary"] = json!(t);
    }
    if let Some(l) = c.location {
        body["location"] = json!(l.trim());
    }
    if c.start.is_some() || c.minutes.is_some() {
        let current = event(&g.get_json(&url, &[]).await?);
        let length = match (current.start, current.end) {
            (Some(s), Some(e)) => (e - s).num_minutes(),
            _ => 60,
        };
        let (s, e) = match c.start.filter(|s| !s.trim().is_empty()) {
            Some(start) => times(start, c.minutes.unwrap_or(length))?,
            None => {
                let s = current.start.ok_or_else(|| AppError::validation("That's an all-day event; give it a start time."))?;
                (s, s + Duration::minutes(c.minutes.unwrap_or(length).max(1)))
            }
        };
        body["start"] = json!({ "dateTime": s.to_rfc3339() });
        body["end"] = json!({ "dateTime": e.to_rfc3339() });
    }
    if body.as_object().is_some_and(|o| o.is_empty()) {
        return Err(AppError::validation("Say what to change."));
    }
    let v = g.patch_json(&url, &[("sendUpdates", "all")], &body).await?;
    Ok(event(&v))
}

pub async fn delete(g: &Google, id: &str) -> AppResult<()> {
    g.delete(&format!("{}/calendars/primary/events/{}", g.ep.calendar, seg(id)), &[("sendUpdates", "all")]).await
}
