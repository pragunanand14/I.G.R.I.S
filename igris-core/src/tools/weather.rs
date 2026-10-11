//! Weather (Open-Meteo): now, today and tomorrow, for the user's home place or
//! a place they name.

use std::sync::Arc;

use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::db::Database;
use crate::weather;

pub struct WeatherTool {
    spec: ToolSpec,
    db: Arc<Database>,
    geocoding: String,
    forecast: String,
}

impl WeatherTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self::with_endpoints(db, weather::GEOCODING, weather::FORECAST)
    }

    pub fn with_endpoints(db: Arc<Database>, geocoding: &str, forecast: &str) -> Self {
        Self {
            spec: ToolSpec {
                name: "get_weather",
                title: "Weather",
                description: "Current weather and today's and tomorrow's forecast. place: a city or town, or \"\" for the user's home place \
(set in Settings). Prefer this over web search for weather.",
                input_schema: json!({
                    "type": "object",
                    "properties": { "place": { "type": "string", "maxLength": 80 } },
                    "required": ["place"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Safe,
            },
            db,
            geocoding: geocoding.into(),
            forecast: forecast.into(),
        }
    }
}

#[async_trait::async_trait]
impl Tool for WeatherTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        match i["place"].as_str().filter(|p| !p.trim().is_empty()) {
            Some(p) => format!("Check the weather in {p}"),
            None => "Check the weather".into(),
        }
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let place = match i["place"].as_str().map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) => weather::find_place(&self.geocoding, p).await.map_err(|e| ToolError::invalid(e.to_string()))?,
            None => {
                let home = weather::home(&*self.db.conn().map_err(|e| ToolError::failed(e.to_string()))?).map_err(|e| ToolError::failed(e.to_string()))?;
                home.ok_or_else(|| {
                    ToolError::invalid("No home place is set. Ask the user which city (and they can set it in Settings → Routines → Daily briefing).")
                })?
            }
        };
        let r = weather::report(&self.forecast, &place).await.map_err(|e| ToolError::failed(e.to_string()))?;
        Ok(ToolOutput { content: r.clone(), summary: place.name, sources: vec![], media: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn home_or_named_place() {
        let base = crate::weather::tests::fake().await;
        let db = Arc::new(Database::open_in_memory().unwrap());
        let t = WeatherTool::with_endpoints(db.clone(), &base, &base);
        assert!(t.execute(&json!({"place": ""})).await.unwrap_err().message.contains("No home place"));
        assert!(t.execute(&json!({"place": "Mumbai"})).await.unwrap().content.contains("Now in Mumbai"));
        crate::weather::set_home(&db.conn().unwrap(), &crate::weather::Place { name: "Mumbai".into(), latitude: 19.0, longitude: 72.8 }).unwrap();
        assert!(t.execute(&json!({"place": ""})).await.unwrap().content.contains("Today: rain"));
    }
}
