//! Cross-device IGRIS in the app: how this platform protects the device keys,
//! how a task from another device runs here (the normal chat path), and
//! starting the device hub. See `igris_core::device` and docs/CROSS_DEVICE.md.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::core::chat;
use crate::device::hub::{DeviceHub, HubEvent, TaskRunner};
use crate::device::registry::Device;
use crate::device::{Capability, Identity, KeyProtector, Platform};
use crate::error::{AppError, AppResult};
use crate::state::{AppState, UiApprover, APPROVAL_TIMEOUT};
use crate::tools::devices::HubSlot;
use crate::tools::executor::Approver;

/// The key protector for this platform: DPAPI on Windows, the Android
/// Keystore on Android; elsewhere none is available (documented as weaker).
pub fn protector(app: &AppHandle) -> Box<dyn KeyProtector> {
    #[cfg(windows)]
    {
        let _ = app;
        Box::new(dpapi::Dpapi)
    }
    #[cfg(target_os = "android")]
    {
        Box::new(keystore::Keystore(app.clone()))
    }
    #[cfg(not(any(windows, target_os = "android")))]
    {
        let _ = app;
        Box::new(crate::device::Unprotected)
    }
}

#[cfg(windows)]
mod dpapi {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};

    /// Windows Data Protection: sealed to this Windows user account.
    pub struct Dpapi;

    fn run(input: &[u8], protect: bool) -> Result<Vec<u8>, String> {
        let blob = CRYPT_INTEGER_BLOB { cbData: input.len() as u32, pbData: input.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `blob` points at `input`, valid for the call; `out` is
        // allocated by the system and freed with LocalFree after copying.
        unsafe {
            let r = if protect {
                CryptProtectData(&blob, windows::core::w!("IGRIS device keys"), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            } else {
                CryptUnprotectData(&blob, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            };
            r.map_err(|e| e.message())?;
            let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            if !protect {
                std::ptr::write_bytes(out.pbData, 0, out.cbData as usize);
            }
            let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
            Ok(v)
        }
    }

    impl crate::device::KeyProtector for Dpapi {
        fn name(&self) -> &'static str {
            "dpapi"
        }
        fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, String> {
            run(plain, true)
        }
        fn unprotect(&self, sealed: &[u8]) -> Result<Vec<u8>, String> {
            run(sealed, false)
        }
    }
}

#[cfg(target_os = "android")]
mod keystore {
    use base64::Engine;
    use tauri_plugin_igris_device::IgrisDeviceExt;
    use zeroize::Zeroize;

    const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

    /// A non-exportable AES key in the Android Keystore seals the device keys.
    pub struct Keystore(pub tauri::AppHandle);

    impl crate::device::KeyProtector for Keystore {
        fn name(&self) -> &'static str {
            "android-keystore"
        }
        fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, String> {
            let mut b = B64.encode(plain);
            let sealed = self.0.igris_device().seal_keys(&b).map_err(|e| e.to_string());
            b.zeroize();
            B64.decode(sealed?).map_err(|e| e.to_string())
        }
        fn unprotect(&self, sealed: &[u8]) -> Result<Vec<u8>, String> {
            let mut b = self.0.igris_device().unseal_keys(&B64.encode(sealed)).map_err(|e| e.to_string())?;
            let plain = B64.decode(&b).map_err(|e| e.to_string());
            b.zeroize();
            plain
        }
    }
}

fn default_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Android => "My phone",
        Platform::Windows => "My PC",
        _ => "My computer",
    }
}

/// Runs a task another device sent through the same path as a chat message
/// typed here: a conversation, `chat::generate`, the orchestrator, this
/// device's tools and permission policy.
pub struct AppRunner(pub AppHandle);

#[async_trait::async_trait]
impl TaskRunner for AppRunner {
    fn open(&self, from: &Device, objective: &str) -> AppResult<String> {
        let state = self.0.state::<AppState>();
        // Refuse up front (with the reason) when this device can't think.
        state.ai_router()?;
        let s = crate::settings::load(&*state.db.conn()?)?;
        let what = if from.platform.is_phone() { "phone" } else { "computer" };
        let content = format!("{objective}\n\n(Sent from my {what} \"{}\" — do this here, on this device.)", from.name);
        let (conversation, _) =
            chat::save_user_message(&state.db, None, &content, &s.user_name, &crate::commands::chat::offered_tools(&state)?, s.memory_enabled)?;
        let _ = self.0.emit("conversations-changed", ());
        Ok(conversation.id)
    }

    async fn run(&self, conversation_id: &str, approver: Arc<dyn Approver>, cancel: CancellationToken) -> AppResult<String> {
        let state = self.0.state::<AppState>();
        let mut params = crate::commands::chat::generation_params(&state)?;
        params.tooling.approver = approver;
        let request_id = format!("remote-{}", uuid::Uuid::new_v4().simple());
        let guard = state.generations.begin(&request_id, Some(conversation_id))?;
        // Stopping it here (Stop on the conversation) stops it for the requester too.
        let (local, remote) = (guard.token.clone(), cancel.clone());
        let link = tauri::async_runtime::spawn(async move {
            local.cancelled().await;
            remote.cancel();
        });
        let message = chat::generate(&state.db, conversation_id, &params, &cancel, &mut |_| {}).await;
        link.abort();
        state.operator.end_turn(conversation_id);
        drop(guard);
        let _ = self.0.emit("conversations-changed", ());
        let m = message?;
        match m.error {
            Some(e) if m.content.trim().is_empty() => Err(AppError::internal(e)),
            _ => Ok(m.content),
        }
    }
}

/// Unlock (or create) this device's identity and start the hub. Runs off the
/// main thread; the hub goes into `slot` for the tools and commands.
pub fn start(app: AppHandle, slot: HubSlot, tool_names: Vec<String>) {
    tauri::async_runtime::spawn(async move {
        let a = app.clone();
        let loaded = tauri::async_runtime::spawn_blocking(move || {
            let state = a.state::<AppState>();
            let protector = protector(&a);
            let platform = Platform::current();
            let conn = state.db.conn()?;
            Identity::load_or_create(&conn, protector.as_ref(), default_name(platform), platform).map(|id| (id, protector.name()))
        })
        .await;
        let (identity, protector) = match loaded {
            Ok(Ok(x)) => x,
            Ok(Err(e)) => {
                tracing::error!(event = "DEVICE_IDENTITY_UNAVAILABLE", error = %e);
                return;
            }
            Err(e) => {
                tracing::error!(event = "DEVICE_IDENTITY_UNAVAILABLE", error = %e);
                return;
            }
        };
        tracing::info!(event = "DEVICE_IDENTITY_READY", protector);
        let state = app.state::<AppState>();
        let hub = DeviceHub::new(state.db.clone(), identity, protector, Some(state.orchestrator.clone()));
        hub.set_capabilities(Capability::from_tools(tool_names.iter().map(String::as_str)));
        hub.set_runner(Arc::new(AppRunner(app.clone())));
        hub.set_local_approver(Arc::new(UiApprover { pending: state.approvals.clone(), timeout: APPROVAL_TIMEOUT }));
        let emitter = app.clone();
        hub.on_event(Arc::new(move |e: &HubEvent| {
            if let HubEvent::Notice { title, body } = e {
                notify(&emitter, title, body);
            }
            let _ = emitter.emit("device-event", e);
        }));
        hub.start();
        let _ = slot.set(hub);
        let _ = app.emit("device-event", serde_json::json!({ "type": "ready" }));
    });
}

/// A meaningful cross-device event as a system notification (desktop and
/// Android, while IGRIS runs). There is no push service: a closed app isn't notified.
fn notify(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(e) = app.notification().builder().title(format!("IGRIS · {title}")).body(crate::orchestrator::task::clip(body, 200)).show() {
        tracing::warn!(event = "NOTIFICATION_FAILED", error = %e);
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn dpapi_seals_and_opens_for_this_user_only() {
        use crate::device::KeyProtector;
        let p = super::dpapi::Dpapi;
        let secret = [7u8; 64];
        let sealed = p.protect(&secret).unwrap();
        assert!(!sealed.windows(64).any(|w| w == secret), "sealed bytes don't contain the key");
        assert_eq!(p.unprotect(&sealed).unwrap(), secret.to_vec());
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(p.unprotect(&tampered).is_err());
    }

    #[test]
    fn default_names_say_what_kind_of_device() {
        use crate::device::Platform;
        assert_eq!(super::default_name(Platform::Android), "My phone");
        assert_eq!(super::default_name(Platform::Windows), "My PC");
    }
}
