//! Real-desktop tests: the Windows driver and the operator tools on a real
//! screen, with real windows, real input, real UI Automation and real
//! applications. No mocks.
//!
//! They need an interactive Windows desktop and take over mouse and keyboard,
//! so they're `#[ignore]`d and run explicitly, one at a time:
//!
//! ```text
//! cargo test --test windows_desktop -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! CI runs them on a GitHub-hosted Windows runner (`.github/workflows/windows-desktop.yml`).
//! Lines starting with `FINDING:` record observed behaviour for the test report.
#![cfg(windows)]

use std::process::{Child, Command};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

use igris_lib::computer::state::Outcome;
use igris_lib::computer::{native, parse_keys, MouseButton, SharedDriver, WindowInfo};
use igris_lib::db::Database;
use igris_lib::operator::{Operator, TaskState};
use igris_lib::orchestrator::task::Verification;
use igris_lib::orchestrator::verify::window_check;
use igris_lib::tools::computer::{ComputerClickTool, ComputerKeyTool, ComputerObserveTool, ComputerTypeTool, OperatorFinishTool};
use igris_lib::tools::Tool;
use serde_json::json;

fn finding(msg: impl AsRef<str>) {
    eprintln!("FINDING: {}", msg.as_ref());
}

/// Wait for a window that matches.
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

fn kill(image: &str) {
    let _ = Command::new("taskkill").args(["/IM", image, "/F"]).output();
    sleep(Duration::from_millis(500));
}

/// A fresh Notepad in front.
fn notepad(d: &SharedDriver) -> (Kill, WindowInfo) {
    kill("notepad.exe");
    let child = Kill(Command::new("notepad.exe").spawn().expect("start notepad"));
    let w = wait_window(d, |w| w.process.eq_ignore_ascii_case("notepad.exe"), Duration::from_secs(15)).expect("a Notepad window appears");
    d.focus_window(w.id).expect("focus Notepad");
    (child, w)
}

/// The operator with a running task, on the real desktop.
fn operator() -> Arc<Operator> {
    let db = Arc::new(Database::open_in_memory().unwrap());
    let op = Arc::new(Operator::new(db, native()));
    op.start(None, None, "Real desktop test", vec![]).expect("operator task");
    op
}

fn observe_input() -> serde_json::Value {
    json!({"screenshot": false, "list_windows": false, "find": ""})
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
    let first = img.get_pixel(0, 0).0;
    assert!(img.pixels().step_by(997).any(|p| p.0 != first), "the capture is a flat image — no desktop is being rendered");
}

#[test]
#[ignore = "needs an interactive Windows desktop"]
fn notepad_focus_type_unicode_and_read_back() {
    let d = native();
    let (_app, w) = notepad(&d);
    finding(format!("notepad window: \"{}\" ({}) rect {:?}", w.title, w.process, w.rect));
    assert_eq!(d.foreground().unwrap().map(|f| f.id), Some(w.id), "Notepad is in front");
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
    let text = "IGRIS operator test — ünïcødé ✓ 日本";
    d.type_text(text, &|| false).expect("type");
    sleep(Duration::from_millis(500));
    let value = d.focused_field().and_then(|f| f.value).unwrap_or_default();
    finding(format!("focused field read-back: {value:?}"));
    assert!(value.contains(text), "typed text is read back through UI Automation (got {value:?})");
    d.press(&parse_keys("ctrl+a").unwrap()).unwrap();
    d.press(&parse_keys("delete").unwrap()).unwrap();
    sleep(Duration::from_millis(300));
    assert!(d.focused_field().and_then(|f| f.value).unwrap_or_default().trim().is_empty(), "ctrl+a, delete empties it");
    kill("notepad.exe");
}

#[test]
#[ignore = "needs an interactive Windows desktop"]
fn input_events_reset_the_idle_timer_and_focus_switches_between_apps() {
    let d = native();
    let (_np, np) = notepad(&d);
    let _c = Kill(Command::new("cmd.exe").args(["/c", "start", "IGRIS-TEST-CONSOLE", "cmd.exe", "/k", "title IGRIS-TEST-CONSOLE"]).spawn().unwrap());
    let console = wait_window(&d, |w| w.title.contains("IGRIS-TEST-CONSOLE"), Duration::from_secs(15)).expect("console window");
    d.focus_window(console.id).expect("switch to the console");
    assert_eq!(d.foreground().unwrap().map(|w| w.id), Some(console.id));
    d.focus_window(np.id).expect("and back");
    assert_eq!(d.foreground().unwrap().map(|w| w.id), Some(np.id));

    sleep(Duration::from_millis(700));
    let before = d.idle_ms().expect("idle time is readable");
    d.move_mouse(300, 300).unwrap();
    let after_move = d.idle_ms().unwrap();
    d.press(&parse_keys("shift").unwrap()).unwrap();
    let after_key = d.idle_ms().unwrap();
    finding(format!("idle before {before} ms; after SetCursorPos move {after_move} ms; after a SendInput key {after_key} ms"));
    assert!(after_key < before, "a real input event resets the idle timer (takeover detection depends on it)");
    let (x, y) = np.rect.center();
    d.scroll(x, y, 0, 3).expect("scroll");
    d.click(x, y, MouseButton::Right, 1).expect("right click");
    sleep(Duration::from_millis(300));
    d.press(&parse_keys("escape").unwrap()).unwrap();
    let _ = Command::new("taskkill").args(["/FI", "WINDOWTITLE eq IGRIS-TEST-CONSOLE*", "/F"]).output();
    kill("notepad.exe");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs an interactive Windows desktop"]
async fn operator_tools_observe_act_and_verify_on_a_real_desktop() {
    let d = native();
    let (_np, _w) = notepad(&d);
    let op = operator();
    let observe = ComputerObserveTool::new(op.clone());
    let typer = ComputerTypeTool::new(op.clone());
    let keys = ComputerKeyTool::new(op.clone());

    let t = Instant::now();
    let seen = observe.execute(&json!({"screenshot": true, "list_windows": true, "find": ""})).await.expect("observe");
    finding(format!("computer_observe with screenshot: {} ms, {} chars, {} image(s)", t.elapsed().as_millis(), seen.content.len(), seen.media.len()));
    let obs = op.observation().expect("observation stored");
    let editor = obs.elements.iter().position(|e| matches!(e.role.as_str(), "edit" | "document")).expect("the editor is listed");

    // Type into the editor by element (semantic target), read back.
    let out = typer.execute(&json!({"element": editor, "x": -1, "y": -1, "text": "IGRIS operator test"})).await.expect("type");
    finding(format!("computer_type: {}", out.content));
    assert_eq!(op.take_verdict("computer_type").map(|v| v.outcome), Some(Outcome::Met), "typed text verified by read-back");

    // A shortcut with a stated expectation: Ctrl+S opens the Save As dialog…
    observe.execute(&observe_input()).await.unwrap();
    let out = keys.execute(&json!({"keys": "ctrl+s", "repeat": 1, "expect": {"kind": "window_present", "value": "Save As"}})).await.expect("ctrl+s");
    finding(format!("ctrl+s → {}", out.content));
    let v = op.take_verdict("computer_key").unwrap();
    assert_eq!(v.outcome, Outcome::Met, "Save As dialog detected: {}", v.note);
    // …and Esc closes it again.
    observe.execute(&observe_input()).await.unwrap();
    let out = keys.execute(&json!({"keys": "esc", "repeat": 1, "expect": {"kind": "window_gone", "value": "Save As"}})).await.expect("esc");
    finding(format!("esc → {}", out.content));
    assert_eq!(op.take_verdict("computer_key").unwrap().outcome, Outcome::Met);

    // An expectation that doesn't come true is reported as not met.
    observe.execute(&observe_input()).await.unwrap();
    keys.execute(&json!({"keys": "end", "repeat": 1, "expect": {"kind": "window_present", "value": "No Such Window"}})).await.expect("end");
    assert_eq!(op.take_verdict("computer_key").unwrap().outcome, Outcome::NotMet);

    // Completion needs a look after the last action.
    let finish = OperatorFinishTool::new(op.clone());
    assert!(finish.execute(&json!({"outcome": "completed", "summary": "typed"})).await.is_err());
    observe.execute(&observe_input()).await.unwrap();
    finish.execute(&json!({"outcome": "completed", "summary": "typed"})).await.expect("finish");
    // Discard the unsaved text so Notepad closes cleanly.
    kill("notepad.exe");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs an interactive Windows desktop"]
async fn user_takeover_is_detected_and_switching_away_pauses() {
    let d = native();
    let (_np, np) = notepad(&d);
    let op = operator();
    ComputerObserveTool::new(op.clone()).execute(&observe_input()).await.unwrap();
    op.set_target_window(np.id);
    op.mark_input();
    tokio::time::sleep(Duration::from_millis(800)).await;

    // "The user" presses a key (real input, not IGRIS's).
    let user = native();
    user.press(&parse_keys("shift").unwrap()).unwrap();
    op.checkpoint().await.expect("same window: keep going");
    assert!(op.is_stale(), "IGRIS must look again before its next action");
    finding("user key press while operating: detected; task kept running but marked stale");

    // "The user" switches to another app: the task pauses instead of fighting for control.
    let _c = Kill(Command::new("cmd.exe").args(["/c", "start", "IGRIS-TAKEOVER", "cmd.exe", "/k", "title IGRIS-TAKEOVER"]).spawn().unwrap());
    let console = wait_window(&user, |w| w.title.contains("IGRIS-TAKEOVER"), Duration::from_secs(15)).expect("console");
    tokio::time::sleep(Duration::from_millis(800)).await;
    user.focus_window(console.id).unwrap();
    user.press(&parse_keys("shift").unwrap()).unwrap();
    let op2 = op.clone();
    let waiting = tokio::spawn(async move { op2.checkpoint().await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    let t = op.snapshot().task.unwrap();
    finding(format!("after switching apps: state {:?}, reason {:?}", t.state, t.pause_reason));
    assert_eq!(t.state, TaskState::Paused);
    assert!(!waiting.is_finished(), "no action proceeds while paused");
    op.resume();
    assert!(waiting.await.unwrap().is_ok());
    assert_eq!(d.foreground().unwrap().map(|w| w.id), Some(np.id), "resume refocuses the window IGRIS was working in");
    op.stop("test done");
    let _ = Command::new("taskkill").args(["/FI", "WINDOWTITLE eq IGRIS-TAKEOVER*", "/F"]).output();
    kill("notepad.exe");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs an interactive Windows desktop"]
async fn stop_interrupts_typing_and_no_further_action_runs() {
    let d = native();
    let (_np, _w) = notepad(&d);
    let op = operator();
    let observe = ComputerObserveTool::new(op.clone());
    observe.execute(&observe_input()).await.unwrap();
    let editor = op.observation().unwrap().elements.iter().position(|e| matches!(e.role.as_str(), "edit" | "document")).unwrap();
    let long = "stop me ".repeat(500);
    let typer = ComputerTypeTool::new(op.clone());
    let op2 = op.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        op2.stop("Stopped with Esc.");
    });
    let r = typer.execute(&json!({"element": editor, "x": -1, "y": -1, "text": long})).await;
    let typed = native().focused_field().and_then(|f| f.value).unwrap_or_default();
    finding(format!(
        "stop during a {}-char type: result {:?}; {} chars reached the field",
        long.len(),
        r.as_ref().err().map(|e| e.message.clone()),
        typed.len()
    ));
    assert!(r.is_err(), "typing reports the stop");
    assert!(typed.len() < long.len(), "typing stopped part-way");
    let again = typer.execute(&json!({"element": -1, "x": -1, "y": -1, "text": "after stop"})).await;
    assert!(again.is_err(), "no action runs after a stop");
    kill("notepad.exe");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs an interactive Windows desktop"]
async fn launches_are_verified_by_their_window() {
    let d = native();
    kill("notepad.exe");
    let _np = Kill(Command::new("notepad.exe").spawn().unwrap());
    let v = window_check(Some(d.clone()), "Notepad", true, Duration::from_secs(10)).await;
    finding(format!("Notepad launch check: {:?} — {}", v.verification, v.note));
    assert_eq!(v.verification, Verification::Passed);
    kill("notepad.exe");

    let _ex = Kill(Command::new("explorer.exe").arg(r"C:\Windows").spawn().unwrap());
    let v = window_check(Some(d.clone()), "File Explorer", false, Duration::from_secs(15)).await;
    finding(format!("File Explorer check: {:?} — {}", v.verification, v.note));
    assert_eq!(v.verification, Verification::Passed);
    if let Some(w) = d.windows().unwrap().into_iter().find(|w| w.process.eq_ignore_ascii_case("explorer.exe")) {
        let els = d.ui_elements(w.id, 4000).unwrap_or_default();
        finding(format!("File Explorer UIA elements: {}", els.len()));
        d.focus_window(w.id).ok();
        d.press(&parse_keys("alt+f4").unwrap()).ok();
    }

    // Something that never opens a window isn't verified.
    let v = window_check(Some(d.clone()), "IGRIS No Such App", true, Duration::from_secs(2)).await;
    assert_eq!(v.verification, Verification::Unverified);

    // VS Code, when the machine has it.
    let code = Command::new("where").arg("code").output().ok().filter(|o| o.status.success());
    match code {
        Some(o) => {
            let path = String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or_default().trim().to_string();
            let dir = tempfile::tempdir().unwrap();
            let _vs = Command::new("cmd").args(["/c", &path, "--new-window", "--disable-extensions", &dir.path().display().to_string()]).spawn();
            let v = window_check(Some(d.clone()), "VS Code", false, Duration::from_secs(40)).await;
            finding(format!("VS Code launch check: {:?} — {}", v.verification, v.note));
            kill("Code.exe");
        }
        None => finding("VS Code is not installed on this machine; skipped"),
    }
}
