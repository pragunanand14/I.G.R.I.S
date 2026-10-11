//! Places and location reminders on the phone: save where the phone is as a
//! named place ("office"), and remind on arriving at or leaving one. Android
//! watches for it (a proximity alert) even with IGRIS closed; `places` keeps
//! the record.

use std::sync::Arc;

use serde_json::{json, Value};
pub use tauri_plugin_igris_device::Location;

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::db::Database;
use crate::places;

/// What these tools need from the phone. Calls may block.
pub trait PhoneLocation: Send + Sync {
    fn current(&self) -> Result<Location, String>;
    fn watch(&self, id: i32, place: &places::Place, on_arrive: bool, text: &str) -> Result<(), String>;
    fn unwatch(&self, id: i32) -> Result<(), String>;
    fn fired(&self, id: i32) -> bool;
}

pub type SharedLocation = Arc<dyn PhoneLocation>;

/// How near counts as "at" a place, in metres.
pub const RADIUS: f32 = 150.0;

async fn on_phone<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, ToolError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| ToolError::failed(e.to_string()))?.map_err(ToolError::failed)
}

fn out(content: String, summary: impl Into<String>) -> ToolResultT {
    Ok(ToolOutput { content, summary: summary.into(), sources: vec![], media: Vec::new() })
}

fn db_err(e: crate::error::AppError) -> ToolError {
    ToolError::invalid(e.to_string())
}

#[derive(Clone)]
pub struct Places {
    phone: SharedLocation,
    db: Arc<Database>,
}

impl Places {
    pub fn new(phone: SharedLocation, db: Arc<Database>) -> Self {
        Self { phone, db }
    }

    /// Reminders that went off on the phone are done.
    fn sync(&self) -> Result<(), ToolError> {
        let conn = self.db.conn().map_err(db_err)?;
        for r in places::reminders(&conn, true).map_err(db_err)? {
            if self.phone.fired(r.id as i32) {
                places::set_status(&conn, r.id, "done").map_err(db_err)?;
            }
        }
        Ok(())
    }
}

fn spec(name: &'static str, title: &'static str, description: &'static str, props: Value, required: &[&str], permission: PermissionLevel) -> ToolSpec {
    ToolSpec {
        name,
        title,
        description,
        input_schema: json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false }),
        permission,
    }
}

macro_rules! places_tool {
    ($name:ident) => {
        pub struct $name {
            spec: ToolSpec,
            p: Places,
        }
    };
}

places_tool!(SavePlaceTool);
impl SavePlaceTool {
    pub fn new(p: Places) -> Self {
        Self {
            spec: spec(
                "save_place",
                "Save this place",
                "Save where the phone is right now as a named place (\"home\", \"office\", \"gym\") for location reminders. Saving \
an existing name moves it here. Only when the user says they are at that place.",
                json!({ "name": { "type": "string", "minLength": 1, "maxLength": 40 } }),
                &["name"],
                PermissionLevel::Low,
            ),
            p,
        }
    }
}

#[async_trait::async_trait]
impl Tool for SavePlaceTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("Save where you are now as \"{}\"", i["name"].as_str().unwrap_or_default())
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let phone = self.p.phone.clone();
        let here = on_phone(move || phone.current()).await?;
        if here.accuracy > 500.0 {
            return Err(ToolError::failed(format!(
                "The location is too rough right now (±{:.0} m). Ask the user to try again outdoors or with Location set to high accuracy.",
                here.accuracy
            )));
        }
        let place = places::save(&*self.p.db.conn().map_err(db_err)?, i["name"].as_str().unwrap_or_default(), here.latitude, here.longitude).map_err(db_err)?;
        out(format!("Saved this spot as \"{}\" (accurate to about {:.0} m).", place.name, here.accuracy), format!("Saved \"{}\"", place.name))
    }
}

places_tool!(ListPlacesTool);
impl ListPlacesTool {
    pub fn new(p: Places) -> Self {
        Self {
            spec: spec(
                "list_places",
                "Saved places",
                "The user's saved places and their location reminders (active and past, with ids for cancel_place_reminder).",
                json!({}),
                &[],
                PermissionLevel::Safe,
            ),
            p,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ListPlacesTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, _i: &Value) -> String {
        "List your saved places".into()
    }
    async fn execute(&self, _i: &Value) -> ToolResultT {
        self.p.sync()?;
        let conn = self.p.db.conn().map_err(db_err)?;
        let saved = places::list(&conn).map_err(db_err)?;
        let reminders = places::reminders(&conn, false).map_err(db_err)?;
        let mut s = if saved.is_empty() {
            "No saved places yet (the user saves one by being there and asking).".to_string()
        } else {
            format!("Saved places: {}.", saved.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", "))
        };
        for r in reminders.iter().take(20) {
            s.push_str(&format!("\n[#{}] {} {}: {} ({})", r.id, if r.on_arrive { "arriving at" } else { "leaving" }, r.place, r.text, r.status));
        }
        out(s, format!("{} place(s)", saved.len()))
    }
}

places_tool!(PlaceReminderTool);
impl PlaceReminderTool {
    pub fn new(p: Places) -> Self {
        Self {
            spec: spec(
                "place_reminder",
                "Location reminder",
                "Remind the user when they arrive at or leave a saved place (\"remind me to buy milk when I leave the office\"). \
place: a saved place's name (list_places). when: \"arrive\" or \"leave\". Goes off once, even with IGRIS closed.",
                json!({
                    "place": { "type": "string", "minLength": 1, "maxLength": 40 },
                    "when": { "type": "string", "enum": ["arrive", "leave"] },
                    "text": { "type": "string", "minLength": 1, "maxLength": 200 }
                }),
                &["place", "when", "text"],
                PermissionLevel::Low,
            ),
            p,
        }
    }
}

#[async_trait::async_trait]
impl Tool for PlaceReminderTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!(
            "Remind you when you {} {}: \"{}\"",
            if i["when"] == "leave" { "leave" } else { "arrive at" },
            i["place"].as_str().unwrap_or_default(),
            i["text"].as_str().unwrap_or_default()
        )
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let on_arrive = i["when"] != "leave";
        let (r, place) = {
            let conn = self.p.db.conn().map_err(db_err)?;
            let r = places::add_reminder(&conn, i["place"].as_str().unwrap_or_default(), on_arrive, i["text"].as_str().unwrap_or_default()).map_err(db_err)?;
            let place = places::get(&conn, &r.place).map_err(db_err)?.ok_or_else(|| ToolError::not_found("That place was just removed."))?;
            (r, place)
        };
        let (phone, text, id) = (self.p.phone.clone(), r.text.clone(), r.id as i32);
        if let Err(e) = on_phone(move || phone.watch(id, &place, on_arrive, &text)).await {
            // Nothing is watching, so don't keep a reminder that will never go off.
            let _ = places::set_status(&*self.p.db.conn().map_err(db_err)?, r.id, "cancelled");
            return Err(e);
        }
        let when = if on_arrive { "arrive at" } else { "leave" };
        out(format!("Reminder #{} set: when you {when} {} — \"{}\".", r.id, r.place, r.text), format!("Reminder at {}", r.place))
    }
}

places_tool!(CancelPlaceReminderTool);
impl CancelPlaceReminderTool {
    pub fn new(p: Places) -> Self {
        Self {
            spec: spec(
                "cancel_place_reminder",
                "Cancel a location reminder",
                "Cancel a location reminder by its id (from list_places).",
                json!({ "id": { "type": "integer", "minimum": 1 } }),
                &["id"],
                PermissionLevel::Low,
            ),
            p,
        }
    }
}

#[async_trait::async_trait]
impl Tool for CancelPlaceReminderTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("Cancel location reminder #{}", i["id"].as_i64().unwrap_or_default())
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let id = i["id"].as_i64().unwrap_or_default();
        places::set_status(&*self.p.db.conn().map_err(db_err)?, id, "cancelled").map_err(db_err)?;
        let phone = self.p.phone.clone();
        on_phone(move || phone.unwatch(id as i32)).await?;
        out(format!("Location reminder #{id} cancelled."), "Cancelled")
    }
}

pub fn all(p: &Places) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(SavePlaceTool::new(p.clone())),
        Arc::new(ListPlacesTool::new(p.clone())),
        Arc::new(PlaceReminderTool::new(p.clone())),
        Arc::new(CancelPlaceReminderTool::new(p.clone())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        watching: Mutex<Vec<(i32, String, bool, String)>>,
        fired: Mutex<Vec<i32>>,
    }

    impl PhoneLocation for Fake {
        fn current(&self) -> Result<Location, String> {
            Ok(Location { latitude: 19.07, longitude: 72.87, accuracy: 20.0 })
        }
        fn watch(&self, id: i32, place: &places::Place, on_arrive: bool, text: &str) -> Result<(), String> {
            self.watching.lock().unwrap().push((id, place.name.clone(), on_arrive, text.into()));
            Ok(())
        }
        fn unwatch(&self, id: i32) -> Result<(), String> {
            self.watching.lock().unwrap().retain(|w| w.0 != id);
            Ok(())
        }
        fn fired(&self, id: i32) -> bool {
            self.fired.lock().unwrap().contains(&id)
        }
    }

    #[tokio::test]
    async fn save_a_place_and_remind_there() {
        let fake = Arc::new(Fake::default());
        let db = Arc::new(Database::open_in_memory().unwrap());
        let [save, list, remind, cancel]: [Arc<dyn Tool>; 4] = all(&Places::new(fake.clone(), db.clone())).try_into().ok().unwrap();

        assert!(remind.execute(&json!({"place": "office", "when": "leave", "text": "Buy milk"})).await.unwrap_err().message.contains("isn't a saved place"));
        assert!(save.execute(&json!({"name": "Office"})).await.unwrap().content.contains("\"office\""));
        let r = remind.execute(&json!({"place": "office", "when": "leave", "text": "Buy milk"})).await.unwrap();
        assert!(r.content.contains("when you leave office"));
        assert_eq!(*fake.watching.lock().unwrap(), [(1, "office".to_string(), false, "Buy milk".to_string())]);
        remind.execute(&json!({"place": "office", "when": "arrive", "text": "Call Sam"})).await.unwrap();

        fake.fired.lock().unwrap().push(1);
        let l = list.execute(&json!({})).await.unwrap().content;
        assert!(l.contains("[#1] leaving office: Buy milk (done)") && l.contains("[#2] arriving at office: Call Sam (active)"), "{l}");
        cancel.execute(&json!({"id": 2})).await.unwrap();
        assert!(fake.watching.lock().unwrap().iter().all(|w| w.0 != 2));
        assert!(list.execute(&json!({})).await.unwrap().content.contains("(cancelled)"));
        assert_eq!(list.spec().permission, PermissionLevel::Safe);
    }
}
