//! Music and media on this device: play/pause/skip, volume, what's playing,
//! and starting a song in the user's music app. Windows uses the system media
//! controls (whatever app is playing: Spotify, a browser, …); Android sends
//! media buttons and asks the music app to play a search. Each platform
//! provides a [`Media`]; the tools below are shared.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaAction {
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
    Stop,
    VolumeUp,
    VolumeDown,
    Mute,
}

impl MediaAction {
    pub const ALL: &'static [&'static str] = &["play_pause", "play", "pause", "next", "previous", "stop", "volume_up", "volume_down", "mute"];

    pub fn parse(s: &str) -> Option<Self> {
        serde_json::from_value(Value::String(s.to_string())).ok()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MediaAction::PlayPause => "play_pause",
            MediaAction::Play => "play",
            MediaAction::Pause => "pause",
            MediaAction::Next => "next",
            MediaAction::Previous => "previous",
            MediaAction::Stop => "stop",
            MediaAction::VolumeUp => "volume_up",
            MediaAction::VolumeDown => "volume_down",
            MediaAction::Mute => "mute",
        }
    }

    fn done(self) -> &'static str {
        match self {
            MediaAction::PlayPause => "Toggled play/pause",
            MediaAction::Play => "Pressed play",
            MediaAction::Pause => "Paused",
            MediaAction::Next => "Skipped to the next track",
            MediaAction::Previous => "Went back to the previous track",
            MediaAction::Stop => "Stopped playback",
            MediaAction::VolumeUp => "Turned the volume up",
            MediaAction::VolumeDown => "Turned the volume down",
            MediaAction::Mute => "Toggled mute",
        }
    }
}

/// What's playing, as far as this device can tell.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    /// Something is playing right now.
    pub playing: bool,
    /// There's a player to control (playing or paused).
    pub active: bool,
    pub title: Option<String>,
    pub artist: Option<String>,
    /// The app playing it, when known.
    pub app: Option<String>,
}

/// Which app "play …" should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicApp {
    /// Whatever the device would use (on the phone: the default music app).
    Default,
    Spotify,
    YoutubeMusic,
}

/// What the media tools need from the device. Calls may block.
pub trait Media: Send + Sync {
    fn control(&self, action: MediaAction) -> Result<(), String>;
    fn now_playing(&self) -> Result<NowPlaying, String>;
    /// Start (or search for) `query` in a music app; returns what really happened.
    fn play(&self, query: &str, app: MusicApp) -> Result<String, String>;
}

pub type SharedMedia = Arc<dyn Media>;

/// Media details come from other apps: one line, no control characters, bounded.
pub fn clean(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).take(120).collect();
    s.trim().to_string()
}

async fn on_device<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, ToolError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| ToolError::failed(e.to_string()))?.map_err(ToolError::failed)
}

pub fn describe_playing(n: &NowPlaying) -> String {
    if !n.active && !n.playing {
        return "Nothing is playing.".into();
    }
    let what = match (n.title.as_deref().map(clean), n.artist.as_deref().map(clean)) {
        (Some(t), Some(a)) if !a.is_empty() => format!("\"{t}\" by {a}"),
        (Some(t), _) => format!("\"{t}\""),
        _ => "Something".into(),
    };
    let app = n.app.as_deref().map(|a| format!(" in {}", clean(a))).unwrap_or_default();
    format!("{what}{app} — {}", if n.playing { "playing" } else { "paused" })
}

// ----- media_control -----

pub struct MediaControlTool {
    spec: ToolSpec,
    media: SharedMedia,
}

impl MediaControlTool {
    pub fn new(media: SharedMedia) -> Self {
        Self {
            spec: ToolSpec {
                name: "media_control",
                title: "Control music",
                description: "Control the music or video playing on this device (any app): play_pause, play, pause, next, previous, stop, \
volume_up, volume_down, mute (toggles). For the TV use tv_control instead.",
                input_schema: json!({
                    "type": "object",
                    "properties": { "action": { "type": "string", "enum": MediaAction::ALL } },
                    "required": ["action"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            media,
        }
    }
}

#[async_trait::async_trait]
impl Tool for MediaControlTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("Music: {}", i["action"].as_str().unwrap_or("control").replace('_', " "))
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let action = MediaAction::parse(i["action"].as_str().unwrap_or_default()).ok_or_else(|| ToolError::invalid("Unknown media action."))?;
        let media = self.media.clone();
        on_device(move || media.control(action)).await?;
        Ok(ToolOutput { content: format!("{}.", action.done()), summary: action.done().into(), sources: vec![], media: Vec::new() })
    }
}

// ----- now_playing -----

pub struct NowPlayingTool {
    spec: ToolSpec,
    media: SharedMedia,
}

impl NowPlayingTool {
    pub fn new(media: SharedMedia) -> Self {
        Self {
            spec: ToolSpec {
                name: "now_playing",
                title: "What's playing",
                description: "What music or media is playing on this device (title, artist, app, playing or paused), as far as the device \
reports it.",
                input_schema: json!({"type": "object", "properties": {}, "required": [], "additionalProperties": false}),
                permission: PermissionLevel::Safe,
            },
            media,
        }
    }
}

#[async_trait::async_trait]
impl Tool for NowPlayingTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, _i: &Value) -> String {
        "Check what's playing".into()
    }
    async fn execute(&self, _i: &Value) -> ToolResultT {
        let media = self.media.clone();
        let n = on_device(move || media.now_playing()).await?;
        let text = describe_playing(&n);
        Ok(ToolOutput { content: text.clone(), summary: text, sources: vec![], media: Vec::new() })
    }
}

// ----- play_music -----

pub struct PlayMusicTool {
    spec: ToolSpec,
    media: SharedMedia,
}

impl PlayMusicTool {
    pub fn new(media: SharedMedia) -> Self {
        Self {
            spec: ToolSpec {
                name: "play_music",
                title: "Play music",
                description: "Play a song, artist, album or playlist in the user's music app. `app`: \"spotify\", \"youtube_music\", or \"\" \
for the default. Report the result as given: on some devices it opens the search and the user presses play.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "minLength": 1, "maxLength": 200 },
                        "app": { "type": "string", "enum": ["", "spotify", "youtube_music"] }
                    },
                    "required": ["query", "app"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Low,
            },
            media,
        }
    }
}

#[async_trait::async_trait]
impl Tool for PlayMusicTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("Play \"{}\"", clean(i["query"].as_str().unwrap_or_default()))
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let query = clean(i["query"].as_str().unwrap_or_default());
        if query.is_empty() {
            return Err(ToolError::invalid("Say what to play."));
        }
        let app = match i["app"].as_str().unwrap_or_default() {
            "spotify" => MusicApp::Spotify,
            "youtube_music" => MusicApp::YoutubeMusic,
            _ => MusicApp::Default,
        };
        let media = self.media.clone();
        let done = on_device(move || media.play(&query, app)).await?;
        Ok(ToolOutput { content: done.clone(), summary: done, sources: vec![], media: Vec::new() })
    }
}

/// For hosts without music control (test harnesses): says so.
pub struct Unavailable;

impl Media for Unavailable {
    fn control(&self, _action: MediaAction) -> Result<(), String> {
        Err("Music control isn't available here.".into())
    }
    fn now_playing(&self) -> Result<NowPlaying, String> {
        Err("Music control isn't available here.".into())
    }
    fn play(&self, _query: &str, _app: MusicApp) -> Result<String, String> {
        Err("Music control isn't available here.".into())
    }
}

pub fn all(media: &SharedMedia) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(MediaControlTool::new(media.clone())), Arc::new(NowPlayingTool::new(media.clone())), Arc::new(PlayMusicTool::new(media.clone()))]
}

/// Search links for desktop music apps (they open the search; the user picks).
pub fn search_link(query: &str, app: MusicApp) -> String {
    let q: String = percent_encoding::utf8_percent_encode(query, percent_encoding::NON_ALPHANUMERIC).to_string();
    match app {
        MusicApp::Spotify => format!("spotify:search:{q}"),
        MusicApp::YoutubeMusic | MusicApp::Default => format!("https://music.youtube.com/search?q={q}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake(Mutex<Vec<String>>);

    impl Media for Fake {
        fn control(&self, action: MediaAction) -> Result<(), String> {
            self.0.lock().unwrap().push(action.as_str().into());
            Ok(())
        }
        fn now_playing(&self) -> Result<NowPlaying, String> {
            Ok(NowPlaying { playing: true, active: true, title: Some("Numb\n".into()), artist: Some("Linkin Park".into()), app: Some("Spotify".into()) })
        }
        fn play(&self, query: &str, _app: MusicApp) -> Result<String, String> {
            self.0.lock().unwrap().push(format!("play {query}"));
            Ok(format!("Playing {query}"))
        }
    }

    #[tokio::test]
    async fn the_tools_drive_the_device() {
        let fake = Arc::new(Fake::default());
        let media: SharedMedia = fake.clone();
        let [control, now, play]: [Arc<dyn Tool>; 3] = all(&media).try_into().ok().unwrap();
        assert_eq!(control.execute(&json!({"action": "next"})).await.unwrap().summary, "Skipped to the next track");
        assert!(control.execute(&json!({"action": "explode"})).await.is_err());
        assert_eq!(now.execute(&json!({})).await.unwrap().content, "\"Numb\" by Linkin Park in Spotify — playing");
        assert_eq!(play.execute(&json!({"query": "lofi beats", "app": ""})).await.unwrap().summary, "Playing lofi beats");
        assert_eq!(*fake.0.lock().unwrap(), ["next", "play lofi beats"]);
    }

    #[test]
    fn actions_round_trip_and_links_are_encoded() {
        for a in MediaAction::ALL {
            assert_eq!(MediaAction::parse(a).unwrap().as_str(), *a);
        }
        assert_eq!(search_link("AC/DC & me", MusicApp::Spotify), "spotify:search:AC%2FDC%20%26%20me");
        assert!(search_link("x", MusicApp::Default).starts_with("https://music.youtube.com/search?q="));
        assert_eq!(describe_playing(&NowPlaying::default()), "Nothing is playing.");
    }
}
