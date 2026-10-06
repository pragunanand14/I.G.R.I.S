//! Real-desktop tests for the Windows driver: real screen, real windows, real
//! input, real UI Automation, real applications. No mocks.
//!
//! They need an interactive Windows desktop and take over mouse and keyboard,
//! so they're `#[ignore]`d and run explicitly (one at a time):
//!
//! ```text
//! cargo test --test windows_desktop -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! CI runs them on a GitHub-hosted Windows runner (`.github/workflows/windows-desktop.yml`).
//! Lines starting with `FINDING:` record observed behaviour for the test report.
#![cfg(windows)]

use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant};

use igris_lib::computer::{native, parse_keys, Driver, MouseButton, SharedDriver, WindowInfo};

fn finding(msg: impl AsRef<str>) {
    eprintln!("FINDING: {}", msg.as_ref());
}

/// Wait for a window whose process or title matches.
fn wait_window(d: &SharedDriver, pred: impl Fn(&WindowInfo) -> bool, timeout: Duration) -> Option<WindowInfo> {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if let Ok(ws) = d.windows() {
            if let Some(w) = ws.into_iter().find(|w| pred(w)) {
                return Some(w);
            }
        }
        sleep(Duration::from_millis(200));
    }
    None
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn close_notepads() {
    let _ = Command::new("taskkill").args(["/IM", "notepad.exe", "/F"]).output();
    sleep(Duration::from_millis(500));
}

#[test]
#[ignore = "needs an interactive Windows desktop"]
fn screen_displays_and_capture() {
    let d = native();
    let displays = d.displays().expect("displays");
    finding(format!("displays: {:?}", displays.iter().map(|d| (d.name.clone(), d.rect, d.primary)).collect::<Vec<_>>()));
    assert!(!displays.is_empty());
    let primary = displays.iter().find(|d| d.primary).unwrap_or(&displays[0]);
    let t = Instant::now();
    let img = d.capture(primary).expect("capture");
    finding(format!("capture {}x{} in {} ms", img.width(), img.height(), t.elapsed().as_millis()));
    assert!(img.width() > 100 && img.height() > 100);
    // A real desktop isn't a single flat colour.
    let first = img.get_pixel(0, 0).0;
    let varied = img.pixels().step_by(997).any(|p| p.0 != first);
    finding(format!("capture has varied pixels: {varied}"));
    assert!(varied, "the capture is a flat image — no desktop is being rendered");
}

#[test]
#[ignore = "needs an interactive Windows desktop"]
fn notepad_focus_type_unicode_and_read_back() {
    close_notepads();
    let d = native();
    let _app = Kill(Command::new("notepad.exe").spawn().expect("start notepad"));
    let w = wait_window(&d, |w| w.process.eq_ignore_ascii_case("notepad.exe"), Duration::from_secs(15)).expect("a Notepad window appears");
    finding(format!("notepad window: \"{}\" ({}) rect {:?}", w.title, w.process, w.rect));
    d.focus_window(w.id).expect("focus");
    let fg = d.foreground().unwrap().expect("foreground");
    assert_eq!(fg.id, w.id, "Notepad is in front");

    let elements = d.ui_elements(w.id, 4000).expect("ui elements");
    finding(format!(
        "notepad UIA elements: {} (roles: {:?})",
        elements.len(),
        elements.iter().map(|e| e.role.clone()).collect::<std::collections::BTreeSet<_>>()
    ));
    let editor = elements.iter().find(|e| matches!(e.role.as_str(), "document" | "edit")).expect("an editing control is exposed");
    let (x, y) = editor.rect.center();
    d.click(x, y, MouseButton::Left, 1).expect("click into the editor");
    sleep(Duration::from_millis(300));
    let focused = d.focused_element().unwrap();
    finding(format!("focused after click: {focused:?}"));

    let text = "IGRIS operator test — ünïcødé ✓ 日本";
    d.type_text(text, &|| false).expect("type");
    sleep(Duration::from_millis(500));
    let field = d.focused_field();
    finding(format!("focused field read-back: {:?}", field.as_ref().map(|f| (f.value.clone(), f.read_only, f.password))));
    let value = field.and_then(|f| f.value).unwrap_or_default();
    assert!(value.contains(text), "typed text is read back through UI Automation (got {value:?})");

    // Shortcuts: select all + delete empties the document.
    d.press(&parse_keys("ctrl+a").unwrap()).unwrap();
    d.press(&parse_keys("delete").unwrap()).unwrap();
    sleep(Duration::from_millis(300));
    let after = d.focused_field().and_then(|f| f.value).unwrap_or_default();
    finding(format!("after ctrl+a, delete: {after:?}"));
    assert!(after.trim().is_empty());
    close_notepads();
}

#[test]
#[ignore = "needs an interactive Windows desktop"]
fn our_input_is_seen_as_input_and_focus_switches_between_apps() {
    close_notepads();
    let d = native();
    let _a = Kill(Command::new("notepad.exe").spawn().unwrap());
    let np = wait_window(&d, |w| w.process.eq_ignore_ascii_case("notepad.exe"), Duration::from_secs(15)).expect("notepad");
    let _b = Kill(Command::new("cmd.exe").args(["/c", "start", "IGRIS-TEST-CONSOLE", "cmd.exe", "/k", "title IGRIS-TEST-CONSOLE"]).spawn().unwrap());
    let console = wait_window(&d, |w| w.title.contains("IGRIS-TEST-CONSOLE"), Duration::from_secs(15));
    finding(format!("console window: {:?}", console.as_ref().map(|w| (&w.title, &w.process))));
    d.focus_window(np.id).unwrap();
    assert_eq!(d.foreground().unwrap().map(|w| w.id), Some(np.id));
    if let Some(c) = &console {
        d.focus_window(c.id).expect("switch to the console");
        assert_eq!(d.foreground().unwrap().map(|w| w.id), Some(c.id));
        d.focus_window(np.id).expect("and back");
        assert_eq!(d.foreground().unwrap().map(|w| w.id), Some(np.id));
    }
    sleep(Duration::from_millis(600));
    let before = d.idle_ms().expect("idle time is readable");
    d.move_mouse(200, 200).unwrap();
    let after = d.idle_ms().unwrap();
    finding(format!("idle before {before} ms, after our mouse move {after} ms"));
    assert!(after < before, "input resets the idle timer (takeover detection depends on it)");
    // Scroll and right-click don't error on a real desktop.
    let (x, y) = np.rect.center();
    d.scroll(x, y, 0, 3).expect("scroll");
    d.click(x, y, MouseButton::Right, 1).expect("right click");
    sleep(Duration::from_millis(300));
    d.press(&parse_keys("escape").unwrap()).unwrap();
    if let Some(c) = console {
        let _ =
            Command::new("taskkill").args(["/FI", &format!("WINDOWTITLE eq {}*", c.title.split(" - ").next().unwrap_or("IGRIS-TEST-CONSOLE")), "/F"]).output();
    }
    close_notepads();
}
