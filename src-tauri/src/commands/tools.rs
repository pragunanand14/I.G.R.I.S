use serde::Serialize;
use serde_json::json;
use tauri::State;
use tokio_util::sync::CancellationToken;

use crate::ai::ToolCall;
use crate::error::{AppError, AppResult};
use crate::settings;
use crate::state::{AppState, UserInitiated};
use crate::tools::apps::{self, AppCandidate, AppEntry};
use crate::tools::audit::{self, AuditEntry};
use crate::tools::executor::{self, ActivityStatus, Actor, ExecContext, Policy, ToolActivity};
use crate::tools::{PermissionLevel, ToolSpec};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    #[serde(flatten)]
    pub spec: ToolSpec,
    pub requires_approval: bool,
}

#[tauri::command]
pub fn list_tools(state: State<'_, AppState>) -> AppResult<Vec<ToolInfo>> {
    let s = settings::load(&*state.db.conn()?)?;
    let policy = Policy { confirm_low: s.confirm_low_risk };
    Ok(state.tools.specs().into_iter().map(|spec| ToolInfo { requires_approval: policy.requires_approval(spec.permission), spec }).collect())
}

#[tauri::command]
pub fn list_tool_audit(state: State<'_, AppState>, limit: Option<u32>) -> AppResult<Vec<AuditEntry>> {
    audit::list(&*state.db.conn()?, limit.unwrap_or(200))
}

#[tauri::command]
pub fn list_applications(state: State<'_, AppState>) -> AppResult<Vec<AppEntry>> {
    apps::list(&*state.db.conn()?)
}

#[tauri::command]
pub fn add_application(state: State<'_, AppState>, name: String, path: String) -> AppResult<AppEntry> {
    apps::add(&*state.db.conn()?, &name, &path)
}

#[tauri::command]
pub fn remove_application(state: State<'_, AppState>, id: String) -> AppResult<()> {
    apps::remove(&*state.db.conn()?, &id)
}

/// Well-known apps found on this machine that aren't in the allowlist yet.
#[tauri::command]
pub async fn detect_applications(state: State<'_, AppState>) -> AppResult<Vec<AppCandidate>> {
    let existing = apps::list(&*state.db.conn()?)?;
    let found = tauri::async_runtime::spawn_blocking(apps::detect).await.map_err(|e| AppError::internal(e.to_string()))?;
    Ok(found.into_iter().filter(|c| !existing.iter().any(|e| e.name.eq_ignore_ascii_case(&c.name) || e.path == c.path)).collect())
}

/// Launch an allowlisted app from the UI. Goes through the same executor
/// (validation + audit) as model-initiated calls, attributed to the user.
#[tauri::command]
pub async fn launch_application(state: State<'_, AppState>, id: String) -> AppResult<ToolActivity> {
    let entry = apps::get(&*state.db.conn()?, &id)?.ok_or_else(|| AppError::validation("That application is no longer in the list."))?;
    let call = ToolCall {
        id: format!("user-{}", uuid::Uuid::new_v4()),
        name: "launch_application".into(),
        input: json!({ "name": entry.name }),
        invalid_input: None,
        extras: None,
    };
    let cancel = CancellationToken::new();
    let ctx = ExecContext {
        registry: &state.tools,
        db: &state.db,
        conversation_id: None,
        actor: Actor::User,
        // The user clicked the button; that is the approval.
        policy: Policy { confirm_low: false },
        allowed: None,
        approver: &UserInitiated,
        cancel: &cancel,
        trust: None,
        operator: None,
        task_id: None,
    };
    let (_, activity) = executor::execute(&call, &ctx, &mut |_| {}).await;
    match activity.status {
        ActivityStatus::Completed => Ok(activity),
        _ => Err(AppError::validation(activity.result.unwrap_or_else(|| format!("Couldn't open {}.", entry.name)))),
    }
}

/// Permission levels and what each currently requires (for the Security page).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    pub level: PermissionLevel,
    pub requires_approval: bool,
    pub configurable: bool,
}

#[tauri::command]
pub fn get_permission_policy(state: State<'_, AppState>) -> AppResult<Vec<PermissionRule>> {
    let s = settings::load(&*state.db.conn()?)?;
    let policy = Policy { confirm_low: s.confirm_low_risk };
    Ok([PermissionLevel::Safe, PermissionLevel::Low, PermissionLevel::Sensitive, PermissionLevel::Critical]
        .into_iter()
        .map(|level| PermissionRule { level, requires_approval: policy.requires_approval(level), configurable: level == PermissionLevel::Low })
        .collect())
}
