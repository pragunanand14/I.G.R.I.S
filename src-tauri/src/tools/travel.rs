//! Getting somewhere: `directions` (Google Maps, navigation on the phone, the
//! route in the browser on a computer) and, on the phone, `book_ride` (opens
//! Uber, Ola or Rapido for phone control to fill in the destination — the
//! booking itself is always the user's tap).

use std::sync::Arc;

use serde_json::{json, Value};

use super::processes::UrlOpener;
use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};

fn out(content: String, summary: impl Into<String>) -> ToolResultT {
    Ok(ToolOutput { content, summary: summary.into(), sources: vec![], media: vec![] })
}

/// Percent-encode a query value.
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A Google Maps directions link (opens the Maps app on a phone, the website on a computer).
pub fn maps_link(destination: &str, mode: &str, origin: &str) -> String {
    let mut url = format!("https://www.google.com/maps/dir/?api=1&destination={}", enc(destination.trim()));
    if !origin.trim().is_empty() {
        url.push_str(&format!("&origin={}", enc(origin.trim())));
    }
    let travelmode = match mode {
        "walk" => "walking",
        "bike" => "two-wheeler",
        "cycle" => "bicycling",
        "transit" => "transit",
        _ => "driving",
    };
    url.push_str(&format!("&travelmode={travelmode}&dir_action=navigate"));
    url
}

pub struct DirectionsTool {
    spec: ToolSpec,
    open: UrlOpener,
}

impl DirectionsTool {
    pub fn new(open: UrlOpener) -> Self {
        Self {
            spec: ToolSpec {
                name: "directions",
                title: "Directions",
                description: "Show the way somewhere in Google Maps: on the phone it starts navigation in the Maps app, on a computer it \
opens the route in the browser. destination: a place name or address (\"Connaught Place, Delhi\", \"the nearest petrol \
pump\"). mode: drive, bike (two-wheeler), walk, cycle or transit. origin: \"\" for where the user is now.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "destination": { "type": "string", "minLength": 2, "maxLength": 200 },
                        "mode": { "type": "string", "enum": ["drive", "bike", "walk", "cycle", "transit"] },
                        "origin": { "type": "string", "maxLength": 200 }
                    },
                    "required": ["destination", "mode", "origin"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Safe,
            },
            open,
        }
    }
}

#[async_trait::async_trait]
impl Tool for DirectionsTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("Directions to {}", i["destination"].as_str().unwrap_or_default())
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let dest = i["destination"].as_str().unwrap_or_default();
        let url = maps_link(dest, i["mode"].as_str().unwrap_or("drive"), i["origin"].as_str().unwrap_or_default());
        let open = self.open.clone();
        let u = url.clone();
        tokio::task::spawn_blocking(move || open(&u)).await.map_err(|e| ToolError::failed(e.to_string()))?.map_err(ToolError::failed)?;
        out(format!("Opened Google Maps with directions to {dest}."), format!("Directions to {dest}"))
    }
}

/// The ride apps IGRIS knows: (name the model uses, package, display name).
const RIDE_APPS: &[(&str, &str, &str)] = &[
    ("uber", "com.ubercab", "Uber"),
    ("ola", "com.olacabs.customer", "Ola"),
    ("rapido", "com.rapido.passenger", "Rapido"),
    ("namma_yatri", "in.juspay.nammayatri", "Namma Yatri"),
];

pub struct BookRideTool {
    spec: ToolSpec,
    phone: super::phone::SharedPhone,
}

impl BookRideTool {
    pub fn new(phone: super::phone::SharedPhone) -> Self {
        Self {
            spec: ToolSpec {
                name: "book_ride",
                title: "Get a ride",
                description: "Start getting a cab, auto or bike taxi: opens the ride app (uber, ola, rapido, namma_yatri). Then use \
phone control (phone_control_start) to enter the destination and pick the ride type the user wants, and stop at the final \
booking button — booking costs money, so the user taps it themselves (IGRIS can't).",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "app": { "type": "string", "enum": ["uber", "ola", "rapido", "namma_yatri"] },
                        "destination": { "type": "string", "minLength": 2, "maxLength": 200 }
                    },
                    "required": ["app", "destination"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            phone,
        }
    }
}

#[async_trait::async_trait]
impl Tool for BookRideTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("Open {} for a ride to {}", i["app"].as_str().unwrap_or_default(), i["destination"].as_str().unwrap_or_default())
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let (_, package, name) = RIDE_APPS
            .iter()
            .find(|(k, ..)| Some(*k) == i["app"].as_str())
            .copied()
            .ok_or_else(|| ToolError::invalid("Pick uber, ola, rapido or namma_yatri."))?;
        let phone: Arc<dyn super::phone::Phone> = self.phone.clone();
        let p = package.to_string();
        let opened = tokio::task::spawn_blocking(move || phone.open(&p)).await.map_err(|e| ToolError::failed(e.to_string()))?;
        if opened.is_err() {
            return Err(ToolError::failed(format!("{name} isn't installed on this phone (or couldn't be opened). Offer another ride app.")));
        }
        let dest = i["destination"].as_str().unwrap_or_default();
        out(
            format!(
                "Opened {name}. Now use phone control to set the destination to \"{dest}\" and choose the ride; stop before the \
final booking button and tell the user it's ready for them to confirm."
            ),
            format!("Opened {name}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_links() {
        assert_eq!(
            maps_link("Connaught Place, Delhi", "bike", ""),
            "https://www.google.com/maps/dir/?api=1&destination=Connaught+Place%2C+Delhi&travelmode=two-wheeler&dir_action=navigate"
        );
        assert!(maps_link("Office", "walk", "Home").contains("&origin=Home&travelmode=walking"));
    }

    #[tokio::test]
    async fn directions_open_maps() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let s = seen.clone();
        let t = DirectionsTool::new(Arc::new(move |u: &str| {
            s.lock().unwrap().push(u.to_string());
            Ok(())
        }));
        t.execute(&json!({"destination": "India Gate", "mode": "drive", "origin": ""})).await.unwrap();
        assert!(seen.lock().unwrap()[0].starts_with("https://www.google.com/maps/dir/?api=1&destination=India+Gate&travelmode=driving"));
    }
}
