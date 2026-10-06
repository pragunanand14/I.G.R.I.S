//! Desktop computer control: the core's driver abstraction plus this
//! platform's drivers (Windows input, windows, UI Automation, OCR) and the
//! Chromium DevTools browser control.

pub use igris_core::computer::*;

pub mod browser;
pub mod cdp;
#[cfg(windows)]
pub mod windows;

/// The platform's driver.
pub fn native() -> SharedDriver {
    #[cfg(windows)]
    {
        std::sync::Arc::new(windows::WindowsDriver::new())
    }
    #[cfg(not(windows))]
    {
        std::sync::Arc::new(Unsupported)
    }
}
