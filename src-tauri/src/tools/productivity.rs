//! Tools for tasks, reminders, timers and the local calendar. Times are
//! natural language resolved by the backend clock, and every result echoes the
//! resolved time so the model never has to guess "now".

use std::sync::Arc;

use chrono::{Local, Utc};
use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::db::Database;
use crate::error::AppError;
use crate::productivity::events::{self, CalendarEvent, EventInput};
use crate::productivity::reminders::{self, Reminder};
use crate::productivity::tasks::{self, Filter, Task, TaskInput};
use crate::productivity::time;

fn app_err(e: AppError) -> ToolError {
    match e {
        AppError::Validation(m) => ToolError::invalid(m),
        other => ToolError::failed(other.to_string()),
    }
}

fn with_conn<T>(db: &Database, f: impl FnOnce(&rusqlite::Connection) -> Result<T, AppError>) -> Result<T, ToolError> {
    let conn = db.conn().map_err(app_err)?;
    f(&conn).map_err(app_err)
}

fn out(content: String, summary: impl Into<String>) -> ToolResultT {
    Ok(ToolOutput { content, summary: summary.into(), sources: vec![] })
}

fn s<'a>(input: &'a Value, key: &str) -> &'a str {
    input[key].as_str().unwrap_or_default().trim()
}

fn id_schema(what: &str) -> Value {
    json!({ "type": "integer", "minimum": 1, "description": format!("The {what}'s id (from the list tool)") })
}

fn text(max: u32, description: &str) -> Value {
    json!({ "type": "string", "maxLength": max, "description": description })
}

const WHEN_HELP: &str = "Natural language or ISO 8601 local time, e.g. \"in 20 minutes\", \"tomorrow at 5pm\", \"friday 9:30am\", \"2026-10-06 17:00\"";

fn when(t: Option<&str>) -> String {
    t.and_then(time::from_db).map(time::describe).unwrap_or_else(|| "no due date".into())
}

fn task_line(t: &Task) -> String {
    let prio = if t.priority == "normal" { String::new() } else { format!(", {} priority", t.priority) };
    let overdue = !t.done && t.due_at.as_deref().and_then(time::from_db).is_some_and(|d| d < Utc::now());
    format!(
        "#{} [{}] {} — {}{}{}{}",
        t.id,
        if t.done { "done" } else { "open" },
        t.title,
        if t.due_at.is_some() { format!("due {}", when(t.due_at.as_deref())) } else { "no due date".into() },
        prio,
        if overdue { " — OVERDUE" } else { "" },
        if t.notes.is_empty() { String::new() } else { format!("\n    notes: {}", t.notes) }
    )
}

fn reminder_line(r: &Reminder) -> String {
    let state = match r.status.as_str() {
        "fired" => "ringing now".to_string(),
        _ => when(Some(&r.due_at)),
    };
    format!("#{} {} \"{}\" — {}", r.id, r.kind, r.title, state)
}

fn event_line(e: &CalendarEvent) -> String {
    let start = time::from_db(&e.starts_at).map(|t| t.with_timezone(&Local));
    let span = match (start, e.all_day) {
        (Some(s), true) => format!("{} (all day)", s.format("%a %-d %b")),
        (Some(s), false) => {
            let end = e.ends_at.as_deref().and_then(time::from_db).map(|t| t.with_timezone(&Local));
            match end {
                Some(en) if en.date_naive() == s.date_naive() => format!("{}–{}", s.format("%a %-d %b, %H:%M"), en.format("%H:%M")),
                Some(en) => format!("{} – {}", s.format("%a %-d %b, %H:%M"), en.format("%a %-d %b, %H:%M")),
                None => s.format("%a %-d %b, %H:%M").to_string(),
            }
        }
        _ => e.starts_at.clone(),
    };
    let loc = if e.location.is_empty() { String::new() } else { format!(" @ {}", e.location) };
    format!("#{} {} — {}{}", e.id, e.title, span, loc)
}

macro_rules! simple_tool {
    ($ty:ident) => {
        pub struct $ty {
            spec: ToolSpec,
            db: Arc<Database>,
        }
    };
}

// ── time ────────────────────────────────────────────────────────────────────

pub struct DateTimeTool {
    spec: ToolSpec,
}

impl Default for DateTimeTool {
    fn default() -> Self {
        Self {
            spec: ToolSpec {
                name: "get_datetime",
                title: "Current date and time",
                description: "Get the current local date, time and time zone. Use it whenever the exact current time matters.",
                input_schema: json!({ "type": "object", "properties": {}, "required": [], "additionalProperties": false }),
                permission: PermissionLevel::Safe,
            },
        }
    }
}

#[async_trait::async_trait]
impl Tool for DateTimeTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, _i: &Value) -> String {
        "Check the time".into()
    }
    async fn execute(&self, _i: &Value) -> ToolResultT {
        let now = Local::now();
        let text = format!("{} (UTC{}), ISO week {}", now.format("%A, %-d %B %Y, %H:%M:%S"), now.format("%:z"), now.format("%V"));
        out(text, now.format("%H:%M").to_string())
    }
}

// ── tasks ───────────────────────────────────────────────────────────────────

simple_tool!(AddTaskTool);

impl AddTaskTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "add_task",
                title: "Add task",
                description: "Add a task to the user's to-do list. Use due \"\" when there's no deadline. To also get a \
notification at a specific time, set a reminder as well.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "minLength": 1, "maxLength": 200 },
                        "due": text(80, &format!("Deadline: {WHEN_HELP}; or \"\" for none")),
                        "priority": { "type": "string", "enum": ["low", "normal", "high"] },
                        "notes": text(2000, "Extra details, or \"\"")
                    },
                    "required": ["title", "due", "priority", "notes"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for AddTaskTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("Add task \"{}\"", s(input, "title"))
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let due = match s(input, "due") {
            "" => String::new(),
            w => time::to_db(time::parse_when(w).map_err(ToolError::invalid)?),
        };
        let t = with_conn(&self.db, |c| {
            tasks::add(c, &TaskInput { title: s(input, "title").into(), notes: s(input, "notes").into(), due_at: due, priority: s(input, "priority").into() })
        })?;
        out(format!("Added: {}", task_line(&t)), format!("Added #{}", t.id))
    }
}

simple_tool!(ListTasksTool);

impl ListTasksTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "list_tasks",
                title: "List tasks",
                description: "List the user's tasks with ids, due dates and priorities.",
                input_schema: json!({
                    "type": "object",
                    "properties": { "status": { "type": "string", "enum": ["open", "done", "all"] } },
                    "required": ["status"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Safe,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ListTasksTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("List {} tasks", s(input, "status"))
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let filter = match s(input, "status") {
            "done" => Filter::Done,
            "all" => Filter::All,
            _ => Filter::Open,
        };
        let list = with_conn(&self.db, |c| tasks::list(c, filter))?;
        if list.is_empty() {
            return out("No tasks.".into(), "None");
        }
        let shown: Vec<String> = list.iter().take(100).map(task_line).collect();
        let more = if list.len() > 100 { format!("\n… and {} more", list.len() - 100) } else { String::new() };
        out(format!("{}{more}", shown.join("\n")), format!("{} tasks", list.len()))
    }
}

simple_tool!(UpdateTaskTool);

impl UpdateTaskTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "update_task",
                title: "Update task",
                description: "Complete, reopen or edit a task by id (find it with list_tasks). For edits, fields set to \"\" \
stay unchanged; set due to \"none\" to remove the deadline.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "id": id_schema("task"),
                        "action": { "type": "string", "enum": ["complete", "reopen", "edit"] },
                        "title": text(200, "New title, or \"\""),
                        "due": text(80, &format!("New deadline ({WHEN_HELP}), \"none\", or \"\"")),
                        "priority": { "type": "string", "enum": ["", "low", "normal", "high"] },
                        "notes": text(2000, "New notes, or \"\"")
                    },
                    "required": ["id", "action", "title", "due", "priority", "notes"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for UpdateTaskTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        let verb = match s(input, "action") {
            "complete" => "Complete",
            "reopen" => "Reopen",
            _ => "Edit",
        };
        format!("{verb} task #{}", input["id"])
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let id = input["id"].as_i64().unwrap_or_default();
        let new_due = match s(input, "due") {
            "" => None,
            "none" => Some(String::new()),
            w => Some(time::to_db(time::parse_when(w).map_err(ToolError::invalid)?)),
        };
        let t = with_conn(&self.db, |c| match s(input, "action") {
            "complete" => tasks::set_done(c, id, true),
            "reopen" => tasks::set_done(c, id, false),
            _ => {
                let cur = tasks::get(c, id)?.ok_or_else(|| AppError::validation(format!("There's no task #{id}.")))?;
                let pick = |k: &str, old: &str| if s(input, k).is_empty() { old.to_string() } else { s(input, k).to_string() };
                tasks::update(
                    c,
                    id,
                    &TaskInput {
                        title: pick("title", &cur.title),
                        notes: pick("notes", &cur.notes),
                        due_at: new_due.clone().unwrap_or_else(|| cur.due_at.clone().unwrap_or_default()),
                        priority: pick("priority", &cur.priority),
                    },
                )
            }
        })?;
        out(format!("Updated: {}", task_line(&t)), if t.done { "Done" } else { "Updated" })
    }
}

simple_tool!(DeleteTaskTool);

impl DeleteTaskTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "delete_task",
                title: "Delete task",
                description: "Permanently delete a task by id when the user asks to remove it (completing is usually better).",
                input_schema: json!({ "type": "object", "properties": { "id": id_schema("task") }, "required": ["id"], "additionalProperties": false }),
                permission: PermissionLevel::Low,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for DeleteTaskTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("Delete task #{}", input["id"])
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let t = with_conn(&self.db, |c| tasks::remove(c, input["id"].as_i64().unwrap_or_default()))?;
        out(format!("Deleted task #{} \"{}\".", t.id, t.title), "Deleted")
    }
}

// ── reminders & timers ──────────────────────────────────────────────────────

simple_tool!(SetReminderTool);

impl SetReminderTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "set_reminder",
                title: "Set reminder",
                description: "Set a reminder that shows a desktop notification at a specific time. The result states the \
exact time it was set for — tell the user that time.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "minLength": 1, "maxLength": 200, "description": "What to remind the user about" },
                        "when": { "type": "string", "minLength": 1, "maxLength": 80, "description": WHEN_HELP }
                    },
                    "required": ["text", "when"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for SetReminderTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("Remind \"{}\" {}", s(input, "text"), s(input, "when"))
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let r = with_conn(&self.db, |c| reminders::add_reminder(c, s(input, "text"), s(input, "when")))?;
        out(format!("Reminder #{} set for {}.", r.id, when(Some(&r.due_at))), format!("Set for {}", when(Some(&r.due_at))))
    }
}

simple_tool!(StartTimerTool);

impl StartTimerTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "start_timer",
                title: "Start timer",
                description: "Start a countdown timer; a notification fires when it ends.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "duration": { "type": "string", "minLength": 1, "maxLength": 60, "description": "e.g. \"10 minutes\", \"1h 30m\", \"90 seconds\"" },
                        "label": text(120, "What it's for, e.g. \"Tea\", or \"\"")
                    },
                    "required": ["duration", "label"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for StartTimerTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("Start a {} timer", s(input, "duration"))
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let r = with_conn(&self.db, |c| reminders::start_timer(c, s(input, "label"), s(input, "duration")))?;
        let len = time::human_duration(chrono::Duration::seconds(r.duration_secs.unwrap_or_default()));
        out(format!("Timer #{} \"{}\" started for {len}; it ends {}.", r.id, r.title, when(Some(&r.due_at))), format!("{len} timer"))
    }
}

simple_tool!(ListRemindersTool);

impl ListRemindersTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "list_reminders",
                title: "List reminders",
                description: "List upcoming reminders and running timers (with ids and time left).",
                input_schema: json!({ "type": "object", "properties": {}, "required": [], "additionalProperties": false }),
                permission: PermissionLevel::Safe,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ListRemindersTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, _i: &Value) -> String {
        "List reminders and timers".into()
    }
    async fn execute(&self, _i: &Value) -> ToolResultT {
        let list = with_conn(&self.db, reminders::active)?;
        if list.is_empty() {
            return out("No upcoming reminders or running timers.".into(), "None");
        }
        out(list.iter().map(reminder_line).collect::<Vec<_>>().join("\n"), format!("{} active", list.len()))
    }
}

simple_tool!(CancelReminderTool);

impl CancelReminderTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "cancel_reminder",
                title: "Cancel reminder or timer",
                description: "Cancel an upcoming reminder or running timer by id (find it with list_reminders), or dismiss \
one that is ringing.",
                input_schema: json!({ "type": "object", "properties": { "id": id_schema("reminder or timer") }, "required": ["id"], "additionalProperties": false }),
                permission: PermissionLevel::Low,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for CancelReminderTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("Cancel reminder #{}", input["id"])
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let id = input["id"].as_i64().unwrap_or_default();
        let r = with_conn(&self.db, |c| match reminders::get(c, id)? {
            Some(r) if r.status == "fired" => reminders::dismiss(c, id),
            _ => reminders::cancel(c, id),
        })?;
        out(format!("{} #{} \"{}\" is {}.", r.kind, r.id, r.title, r.status), r.status.clone())
    }
}

// ── calendar ────────────────────────────────────────────────────────────────

simple_tool!(AddEventTool);

impl AddEventTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "add_event",
                title: "Add calendar event",
                description: "Add an event to IGRIS's local calendar (not synced with Google or Outlook). It doesn't notify \
by itself — set a reminder too if the user wants one.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "minLength": 1, "maxLength": 200 },
                        "start": { "type": "string", "minLength": 1, "maxLength": 80, "description": WHEN_HELP },
                        "end": text(80, "End time (same formats), a length like \"for 1 hour\", or \"\""),
                        "all_day": { "type": "boolean" },
                        "location": text(200, "Where, or \"\""),
                        "notes": text(2000, "Details, or \"\"")
                    },
                    "required": ["title", "start", "end", "all_day", "location", "notes"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for AddEventTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("Add event \"{}\" {}", s(input, "title"), s(input, "start"))
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let start = time::parse_when(s(input, "start")).map_err(ToolError::invalid)?;
        let end = match s(input, "end") {
            "" => String::new(),
            e => match time::parse_duration(e) {
                Ok(d) => time::to_db(start + d),
                Err(_) => time::to_db(time::parse_when(e).map_err(ToolError::invalid)?),
            },
        };
        let e = with_conn(&self.db, |c| {
            events::add(
                c,
                &EventInput {
                    title: s(input, "title").into(),
                    starts_at: time::to_db(start),
                    ends_at: end,
                    all_day: input["all_day"].as_bool().unwrap_or(false),
                    location: s(input, "location").into(),
                    notes: s(input, "notes").into(),
                },
            )
        })?;
        out(format!("Added: {}", event_line(&e)), format!("Added #{}", e.id))
    }
}

simple_tool!(ListEventsTool);

impl ListEventsTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "list_events",
                title: "List calendar events",
                description: "List events on IGRIS's local calendar for a period.",
                input_schema: json!({
                    "type": "object",
                    "properties": { "range": { "type": "string", "maxLength": 20, "description": "\"today\", \"tomorrow\", \"this week\", \"next 30 days\" or a date YYYY-MM-DD" } },
                    "required": ["range"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Safe,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ListEventsTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("Check calendar ({})", s(input, "range"))
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let (from, to) = events::parse_range(s(input, "range")).map_err(ToolError::invalid)?;
        let list = with_conn(&self.db, |c| events::range(c, from, to))?;
        if list.is_empty() {
            return out(format!("No events {}.", if s(input, "range").is_empty() { "today" } else { s(input, "range") }), "None");
        }
        out(list.iter().map(event_line).collect::<Vec<_>>().join("\n"), format!("{} events", list.len()))
    }
}

simple_tool!(DeleteEventTool);

impl DeleteEventTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "delete_event",
                title: "Delete calendar event",
                description: "Delete an event from IGRIS's local calendar by id (find it with list_events). To reschedule, \
delete it and add it again with the new time.",
                input_schema: json!({ "type": "object", "properties": { "id": id_schema("event") }, "required": ["id"], "additionalProperties": false }),
                permission: PermissionLevel::Low,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for DeleteEventTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("Delete event #{}", input["id"])
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let e = with_conn(&self.db, |c| events::remove(c, input["id"].as_i64().unwrap_or_default()))?;
        out(format!("Deleted event #{} \"{}\".", e.id, e.title), "Deleted")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::schema::{is_strict_compatible, validate};

    fn db() -> Arc<Database> {
        Arc::new(Database::open_in_memory().unwrap())
    }

    async fn run(t: &dyn Tool, input: Value) -> ToolResultT {
        validate(&t.spec().input_schema, &input).map_err(ToolError::invalid)?;
        t.execute(&input).await
    }

    #[tokio::test]
    async fn schemas_are_strict() {
        let d = db();
        let all: Vec<Box<dyn Tool>> = vec![
            Box::new(DateTimeTool::default()),
            Box::new(AddTaskTool::new(d.clone())),
            Box::new(ListTasksTool::new(d.clone())),
            Box::new(UpdateTaskTool::new(d.clone())),
            Box::new(DeleteTaskTool::new(d.clone())),
            Box::new(SetReminderTool::new(d.clone())),
            Box::new(StartTimerTool::new(d.clone())),
            Box::new(ListRemindersTool::new(d.clone())),
            Box::new(CancelReminderTool::new(d.clone())),
            Box::new(AddEventTool::new(d.clone())),
            Box::new(ListEventsTool::new(d.clone())),
            Box::new(DeleteEventTool::new(d)),
        ];
        for t in all {
            assert!(is_strict_compatible(&t.spec().input_schema), "{}", t.spec().name);
        }
    }

    #[tokio::test]
    async fn task_flow() {
        let d = db();
        let add = AddTaskTool::new(d.clone());
        let r = run(&add, json!({"title": "Submit report", "due": "tomorrow at 5pm", "priority": "high", "notes": ""})).await.unwrap();
        assert!(r.content.contains("#1 [open] Submit report — due tomorrow at 17:00"), "{}", r.content);
        assert!(r.content.contains("high priority"));
        let bad = run(&add, json!({"title": "x", "due": "whenever", "priority": "low", "notes": ""})).await.unwrap_err();
        assert!(bad.message.contains("couldn't understand"));

        let upd = UpdateTaskTool::new(d.clone());
        let edit = json!({"id": 1, "action": "edit", "title": "", "due": "none", "priority": "", "notes": "PDF only"});
        let r = run(&upd, edit).await.unwrap();
        assert!(r.content.contains("no due date") && r.content.contains("high priority") && r.content.contains("PDF only"), "{}", r.content);
        run(&upd, json!({"id": 1, "action": "complete", "title": "", "due": "", "priority": "", "notes": ""})).await.unwrap();
        let open = run(&ListTasksTool::new(d.clone()), json!({"status": "open"})).await.unwrap();
        assert_eq!(open.content, "No tasks.");
        let all = run(&ListTasksTool::new(d.clone()), json!({"status": "all"})).await.unwrap();
        assert!(all.content.contains("[done]"));
        assert!(run(&DeleteTaskTool::new(d.clone()), json!({"id": 1})).await.is_ok());
        assert!(run(&DeleteTaskTool::new(d), json!({"id": 1})).await.is_err());
    }

    #[tokio::test]
    async fn reminder_and_timer_flow() {
        let d = db();
        let r = run(&SetReminderTool::new(d.clone()), json!({"text": "Stretch", "when": "in 2 hours"})).await.unwrap();
        assert!(r.content.starts_with("Reminder #1 set for ") && r.content.contains("(in 2 h"), "{}", r.content);
        let t = run(&StartTimerTool::new(d.clone()), json!({"duration": "10 minutes", "label": "Tea"})).await.unwrap();
        assert!(t.content.contains("\"Tea\" started for 10 min"), "{}", t.content);
        let list = run(&ListRemindersTool::new(d.clone()), json!({})).await.unwrap();
        assert!(list.content.starts_with("#2 timer \"Tea\""), "{}", list.content);
        let c = run(&CancelReminderTool::new(d.clone()), json!({"id": 2})).await.unwrap();
        assert!(c.content.contains("cancelled"));
        let past = run(&SetReminderTool::new(d), json!({"text": "x", "when": "2020-01-01 09:00"})).await.unwrap_err();
        assert!(past.message.contains("in the past"));
    }

    #[tokio::test]
    async fn event_flow() {
        let d = db();
        let add = AddEventTool::new(d.clone());
        let r = run(&add, json!({"title": "Review", "start": "tomorrow at 3pm", "end": "for 1 hour", "all_day": false, "location": "Room 4", "notes": ""})).await.unwrap();
        assert!(r.content.contains("15:00–16:00 @ Room 4"), "{}", r.content);
        let r = run(&add, json!({"title": "Holiday", "start": "tomorrow", "end": "", "all_day": true, "location": "", "notes": ""})).await.unwrap();
        assert!(r.content.contains("(all day)"), "{}", r.content);
        let list = run(&ListEventsTool::new(d.clone()), json!({"range": "tomorrow"})).await.unwrap();
        assert_eq!(list.content.lines().count(), 2);
        assert!(list.content.lines().next().unwrap().contains("Holiday"));
        assert_eq!(run(&ListEventsTool::new(d.clone()), json!({"range": "today"})).await.unwrap().content, "No events today.");
        run(&DeleteEventTool::new(d.clone()), json!({"id": 1})).await.unwrap();
        assert_eq!(run(&ListEventsTool::new(d), json!({"range": "tomorrow"})).await.unwrap().content.lines().count(), 1);
    }
}
