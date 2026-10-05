//! The operator overlay: two always-on-top windows that make computer control
//! impossible to miss — a click-through animated border around the display
//! IGRIS works on, and a small draggable orb with the status and Pause/Stop.
//! Both are excluded from screen captures (the model sees the user's apps,
//! not the overlay). While a task runs, a global stop hotkey (Esc by default)
//! ends it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::computer::Rect;
use crate::operator::{Operator, Snapshot};

pub const BORDER: &str = "operator-border";
pub const ORB: &str = "operator-orb";
/// Orb panel size in logical pixels.
const ORB_SIZE: (f64, f64) = (300.0, 92.0);
const ORB_MARGIN: i32 = 28;
/// How long the success / stopped state stays visible before the overlay hides.
const LINGER: Duration = Duration::from_millis(1800);

#[derive(Default)]
struct OverlayState {
    hotkey: Option<String>,
    registered: bool,
    /// Bumped on every change, so a delayed hide only applies to its own snapshot.
    generation: u64,
}

pub struct Overlay {
    app: AppHandle,
    op: Arc<Operator>,
    state: Mutex<OverlayState>,
}

impl Overlay {
    pub fn install(app: &AppHandle, op: Arc<Operator>, stop_hotkey: &str) -> Arc<Overlay> {
        let overlay = Arc::new(Overlay {
            app: app.clone(),
            op: op.clone(),
            state: Mutex::new(OverlayState { hotkey: Some(stop_hotkey.to_string()), ..Default::default() }),
        });
        let o = overlay.clone();
        op.on_change(Arc::new(move |snap: &Snapshot| o.changed(snap)));
        let o = overlay.clone();
        op.on_point(Arc::new(move |x, y| o.keep_orb_away_from(x, y)));
        let o = overlay.clone();
        op.on_hotkey(Arc::new(move |on| if on { o.register_hotkey() } else { o.unregister_hotkey() }));
        overlay
    }

    pub fn set_stop_hotkey(&self, hotkey: &str) {
        let was = self.state.lock().map(|s| s.registered).unwrap_or(false);
        self.unregister_hotkey();
        if let Ok(mut s) = self.state.lock() {
            s.hotkey = Some(hotkey.to_string());
        }
        if was {
            self.register_hotkey();
        }
    }

    fn changed(self: &Arc<Self>, snap: &Snapshot) {
        let _ = self.app.emit("operator-state", snap);
        let generation = match self.state.lock() {
            Ok(mut s) => {
                s.generation += 1;
                s.generation
            }
            Err(_) => return,
        };
        let me = self.clone();
        let snap = snap.clone();
        // Window work happens off the event-loop thread (building windows from it can deadlock on Windows).
        tauri::async_runtime::spawn(async move {
            if snap.active {
                me.register_hotkey();
                me.show(&snap);
            } else {
                me.unregister_hotkey();
                tokio::time::sleep(LINGER).await;
                if me.state.lock().map(|s| s.generation == generation).unwrap_or(false) {
                    for label in [BORDER, ORB] {
                        if let Some(w) = me.app.get_webview_window(label) {
                            let _ = w.hide();
                        }
                    }
                }
            }
        });
    }

    fn window(&self, label: &str, route: &str, click_through: bool) -> Option<WebviewWindow> {
        if let Some(w) = self.app.get_webview_window(label) {
            return Some(w);
        }
        let built = WebviewWindowBuilder::new(&self.app, label, WebviewUrl::App(format!("index.html#{route}").into()))
            .title("IGRIS Operator")
            .transparent(true)
            .decorations(false)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(false)
            .focusable(false)
            .content_protected(true)
            .visible(false)
            .inner_size(ORB_SIZE.0, ORB_SIZE.1)
            .build();
        match built {
            Ok(w) => {
                if click_through {
                    let _ = w.set_ignore_cursor_events(true);
                }
                Some(w)
            }
            Err(e) => {
                tracing::warn!(event = "OVERLAY_WINDOW_FAILED", label, error = %e);
                None
            }
        }
    }

    fn display_rect(&self, snap: &Snapshot) -> Option<Rect> {
        snap.task.as_ref().and_then(|t| t.display.as_ref()).map(|d| d.rect).or_else(|| {
            let m = self.app.primary_monitor().ok().flatten()?;
            Some(Rect { x: m.position().x, y: m.position().y, w: m.size().width as i32, h: m.size().height as i32 })
        })
    }

    fn show(&self, snap: &Snapshot) {
        let Some(area) = self.display_rect(snap) else { return };
        if let Some(border) = self.window(BORDER, "/operator/border", true) {
            let _ = border.set_position(PhysicalPosition::new(area.x, area.y));
            let _ = border.set_size(PhysicalSize::new(area.w as u32, area.h as u32));
            let _ = border.show();
        }
        if let Some(orb) = self.window(ORB, "/operator/orb", false) {
            let visible = orb.is_visible().unwrap_or(false);
            let inside = orb.outer_position().ok().is_some_and(|p| area.contains(p.x, p.y));
            if !visible || !inside {
                let scale = orb.scale_factor().unwrap_or(1.0);
                let (w, h) = ((ORB_SIZE.0 * scale) as i32, (ORB_SIZE.1 * scale) as i32);
                let _ = orb.set_position(PhysicalPosition::new(area.x + area.w - w - ORB_MARGIN, area.y + area.h - h - ORB_MARGIN - 48));
            }
            let _ = orb.show();
        }
    }

    /// IGRIS is about to click at (x, y): move the orb if it's in the way.
    fn keep_orb_away_from(&self, x: i32, y: i32) {
        let Some(orb) = self.app.get_webview_window(ORB) else { return };
        if !orb.is_visible().unwrap_or(false) {
            return;
        }
        let (Ok(p), Ok(s)) = (orb.outer_position(), orb.outer_size()) else { return };
        let r = Rect { x: p.x - 16, y: p.y - 16, w: s.width as i32 + 32, h: s.height as i32 + 32 };
        if !r.contains(x, y) {
            return;
        }
        let area = self.display_rect(&self.op.snapshot()).unwrap_or(r);
        // Opposite vertical half of the display.
        let to_top = y > area.y + area.h / 2;
        let ny = if to_top { area.y + ORB_MARGIN + 48 } else { area.y + area.h - s.height as i32 - ORB_MARGIN - 48 };
        let _ = orb.set_position(PhysicalPosition::new(p.x, ny));
        std::thread::sleep(Duration::from_millis(60));
    }

    fn register_hotkey(&self) {
        use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
        let Ok(mut s) = self.state.lock() else { return };
        if s.registered {
            return;
        }
        let Some(key) = s.hotkey.clone() else { return };
        let op = self.op.clone();
        let r = self.app.global_shortcut().on_shortcut(key.as_str(), move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed && !op.is_injecting() {
                op.stop("Stopped with the stop hotkey.");
            }
        });
        match r {
            Ok(()) => s.registered = true,
            Err(e) => tracing::warn!(event = "OPERATOR_HOTKEY_UNAVAILABLE", hotkey = %key, error = %e),
        }
    }

    fn unregister_hotkey(&self) {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        let Ok(mut s) = self.state.lock() else { return };
        if !s.registered {
            return;
        }
        if let Some(key) = s.hotkey.clone() {
            let _ = self.app.global_shortcut().unregister(key.as_str());
        }
        s.registered = false;
    }
}
