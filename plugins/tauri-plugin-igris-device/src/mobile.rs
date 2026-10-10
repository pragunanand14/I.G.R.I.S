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

    /// A media button or volume step ("play_pause", "next", "volume_up", …).
    pub fn media_control(&self, action: &str) -> Result<()> {
        #[derive(Serialize)]
        struct A<'a> {
            action: &'a str,
        }
        self.0.run_mobile_plugin::<Empty>("mediaControl", A { action }).map(|_| ()).map_err(failed)
    }

    /// Whether music is playing on the phone.
    pub fn media_playing(&self) -> Result<bool> {
        #[derive(Deserialize)]
        struct S {
            playing: bool,
        }
        self.0.run_mobile_plugin::<S>("mediaState", ()).map(|s| s.playing).map_err(failed)
    }

    /// Ask a music app (or the default one) to play `query`; returns the app's name.
    pub fn play_from_search(&self, query: &str, package: Option<&str>) -> Result<String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct A<'a> {
            query: &'a str,
            package_name: Option<&'a str>,
        }
        #[derive(Deserialize)]
        struct R {
            app: String,
        }
        self.0.run_mobile_plugin::<R>("playFromSearch", A { query, package_name: package }).map(|r| r.app).map_err(failed)
    }

    /// What a home-screen widget asked for when it opened IGRIS ("listen"), once.
    pub fn take_launch_action(&self) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct R {
            action: Option<String>,
        }
        self.0.run_mobile_plugin::<R>("takeLaunchAction", ()).map(|r| r.action).map_err(failed)
    }

    /// Set the weather widget's place (name and coordinates) and refresh it.
    pub fn set_weather_place(&self, name: &str, latitude: f64, longitude: f64) -> Result<()> {
        #[derive(Serialize)]
        struct A<'a> {
            name: &'a str,
            latitude: f64,
            longitude: f64,
        }
        self.0.run_mobile_plugin::<Empty>("setWeatherPlace", A { name, latitude, longitude }).map(|_| ()).map_err(failed)
    }

    /// The weather widget's place, if one was chosen.
    pub fn weather_place(&self) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct R {
            name: Option<String>,
        }
        self.0.run_mobile_plugin::<R>("getWeatherPlace", ()).map(|r| r.name).map_err(failed)
    }
}
