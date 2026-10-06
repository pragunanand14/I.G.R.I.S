use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::plugin::{PluginApi, PluginHandle};
use tauri::{AppHandle, Runtime};

use crate::{Error, PhoneApp, PhoneStatus, Result};

pub fn init<R: Runtime, C: DeserializeOwned>(_app: &AppHandle<R>, api: PluginApi<R, C>) -> std::result::Result<Device<R>, Box<dyn std::error::Error>> {
    #[cfg(target_os = "android")]
    let handle = api.register_android_plugin("dev.igris.device", "DevicePlugin")?;
    #[cfg(not(target_os = "android"))]
    let handle: PluginHandle<R> = return Err("IGRIS device capabilities are only implemented for Android.".into());
    Ok(Device(handle))
}

/// The phone, through the Kotlin plugin. Calls block until the phone answers.
pub struct Device<R: Runtime>(PluginHandle<R>);

#[derive(Deserialize)]
struct Apps {
    apps: Vec<PhoneApp>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchArgs<'a> {
    package_name: &'a str,
}

#[derive(Deserialize)]
struct Empty {}

fn failed(e: impl std::fmt::Display) -> Error {
    Error::Device(e.to_string())
}

impl<R: Runtime> Device<R> {
    pub fn list_apps(&self) -> Result<Vec<PhoneApp>> {
        self.0.run_mobile_plugin::<Apps>("listApps", ()).map(|a| a.apps).map_err(failed)
    }

    pub fn launch_app(&self, package: &str) -> Result<()> {
        self.0.run_mobile_plugin::<Empty>("launchApp", LaunchArgs { package_name: package }).map(|_| ()).map_err(failed)
    }

    pub fn status(&self) -> Result<PhoneStatus> {
        self.0.run_mobile_plugin::<PhoneStatus>("status", ()).map_err(failed)
    }
}
