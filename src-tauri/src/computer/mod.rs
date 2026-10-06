//! Computer control: seeing the screen and driving mouse and keyboard.
//!
//! Everything goes through the [`Driver`] trait so the operator tools can be
//! tested with a fake desktop. The real driver is Windows-only for now
//! ([`windows`]); other platforms get [`Unsupported`], which says so instead of
//! pretending. Targeting prefers accessibility data (UI Automation element
//! names, roles and bounds) over raw coordinates.

use std::sync::Arc;

use image::RgbaImage;
use serde::Serialize;

pub mod browser;
pub mod cdp;
pub mod state;
#[cfg(windows)]
pub mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Display {
    pub id: u32,
    pub name: String,
    /// Physical pixels in virtual-desktop coordinates.
    pub rect: Rect,
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowInfo {
    /// Native handle; stable while the window exists.
    pub id: u64,
    pub title: String,
    /// Executable name, e.g. `Code.exe`.
    pub process: String,
    pub pid: u32,
    pub rect: Rect,
    pub minimized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UiElement {
    pub name: String,
    /// Control type, e.g. `button`, `edit`, `hyperlink`.
    pub role: String,
    pub rect: Rect,
    pub enabled: bool,
    pub focused: bool,
}

/// A line of text recognised on screen (OCR), in image pixels.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OcrLine {
    pub text: String,
    pub rect: Rect,
}

/// The focused text field's state, read through accessibility.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FieldInfo {
    /// Current text, when the control exposes it (never for password fields).
    pub value: Option<String>,
    /// `Some(true)` read-only, `Some(false)` editable, `None` unknown.
    pub read_only: Option<bool>,
    pub password: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// A key in a combination. Letters/digits are layout-independent virtual keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Ctrl,
    Shift,
    Alt,
    Win,
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Insert,
    Space,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    Char(char),
}

impl Key {
    pub fn is_modifier(self) -> bool {
        matches!(self, Key::Ctrl | Key::Shift | Key::Alt | Key::Win)
    }
}

/// Parse "ctrl+shift+s", "enter", "alt+f4", "f5". Modifiers first, one main key.
pub fn parse_keys(spec: &str) -> Result<Vec<Key>, String> {
    let parts: Vec<String> = spec.split('+').map(|p| p.trim().to_ascii_lowercase()).collect();
    if parts.iter().any(String::is_empty) {
        return Err(format!("Invalid key combination \"{spec}\"."));
    }
    let mut keys = Vec::new();
    for p in &parts {
        let k = match p.as_str() {
            "ctrl" | "control" => Key::Ctrl,
            "shift" => Key::Shift,
            "alt" => Key::Alt,
            "win" | "windows" | "meta" | "super" | "cmd" => Key::Win,
            "enter" | "return" => Key::Enter,
            "tab" => Key::Tab,
            "esc" | "escape" => Key::Escape,
            "backspace" => Key::Backspace,
            "delete" | "del" => Key::Delete,
            "insert" | "ins" => Key::Insert,
            "space" => Key::Space,
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" | "pgup" => Key::PageUp,
            "pagedown" | "pgdn" => Key::PageDown,
            f if f.len() >= 2 && f.starts_with('f') && f[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => Key::F(f[1..].parse().unwrap_or(1)),
            c if c.chars().count() == 1 && c.chars().all(|c| c.is_ascii_alphanumeric() || ",./;'[]\\-=`".contains(c)) => {
                Key::Char(c.chars().next().unwrap_or('a'))
            }
            other => return Err(format!("Unknown key \"{other}\".")),
        };
        keys.push(k);
    }
    let main: Vec<&Key> = keys.iter().filter(|k| !k.is_modifier()).collect();
    if main.len() > 1 {
        return Err(format!("\"{spec}\" has more than one non-modifier key; press them separately."));
    }
    if keys.iter().rev().skip(1).any(|k| !k.is_modifier()) {
        return Err(format!("Put modifiers first in \"{spec}\" (e.g. ctrl+s)."));
    }
    Ok(keys)
}

/// The OS interface. Implementations must never report success for an action
/// they couldn't perform.
pub trait Driver: Send + Sync {
    fn displays(&self) -> Result<Vec<Display>, String>;
    /// Visible top-level windows of other applications, front to back.
    fn windows(&self) -> Result<Vec<WindowInfo>, String>;
    fn foreground(&self) -> Result<Option<WindowInfo>, String>;
    /// Bring a window to the front (restoring it if minimized).
    fn focus_window(&self, id: u64) -> Result<(), String>;
    fn capture(&self, display: &Display) -> Result<RgbaImage, String>;
    /// Interactive accessibility elements of a window, top-to-bottom.
    fn ui_elements(&self, window: u64, max: usize) -> Result<Vec<UiElement>, String>;
    fn element_at(&self, x: i32, y: i32) -> Result<Option<UiElement>, String>;
    fn focused_element(&self) -> Result<Option<UiElement>, String>;
    fn click(&self, x: i32, y: i32, button: MouseButton, count: u32) -> Result<(), String>;
    fn move_mouse(&self, x: i32, y: i32) -> Result<(), String>;
    fn scroll(&self, x: i32, y: i32, dx: i32, dy: i32) -> Result<(), String>;
    fn drag(&self, from: (i32, i32), to: (i32, i32)) -> Result<(), String>;
    /// Type text as characters (layout independent). `stop` is polled between chunks.
    fn type_text(&self, text: &str, stop: &dyn Fn() -> bool) -> Result<(), String>;
    fn press(&self, keys: &[Key]) -> Result<(), String>;
    /// Milliseconds since the last keyboard/mouse input from any source (incl. ours).
    fn idle_ms(&self) -> Option<u64>;
    /// Process id owning the top-level window at a screen point.
    fn owner_at(&self, _x: i32, _y: i32) -> Option<u32> {
        None
    }
    /// Value / editability of the focused control.
    fn focused_field(&self) -> Option<FieldInfo> {
        None
    }
    /// Recognise text in an image (OCR) — a fallback for apps whose
    /// accessibility data doesn't expose their text.
    fn ocr(&self, _image: &RgbaImage) -> Result<Vec<OcrLine>, String> {
        Err("Text recognition (OCR) isn't available on this system.".into())
    }
}

/// Roles you type into.
pub fn is_field_role(role: &str) -> bool {
    matches!(role, "edit" | "combo box" | "document")
}

/// Pick the controls worth showing from a large accessibility tree: the focused
/// control and text fields first (so a form opened at the bottom of a busy page,
/// like Gmail's compose box, is never cut off), then buttons and other
/// controls; shown top-to-bottom. `find` keeps only names containing it.
pub fn select_elements(all: Vec<UiElement>, find: &str, max: usize) -> (Vec<UiElement>, usize) {
    let needle = find.trim().to_lowercase();
    let mut pool: Vec<UiElement> = all.into_iter().filter(|e| needle.is_empty() || e.name.to_lowercase().contains(&needle)).collect();
    let total = pool.len();
    let rank = |e: &UiElement| -> u8 {
        if e.focused {
            0
        } else if is_field_role(&e.role) {
            1
        } else {
            match e.role.as_str() {
                "button" | "split button" | "menu item" | "tab" | "checkbox" | "radio button" => 2,
                "hyperlink" => 3,
                _ => 4,
            }
        }
    };
    pool.sort_by_key(|e| (rank(e), e.rect.y / 8, e.rect.x));
    pool.truncate(max);
    pool.sort_by_key(|e| (e.rect.y / 8, e.rect.x));
    (pool, total)
}

/// Whether the control found at a point now is (part of) the one observed
/// there: the same control, something inside it, or a container around it.
/// A different control covering the spot (a popup, a moved layout) isn't.
pub fn same_spot(observed: &UiElement, now: &UiElement) -> bool {
    let grow = |r: &Rect, d: i32| Rect { x: r.x - d, y: r.y - d, w: r.w + 2 * d, h: r.h + 2 * d };
    let inside = |a: &Rect, b: &Rect| a.x >= b.x && a.y >= b.y && a.x + a.w <= b.x + b.w && a.y + a.h <= b.y + b.h;
    (!observed.name.is_empty() && observed.name == now.name)
        // Something inside it (e.g. the text in a field) — but not a different control in its place.
        || (inside(&now.rect, &grow(&observed.rect, 6))
            && (now.name.trim().is_empty() || (now.rect.w as i64 * now.rect.h as i64) * 10 < (observed.rect.w as i64 * observed.rect.h as i64) * 9))
        // An unnamed container (layout group, page) — a named one covering it is a dialog or popup.
        || (inside(&observed.rect, &grow(&now.rect, 6)) && now.name.trim().is_empty())
}

/// Whether a window belongs to IGRIS itself (it must never operate its own UI,
/// e.g. its approval buttons).
pub fn is_own(pid: u32) -> bool {
    pid == std::process::id()
}

pub type SharedDriver = Arc<dyn Driver>;

/// The platform's driver.
pub fn native() -> SharedDriver {
    #[cfg(windows)]
    {
        Arc::new(windows::WindowsDriver::new())
    }
    #[cfg(not(windows))]
    {
        Arc::new(Unsupported)
    }
}

pub const UNSUPPORTED: &str = "Computer control (mouse, keyboard and window automation) is only available on Windows in this version of IGRIS.";

/// Platforms without a driver yet: every call fails honestly.
pub struct Unsupported;

impl Driver for Unsupported {
    fn displays(&self) -> Result<Vec<Display>, String> {
        Err(UNSUPPORTED.into())
    }
    fn windows(&self) -> Result<Vec<WindowInfo>, String> {
        Err(UNSUPPORTED.into())
    }
    fn foreground(&self) -> Result<Option<WindowInfo>, String> {
        Err(UNSUPPORTED.into())
    }
    fn focus_window(&self, _id: u64) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn capture(&self, _d: &Display) -> Result<RgbaImage, String> {
        Err(UNSUPPORTED.into())
    }
    fn ui_elements(&self, _w: u64, _m: usize) -> Result<Vec<UiElement>, String> {
        Err(UNSUPPORTED.into())
    }
    fn element_at(&self, _x: i32, _y: i32) -> Result<Option<UiElement>, String> {
        Err(UNSUPPORTED.into())
    }
    fn focused_element(&self) -> Result<Option<UiElement>, String> {
        Err(UNSUPPORTED.into())
    }
    fn click(&self, _x: i32, _y: i32, _b: MouseButton, _c: u32) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn move_mouse(&self, _x: i32, _y: i32) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn scroll(&self, _x: i32, _y: i32, _dx: i32, _dy: i32) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn drag(&self, _f: (i32, i32), _t: (i32, i32)) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn type_text(&self, _t: &str, _s: &dyn Fn() -> bool) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn press(&self, _k: &[Key]) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }
    fn idle_ms(&self) -> Option<u64> {
        None
    }
}

// ---------------------------------------------------------------------------
// Safety heuristics. These are guard rails, not guarantees: they catch the
// common ways an automated click or keystroke sends, publishes, pays or
// deletes, and route those through an explicit confirmation instead.

const CONSEQUENTIAL_WORDS: &[&str] = &[
    "send",
    "send now",
    "post",
    "publish",
    "tweet",
    "submit",
    "buy",
    "buy now",
    "purchase",
    "pay",
    "pay now",
    "checkout",
    "check out",
    "place order",
    "confirm",
    "delete",
    "delete forever",
    "permanently delete",
    "remove",
    "transfer",
    "donate",
    "subscribe",
    "unsubscribe",
    "upload",
];

/// Whether a button/link name looks like it commits something irreversible
/// (send, publish, buy, delete…). Long names (text blocks) don't count.
pub fn is_consequential_name(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    if n.is_empty() || n.chars().count() > 40 {
        return false;
    }
    let words: Vec<&str> = n.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let joined = words.join(" ");
    CONSEQUENTIAL_WORDS
        .iter()
        .any(|w| joined == *w || joined.starts_with(&format!("{w} ")) || (w.contains(' ') && joined.contains(w)) || words.first() == Some(w))
}

/// Roles whose activation can commit something (vs. text fields, lists…).
pub fn is_actionable_role(role: &str) -> bool {
    matches!(role, "button" | "split button" | "hyperlink" | "menu item" | "list item" | "image" | "custom" | "group" | "text")
}

/// Chat apps (by executable) and chat sites (by a whole word in the window title).
const MESSAGING_PROCESSES: &[&str] = &["whatsapp", "telegram", "discord", "slack", "ms-teams", "teams", "signal", "messenger", "skype", "wechat"];
const MESSAGING_TITLES: &[&str] = &["whatsapp", "telegram", "discord", "slack", "messenger", "instagram", "skype", "wechat"];

/// Apps where Enter sends a message.
pub fn is_messaging_app(w: &WindowInfo) -> bool {
    let p = w.process.to_lowercase();
    let p = p.trim_end_matches(".exe");
    let t = w.title.to_lowercase();
    let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    MESSAGING_PROCESSES.contains(&p) || MESSAGING_TITLES.iter().any(|m| words.contains(m))
}

const TERMINALS: &[&str] = &[
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "windowsterminal.exe",
    "wt.exe",
    "conhost.exe",
    "openconsole.exe",
    "bash.exe",
    "wsl.exe",
    "wslhost.exe",
    "mintty.exe",
    "alacritty.exe",
    "wezterm-gui.exe",
    "hyper.exe",
    "tabby.exe",
    "putty.exe",
];

/// Windows where typed text would run as a shell command. IGRIS runs commands
/// only through its validated `run_command` tool, never by typing into these.
pub fn is_command_window(w: &WindowInfo, focused: Option<&UiElement>) -> bool {
    let p = w.process.to_lowercase();
    let run_dialog = p == "explorer.exe" && w.title == "Run";
    // VS Code / IDE integrated terminals (xterm.js exposes "Terminal 1, …").
    let ide_terminal = focused.is_some_and(|e| e.name.to_lowercase().starts_with("terminal"));
    TERMINALS.contains(&p.as_str()) || run_dialog || ide_terminal
}

/// Key combinations that commonly send or submit (or open a command prompt).
pub fn combo_risk(keys: &[Key], fg: Option<&WindowInfo>, focused: Option<&UiElement>) -> Option<&'static str> {
    let has = |k: Key| keys.contains(&k);
    let main = keys.iter().copied().find(|k| !k.is_modifier());
    if has(Key::Win) && matches!(main, Some(Key::Char('r')) | Some(Key::Char('x'))) {
        return Some("blocked");
    }
    match main {
        Some(Key::Enter) if has(Key::Ctrl) || has(Key::Alt) => Some("sends or submits in many apps"),
        Some(Key::Char('s')) if has(Key::Alt) && !has(Key::Ctrl) => Some("sends email in Outlook"),
        Some(Key::Enter) if !has(Key::Shift) && fg.is_some_and(is_messaging_app) => Some("sends the message"),
        Some(Key::Enter | Key::Space) if focused.is_some_and(|e| is_actionable_role(&e.role) && is_consequential_name(&e.name)) => {
            Some("activates a send/submit/delete control")
        }
        _ => None,
    }
}

/// Map a point in a downscaled screenshot back to physical screen coordinates.
pub fn map_point(display: &Rect, image: (u32, u32), x: i32, y: i32) -> Option<(i32, i32)> {
    let (iw, ih) = (image.0 as i64, image.1 as i64);
    if iw == 0 || ih == 0 || x < 0 || y < 0 || x as i64 >= iw || y as i64 >= ih {
        return None;
    }
    let px = display.x as i64 + (x as i64 * display.w as i64) / iw;
    let py = display.y as i64 + (y as i64 * display.h as i64) / ih;
    Some((px as i32, py as i32))
}

#[cfg(test)]
pub mod fake {
    //! An in-memory desktop for tests.
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct FakeState {
        pub windows: Vec<WindowInfo>,
        pub foreground: Option<u64>,
        pub elements: Vec<UiElement>,
        pub focused: Option<UiElement>,
        pub log: Vec<String>,
        pub fail_input: bool,
        pub idle_ms: Option<u64>,
        /// Window to bring to front after the next click (simulates opening a dialog).
        pub on_click_focus: Option<u64>,
        /// Pid reported by `owner_at` (to simulate IGRIS's own window under a point).
        pub owner: Option<u32>,
        pub field: Option<FieldInfo>,
        /// Element reported at every point instead of the observed list (simulates a popup).
        pub cover: Option<UiElement>,
    }

    #[derive(Default)]
    pub struct FakeDriver(pub Mutex<FakeState>);

    impl FakeDriver {
        pub fn with(windows: Vec<WindowInfo>, elements: Vec<UiElement>) -> Self {
            let fg = windows.first().map(|w| w.id);
            Self(Mutex::new(FakeState { windows, foreground: fg, elements, idle_ms: Some(10_000), ..Default::default() }))
        }
        pub fn log(&self) -> Vec<String> {
            self.0.lock().unwrap().log.clone()
        }
        fn act(&self, entry: String) -> Result<(), String> {
            let mut s = self.0.lock().unwrap();
            if s.fail_input {
                return Err("SendInput was blocked".into());
            }
            s.log.push(entry);
            Ok(())
        }
    }

    pub fn window(id: u64, title: &str, process: &str) -> WindowInfo {
        WindowInfo { id, title: title.into(), process: process.into(), pid: id as u32, rect: Rect { x: 0, y: 0, w: 1000, h: 800 }, minimized: false }
    }

    pub fn element(name: &str, role: &str, x: i32, y: i32) -> UiElement {
        UiElement { name: name.into(), role: role.into(), rect: Rect { x, y, w: 80, h: 30 }, enabled: true, focused: false }
    }

    impl Driver for FakeDriver {
        fn displays(&self) -> Result<Vec<Display>, String> {
            Ok(vec![Display { id: 1, name: "Display 1".into(), rect: Rect { x: 0, y: 0, w: 2000, h: 1000 }, primary: true }])
        }
        fn windows(&self) -> Result<Vec<WindowInfo>, String> {
            Ok(self.0.lock().unwrap().windows.clone())
        }
        fn foreground(&self) -> Result<Option<WindowInfo>, String> {
            let s = self.0.lock().unwrap();
            Ok(s.foreground.and_then(|id| s.windows.iter().find(|w| w.id == id).cloned()))
        }
        fn focus_window(&self, id: u64) -> Result<(), String> {
            let mut s = self.0.lock().unwrap();
            if !s.windows.iter().any(|w| w.id == id) {
                return Err("That window no longer exists.".into());
            }
            s.foreground = Some(id);
            s.log.push(format!("focus {id}"));
            Ok(())
        }
        fn capture(&self, d: &Display) -> Result<RgbaImage, String> {
            Ok(RgbaImage::new(d.rect.w as u32, d.rect.h as u32))
        }
        fn ui_elements(&self, _w: u64, max: usize) -> Result<Vec<UiElement>, String> {
            Ok(self.0.lock().unwrap().elements.iter().take(max).cloned().collect())
        }
        fn element_at(&self, x: i32, y: i32) -> Result<Option<UiElement>, String> {
            let s = self.0.lock().unwrap();
            if let Some(c) = &s.cover {
                return Ok(Some(c.clone()));
            }
            Ok(s.elements.iter().find(|e| e.rect.contains(x, y)).cloned())
        }
        fn focused_element(&self) -> Result<Option<UiElement>, String> {
            Ok(self.0.lock().unwrap().focused.clone())
        }
        fn click(&self, x: i32, y: i32, b: MouseButton, count: u32) -> Result<(), String> {
            self.act(format!("click {x},{y} {b:?} x{count}"))?;
            let mut s = self.0.lock().unwrap();
            if let Some(id) = s.on_click_focus.take() {
                s.foreground = Some(id);
            }
            Ok(())
        }
        fn move_mouse(&self, x: i32, y: i32) -> Result<(), String> {
            self.act(format!("move {x},{y}"))
        }
        fn scroll(&self, x: i32, y: i32, dx: i32, dy: i32) -> Result<(), String> {
            self.act(format!("scroll {x},{y} {dx},{dy}"))
        }
        fn drag(&self, f: (i32, i32), t: (i32, i32)) -> Result<(), String> {
            self.act(format!("drag {f:?}->{t:?}"))
        }
        fn type_text(&self, text: &str, stop: &dyn Fn() -> bool) -> Result<(), String> {
            if stop() {
                return Err("Stopped by the user.".into());
            }
            self.act(format!("type {text}"))
        }
        fn press(&self, keys: &[Key]) -> Result<(), String> {
            self.act(format!("press {keys:?}"))
        }
        fn idle_ms(&self) -> Option<u64> {
            self.0.lock().unwrap().idle_ms
        }
        fn owner_at(&self, _x: i32, _y: i32) -> Option<u32> {
            self.0.lock().unwrap().owner
        }
        fn focused_field(&self) -> Option<FieldInfo> {
            self.0.lock().unwrap().field.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_key_combinations() {
        assert_eq!(parse_keys("ctrl+shift+s").unwrap(), vec![Key::Ctrl, Key::Shift, Key::Char('s')]);
        assert_eq!(parse_keys("Enter").unwrap(), vec![Key::Enter]);
        assert_eq!(parse_keys("alt+F4").unwrap(), vec![Key::Alt, Key::F(4)]);
        assert_eq!(parse_keys("ctrl").unwrap(), vec![Key::Ctrl]);
        assert!(parse_keys("ctrl+").is_err());
        assert!(parse_keys("a+b").is_err());
        assert!(parse_keys("s+ctrl").is_err());
        assert!(parse_keys("hyper").is_err());
        assert!(parse_keys("f25").is_err());
    }

    #[test]
    fn recognises_consequential_controls() {
        for n in ["Send", "send now", "Post", "Publish", "Buy now", "Place order", "Delete", "Pay ₹499", "Submit form", "Send (Ctrl+Enter)"] {
            assert!(is_consequential_name(n), "{n}");
        }
        for n in ["Sender", "Postpone", "New mail", "Subject", "Draft", "To", "Cancel", "", "Discard", "Ordering tips and tricks for your next project setup"] {
            assert!(!is_consequential_name(n), "{n}");
        }
    }

    #[test]
    fn spots_terminals_messaging_and_risky_keys() {
        let w = |t: &str, p: &str| WindowInfo { id: 1, title: t.into(), process: p.into(), pid: 1, rect: Rect::default(), minimized: false };
        assert!(is_command_window(&w("Windows PowerShell", "powershell.exe"), None));
        assert!(is_command_window(&w("Run", "explorer.exe"), None));
        let term = UiElement { name: "Terminal 1, pwsh".into(), role: "edit".into(), rect: Rect::default(), enabled: true, focused: true };
        assert!(is_command_window(&w("main.rs - Visual Studio Code", "Code.exe"), Some(&term)));
        assert!(!is_command_window(&w("main.rs - Visual Studio Code", "Code.exe"), None));

        let chat = w("WhatsApp - Google Chrome", "chrome.exe");
        assert!(is_messaging_app(&chat));
        assert!(is_messaging_app(&w("WhatsApp", "WhatsApp.exe")));
        assert!(!is_messaging_app(&w("Inbox - Outlook", "OUTLOOK.EXE")));
        assert!(!is_messaging_app(&w("Deadline online - Google Chrome", "chrome.exe")), "no substring matches");
        assert!(!is_messaging_app(&w("Inbox (3) - me@gmail.com - Gmail - Google Chrome", "chrome.exe")));
        assert!(is_messaging_app(&w("Slack | general", "chrome.exe")));

        let k = |s: &str| parse_keys(s).unwrap();
        assert_eq!(combo_risk(&k("win+r"), None, None), Some("blocked"));
        assert!(combo_risk(&k("ctrl+enter"), None, None).is_some());
        assert!(combo_risk(&k("enter"), Some(&chat), None).is_some());
        assert!(combo_risk(&k("shift+enter"), Some(&chat), None).is_none());
        assert!(combo_risk(&k("enter"), Some(&w("Untitled - Notepad", "notepad.exe")), None).is_none());
        let send = UiElement { name: "Send".into(), role: "button".into(), rect: Rect::default(), enabled: true, focused: true };
        assert!(combo_risk(&k("enter"), None, Some(&send)).is_some());
        assert!(combo_risk(&k("ctrl+s"), None, None).is_none());
    }

    #[test]
    fn keeps_fields_of_busy_pages() {
        let el = |name: &str, role: &str, y: i32| UiElement {
            name: name.into(),
            role: role.into(),
            rect: Rect { x: 10, y, w: 50, h: 20 },
            enabled: true,
            focused: false,
        };
        // 300 inbox rows above a compose box at the bottom of the page.
        let mut all: Vec<UiElement> = (0..300).map(|i| el(&format!("Email {i}"), "list item", 100 + i)).collect();
        all.push(el("To recipients", "combo box", 900));
        all.push(el("Subject", "edit", 940));
        all.push(el("Message Body", "edit", 980));
        all.push(el("Send", "button", 1100));
        let (shown, total) = select_elements(all.clone(), "", 50);
        assert_eq!((shown.len(), total), (50, 304));
        for n in ["To recipients", "Subject", "Message Body", "Send"] {
            assert!(shown.iter().any(|e| e.name == n), "{n}");
        }
        assert!(shown.windows(2).all(|w| w[0].rect.y <= w[1].rect.y), "shown top to bottom");
        let (found, total) = select_elements(all, "message", 50);
        assert_eq!((found.len(), total), (1, 1));
    }

    #[test]
    fn recognises_the_same_spot() {
        let r = |x, y, w, h| Rect { x, y, w, h };
        let el = |name: &str, rect| UiElement { name: name.into(), role: "edit".into(), rect, enabled: true, focused: false };
        let body = el("Message Body", r(100, 100, 400, 200));
        assert!(same_spot(&body, &el("Message Body", r(100, 100, 400, 200))));
        assert!(same_spot(&body, &el("Hello Rahul,", r(110, 110, 100, 20))), "text inside the field");
        assert!(same_spot(&body, &el("", r(90, 90, 420, 220))), "its container");
        assert!(!same_spot(&body, &el("Discard draft?", r(0, 0, 2000, 1200))), "a dialog covering it");
        assert!(!same_spot(&body, &el("Suggestion", r(450, 250, 300, 40))), "a popup over part of it");
        assert!(!same_spot(&body, &el("Delete", r(100, 100, 400, 200))), "a different control in its place");
    }

    #[test]
    fn maps_screenshot_points_to_the_display() {
        let d = Rect { x: 1920, y: 0, w: 2560, h: 1440 };
        assert_eq!(map_point(&d, (1280, 720), 640, 360), Some((1920 + 1280, 720)));
        assert_eq!(map_point(&d, (1280, 720), 0, 0), Some((1920, 0)));
        assert_eq!(map_point(&d, (1280, 720), 1280, 10), None);
        assert_eq!(map_point(&d, (1280, 720), -1, 10), None);
    }

    #[test]
    fn unsupported_platforms_say_so() {
        assert_eq!(Unsupported.windows().unwrap_err(), UNSUPPORTED);
        assert!(Unsupported.click(1, 1, MouseButton::Left, 1).is_err());
    }
}
