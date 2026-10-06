//! What the desktop looks like right now, cheaply, and whether an action had
//! the effect it was meant to have.
//!
//! [`ScreenState`] is read from window and accessibility data only — no
//! screenshot — so it can be taken before and after every action. An action
//! may state an [`Expect`]ation ("a window titled Save As appears", "the field
//! contains …"); [`evaluate`] checks it against the state after the action,
//! polling briefly because UIs take a moment to update. Without an expectation,
//! the before/after comparison still tells whether anything visibly changed.

use std::time::{Duration, Instant};

use serde_json::Value;

use super::{Driver, UiElement, WindowInfo};

/// A cheap snapshot of the desktop (no screenshot).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScreenState {
    pub foreground: Option<WindowInfo>,
    pub windows: Vec<WindowInfo>,
    pub focused: Option<UiElement>,
    /// Text of the focused field (never read from password fields).
    pub field: Option<String>,
}

pub fn snapshot(d: &dyn Driver) -> ScreenState {
    let focused = d.focused_element().ok().flatten();
    let field = focused.as_ref().filter(|f| super::is_field_role(&f.role)).and_then(|_| d.focused_field()).filter(|f| !f.password).and_then(|f| f.value);
    ScreenState { foreground: d.foreground().ok().flatten(), windows: d.windows().unwrap_or_default(), focused, field }
}

fn label(w: &WindowInfo) -> String {
    format!("\"{}\"", w.title.chars().take(80).collect::<String>())
}

fn el_label(e: &UiElement) -> String {
    if e.name.trim().is_empty() {
        e.role.clone()
    } else {
        format!("{} \"{}\"", e.role, e.name.chars().take(60).collect::<String>())
    }
}

impl ScreenState {
    /// What visibly differs from `before` (empty = nothing detectable changed).
    pub fn changes_since(&self, before: &ScreenState) -> Vec<String> {
        let mut out = Vec::new();
        match (&before.foreground, &self.foreground) {
            (Some(a), Some(b)) if a.id != b.id => out.push(format!("active window is now {}", label(b))),
            (Some(a), Some(b)) if a.title != b.title => out.push(format!("window title changed to {}", label(b))),
            (None, Some(b)) => out.push(format!("active window is now {}", label(b))),
            (Some(_), None) => out.push("no window is active now".into()),
            _ => {}
        }
        for w in &self.windows {
            if !before.windows.iter().any(|o| o.id == w.id) {
                out.push(format!("window opened: {}", label(w)));
            }
        }
        for w in &before.windows {
            if !self.windows.iter().any(|n| n.id == w.id) {
                out.push(format!("window closed: {}", label(w)));
            }
        }
        if before.focused.as_ref().map(|f| (&f.name, &f.role, f.rect)) != self.focused.as_ref().map(|f| (&f.name, &f.role, f.rect)) {
            if let Some(f) = &self.focused {
                out.push(format!("focus moved to {}", el_label(f)));
            }
        }
        if before.field != self.field && self.field.is_some() {
            out.push("the focused field's text changed".into());
        }
        out
    }

    /// A few lines for the model's task context.
    pub fn summary(&self) -> String {
        let mut s = match &self.foreground {
            Some(w) => format!("Active window: {} ({})", label(w), w.process),
            None => "Active window: none".into(),
        };
        if let Some(f) = &self.focused {
            s.push_str(&format!("; focus: {}", el_label(f)));
        }
        let others: Vec<String> =
            self.windows.iter().filter(|w| Some(w.id) != self.foreground.as_ref().map(|f| f.id)).take(6).map(|w| w.title.chars().take(40).collect()).collect();
        if !others.is_empty() {
            s.push_str(&format!("; other windows: {}", others.join(" | ")));
        }
        s
    }
}

/// What an action is expected to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// Nothing stated: only report whether anything visibly changed.
    Nothing,
    ScreenChanged,
    WindowPresent(String),
    WindowGone(String),
    ElementPresent(String),
    ElementGone(String),
    FieldContains(String),
    FocusOn(String),
}

/// JSON schema of the `expect` parameter of computer actions.
pub fn expect_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "description": "What should be true after the action, checked by IGRIS. kind none = just report what changed.",
        "properties": {
            "kind": { "type": "string", "enum": ["none", "screen_changed", "window_present", "window_gone", "element_present", "element_gone", "field_contains", "focus_on"] },
            "value": { "type": "string", "maxLength": 200, "description": "Window title/app, control name, or text (\"\" for none/screen_changed)." }
        },
        "required": ["kind", "value"],
        "additionalProperties": false
    })
}

pub fn parse_expect(v: &Value) -> Result<Expect, String> {
    let value = v["value"].as_str().unwrap_or_default().trim().to_string();
    let need = |e: fn(String) -> Expect| if value.is_empty() { Err(format!("expect.kind {} needs a value.", v["kind"])) } else { Ok(e(value.clone())) };
    match v["kind"].as_str().unwrap_or("none") {
        "none" => Ok(Expect::Nothing),
        "screen_changed" => Ok(Expect::ScreenChanged),
        "window_present" => need(Expect::WindowPresent),
        "window_gone" => need(Expect::WindowGone),
        "element_present" => need(Expect::ElementPresent),
        "element_gone" => need(Expect::ElementGone),
        "field_contains" => need(Expect::FieldContains),
        "focus_on" => need(Expect::FocusOn),
        k => Err(format!("Unknown expect.kind \"{k}\".")),
    }
}

impl Expect {
    pub fn describe(&self) -> String {
        match self {
            Expect::Nothing => "no stated expectation".into(),
            Expect::ScreenChanged => "the screen changes".into(),
            Expect::WindowPresent(v) => format!("a window \"{v}\" is open"),
            Expect::WindowGone(v) => format!("no window \"{v}\" is open"),
            Expect::ElementPresent(v) => format!("a control \"{v}\" is shown"),
            Expect::ElementGone(v) => format!("no control \"{v}\" is shown"),
            Expect::FieldContains(v) => format!("the focused field contains \"{v}\""),
            Expect::FocusOn(v) => format!("focus is on \"{v}\""),
        }
    }
}

/// The result of checking an expectation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Met,
    NotMet,
    /// Couldn't be determined (e.g. the app doesn't expose the needed data).
    Unknown,
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn window_matches(w: &WindowInfo, needle: &str) -> bool {
    let n = norm(needle);
    norm(&w.title).contains(&n) || norm(w.process.trim_end_matches(".exe")).contains(n.trim_end_matches(".exe"))
}

/// Check `e` against the state after the action. `elements` reads the active
/// window's controls (only called for element expectations).
pub fn check(e: &Expect, before: &ScreenState, after: &ScreenState, elements: &dyn Fn() -> Option<Vec<UiElement>>) -> Outcome {
    let bool_outcome = |b: bool| if b { Outcome::Met } else { Outcome::NotMet };
    match e {
        Expect::Nothing => Outcome::Unknown,
        Expect::ScreenChanged => bool_outcome(!after.changes_since(before).is_empty()),
        Expect::WindowPresent(v) => bool_outcome(after.windows.iter().chain(after.foreground.iter()).any(|w| window_matches(w, v))),
        Expect::WindowGone(v) => bool_outcome(!after.windows.iter().chain(after.foreground.iter()).any(|w| window_matches(w, v))),
        Expect::ElementPresent(v) => match elements() {
            Some(els) => bool_outcome(els.iter().any(|el| norm(&el.name).contains(&norm(v)))),
            None => Outcome::Unknown,
        },
        Expect::ElementGone(v) => match elements() {
            Some(els) => bool_outcome(!els.iter().any(|el| norm(&el.name).contains(&norm(v)))),
            None => Outcome::Unknown,
        },
        Expect::FieldContains(v) => match &after.field {
            Some(text) => bool_outcome(norm(text).contains(&norm(v))),
            None => Outcome::Unknown,
        },
        Expect::FocusOn(v) => match &after.focused {
            Some(f) => bool_outcome(norm(&f.name).contains(&norm(v))),
            None => Outcome::Unknown,
        },
    }
}

/// Take states until the expectation is met or `wait` passes (UIs update
/// asynchronously). Returns the last state, the outcome and a note for the model.
pub fn evaluate(d: &dyn Driver, e: &Expect, before: &ScreenState, wait: Duration) -> (ScreenState, Outcome, String) {
    let deadline = Instant::now() + wait;
    loop {
        let after = snapshot(d);
        let elements = || after.foreground.as_ref().and_then(|w| d.ui_elements(w.id, 4000).ok());
        let outcome = check(e, before, &after, &elements);
        if outcome == Outcome::Met || Instant::now() >= deadline {
            let changes = after.changes_since(before);
            let what_changed = if changes.is_empty() { "nothing visible changed".to_string() } else { changes.join("; ") };
            let note = match (e, outcome) {
                (Expect::Nothing, _) if changes.is_empty() => {
                    "Nothing visibly changed (active window, windows, focus and field text are the same) — the action may not have worked; check before continuing.".to_string()
                }
                (Expect::Nothing, _) => format!("What changed: {what_changed}."),
                (_, Outcome::Met) => format!("Expected {} — verified. ({what_changed})", e.describe()),
                (_, Outcome::NotMet) => format!("Expected {} — NOT met. Now: {what_changed}. Observe and adjust; don't repeat blindly.", e.describe()),
                (_, Outcome::Unknown) => format!("Expected {} — couldn't be checked (the app doesn't expose it). ({what_changed})", e.describe()),
            };
            return (after, outcome, note);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer::fake::{element, window, FakeDriver};
    use serde_json::json;

    #[test]
    fn changes_are_described_and_none_means_none() {
        let a = ScreenState {
            foreground: Some(window(1, "Notepad", "notepad.exe")),
            windows: vec![window(1, "Notepad", "notepad.exe")],
            focused: None,
            field: None,
        };
        assert!(a.changes_since(&a).is_empty());
        let mut b = a.clone();
        b.windows.push(window(2, "Save As", "notepad.exe"));
        b.foreground = Some(window(2, "Save As", "notepad.exe"));
        let c = b.changes_since(&a);
        assert!(c.iter().any(|s| s.contains("active window is now \"Save As\"")) && c.iter().any(|s| s.contains("window opened")), "{c:?}");
        assert!(b.summary().contains("Active window: \"Save As\"") && b.summary().contains("other windows: Notepad"));
    }

    #[test]
    fn expectations_parse_and_check() {
        assert_eq!(parse_expect(&json!({"kind": "window_present", "value": "Save As"})), Ok(Expect::WindowPresent("Save As".into())));
        assert_eq!(parse_expect(&json!({"kind": "none", "value": ""})), Ok(Expect::Nothing));
        assert!(parse_expect(&json!({"kind": "field_contains", "value": " "})).is_err(), "a value is required");
        assert!(crate::tools::schema::is_strict_compatible(
            &json!({"type":"object","properties":{"expect": expect_schema()},"required":["expect"],"additionalProperties":false})
        ));

        let before =
            ScreenState { foreground: Some(window(1, "Notepad", "notepad.exe")), windows: vec![window(1, "Notepad", "notepad.exe")], ..Default::default() };
        let mut after = before.clone();
        after.field = Some("IGRIS  operator test".into());
        let none = || None;
        assert_eq!(check(&Expect::FieldContains("igris operator TEST".into()), &before, &after, &none), Outcome::Met);
        assert_eq!(check(&Expect::FieldContains("other".into()), &before, &after, &none), Outcome::NotMet);
        assert_eq!(check(&Expect::WindowPresent("notepad".into()), &before, &after, &none), Outcome::Met, "by app name");
        assert_eq!(check(&Expect::WindowGone("Notepad".into()), &before, &after, &none), Outcome::NotMet);
        assert_eq!(check(&Expect::ElementPresent("Run".into()), &before, &after, &none), Outcome::Unknown, "no element data → unknown, not met");
        let els = || Some(vec![element("Run and Debug", "button", 0, 0)]);
        assert_eq!(check(&Expect::ElementPresent("run".into()), &before, &after, &els), Outcome::Met);
        assert_eq!(check(&Expect::ScreenChanged, &before, &before, &none), Outcome::NotMet);
    }

    #[test]
    fn evaluation_waits_for_slow_ui_and_reports_honestly() {
        let d = FakeDriver::with(vec![window(1, "Notepad", "notepad.exe")], vec![]);
        let before = snapshot(&d);
        // Nothing happens: reported as such, not as success.
        let (_, o, note) = evaluate(&d, &Expect::Nothing, &before, Duration::from_millis(0));
        assert_eq!(o, Outcome::Unknown);
        assert!(note.contains("Nothing visibly changed"));
        let (_, o, note) = evaluate(&d, &Expect::WindowPresent("Save As".into()), &before, Duration::from_millis(300));
        assert_eq!(o, Outcome::NotMet);
        assert!(note.contains("NOT met"));
        // The dialog shows up a moment later: the wait catches it.
        let d = std::sync::Arc::new(d);
        let d2 = d.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            let mut s = d2.0.lock().unwrap();
            s.windows.push(window(2, "Save As", "notepad.exe"));
            s.foreground = Some(2);
        });
        let (_, o, note) = evaluate(d.as_ref(), &Expect::WindowPresent("Save As".into()), &before, Duration::from_millis(1500));
        assert_eq!(o, Outcome::Met, "{note}");
    }
}
