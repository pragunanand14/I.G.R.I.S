use serde::Serialize;
use tauri::State;

use crate::ai::{AiRuntime, AiStatus, ResponseDepth};
use crate::config::{AppConfig, PublicConfig};
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
    let config = AppConfig::load(Some(&state.paths.config_dir));
    let runtime = AiRuntime::from_config(&config);
    tracing::info!(event = "CONFIG_RELOADED", env_files = config.env_files.len(), ai_ready = runtime.status.ready);
    let public = config.public();
    *state.config.write().map_err(|_| AppError::internal("config lock poisoned"))? = config;
    *state.ai.write().map_err(|_| AppError::internal("AI runtime lock poisoned"))? = runtime;
    Ok(ReloadResult { config: public, ai: view(&state)? })
}
