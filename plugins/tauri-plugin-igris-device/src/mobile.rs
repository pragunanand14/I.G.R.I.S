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

#[derive(Deserialize)]
struct Picked {
    data: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Data {
    data: String,
}

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

    /// Seal bytes (standard base64 in and out) with a key held by the Android Keystore.
    pub fn seal_keys(&self, data_b64: &str) -> Result<String> {
        self.0.run_mobile_plugin::<Data>("sealKeys", Data { data: data_b64.to_string() }).map(|d| d.data).map_err(failed)
    }

    /// Open what [`Device::seal_keys`] sealed.
    pub fn unseal_keys(&self, sealed_b64: &str) -> Result<String> {
        self.0.run_mobile_plugin::<Data>("unsealKeys", Data { data: sealed_b64.to_string() }).map(|d| d.data).map_err(failed)
    }

    /// Let the user pick a file; its bytes (standard base64), or `None` if they cancelled.
    /// Blocks until they choose, so call it off the async runtime.
    pub fn pick_file(&self) -> Result<Option<String>> {
        self.0.run_mobile_plugin::<Picked>("pickFile", ()).map(|p| p.data).map_err(failed)
    }
}
