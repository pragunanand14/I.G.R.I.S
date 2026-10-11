//! Routines from chat: "every morning at 8, tell me the weather and my
//! meetings", "when Rahul emails, tell me right away". The actions a routine
//! takes still go through the permission checks when it runs.

use std::sync::Arc;

use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::db::Database;
use crate::routines::{self, schedule::Kind, Routine, Schedule};

fn out(content: String, summary: impl Into<String>) -> ToolResultT {
    Ok(ToolOutput { content, summary: summary.into(), sources: vec![], media: Vec::new() })
}

fn line(r: &Routine) -> String {
    let state = if r.enabled { "" } else { " [paused]" };
    format!("#{} \"{}\" — {}{state}: {}", r.id, r.name, r.when, crate::orchestrator::task::clip(&r.instruction, 160))
}

fn schedule(i: &Value) -> Result<Schedule, ToolError> {
    let kind = match i["kind"].as_str().unwrap_or_default() {
        "daily" => Kind::Daily,
        "weekdays" => Kind::Weekdays,
        "weekends" => Kind::Weekends,
        "weekly" => Kind::Weekly,
        "once" => Kind::Once,
        "email" => Kind::Email,
        _ => return Err(ToolError::invalid("kind must be daily, weekdays, weekends, weekly, once or email.")),
    };
    let days = i["days"].as_array().into_iter().flatten().filter_map(|d| d.as_str().map(str::to_string)).collect();
    let s = |k: &str| i[k].as_str().unwrap_or_default().trim().to_string();
    Ok(Schedule { kind, time: s("time"), days, date: s("date"), email: s("email") })
}

pub struct CreateRoutineTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl CreateRoutineTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "create_routine",
                title: "Create a routine",
                description: "Make IGRIS do something by itself, repeatedly or later: on a schedule (kind daily / weekdays / weekends / \
weekly with days / once with date; time \"HH:MM\" 24-hour local), or when an email arrives (kind email; email = a Gmail \
search like \"from:boss@work.com\"). instruction: what to do each time, complete on its own, in plain words (e.g. \"Tell me \
today's weather, my meetings and any important unread emails\"; \"Turn off the TV and pause the music\"). Not for one-off \
reminders — use set_reminder for those.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "minLength": 1, "maxLength": 60 },
                        "instruction": { "type": "string", "minLength": 1, "maxLength": 2000 },
                        "kind": { "type": "string", "enum": ["daily", "weekdays", "weekends", "weekly", "once", "email"] },
                        "time": { "type": "string", "maxLength": 5, "description": "HH:MM, 24-hour; \"\" for email." },
                        "days": { "type": "array", "maxItems": 7, "items": { "type": "string", "enum": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] } },
                        "date": { "type": "string", "maxLength": 10, "description": "YYYY-MM-DD for once; otherwise \"\"." },
                        "email": { "type": "string", "maxLength": 200, "description": "Gmail search for email; otherwise \"\"." }
                    },
                    "required": ["name", "instruction", "kind", "time", "days", "date", "email"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for CreateRoutineTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        let when = schedule(i).map(|s| s.describe()).unwrap_or_default();
        format!(
            "New routine \"{}\" — {when}: {}",
            i["name"].as_str().unwrap_or_default(),
            crate::orchestrator::task::clip(i["instruction"].as_str().unwrap_or_default(), 120)
        )
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let s = schedule(i)?;
        let conn = self.db.conn().map_err(|e| ToolError::failed(e.to_string()))?;
        let r = routines::create(&conn, i["name"].as_str().unwrap_or_default(), i["instruction"].as_str().unwrap_or_default(), s)
            .map_err(|e| ToolError::invalid(e.to_string()))?;
        let next = r.next_run_at.as_deref().and_then(crate::productivity::time::from_db).map(crate::productivity::time::describe);
        let first = next.map(|n| format!(" First run: {n}.")).unwrap_or_default();
        out(
            format!(
                "Created routine #{}: \"{}\" — {}.{first} It runs while IGRIS is running on this device; it can be paused or deleted on the Routines page.",
                r.id, r.name, r.when
            ),
            format!("Routine \"{}\" — {}", r.name, r.when),
        )
    }
}

pub struct ListRoutinesTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl ListRoutinesTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "list_routines",
                title: "List routines",
                description: "The user's routines: id, name, when, what it does, paused or not, and how its last run went.",
                input_schema: json!({"type": "object", "properties": {}, "required": [], "additionalProperties": false}),
                permission: PermissionLevel::Safe,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ListRoutinesTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, _i: &Value) -> String {
        "Check your routines".into()
    }
    async fn execute(&self, _i: &Value) -> ToolResultT {
        let conn = self.db.conn().map_err(|e| ToolError::failed(e.to_string()))?;
        let all = routines::list(&conn).map_err(|e| ToolError::failed(e.to_string()))?;
        if all.is_empty() {
            return out("No routines yet.".into(), "None");
        }
        let mut lines = Vec::new();
        for r in &all {
            let last = routines::runs(&conn, Some(r.id), 1).ok().and_then(|v| v.into_iter().next());
            let last = last.map(|l| format!(" (last run: {})", l.status)).unwrap_or_default();
            lines.push(format!("{}{last}", line(r)));
        }
        out(lines.join("\n"), format!("{} routine(s)", all.len()))
    }
}

pub struct ChangeRoutineTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl ChangeRoutineTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "change_routine",
                title: "Change a routine",
                description: "Pause, resume or delete a routine (id from list_routines).",
                input_schema: json!({
                    "type": "object",
                    "properties": { "id": { "type": "integer", "minimum": 1 }, "action": { "type": "string", "enum": ["pause", "resume", "delete"] } },
                    "required": ["id", "action"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ChangeRoutineTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!(
            "{} routine #{}",
            match i["action"].as_str() {
                Some("delete") => "Delete",
                Some("pause") => "Pause",
                _ => "Resume",
            },
            i["id"]
        )
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let conn = self.db.conn().map_err(|e| ToolError::failed(e.to_string()))?;
        let id = i["id"].as_i64().unwrap_or_default();
        let r = match i["action"].as_str() {
            Some("delete") => routines::delete(&conn, id).map(|r| format!("Deleted \"{}\".", r.name)),
            Some("pause") => routines::set_enabled(&conn, id, false).map(|r| format!("Paused \"{}\".", r.name)),
            _ => routines::set_enabled(&conn, id, true).map(|r| format!("Resumed \"{}\" — {}.", r.name, r.when)),
        }
        .map_err(|e| ToolError::invalid(e.to_string()))?;
        out(r.clone(), r)
    }
}

pub fn all(db: &Arc<Database>) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(CreateRoutineTool::new(db.clone())), Arc::new(ListRoutinesTool::new(db.clone())), Arc::new(ChangeRoutineTool::new(db.clone()))]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_list_pause_delete() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let [create, list, change]: [Arc<dyn Tool>; 3] = all(&db).try_into().ok().unwrap();
        let made = create
            .execute(
                &json!({"name": "Morning", "instruction": "Weather and meetings", "kind": "weekdays", "time": "08:00", "days": [], "date": "", "email": ""}),
            )
            .await
            .unwrap();
        assert!(made.summary.contains("Every weekday at 08:00"));
        assert!(create.execute(&json!({"name": "x", "instruction": "y", "kind": "daily", "time": "8am", "days": [], "date": "", "email": ""})).await.is_err());
        assert!(list.execute(&json!({})).await.unwrap().content.contains("#1 \"Morning\""));
        change.execute(&json!({"id": 1, "action": "pause"})).await.unwrap();
        assert!(list.execute(&json!({})).await.unwrap().content.contains("[paused]"));
        change.execute(&json!({"id": 1, "action": "delete"})).await.unwrap();
        assert_eq!(list.execute(&json!({})).await.unwrap().content, "No routines yet.");
    }
}
