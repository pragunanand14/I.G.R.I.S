//! IGRIS's Android device capabilities.
//!
//! The Kotlin side (`android/`) lists the apps that can be launched, opens one
//! and reads battery and network status. Nothing is exposed to the webview:
//! IGRIS's own tools call [`Device`] from Rust, so every use goes through the
//! tool executor (permissions, approvals, audit). On other platforms every
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
