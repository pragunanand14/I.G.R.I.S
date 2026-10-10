//! Music control on Windows through the system media controls (the same ones
//! as the media overlay and keyboard media keys), so it works with whatever
//! app is playing: Spotify, a browser tab, the Media Player, …

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE,
    VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP, VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
};

use crate::tools::media::{MediaAction, NowPlaying};

fn err(e: windows::core::Error) -> String {
    format!("Windows media controls failed: {e}")
}

fn session() -> Result<Option<Session>, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let manager = Manager::RequestAsync().map_err(err)?.join().map_err(err)?;
    Ok(manager.GetCurrentSession().ok())
}

/// Press a media/volume key (what the keyboard's media keys do).
fn press(vk: VIRTUAL_KEY) -> Result<(), String> {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err("Windows blocked the media key.".into());
    }
    Ok(())
}

pub fn control(action: MediaAction) -> Result<(), String> {
    let key = match action {
        MediaAction::VolumeUp => return press(VK_VOLUME_UP),
        MediaAction::VolumeDown => return press(VK_VOLUME_DOWN),
        MediaAction::Mute => return press(VK_VOLUME_MUTE),
        MediaAction::PlayPause | MediaAction::Play | MediaAction::Pause => VK_MEDIA_PLAY_PAUSE,
        MediaAction::Next => VK_MEDIA_NEXT_TRACK,
        MediaAction::Previous => VK_MEDIA_PREV_TRACK,
        MediaAction::Stop => VK_MEDIA_STOP,
    };
    if let Some(s) = session()? {
        let op = match action {
            MediaAction::Play => s.TryPlayAsync(),
            MediaAction::Pause => s.TryPauseAsync(),
            MediaAction::Next => s.TrySkipNextAsync(),
            MediaAction::Previous => s.TrySkipPreviousAsync(),
            MediaAction::Stop => s.TryStopAsync(),
            _ => s.TryTogglePlayPauseAsync(),
        };
        if op.map_err(err)?.join().map_err(err)? {
            return Ok(());
        }
    }
    // No player answered through the media controls: the media key reaches whatever listens for it.
    press(key)
}

/// "Spotify.exe", "SpotifyAB.SpotifyMusic_…!Spotify", "Chrome" → a name people know.
fn app_name(id: &str) -> Option<String> {
    let last = id.rsplit('!').next().unwrap_or(id);
    let name = last.trim_end_matches(".exe").trim_end_matches(".EXE");
    let name = name.rsplit(['\\', '/']).next().unwrap_or(name);
    let known = [("spotify", "Spotify"), ("chrome", "Chrome"), ("msedge", "Edge"), ("firefox", "Firefox"), ("zunemusic", "Media Player"), ("vlc", "VLC")];
    let lower = id.to_lowercase();
    known.iter().find(|(k, _)| lower.contains(k)).map(|(_, v)| v.to_string()).or_else(|| (!name.is_empty()).then(|| name.to_string()))
}

pub fn now_playing() -> Result<NowPlaying, String> {
    let Some(s) = session()? else { return Ok(NowPlaying::default()) };
    let status = s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).ok();
    let props = s.TryGetMediaPropertiesAsync().ok().and_then(|op| op.join().ok());
    let text = |h: windows::core::Result<windows::core::HSTRING>| h.ok().map(|h| h.to_string()).filter(|t| !t.trim().is_empty());
    Ok(NowPlaying {
        playing: status == Some(Status::Playing),
        active: matches!(status, Some(Status::Playing) | Some(Status::Paused) | Some(Status::Changing) | Some(Status::Opened)),
        title: props.as_ref().and_then(|p| text(p.Title())),
        artist: props.as_ref().and_then(|p| text(p.Artist())),
        app: s.SourceAppUserModelId().ok().and_then(|id| app_name(&id.to_string())),
    })
}
