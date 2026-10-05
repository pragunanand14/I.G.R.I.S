//! Tasks, reminders/timers and the local calendar (Tasks page).

use serde::Serialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::productivity::events::{self, CalendarEvent, EventInput};
use crate::productivity::reminders::{self, Reminder};
use crate::productivity::tasks::{self, Filter, Task, TaskInput};
use crate::productivity::time;
use crate::state::AppState;

#[tauri::command]
pub fn list_tasks(state: State<'_, AppState>, filter: String) -> AppResult<Vec<Task>> {
    let f = match filter.as_str() {
        "open" => Filter::Open,
        "done" => Filter::Done,
        "all" => Filter::All,
        _ => return Err(AppError::validation("filter must be open, done or all")),
    };
    tasks::list(&*state.db.conn()?, f)
}

#[tauri::command]
pub fn add_task(state: State<'_, AppState>, task: TaskInput) -> AppResult<Task> {
    tasks::add(&*state.db.conn()?, &task)
}

#[tauri::command]
pub fn update_task(state: State<'_, AppState>, id: i64, task: TaskInput) -> AppResult<Task> {
    tasks::update(&*state.db.conn()?, id, &task)
}

#[tauri::command]
pub fn set_task_done(state: State<'_, AppState>, id: i64, done: bool) -> AppResult<Task> {
    tasks::set_done(&*state.db.conn()?, id, done)
}

#[tauri::command]
pub fn delete_task(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    tasks::remove(&*state.db.conn()?, id).map(|_| ())
}

#[tauri::command]
pub fn clear_done_tasks(state: State<'_, AppState>) -> AppResult<usize> {
    tasks::clear_done(&*state.db.conn()?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTime {
    /// UTC RFC 3339.
    pub at: String,
    pub description: String,
}

/// Resolve natural-language time input for live previews in the UI.
#[tauri::command]
pub fn parse_time(text: String) -> AppResult<ParsedTime> {
    let t = time::parse_when(&text).map_err(AppError::validation)?;
    Ok(ParsedTime { at: time::to_db(t), description: time::describe(t) })
}

#[tauri::command]
pub fn list_reminders(state: State<'_, AppState>) -> AppResult<Vec<Reminder>> {
    reminders::active(&*state.db.conn()?)
}

#[tauri::command]
pub fn reminder_history(state: State<'_, AppState>) -> AppResult<Vec<Reminder>> {
    reminders::history(&*state.db.conn()?, 50)
}

#[tauri::command]
pub fn add_reminder(state: State<'_, AppState>, title: String, when: String) -> AppResult<Reminder> {
    reminders::add_reminder(&*state.db.conn()?, &title, &when)
}

#[tauri::command]
pub fn start_timer(state: State<'_, AppState>, label: String, duration: String) -> AppResult<Reminder> {
    reminders::start_timer(&*state.db.conn()?, &label, &duration)
}

#[tauri::command]
pub fn cancel_reminder(state: State<'_, AppState>, id: i64) -> AppResult<Reminder> {
    reminders::cancel(&*state.db.conn()?, id)
}

#[tauri::command]
pub fn dismiss_reminder(state: State<'_, AppState>, id: i64) -> AppResult<Reminder> {
    reminders::dismiss(&*state.db.conn()?, id)
}

#[tauri::command]
pub fn snooze_reminder(state: State<'_, AppState>, id: i64, minutes: i64) -> AppResult<Reminder> {
    reminders::snooze(&*state.db.conn()?, id, minutes)
}

#[tauri::command]
pub fn clear_reminder_history(state: State<'_, AppState>) -> AppResult<usize> {
    reminders::clear_history(&*state.db.conn()?)
}

/// Events overlapping [from, to) (UTC RFC 3339).
#[tauri::command]
pub fn list_events(state: State<'_, AppState>, from: String, to: String) -> AppResult<Vec<CalendarEvent>> {
    let bad = || AppError::validation("from/to must be RFC 3339 times");
    let (f, t) = (time::from_db(&from).ok_or_else(bad)?, time::from_db(&to).ok_or_else(bad)?);
    if (t - f).num_days() > 400 {
        return Err(AppError::validation("Range too large"));
    }
    events::range(&*state.db.conn()?, f, t)
}

#[tauri::command]
pub fn add_event(state: State<'_, AppState>, event: EventInput) -> AppResult<CalendarEvent> {
    events::add(&*state.db.conn()?, &event)
}

#[tauri::command]
pub fn update_event(state: State<'_, AppState>, id: i64, event: EventInput) -> AppResult<CalendarEvent> {
    events::update(&*state.db.conn()?, id, &event)
}

#[tauri::command]
pub fn delete_event(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    events::remove(&*state.db.conn()?, id).map(|_| ())
}
