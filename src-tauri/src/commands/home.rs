//! Smart home: find, pair and control devices on the local network (the TV).
//!
//! Direct button presses from the remote screen are the user's own actions;
//! the AI uses the same devices only through the `tv_control` tool (policy,
//! audit). Pairing secrets never leave the native side.

use serde_json::Value;
use tauri::State;

use crate::error::AppResult;
use crate::home::{self, discover, HomeDevice, SmartHome, TvAction, TvState};
use crate::state::AppState;

#[tauri::command]
pub fn home_devices(state: State<'_, AppState>) -> AppResult<Vec<HomeDevice>> {
    home::list(&*state.db.conn()?)
}

/// Look for TVs on the network for a few seconds.
#[tauri::command]
pub async fn home_discover(state: State<'_, AppState>) -> AppResult<Vec<discover::Found>> {
    let mut found = discover::find(std::time::Duration::from_secs(3), None).await;
    let known: Vec<String> = home::list(&*state.db.conn()?)?.into_iter().map(|d| d.host).collect();
    found.retain(|f| !known.contains(&f.host));
    Ok(found)
}

/// Pair a TV; it shows a prompt and waits (up to a minute) for Allow on the TV.
#[tauri::command]
pub async fn home_pair(state: State<'_, AppState>, host: String, name: Option<String>) -> AppResult<HomeDevice> {
    let host = home::validate_host(&host)?;
    SmartHome::new(state.db.clone()).pair_lg(&host, name.as_deref()).await
}

#[tauri::command]
pub fn home_remove(state: State<'_, AppState>, id: String) -> AppResult<()> {
    home::remove(&*state.db.conn()?, &id)
}

#[tauri::command]
pub fn home_rename(state: State<'_, AppState>, id: String, name: String) -> AppResult<()> {
    home::rename(&*state.db.conn()?, &id, &name)
}

/// One remote-control action; the answer says what the TV did.
#[tauri::command]
pub async fn home_tv(state: State<'_, AppState>, id: String, action: String, value: Option<Value>) -> AppResult<String> {
    let action = TvAction::parse(&action, value.as_ref())?;
    SmartHome::new(state.db.clone()).tv(&id, &action).await
}

#[tauri::command]
pub async fn home_tv_state(state: State<'_, AppState>, id: String) -> AppResult<TvState> {
    SmartHome::new(state.db.clone()).tv_state(&id).await
}

/// Touchpad: move the TV's cursor, click, scroll (a batch of inputs).
#[tauri::command]
pub async fn home_pointer(state: State<'_, AppState>, id: String, events: Vec<crate::home::lg::PointerEvent>) -> AppResult<()> {
    if events.len() > 200 {
        return Err(crate::error::AppError::validation("Too many touchpad inputs at once."));
    }
    SmartHome::new(state.db.clone()).pointer(&id, &events).await
}

/// Touchpad closed: release the TV's input channel.
#[tauri::command]
pub async fn home_pointer_stop(state: State<'_, AppState>, id: String) -> AppResult<()> {
    SmartHome::new(state.db.clone()).pointer_stop(&id).await;
    Ok(())
}
