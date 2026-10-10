use std::marker::PhantomData;

use tauri::{AppHandle, Runtime};

use crate::{Error, PhoneApp, PhoneStatus, Result, ScreenRead};

/// Not a phone: every call says so.
pub struct Device<R: Runtime>(PhantomData<fn() -> R>);

impl<R: Runtime> Device<R> {
    pub(crate) fn new(_app: AppHandle<R>) -> Self {
        Self(PhantomData)
    }

    pub fn list_apps(&self) -> Result<Vec<PhoneApp>> {
        Err(Error::Unsupported)
    }

    pub fn launch_app(&self, _package: &str) -> Result<()> {
        Err(Error::Unsupported)
    }

    pub fn status(&self) -> Result<PhoneStatus> {
        Err(Error::Unsupported)
    }

    pub fn seal_keys(&self, _data_b64: &str) -> Result<String> {
        Err(Error::Unsupported)
    }

    pub fn unseal_keys(&self, _sealed_b64: &str) -> Result<String> {
        Err(Error::Unsupported)
    }

    pub fn pick_file(&self) -> Result<Option<String>> {
        Err(Error::Unsupported)
    }

    pub fn media_control(&self, _action: &str) -> Result<()> {
        Err(Error::Unsupported)
    }

    pub fn media_playing(&self) -> Result<bool> {
        Err(Error::Unsupported)
    }

    pub fn play_from_search(&self, _query: &str, _package: Option<&str>) -> Result<String> {
        Err(Error::Unsupported)
    }

    pub fn take_launch_action(&self) -> Result<Option<String>> {
        Err(Error::Unsupported)
    }

    pub fn set_weather_place(&self, _name: &str, _latitude: f64, _longitude: f64) -> Result<()> {
        Err(Error::Unsupported)
    }

    pub fn weather_place(&self) -> Result<Option<String>> {
        Err(Error::Unsupported)
    }

    pub fn screen_access(&self) -> Result<bool> {
        Err(Error::Unsupported)
    }

    pub fn open_accessibility_settings(&self) -> Result<()> {
        Err(Error::Unsupported)
    }

    pub fn screen_read(&self) -> Result<ScreenRead> {
        Err(Error::Unsupported)
    }

    pub fn screen_tap(&self, _index: Option<usize>, _x: f32, _y: f32, _long: bool) -> Result<()> {
        Err(Error::Unsupported)
    }

    pub fn screen_type(&self, _text: &str, _index: Option<usize>, _enter: bool) -> Result<()> {
        Err(Error::Unsupported)
    }

    pub fn screen_swipe(&self, _direction: &str) -> Result<()> {
        Err(Error::Unsupported)
    }

    pub fn screen_button(&self, _action: &str) -> Result<()> {
        Err(Error::Unsupported)
    }

    pub fn screen_overlay(&self, _text: Option<&str>) -> Result<()> {
        Err(Error::Unsupported)
    }

    pub fn screen_take_stop(&self) -> Result<bool> {
        Err(Error::Unsupported)
    }
}
