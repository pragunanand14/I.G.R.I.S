//! Screen capture on the desktop, for the core's screenshot tool.

use std::sync::Arc;

use image::RgbaImage;

pub use igris_core::tools::screen::*;

use crate::attachments::AttachmentStore;
use crate::db::Database;

/// The screenshot tool with this platform's screen capture.
pub fn screenshot_tool(db: Arc<Database>, store: Arc<AttachmentStore>) -> ScreenshotTool {
    ScreenshotTool::with_capturer(db, store, Arc::new(capture))
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn capture() -> Result<RgbaImage, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("Couldn't list displays: {e}"))?;
    let primary = monitors.iter().find(|m| m.is_primary().unwrap_or(false)).or(monitors.first()).ok_or("No display found.")?;
    let img = primary.capture_image().map_err(|e| format!("Couldn't capture the screen: {e}"))?;
    RgbaImage::from_raw(img.width(), img.height(), img.into_raw()).ok_or_else(|| "The captured image was malformed.".to_string())
}

#[cfg(target_os = "linux")]
fn capture() -> Result<RgbaImage, String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};

    let wayland = std::env::var("XDG_SESSION_TYPE").map(|t| t.eq_ignore_ascii_case("wayland")).unwrap_or(false)
        || (std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_none());
    if wayland {
        return Err("Screenshots aren't supported on Wayland sessions yet (X11 only on Linux).".into());
    }
    let (conn, screen_num) = x11rb::connect(None).map_err(|e| format!("Couldn't connect to the display: {e}"))?;
    let screen = &conn.setup().roots[screen_num];
    let (w, h) = (screen.width_in_pixels, screen.height_in_pixels);
    let reply = conn
        .get_image(ImageFormat::Z_PIXMAP, screen.root, 0, 0, w, h, !0)
        .map_err(|e| format!("Couldn't capture the screen: {e}"))?
        .reply()
        .map_err(|e| format!("Couldn't capture the screen: {e}"))?;
    let (w, h) = (w as u32, h as u32);
    if reply.data.len() != (w * h * 4) as usize {
        return Err(format!("Unsupported display format (depth {}).", reply.depth));
    }
    // 24/32-bit ZPixmap is BGRX in memory.
    let mut rgba = reply.data;
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    RgbaImage::from_raw(w, h, rgba).ok_or_else(|| "The captured image was malformed.".to_string())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn capture() -> Result<RgbaImage, String> {
    Err("Screenshots aren't supported on this platform.".into())
}
