use std::marker::PhantomData;

use tauri::{AppHandle, Runtime};

use crate::{Error, PhoneApp, PhoneStatus, Result};

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
}
