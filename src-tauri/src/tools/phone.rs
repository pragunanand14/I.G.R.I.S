//! Phone tools (Android): the apps on the phone, opening one, and the phone's
//! battery and network status. The phone itself is reached through the
//! `tauri-plugin-igris-device` Kotlin plugin, behind the [`Phone`] trait.

use std::sync::Arc;

use serde_json::{json, Value};
pub use tauri_plugin_igris_device::{PhoneApp, PhoneStatus};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};

/// What the phone tools need from the phone. Calls may block.
pub trait Phone: Send + Sync {
    fn apps(&self) -> Result<Vec<PhoneApp>, String>;
    fn open(&self, package: &str) -> Result<(), String>;
    fn status(&self) -> Result<PhoneStatus, String>;
}

pub type SharedPhone = Arc<dyn Phone>;

/// Most apps listed in one result.
const MAX_LISTED: usize = 150;

/// App labels come from third-party apps: one line, no control characters, bounded.
fn clean(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).take(80).collect();
    s.trim().to_string()
}

async fn on_phone<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, ToolError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| ToolError::failed(e.to_string()))?.map_err(ToolError::failed)
}

fn sorted(mut apps: Vec<PhoneApp>) -> Vec<PhoneApp> {
    apps.sort_by_key(|a| a.label.to_lowercase());
    apps
}

/// Find the app the user means: exact package, exact label, then a single label containing the text.
pub fn resolve<'a>(apps: &'a [PhoneApp], wanted: &str) -> Result<&'a PhoneApp, ToolError> {
    let w = wanted.trim().to_lowercase();
    if w.is_empty() {
        return Err(ToolError::invalid("Say which app to open."));
    }
    if let Some(a) = apps.iter().find(|a| a.package.to_lowercase() == w) {
        return Ok(a);
    }
    let exact: Vec<&PhoneApp> = apps.iter().filter(|a| a.label.to_lowercase() == w).collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    let partial: Vec<&PhoneApp> = if exact.is_empty() { apps.iter().filter(|a| a.label.to_lowercase().contains(&w)).collect() } else { exact };
    match partial.as_slice() {
        [one] => Ok(one),
        [] => Err(ToolError::not_found(format!("No app called \"{}\" is installed on the phone. Use list_phone_apps to see what is.", clean(wanted)))),
        many => Err(ToolError::invalid(format!(
            "\"{}\" matches several apps: {}. Say which one (or give its package name).",
            clean(wanted),
            many.iter().take(6).map(|a| format!("{} ({})", clean(&a.label), a.package)).collect::<Vec<_>>().join(", ")
        ))),
    }
}

// ----- list_phone_apps -----

pub struct ListPhoneAppsTool {
    spec: ToolSpec,
    phone: SharedPhone,
}

impl ListPhoneAppsTool {
    pub fn new(phone: SharedPhone) -> Self {
        Self {
            spec: ToolSpec {
                name: "list_phone_apps",
                title: "List phone apps",
                description: "List the apps installed on the user's phone that can be opened (name and package). Use find to filter by \
name (empty for all).",
                input_schema: json!({
                    "type": "object",
                    "properties": { "find": { "type": "string", "maxLength": 80, "description": "Part of an app's name, or empty." } },
                    "required": ["find"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Safe,
            },
            phone,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ListPhoneAppsTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, _i: &Value) -> String {
        "List the apps on your phone".into()
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let find = i["find"].as_str().unwrap_or_default().trim().to_lowercase();
        let phone = self.phone.clone();
        let apps = sorted(on_phone(move || phone.apps()).await?);
        let matching: Vec<&PhoneApp> = apps.iter().filter(|a| find.is_empty() || a.label.to_lowercase().contains(&find)).collect();
        let mut content = if find.is_empty() {
            format!("{} apps on the phone:\n", matching.len())
        } else {
            format!("{} of {} apps match \"{}\":\n", matching.len(), apps.len(), clean(&find))
        };
        for a in matching.iter().take(MAX_LISTED) {
            content.push_str(&format!("- {} ({})\n", clean(&a.label), a.package));
        }
        if matching.len() > MAX_LISTED {
            content.push_str(&format!("…and {} more; use find to narrow it down.\n", matching.len() - MAX_LISTED));
        }
        Ok(ToolOutput { summary: format!("{} apps", matching.len()), content, sources: vec![], media: vec![] })
    }
}

// ----- open_phone_app -----

pub struct OpenPhoneAppTool {
    spec: ToolSpec,
    phone: SharedPhone,
}

impl OpenPhoneAppTool {
    pub fn new(phone: SharedPhone) -> Self {
        Self {
            spec: ToolSpec {
                name: "open_phone_app",
                title: "Open phone app",
                description: "Open an app on the user's phone, by its name (as listed by list_phone_apps) or package name. IGRIS stays \
in the background while the app is open.",
                input_schema: json!({
                    "type": "object",
                    "properties": { "app": { "type": "string", "minLength": 1, "maxLength": 120 } },
                    "required": ["app"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            phone,
        }
    }
}

#[async_trait::async_trait]
impl Tool for OpenPhoneAppTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("Open {} on your phone", clean(i["app"].as_str().unwrap_or_default()))
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let wanted = i["app"].as_str().unwrap_or_default().to_string();
        let phone = self.phone.clone();
        let apps = on_phone(move || phone.apps()).await?;
        let app = resolve(&apps, &wanted)?.clone();
        let phone = self.phone.clone();
        let package = app.package.clone();
        on_phone(move || phone.open(&package)).await?;
        tracing::info!(event = "PHONE_APP_OPENED", package = %app.package);
        Ok(ToolOutput {
            content: format!("Asked Android to open {} ({}); it accepted the request.", clean(&app.label), app.package),
            summary: format!("Opened {}", clean(&app.label)),
            sources: vec![],
            media: vec![],
        })
    }
}

// ----- device_status -----

pub struct DeviceStatusTool {
    spec: ToolSpec,
    phone: SharedPhone,
}

impl DeviceStatusTool {
    pub fn new(phone: SharedPhone) -> Self {
        Self {
            spec: ToolSpec {
                name: "device_status",
                title: "Phone status",
                description: "The phone's battery level and charging state, its network connection (Wi-Fi, mobile data or none), and \
its model and Android version.",
                input_schema: json!({ "type": "object", "properties": {}, "required": [], "additionalProperties": false }),
                permission: PermissionLevel::Safe,
            },
            phone,
        }
    }
}

pub fn describe_status(s: &PhoneStatus) -> String {
    let battery = match (s.battery_percent, s.charging) {
        (Some(p), Some(true)) => format!("{p:.0}% (charging)"),
        (Some(p), Some(false)) => format!("{p:.0}% (not charging)"),
        (Some(p), None) => format!("{p:.0}%"),
        (None, _) => "unknown".into(),
    };
    let network = match s.network.as_deref() {
        Some("wifi") => "Wi-Fi",
        Some("cellular") => "mobile data",
        Some("ethernet") => "Ethernet",
        Some("none") => "not connected",
        Some(_) => "connected (other)",
        None => "unknown",
    };
    let device = [s.manufacturer.as_deref(), s.model.as_deref()].into_iter().flatten().map(clean).collect::<Vec<_>>().join(" ");
    let android = match (&s.android_version, s.sdk_int) {
        (Some(v), Some(n)) => format!("Android {} (API {n})", clean(v)),
        (Some(v), None) => format!("Android {}", clean(v)),
        _ => "Android".into(),
    };
    format!("Battery: {battery}. Network: {network}. Device: {}, {android}.", if device.is_empty() { "unknown model".to_string() } else { device })
}

#[async_trait::async_trait]
impl Tool for DeviceStatusTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, _i: &Value) -> String {
        "Check your phone's battery and network".into()
    }
    async fn execute(&self, _i: &Value) -> ToolResultT {
        let phone = self.phone.clone();
        let s = on_phone(move || phone.status()).await?;
        Ok(ToolOutput { content: describe_status(&s), summary: "Phone status".into(), sources: vec![], media: vec![] })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct FakePhone {
        opened: Mutex<Vec<String>>,
        fail_open: bool,
    }

    fn app(label: &str, package: &str) -> PhoneApp {
        PhoneApp { label: label.into(), package: package.into() }
    }

    impl Phone for FakePhone {
        fn apps(&self) -> Result<Vec<PhoneApp>, String> {
            Ok(vec![
                app("YouTube", "com.google.android.youtube"),
                app("YouTube Music", "com.google.android.apps.youtube.music"),
                app("Gmail", "com.google.android.gm"),
                app("Calculator\u{0007}", "com.android.calculator2"),
            ])
        }
        fn open(&self, package: &str) -> Result<(), String> {
            if self.fail_open {
                return Err(format!("{package} isn't installed or can't be opened."));
            }
            self.opened.lock().unwrap().push(package.to_string());
            Ok(())
        }
        fn status(&self) -> Result<PhoneStatus, String> {
            Ok(PhoneStatus {
                manufacturer: Some("Google".into()),
                model: Some("Pixel 8".into()),
                android_version: Some("15".into()),
                sdk_int: Some(35),
                battery_percent: Some(81.6),
                charging: Some(true),
                network: Some("wifi".into()),
            })
        }
    }

    #[test]
    fn resolves_apps_by_package_label_or_unique_part() {
        let apps = FakePhone::default().apps().unwrap();
        assert_eq!(resolve(&apps, "com.google.android.gm").unwrap().label, "Gmail");
        assert_eq!(resolve(&apps, "youtube").unwrap().package, "com.google.android.youtube", "an exact name wins over partial ones");
        assert_eq!(resolve(&apps, "music").unwrap().label, "YouTube Music");
        assert_eq!(resolve(&apps, "Spotify").unwrap_err().kind, crate::tools::ToolErrorKind::NotFound);
        let ambiguous = resolve(&apps, "you tube").err().map(|e| e.kind);
        assert_eq!(ambiguous, Some(crate::tools::ToolErrorKind::NotFound));
        let several = resolve(&[app("Notes", "a.notes"), app("Notes", "b.notes")], "notes").unwrap_err();
        assert!(several.message.contains("several apps") && several.message.contains("a.notes"));
    }

    #[tokio::test]
    async fn lists_filters_and_opens_apps() {
        let phone = Arc::new(FakePhone::default());
        let list = ListPhoneAppsTool::new(phone.clone());
        let out = list.execute(&json!({"find": ""})).await.unwrap();
        assert!(out.content.starts_with("4 apps on the phone:"));
        assert!(out.content.contains("- Calculator (com.android.calculator2)"), "labels are cleaned: {}", out.content);
        let out = list.execute(&json!({"find": "tube"})).await.unwrap();
        assert!(out.content.starts_with("2 of 4 apps match \"tube\""));

        let open = OpenPhoneAppTool::new(phone.clone());
        assert_eq!(open.spec().permission, PermissionLevel::Low);
        let out = open.execute(&json!({"app": "gmail"})).await.unwrap();
        assert!(out.content.contains("Gmail (com.google.android.gm)"));
        assert_eq!(*phone.opened.lock().unwrap(), vec!["com.google.android.gm"]);
        assert!(open.execute(&json!({"app": "Spotify"})).await.is_err());
        assert_eq!(phone.opened.lock().unwrap().len(), 1, "nothing is opened when the app isn't found");

        let failing = OpenPhoneAppTool::new(Arc::new(FakePhone { fail_open: true, ..Default::default() }));
        assert!(failing.execute(&json!({"app": "Gmail"})).await.unwrap_err().message.contains("can't be opened"));
    }

    #[tokio::test]
    async fn reports_status_and_says_what_is_unknown() {
        let out = DeviceStatusTool::new(Arc::new(FakePhone::default())).execute(&json!({})).await.unwrap();
        assert_eq!(out.content, "Battery: 82% (charging). Network: Wi-Fi. Device: Google Pixel 8, Android 15 (API 35).");
        assert_eq!(describe_status(&PhoneStatus::default()), "Battery: unknown. Network: unknown. Device: unknown model, Android.");
    }
}
