//! Devices: this device, the user's paired devices, pairing, the relay
//! connection, and tasks/approvals between devices.
//!
//! Commands that start network work are `async` so they run on the async
//! runtime. Who may do what is decided here and in the hub: nothing in these
//! commands lets another device change settings on this one.

use serde::Serialize;
use tauri::State;

use crate::device::hub::{ApprovalView, DeviceHub, DeviceSettings, DeviceView, LinkStatus, PairingCode, TaskView, ThisDevice};
use crate::device::protocol::ControlAction;
use crate::device::registry::{self, AuditRow, Device};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

fn hub(state: &AppState) -> AppResult<std::sync::Arc<DeviceHub>> {
    state.devices.get().cloned().ok_or_else(|| AppError::internal("Cross-device features are still starting up, or couldn't start (see the log)."))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicesOverview {
    pub this_device: ThisDevice,
    pub settings: DeviceSettings,
    pub status: LinkStatus,
    pub devices: Vec<DeviceView>,
    pub tasks: Vec<TaskView>,
    pub approvals: Vec<ApprovalView>,
}

#[tauri::command]
pub fn devices_overview(state: State<'_, AppState>) -> AppResult<DevicesOverview> {
    let h = hub(&state)?;
    Ok(DevicesOverview {
        this_device: h.this_device(),
        settings: h.settings()?,
        status: h.status(),
        devices: h.devices()?,
        tasks: h.tasks(30)?,
        approvals: h.pending_approvals(),
    })
}

/// Set the relay address and turn cross-device on or off (local use is unaffected).
#[tauri::command]
pub async fn devices_configure(state: State<'_, AppState>, relay_url: Option<String>, enabled: bool, sync_memory: bool) -> AppResult<DeviceSettings> {
    hub(&state)?.configure(relay_url.as_deref(), enabled, sync_memory)
}

#[tauri::command]
pub async fn devices_start_pairing(state: State<'_, AppState>) -> AppResult<PairingCode> {
    hub(&state)?.start_pairing().await
}

#[tauri::command]
pub async fn devices_cancel_pairing(state: State<'_, AppState>) -> AppResult<()> {
    hub(&state)?.cancel_pairing();
    Ok(())
}

/// The user's answer to "Allow this device to join?".
#[tauri::command]
pub async fn devices_confirm_pairing(state: State<'_, AppState>, pairing_id: String, allow: bool) -> AppResult<Option<Device>> {
    let h = hub(&state)?;
    match h.confirm_pairing(&pairing_id, allow) {
        Ok(d) => Ok(Some(d)),
        Err(_) if !allow => Ok(None),
        Err(e) => Err(e),
    }
}

/// Join another device's IGRIS with the code it shows.
#[tauri::command]
pub async fn devices_join(state: State<'_, AppState>, code: String) -> AppResult<Device> {
    let h = hub(&state)?;
    h.join(&code).await
}

#[tauri::command]
pub async fn devices_revoke(state: State<'_, AppState>, device_id: String) -> AppResult<()> {
    hub(&state)?.revoke(&device_id)
}

#[tauri::command]
pub fn devices_forget(state: State<'_, AppState>, device_id: String) -> AppResult<()> {
    hub(&state)?.forget(&device_id)
}

#[tauri::command]
pub fn devices_rename(state: State<'_, AppState>, device_id: String, name: String) -> AppResult<Device> {
    hub(&state)?.rename_device(&device_id, &name)
}

#[tauri::command]
pub async fn devices_rename_this(state: State<'_, AppState>, name: String) -> AppResult<ThisDevice> {
    hub(&state)?.rename_this_device(&name)
}

/// Send a task from the Devices screen (the assistant uses `send_to_device`).
#[tauri::command]
pub async fn devices_send_task(state: State<'_, AppState>, device_id: String, objective: String) -> AppResult<TaskView> {
    hub(&state)?.send_task(&device_id, &objective)
}

#[tauri::command]
pub async fn devices_control_task(state: State<'_, AppState>, request_id: String, action: ControlAction) -> AppResult<TaskView> {
    hub(&state)?.control_task(&request_id, action)
}

/// Answer another device's approval request (signed with this device's key).
#[tauri::command]
pub async fn devices_answer_approval(state: State<'_, AppState>, call_id: String, approve: bool) -> AppResult<()> {
    hub(&state)?.answer_approval(&call_id, approve)
}

#[tauri::command]
pub fn devices_audit(state: State<'_, AppState>, limit: Option<u32>) -> AppResult<Vec<AuditRow>> {
    registry::audit_log(&*state.db.conn()?, limit.unwrap_or(100))
}
