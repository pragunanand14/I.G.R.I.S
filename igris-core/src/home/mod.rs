//! Smart-home devices on the local network, controlled directly by IGRIS (no
//! vendor app or cloud in between). Each kind of device has its own driver;
//! today: LG webOS TVs ([`lg`]). Adding a brand means adding a driver and a
//! [`DeviceKind`] — the store, the tools and the UI are shared.
//!
//! Pairing secrets and pinned certificates stay in the database on the native
//! side; they are never sent to the UI or to the AI model.

pub mod discover;
pub mod lg;

use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};

use crate::db::Database;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    LgTv,
}

impl DeviceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceKind::LgTv => "lg_tv",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "lg_tv" => Some(DeviceKind::LgTv),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            DeviceKind::LgTv => "LG TV",
        }
    }
}

/// A paired device, as the UI and the AI see it (no secrets).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeDevice {
    pub id: String,
    pub kind: DeviceKind,
    pub name: String,
    pub host: String,
    /// Known, so the device can be switched on remotely (Wake-on-LAN).
    pub can_power_on: bool,
    pub created_at: String,
}

struct Stored {
    device: HomeDevice,
    mac: Option<String>,
    secret: Option<String>,
    cert: Option<String>,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Stored> {
    let kind: String = r.get("kind")?;
    let mac: Option<String> = r.get("mac")?;
    Ok(Stored {
        device: HomeDevice {
            id: r.get("id")?,
            kind: DeviceKind::parse(&kind).unwrap_or(DeviceKind::LgTv),
            name: r.get("name")?,
            host: r.get("host")?,
            can_power_on: mac.is_some(),
            created_at: r.get("created_at")?,
        },
        mac,
        secret: r.get("secret")?,
        cert: r.get("cert_sha256")?,
    })
}

pub fn list(conn: &Connection) -> AppResult<Vec<HomeDevice>> {
    let mut st = conn.prepare("SELECT * FROM home_devices ORDER BY created_at")?;
    let rows = st.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().map(|s| s.device).collect())
}

fn get(conn: &Connection, id: &str) -> AppResult<Stored> {
    conn.query_row("SELECT * FROM home_devices WHERE id = ?1", [id], row).optional()?.ok_or_else(|| AppError::validation("That device isn't set up in IGRIS."))
}

pub fn remove(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM home_devices WHERE id = ?1", [id])?;
    Ok(())
}

pub fn rename(conn: &Connection, id: &str, name: &str) -> AppResult<()> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err(AppError::validation("Give it a name of 1–40 characters."));
    }
    conn.execute("UPDATE home_devices SET name = ?2, updated_at = ?3 WHERE id = ?1", params![id, name, now()])?;
    Ok(())
}

/// Validate a host the user typed (an IP address or a local name; no URLs, no ports).
pub fn validate_host(host: &str) -> AppResult<String> {
    let h = host.trim();
    let ok = !h.is_empty() && h.len() <= 253 && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if !ok {
        return Err(AppError::validation("Enter the TV's IP address, like 192.168.1.20 (TV: Settings → Network → Wi-Fi → Advanced)."));
    }
    Ok(h.to_string())
}

/// Pick a device: by id, by (part of its) name, or the only one of that kind.
pub fn find(conn: &Connection, which: Option<&str>, kind: DeviceKind) -> AppResult<HomeDevice> {
    let all: Vec<HomeDevice> = list(conn)?.into_iter().filter(|d| d.kind == kind).collect();
    let which = which.map(str::trim).filter(|w| !w.is_empty());
    let pick = match which {
        Some(w) => {
            let w = w.to_lowercase();
            all.iter()
                .find(|d| d.id == w)
                .or_else(|| all.iter().find(|d| d.name.to_lowercase() == w))
                .or_else(|| all.iter().find(|d| d.name.to_lowercase().contains(&w)))
        }
        None if all.len() == 1 => all.first(),
        None => None,
    };
    match pick {
        Some(d) => Ok(d.clone()),
        None if all.is_empty() => Err(AppError::validation(format!("No {} is set up yet. Add it in IGRIS → Smart home.", kind.label()))),
        None => Err(AppError::validation(format!("Which one? {}", all.iter().map(|d| format!("\"{}\"", d.name)).collect::<Vec<_>>().join(", ")))),
    }
}

/// Things IGRIS can ask a TV to do.
#[derive(Debug, Clone, PartialEq)]
pub enum TvAction {
    PowerOn,
    PowerOff,
    VolumeUp,
    VolumeDown,
    SetVolume(u8),
    Mute(bool),
    ChannelUp,
    ChannelDown,
    Play,
    Pause,
    Stop,
    Rewind,
    FastForward,
    OpenApp(String),
    SwitchInput(String),
    Button(String),
    Message(String),
}

impl TvAction {
    /// From the tool/command form: an action name and an optional value.
    pub fn parse(action: &str, value: Option<&Value>) -> AppResult<TvAction> {
        let text = || value.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        let need = |what: &str| AppError::validation(format!("Say which {what}."));
        Ok(match action {
            "power_on" => TvAction::PowerOn,
            "power_off" => TvAction::PowerOff,
            "volume_up" => TvAction::VolumeUp,
            "volume_down" => TvAction::VolumeDown,
            "set_volume" => {
                let v = value.and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).ok_or_else(|| need("volume (0–100)"))?;
                TvAction::SetVolume(v.min(100) as u8)
            }
            "mute" => TvAction::Mute(true),
            "unmute" => TvAction::Mute(false),
            "channel_up" => TvAction::ChannelUp,
            "channel_down" => TvAction::ChannelDown,
            "play" => TvAction::Play,
            "pause" => TvAction::Pause,
            "stop" => TvAction::Stop,
            "rewind" => TvAction::Rewind,
            "fast_forward" => TvAction::FastForward,
            "open_app" => TvAction::OpenApp(text().ok_or_else(|| need("app"))?),
            "switch_input" => TvAction::SwitchInput(text().ok_or_else(|| need("input (e.g. HDMI 1)"))?),
            "button" => TvAction::Button(text().ok_or_else(|| need("button"))?),
            "show_message" => TvAction::Message(text().ok_or_else(|| need("message"))?.chars().take(200).collect()),
            other => return Err(AppError::validation(format!("Unknown TV action \"{other}\"."))),
        })
    }
}

/// Well-known webOS app ids, for names that don't match the TV's own titles.
const KNOWN_APPS: &[(&str, &str)] = &[
    ("youtube", "youtube.leanback.v4"),
    ("netflix", "netflix"),
    ("prime video", "amazon"),
    ("amazon", "amazon"),
    ("disney", "com.disney.disneyplus-prod"),
    ("hotstar", "hotstar"),
    ("spotify", "spotify-beehive"),
    ("browser", "com.webos.app.browser"),
    ("live tv", "com.webos.app.livetv"),
    ("settings", "com.palm.app.settings"),
];

/// The smart-home service: pairing, control, state.
#[derive(Clone)]
pub struct SmartHome {
    db: Arc<Database>,
}

/// What the TV reported after a status request.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TvState {
    pub on: bool,
    pub volume: Option<u64>,
    pub muted: Option<bool>,
    pub app: Option<String>,
    pub apps: Vec<TvApp>,
    pub inputs: Vec<TvApp>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TvApp {
    pub id: String,
    pub title: String,
}

impl SmartHome {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn devices(&self) -> AppResult<Vec<HomeDevice>> {
        list(&*self.db.conn()?)
    }

    /// Pair an LG TV: the TV shows a prompt, the user presses Allow on it.
    pub async fn pair_lg(&self, host: &str, name: Option<&str>) -> AppResult<HomeDevice> {
        let host = if host.contains(':') { host.to_string() } else { validate_host(host)? };
        let mut tv = lg::Tv::connect(&host, None, None).await?;
        let key = tv.session.client_key.clone().ok_or_else(|| AppError::validation("The TV didn't confirm the pairing."))?;
        let info = tv.request("system/getSystemInfo", json!({})).await.unwrap_or_default();
        let net = tv.request("com.webos.service.connectionmanager/getinfo", json!({})).await.unwrap_or_default();
        let mac = net["wiredInfo"]["macAddress"].as_str().or(net["wifiInfo"]["macAddress"].as_str()).filter(|m| m.len() >= 12).map(str::to_string);
        let cert = tv.session.cert_sha256.clone();
        tv.close().await;
        let model = info["modelName"].as_str().unwrap_or("").to_string();
        let name = name.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| {
            if model.is_empty() {
                "LG TV".into()
            } else {
                format!("LG TV ({model})")
            }
        });
        let id = format!("home_{}", &lg::fingerprint(format!("{host}{}", now()).as_bytes())[..12]);
        let c = self.db.conn()?;
        // Re-pairing the same TV replaces the old entry.
        c.execute("DELETE FROM home_devices WHERE kind = ?1 AND host = ?2", params![DeviceKind::LgTv.as_str(), host])?;
        c.execute(
            "INSERT INTO home_devices (id, kind, name, host, mac, secret, cert_sha256, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![id, DeviceKind::LgTv.as_str(), name, host, mac, key, cert, now()],
        )?;
        tracing::info!(event = "HOME_DEVICE_PAIRED", kind = "lg_tv", can_power_on = mac.is_some());
        get(&c, &id).map(|s| s.device)
    }

    async fn open(&self, id: &str) -> AppResult<(lg::Tv, Stored)> {
        let s = get(&*self.db.conn()?, id)?;
        let tv = lg::Tv::connect(&s.device.host, s.secret.as_deref(), s.cert.as_deref()).await?;
        // The TV may hand out a new key; keep it.
        if tv.session.client_key.is_some() && tv.session.client_key != s.secret {
            self.db.conn()?.execute("UPDATE home_devices SET secret = ?2, updated_at = ?3 WHERE id = ?1", params![id, tv.session.client_key, now()])?;
        }
        Ok((tv, s))
    }

    /// Do one thing on a TV; the answer says what really happened.
    pub async fn tv(&self, id: &str, action: &TvAction) -> AppResult<String> {
        if *action == TvAction::PowerOn {
            let s = get(&*self.db.conn()?, id)?;
            let mac = s
                .mac
                .ok_or_else(|| AppError::validation("IGRIS doesn't know this TV's network address, so it can't switch it on. Pair it again while it's on."))?;
            lg::wake(&mac).await?;
            // It takes a few seconds to come up; report what we can confirm.
            for _ in 0..12 {
                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                if let Ok((tv, _)) = self.open(id).await {
                    tv.close().await;
                    return Ok(format!("{} is on.", s.device.name));
                }
            }
            return Ok(format!(
                "Sent the wake-up signal to {}, but it hasn't answered yet. If it stays off, turn on \"Turn on via Wi-Fi\" (or \"Mobile TV On\") in the TV's settings.",
                s.device.name
            ));
        }
        let (mut tv, s) = self.open(id).await?;
        let name = s.device.name.clone();
        let result = run(&mut tv, action).await.map(|done| format!("{name}: {done}"));
        tv.close().await;
        result
    }

    /// The TV's state for the remote screen: on/off, volume, current app, apps and inputs.
    pub async fn tv_state(&self, id: &str) -> AppResult<TvState> {
        let Ok((mut tv, _)) = self.open(id).await else {
            return Ok(TvState::default()); // off or unreachable
        };
        let vol = tv.request("audio/getVolume", json!({})).await.unwrap_or_default();
        let vs = if vol["volumeStatus"].is_object() { vol["volumeStatus"].clone() } else { vol.clone() };
        let app = tv.request("com.webos.applicationManager/getForegroundAppInfo", json!({})).await.unwrap_or_default();
        let apps = tv.request("com.webos.applicationManager/listLaunchPoints", json!({})).await.unwrap_or_default();
        let inputs = tv.request("tv/getExternalInputList", json!({})).await.unwrap_or_default();
        tv.close().await;
        let list = |v: &Value, key: &str, id: &str, title: &str| -> Vec<TvApp> {
            v[key]
                .as_array()
                .map(|a| {
                    a.iter().filter_map(|x| Some(TvApp { id: x[id].as_str()?.to_string(), title: x[title].as_str().unwrap_or_default().to_string() })).collect()
                })
                .unwrap_or_default()
        };
        Ok(TvState {
            on: true,
            volume: vs["volume"].as_u64(),
            muted: vs["muteStatus"].as_bool().or(vs["mute"].as_bool()),
            app: app["appId"].as_str().map(str::to_string),
            apps: list(&apps, "launchPoints", "id", "title"),
            inputs: list(&inputs, "devices", "id", "label"),
        })
    }
}

/// Open touchpad sessions: the TV connection and its input channel, kept while
/// the user moves the cursor (closed after [`POINTER_IDLE`] without input).
struct Held {
    tv: lg::Tv,
    input: lg::Pointer,
    last: std::time::Instant,
}

const POINTER_IDLE: std::time::Duration = std::time::Duration::from_secs(60);

fn pointers() -> &'static tokio::sync::Mutex<std::collections::HashMap<String, Held>> {
    static P: std::sync::OnceLock<tokio::sync::Mutex<std::collections::HashMap<String, Held>>> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        // Close idle sessions in the background.
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt.spawn(async {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    let idle: Vec<Held> = {
                        let mut m = pointers().lock().await;
                        let ids: Vec<String> = m.iter().filter(|(_, h)| h.last.elapsed() > POINTER_IDLE).map(|(k, _)| k.clone()).collect();
                        ids.into_iter().filter_map(|k| m.remove(&k)).collect()
                    };
                    for h in idle {
                        h.input.close().await;
                        h.tv.close().await;
                    }
                }
            });
        }
        tokio::sync::Mutex::new(std::collections::HashMap::new())
    })
}

impl SmartHome {
    /// Move the TV's cursor, click or scroll. Opens the input channel on first use and keeps it open.
    pub async fn pointer(&self, id: &str, events: &[lg::PointerEvent]) -> AppResult<()> {
        if events.is_empty() {
            return Ok(());
        }
        let mut map = pointers().lock().await;
        for attempt in 0..2 {
            if !map.contains_key(id) {
                let (mut tv, _) = self.open(id).await?;
                let input = tv.pointer().await?;
                map.insert(id.to_string(), Held { tv, input, last: std::time::Instant::now() });
            }
            let held = map.get_mut(id).expect("just inserted");
            held.last = std::time::Instant::now();
            let mut failed = None;
            for e in events {
                if let Err(err) = held.input.send(e).await {
                    failed = Some(err);
                    break;
                }
            }
            match failed {
                None => return Ok(()),
                // The TV closed the channel (e.g. it slept): reconnect once and resend.
                Some(err) => {
                    if let Some(h) = map.remove(id) {
                        h.tv.close().await;
                    }
                    if attempt == 1 {
                        return Err(err);
                    }
                }
            }
        }
        Ok(())
    }

    /// Close the touchpad session (leaving the screen, or Esc on the desktop).
    pub async fn pointer_stop(&self, id: &str) {
        let held = pointers().lock().await.remove(id);
        if let Some(h) = held {
            h.input.close().await;
            h.tv.close().await;
        }
    }
}

async fn run(tv: &mut lg::Tv, action: &TvAction) -> AppResult<String> {
    let simple = |uri: &'static str, done: &'static str| (uri, done);
    let (uri, done) = match action {
        TvAction::PowerOff => simple("system/turnOff", "turned off"),
        TvAction::VolumeUp => simple("audio/volumeUp", "volume up"),
        TvAction::VolumeDown => simple("audio/volumeDown", "volume down"),
        TvAction::ChannelUp => simple("tv/channelUp", "next channel"),
        TvAction::ChannelDown => simple("tv/channelDown", "previous channel"),
        TvAction::Play => simple("media.controls/play", "playing"),
        TvAction::Pause => simple("media.controls/pause", "paused"),
        TvAction::Stop => simple("media.controls/stop", "stopped"),
        TvAction::Rewind => simple("media.controls/rewind", "rewinding"),
        TvAction::FastForward => simple("media.controls/fastForward", "fast-forwarding"),
        TvAction::SetVolume(v) => {
            tv.request("audio/setVolume", json!({ "volume": v })).await?;
            return Ok(format!("volume set to {v}"));
        }
        TvAction::Mute(m) => {
            tv.request("audio/setMute", json!({ "mute": m })).await?;
            return Ok(if *m { "muted".into() } else { "unmuted".into() });
        }
        TvAction::Message(text) => {
            tv.request("system.notifications/createToast", json!({ "message": text })).await?;
            return Ok("message shown on screen".into());
        }
        TvAction::Button(b) => {
            tv.button(b).await?;
            return Ok(format!("pressed {}", b.to_ascii_uppercase()));
        }
        TvAction::OpenApp(name) => {
            let apps = tv.request("com.webos.applicationManager/listLaunchPoints", json!({})).await.unwrap_or_default();
            let want = name.to_lowercase();
            let installed: Vec<(String, String)> = apps["launchPoints"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| Some((x["id"].as_str()?.to_string(), x["title"].as_str().unwrap_or_default().to_string()))).collect())
                .unwrap_or_default();
            let hit = installed
                .iter()
                .find(|(id, t)| t.to_lowercase() == want || *id == want)
                .or_else(|| installed.iter().find(|(_, t)| t.to_lowercase().contains(&want)))
                .map(|(id, t)| (id.clone(), t.clone()))
                .or_else(|| KNOWN_APPS.iter().find(|(k, _)| want.contains(k)).map(|(_, id)| (id.to_string(), name.clone())));
            let Some((id, title)) = hit else {
                let names: Vec<&str> = installed.iter().map(|(_, t)| t.as_str()).take(30).collect();
                return Err(AppError::validation(format!("\"{name}\" isn't installed on the TV. Apps: {}.", names.join(", "))));
            };
            tv.request("system.launcher/launch", json!({ "id": id })).await?;
            return Ok(format!("opened {title}"));
        }
        TvAction::SwitchInput(name) => {
            let inputs = tv.request("tv/getExternalInputList", json!({})).await?;
            let norm = |s: &str| s.to_lowercase().replace([' ', '_', '-'], "");
            let want = norm(name);
            let devices = inputs["devices"].as_array().cloned().unwrap_or_default();
            let hit = devices
                .iter()
                .find(|d| norm(d["label"].as_str().unwrap_or_default()) == want || norm(d["id"].as_str().unwrap_or_default()) == want)
                .or_else(|| {
                    devices
                        .iter()
                        .find(|d| norm(d["label"].as_str().unwrap_or_default()).contains(&want) || norm(d["id"].as_str().unwrap_or_default()).contains(&want))
                });
            let Some(d) = hit else {
                let labels: Vec<&str> = devices.iter().filter_map(|d| d["label"].as_str()).collect();
                return Err(AppError::validation(format!("No input called \"{name}\". Inputs: {}.", labels.join(", "))));
            };
            let id = d["id"].as_str().unwrap_or_default();
            tv.request("tv/switchInput", json!({ "inputId": id })).await?;
            return Ok(format!("switched to {}", d["label"].as_str().unwrap_or(id)));
        }
        TvAction::PowerOn => unreachable!("handled before connecting"),
    };
    tv.request(uri, json!({})).await?;
    Ok(done.into())
}

#[cfg(test)]
mod tests;
