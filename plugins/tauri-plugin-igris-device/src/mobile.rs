use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::plugin::{PluginApi, PluginHandle};
use tauri::{AppHandle, Runtime};

use crate::{Contact, Error, GoogleSetupInfo, Location, PhoneApp, PhoneMessage, PhoneStatus, RecordPoll, RecordRules, Result, ScreenRead, WakeStatus};

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

    /// Whether the user turned IGRIS's screen control on (Android accessibility settings).
    pub fn screen_access(&self) -> Result<bool> {
        #[derive(Deserialize)]
        struct R {
            enabled: bool,
        }
        self.0.run_mobile_plugin::<R>("screenAccess", ()).map(|r| r.enabled).map_err(failed)
    }

    pub fn open_accessibility_settings(&self) -> Result<()> {
        self.0.run_mobile_plugin::<Empty>("openAccessibilitySettings", ()).map(|_| ()).map_err(failed)
    }

    pub fn screen_read(&self) -> Result<ScreenRead> {
        self.0.run_mobile_plugin::<ScreenRead>("screenRead", ()).map_err(failed)
    }

    fn screen(&self, cmd: &str, args: ScreenArgs) -> Result<()> {
        self.0.run_mobile_plugin::<Empty>(cmd, args).map(|_| ()).map_err(failed)
    }

    /// Tap item `index` of the last read, or the point (x, y).
    pub fn screen_tap(&self, index: Option<usize>, x: f32, y: f32, long: bool) -> Result<()> {
        self.screen("screenTap", ScreenArgs { index: index.map(|i| i as i32).unwrap_or(-1), x, y, long, ..Default::default() })
    }

    /// Type into the focused field (or item `index`); `enter` presses the keyboard's Enter after.
    pub fn screen_type(&self, text: &str, index: Option<usize>, enter: bool) -> Result<()> {
        self.screen("screenType", ScreenArgs { index: index.map(|i| i as i32).unwrap_or(-1), text: Some(text.into()), enter, ..Default::default() })
    }

    pub fn screen_swipe(&self, direction: &str) -> Result<()> {
        self.screen("screenSwipe", ScreenArgs { direction: Some(direction.into()), ..Default::default() })
    }

    /// back, home, recents or notifications.
    pub fn screen_button(&self, action: &str) -> Result<()> {
        self.screen("screenButton", ScreenArgs { action: Some(action.into()), ..Default::default() })
    }

    /// Show the Stop pill with `text`, or hide it (`None`).
    pub fn screen_overlay(&self, text: Option<&str>) -> Result<()> {
        self.screen("screenOverlay", ScreenArgs { show: text.is_some(), text: text.map(str::to_string), ..Default::default() })
    }

    /// Whether the user tapped Stop since the last check.
    pub fn screen_take_stop(&self) -> Result<bool> {
        #[derive(Deserialize)]
        struct R {
            stop: bool,
        }
        self.0.run_mobile_plugin::<R>("screenTakeStop", ()).map(|r| r.stop).map_err(failed)
    }

    /// Whether the user gave IGRIS Notification access.
    pub fn messages_access(&self) -> Result<bool> {
        #[derive(Deserialize)]
        struct R {
            enabled: bool,
        }
        self.0.run_mobile_plugin::<R>("messagesAccess", ()).map(|r| r.enabled).map_err(failed)
    }

    pub fn open_notification_access_settings(&self) -> Result<()> {
        self.0.run_mobile_plugin::<Empty>("openNotificationAccessSettings", ()).map(|_| ()).map_err(failed)
    }

    /// Message notifications from the last `since_minutes` (messaging apps, or every app).
    pub fn recent_messages(&self, since_minutes: u32, all_apps: bool) -> Result<Vec<PhoneMessage>> {
        #[derive(Deserialize)]
        struct R {
            items: Vec<PhoneMessage>,
        }
        self.0.run_mobile_plugin::<R>("recentMessages", MessageArgs { since_minutes, all_apps, ..Default::default() }).map(|r| r.items).map_err(failed)
    }

    /// Reply through a notification's Reply button.
    pub fn reply_message(&self, id: &str, text: &str) -> Result<()> {
        self.0
            .run_mobile_plugin::<Empty>("replyMessage", MessageArgs { id: Some(id.into()), text: Some(text.into()), ..Default::default() })
            .map(|_| ())
            .map_err(failed)
    }

    /// Contacts whose name contains `name` (Android asks for Contacts permission the first time).
    pub fn find_contact(&self, name: &str) -> Result<Vec<Contact>> {
        #[derive(Deserialize)]
        struct R {
            contacts: Vec<Contact>,
        }
        self.0.run_mobile_plugin::<R>("findContact", MessageArgs { name: Some(name.into()), ..Default::default() }).map(|r| r.contacts).map_err(failed)
    }

    /// Open WhatsApp ("whatsapp") or the SMS app ("sms") with `text` typed to `number`.
    pub fn start_message(&self, app: &str, number: &str, text: &str) -> Result<()> {
        self.0
            .run_mobile_plugin::<Empty>(
                "startMessage",
                MessageArgs { app: Some(app.into()), number: Some(number.into()), text: Some(text.into()), ..Default::default() },
            )
            .map(|_| ())
            .map_err(failed)
    }

    /// Call `number`; `true` when only the dialer opened (no Phone permission).
    pub fn call_number(&self, number: &str) -> Result<bool> {
        #[derive(Deserialize)]
        struct R {
            dialed: bool,
        }
        self.0.run_mobile_plugin::<R>("callNumber", MessageArgs { number: Some(number.into()), ..Default::default() }).map(|r| r.dialed).map_err(failed)
    }

    /// "Hey IGRIS" and assistant status.
    pub fn wake_status(&self) -> Result<WakeStatus> {
        self.0.run_mobile_plugin::<WakeStatus>("wakeStatus", ()).map_err(failed)
    }

    /// Turn "Hey IGRIS" on or off (Android asks for the microphone the first time).
    pub fn wake_set(&self, on: bool, charging_only: bool) -> Result<()> {
        self.0.run_mobile_plugin::<Empty>("wakeSet", WakeArgs { on, charging_only }).map(|_| ()).map_err(failed)
    }

    /// IGRIS is using the microphone itself: the wake listener lets go until `on` is false.
    pub fn wake_hold(&self, on: bool) -> Result<()> {
        self.0.run_mobile_plugin::<Empty>("wakeHold", WakeArgs { on, charging_only: false }).map(|_| ()).map_err(failed)
    }

    /// Whether the wake phrase was heard while IGRIS was on screen (cleared once read).
    pub fn wake_take_heard(&self) -> Result<bool> {
        #[derive(Deserialize)]
        struct R {
            heard: bool,
        }
        self.0.run_mobile_plugin::<R>("wakeTakeHeard", ()).map(|r| r.heard).map_err(failed)
    }

    /// Android settings pages: "Display over other apps", battery, digital assistant app.
    pub fn open_settings_page(&self, page: &str) -> Result<()> {
        let cmd = match page {
            "overlay" => "openOverlaySettings",
            "battery" => "openBatterySettings",
            "assistant" => "openAssistantSettings",
            "app_info" => "openAppInfo",
            _ => return Err(Error::Unsupported),
        };
        self.0.run_mobile_plugin::<Empty>(cmd, ()).map(|_| ()).map_err(failed)
    }

    /// Start recording a spoken request natively (voice-recognition microphone; Android asks the first time).
    /// Returns the recording's session id (the other calls name it, so an old recorder can't touch a new one).
    pub fn record_start(&self, rules: RecordRules) -> Result<i32> {
        #[derive(Deserialize)]
        struct R {
            session: i32,
        }
        self.0.run_mobile_plugin::<R>("recordStart", rules).map(|r| r.session).map_err(failed)
    }

    /// Where session `session` is: state (idle, recording, done, failed), level 0–1, whether speech was heard.
    pub fn record_poll(&self, session: i32) -> Result<RecordPoll> {
        self.0.run_mobile_plugin::<RecordPoll>("recordPoll", RecordArgs { session, cancel: false }).map_err(failed)
    }

    /// Finish the session now, or `cancel` to drop it.
    pub fn record_stop(&self, session: i32, cancel: bool) -> Result<()> {
        self.0.run_mobile_plugin::<Empty>("recordStop", RecordArgs { session, cancel }).map(|_| ()).map_err(failed)
    }

    /// The session's finished recording as base64 WAV (None until it's done).
    pub fn record_take(&self, session: i32) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct R {
            wav: Option<String>,
        }
        self.0.run_mobile_plugin::<R>("recordTake", RecordArgs { session, cancel: false }).map(|r| r.wav).map_err(failed)
    }

    /// The assistant panel: see-through over other apps and the lock screen (`on`), or a normal app.
    pub fn sheet_mode(&self, on: bool) -> Result<()> {
        self.0.run_mobile_plugin::<Empty>("sheetMode", WakeArgs { on, charging_only: false }).map(|_| ()).map_err(failed)
    }

    /// Close the panel: back to whatever was on screen.
    pub fn sheet_hide(&self) -> Result<()> {
        self.0.run_mobile_plugin::<Empty>("sheetHide", ()).map(|_| ()).map_err(failed)
    }

    /// Ask the user to unlock the phone (fingerprint, face, PIN); `true` once it is unlocked.
    pub fn unlock(&self) -> Result<bool> {
        #[derive(Deserialize)]
        struct R {
            unlocked: bool,
        }
        self.0.run_mobile_plugin::<R>("unlock", ()).map(|r| r.unlocked).map_err(failed)
    }

    /// Where the phone is now (Android asks for location the first time).
    pub fn current_location(&self) -> Result<Location> {
        self.0.run_mobile_plugin::<Location>("currentLocation", ()).map_err(failed)
    }

    /// Remind (a notification) when arriving at (`enter`) or leaving a place. One-shot.
    #[allow(clippy::too_many_arguments)]
    pub fn add_place_alert(&self, id: i32, latitude: f64, longitude: f64, radius: f32, enter: bool, title: &str, text: &str) -> Result<()> {
        self.0
            .run_mobile_plugin::<Empty>(
                "addPlaceAlert",
                PlaceArgs { id, latitude, longitude, radius, enter, title: Some(title.into()), text: Some(text.into()) },
            )
            .map(|_| ())
            .map_err(failed)
    }

    pub fn remove_place_alert(&self, id: i32) -> Result<()> {
        self.0.run_mobile_plugin::<Empty>("removePlaceAlert", PlaceArgs { id, ..Default::default() }).map(|_| ()).map_err(failed)
    }

    /// Whether that place alert has gone off.
    pub fn place_alert_fired(&self, id: i32) -> Result<bool> {
        #[derive(Deserialize)]
        struct R {
            fired: bool,
        }
        self.0.run_mobile_plugin::<R>("placeAlertFired", PlaceArgs { id, ..Default::default() }).map(|r| r.fired).map_err(failed)
    }
}

#[derive(Serialize)]
struct RecordArgs {
    session: i32,
    cancel: bool,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct WakeArgs {
    on: bool,
    charging_only: bool,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct PlaceArgs {
    id: i32,
    latitude: f64,
    longitude: f64,
    radius: f32,
    enter: bool,
    title: Option<String>,
    text: Option<String>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct MessageArgs {
    id: Option<String>,
    text: Option<String>,
    since_minutes: u32,
    all_apps: bool,
    name: Option<String>,
    number: Option<String>,
    app: Option<String>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct ScreenArgs {
    index: i32,
    x: f32,
    y: f32,
    long: bool,
    text: Option<String>,
    enter: bool,
    direction: Option<String>,
    action: Option<String>,
    show: bool,
}

impl<R: Runtime> Device<R> {
    /// A Google access token for `scopes` from the account on the phone (Google Play services). With
    /// `interactive`, Google's own account picker / consent sheet is shown if needed (blocks until the user
    /// answers); without, `None` means the user has to be asked.
    pub fn google_authorize(&self, scopes: &[&str], interactive: bool) -> Result<Option<String>> {
        #[derive(Serialize)]
        struct A<'a> {
            scopes: &'a [&'a str],
            interactive: bool,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct T {
            access_token: Option<String>,
        }
        self.0.run_mobile_plugin::<T>("googleAuthorize", A { scopes, interactive }).map(|t| t.access_token).map_err(failed)
    }

    pub fn google_setup_info(&self) -> Result<GoogleSetupInfo> {
        self.0.run_mobile_plugin::<GoogleSetupInfo>("googleSetupInfo", ()).map_err(failed)
    }
}
