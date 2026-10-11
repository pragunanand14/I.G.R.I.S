//! Running routines in the app: each run is a conversation generated like a
//! chat, with an approver that reaches the user wherever they are — a system
//! notification plus the approval card (with "Always allow") on any IGRIS
//! screen. "Always allow" rules apply as usual, so a routine the user has
//! approved once runs on its own from then on.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::core::chat;
use crate::error::{AppError, AppResult};
use crate::routines::{Routine, Runner};
use crate::state::{AppState, UiApprover, APPROVAL_TIMEOUT};
use crate::tools::executor::{Approval, Approver, ToolActivity};

pub struct AppRoutines(pub AppHandle);

fn notify(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(e) = app.notification().builder().title(format!("IGRIS · {title}")).body(crate::orchestrator::task::clip(body, 220)).show() {
        tracing::warn!(event = "NOTIFICATION_FAILED", error = %e);
    }
}

/// Asks the user — notification + approval card — and waits like a chat approval.
struct RoutineApprover {
    app: AppHandle,
    name: String,
    inner: UiApprover,
}

#[async_trait::async_trait]
impl Approver for RoutineApprover {
    async fn request(&self, activity: &ToolActivity, cancel: &CancellationToken) -> Approval {
        notify(&self.app, &format!("Routine “{}” needs your OK", self.name), &activity.description);
        let _ = self.app.emit(
            "device-event",
            serde_json::json!({
                "type": "local_approval",
                "callId": activity.id,
                "requestId": "",
                "deviceName": format!("Routine “{}”", self.name),
                "title": activity.title,
                "description": activity.description,
                "source": "routine",
                "allowRule": activity.allow_rule,
            }),
        );
        let answer = self.inner.request(activity, cancel).await;
        let _ = self.app.emit("device-event", serde_json::json!({ "type": "approval_closed", "callId": activity.id }));
        answer
    }
}

#[async_trait::async_trait]
impl Runner for AppRoutines {
    fn open(&self, _routine: &Routine, message: &str) -> AppResult<String> {
        let state = self.0.state::<AppState>();
        state.ai_router()?;
        let s = crate::settings::load(&*state.db.conn()?)?;
        let (conversation, _) =
            chat::save_user_message(&state.db, None, message, &s.user_name, &crate::commands::chat::offered_tools(&state)?, s.memory_enabled)?;
        let _ = self.0.emit("conversations-changed", ());
        Ok(conversation.id)
    }

    async fn run(&self, routine: &Routine, conversation_id: &str) -> AppResult<String> {
        let state = self.0.state::<AppState>();
        let mut params = crate::commands::chat::generation_params(&state)?;
        params.tooling.approver = Arc::new(RoutineApprover {
            app: self.0.clone(),
            name: routine.name.clone(),
            inner: UiApprover { pending: state.approvals.clone(), timeout: APPROVAL_TIMEOUT },
        });
        let request_id = format!("routine-{}", uuid::Uuid::new_v4().simple());
        let guard = state.generations.begin(&request_id, Some(conversation_id))?;
        let message = chat::generate(&state.db, conversation_id, &params, &guard.token, &mut |_| {}).await;
        state.operator.end_turn(conversation_id);
        drop(guard);
        let _ = self.0.emit("conversations-changed", ());
        let _ = self.0.emit("routines-changed", ());
        let m = message?;
        let reply = match m.error {
            Some(e) if m.content.trim().is_empty() => return Err(AppError::internal(e)),
            _ => m.content,
        };
        notify(&self.0, &routine.name, &reply);
        Ok(reply)
    }
}

/// Start the routine scheduler (after the app state exists).
pub fn start(app: &AppHandle, google: Arc<crate::google::Google>) {
    let db = app.state::<AppState>().db.clone();
    tauri::async_runtime::spawn(crate::routines::run(db, google, Arc::new(AppRoutines(app.clone()))));
}
