//! IGRIS's Android device capabilities.
//!
//! The Kotlin side (`android/`) lists the apps that can be launched, opens one,
//! reads battery and network status, controls music, lets the user pick a
//! file, and seals IGRIS's device identity keys with a key held by the Android
//! Keystore. Nothing is exposed to the webview: IGRIS's own tools call
//! [`Device`] from Rust, so every use goes through the tool executor
//! (permissions, approvals, audit). On other platforms every
//! call fails with [`Error::Unsupported`].

use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

/// An app the phone's launcher can open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneApp {
    pub label: String,
    pub package: String,
}

/// Battery, network and device facts. Fields the phone couldn't report are `None`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneStatus {
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub android_version: Option<String>,
    pub sdk_int: Option<u32>,
    pub battery_percent: Option<f32>,
    pub charging: Option<bool>,
    /// "wifi", "cellular", "ethernet", "other" or "none".
    pub network: Option<String>,
}

/// One visible item on the phone's screen (from accessibility).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenItem {
    /// Its text or description ("(password field — not read)" for passwords).
    pub label: String,
    /// "button", "field", "checked"/"unchecked", "list" or "text".
    pub role: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    #[serde(default)]
    pub focused: bool,
}

/// What's on the phone's screen: the app in front and its visible items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenRead {
    pub package: String,
    pub app: String,
    pub width: i32,
    pub height: i32,
    pub items: Vec<ScreenItem>,
}

/// A message notification (one-time codes already hidden).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneMessage {
    /// The notification's key (for replying).
    pub id: String,
    pub app: String,
    /// Usually the sender or chat name.
    pub title: String,
    pub text: String,
    /// Milliseconds since the epoch.
    pub time: i64,
    pub can_reply: bool,
}

/// "Hey IGRIS" on the phone, and whether IGRIS is the phone's assistant.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WakeStatus {
    /// The user turned it on.
    pub on: bool,
    pub charging_only: bool,
    /// The listening service is running / hearing right now.
    pub running: bool,
    pub listening: bool,
    pub problem: Option<String>,
    /// "Display over other apps": IGRIS can open itself when it hears its name.
    pub can_open_itself: bool,
    /// Battery saving won't stop it.
    pub battery_unrestricted: bool,
    /// IGRIS is the phone's digital assistant app.
    pub assistant: bool,
}

/// Where the phone is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    /// Metres.
    pub accuracy: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub name: String,
    pub number: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("This is only available on Android.")]
    Unsupported,
    #[error("{0}")]
    Device(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(mobile)]
mod mobile;
#[cfg(mobile)]
pub use mobile::Device;

#[cfg(not(mobile))]
mod unsupported;
#[cfg(not(mobile))]
pub use unsupported::Device;

pub trait IgrisDeviceExt<R: Runtime> {
    fn igris_device(&self) -> &Device<R>;
}

impl<R: Runtime, T: Manager<R>> IgrisDeviceExt<R> for T {
    fn igris_device(&self) -> &Device<R> {
        self.state::<Device<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("igris-device")
        .setup(|app, api| {
            #[cfg(mobile)]
            let device = mobile::init(app, api)?;
            #[cfg(not(mobile))]
            let device = {
                let _ = api;
                unsupported::Device::new(app.clone())
            };
            app.manage(device);
            Ok(())
        })
        .build()
}
