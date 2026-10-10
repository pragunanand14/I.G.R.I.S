use serde::Serialize;
use tauri::State;

use crate::ai::{AiRuntime, AiStatus, ResponseDepth};
use crate::config::{AppConfig, EnvImport, PublicConfig};
use crate::error::{AppError, AppResult};
use crate::settings;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatusView {
    #[serde(flatten)]
    pub status: AiStatus,
    /// Model a new message would use (settings override > AI_MODEL > default).
    pub effective_model: Option<String>,
    pub effort: ResponseDepth,
}

fn view(state: &AppState) -> AppResult<AiStatusView> {
    let status = state.ai.read().map_err(|_| AppError::internal("AI runtime lock poisoned"))?.status.clone();
    let s = settings::load(&*state.db.conn()?)?;
    let effective_model = Some(s.ai_model).filter(|m| !m.is_empty()).or_else(|| status.configured_model.clone());
    Ok(AiStatusView { status, effective_model, effort: s.ai_effort })
}

#[tauri::command]
pub fn get_ai_status(state: State<'_, AppState>) -> AppResult<AiStatusView> {
    view(&state)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReloadResult {
    pub config: PublicConfig,
    pub ai: AiStatusView,
}

/// Re-read `.env` / environment and rebuild the AI provider without restarting.
#[tauri::command]
pub fn reload_config(state: State<'_, AppState>) -> AppResult<ReloadResult> {
    reload(&state)
}

fn reload(state: &AppState) -> AppResult<ReloadResult> {
    let config = AppConfig::load(Some(&state.paths.config_dir));
    let runtime = AiRuntime::from_config(&config);
    tracing::info!(event = "CONFIG_RELOADED", env_files = config.env_files.len(), ai_ready = runtime.status.ready);
    let public = config.public();
    *state.config.write().map_err(|_| AppError::internal("config lock poisoned"))? = config;
    *state.ai.write().map_err(|_| AppError::internal("AI runtime lock poisoned"))? = runtime;
    Ok(ReloadResult { config: public, ai: view(state)? })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    /// Names of the settings found (never their values).
    pub settings: EnvImport,
    #[serde(flatten)]
    pub reload: ReloadResult,
}

/// Let the user pick their settings (`.env`) file, check it, make it IGRIS's
/// settings file and reload. The file goes from the picker straight to the
/// backend; the UI only learns which setting names it had. `None`: cancelled.
#[tauri::command]
pub async fn import_settings_file(app: tauri::AppHandle, state: State<'_, AppState>) -> AppResult<Option<ImportResult>> {
    let picked = tauri::async_runtime::spawn_blocking(move || pick(&app)).await.map_err(|e| AppError::internal(e.to_string()))??;
    let Some(bytes) = picked else { return Ok(None) };
    let settings = crate::config::import_env(&state.paths.config_dir, &bytes).map_err(AppError::validation)?;
    Ok(Some(ImportResult { settings, reload: reload(&state)? }))
}

/// The picked file's bytes, read on the native side.
#[cfg(mobile)]
fn pick(app: &tauri::AppHandle) -> AppResult<Option<Vec<u8>>> {
    use base64::Engine;
    use tauri_plugin_igris_device::IgrisDeviceExt;
    let Some(b64) = app.igris_device().pick_file().map_err(|e| AppError::validation(e.to_string()))? else { return Ok(None) };
    base64::engine::general_purpose::STANDARD.decode(b64).map(Some).map_err(|_| AppError::internal("the picked file came back garbled"))
}

#[cfg(desktop)]
fn pick(app: &tauri::AppHandle) -> AppResult<Option<Vec<u8>>> {
    use std::io::Read;
    use tauri_plugin_dialog::DialogExt;
    let Some(file) = app.dialog().file().set_title("Pick your IGRIS settings (.env) file").blocking_pick_file() else { return Ok(None) };
    let path = file.into_path().map_err(|e| AppError::validation(e.to_string()))?;
    let mut bytes = Vec::new();
    // Read a little more than import_env accepts, so a huge file is refused, not loaded.
    std::fs::File::open(path)?.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}
