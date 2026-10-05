use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::ai::{AiProvider, AiRuntime, AiStatus};
use crate::config::AppConfig;
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::system::{ConnectivityMonitor, SystemMonitor};
use crate::tools::executor::{Approval, Approver, ToolActivity};
use crate::tools::ToolRegistry;

/// Shared application state, managed by Tauri and injected into commands.
pub struct AppState {
    /// Reloadable via the `reload_config` command.
    pub config: Arc<RwLock<AppConfig>>,
    pub ai: RwLock<AiRuntime>,
    pub db: Arc<Database>,
    pub system: Arc<Mutex<SystemMonitor>>,
    pub connectivity: ConnectivityMonitor,
    pub generations: Generations,
    pub tools: Arc<ToolRegistry>,
    pub approvals: Arc<PendingApprovals>,
    pub paths: AppPaths,
    pub attachments: Arc<crate::attachments::AttachmentStore>,
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub log_dir: PathBuf,
    pub db_path: PathBuf,
}

impl AppState {
    pub fn ai_provider(&self) -> AppResult<(Arc<dyn AiProvider>, AiStatus)> {
        let rt = self.ai.read().map_err(|_| AppError::internal("AI runtime lock poisoned"))?;
        match &rt.provider {
            Some(p) => Ok((Arc::clone(p), rt.status.clone())),
            None => Err(AppError::AiUnavailable(rt.status.problem.clone().unwrap_or_else(|| "AI is not configured.".into()))),
        }
    }
}

/// In-flight generations, keyed by request id. At most one per conversation.
#[derive(Default)]
pub struct Generations {
    inner: Mutex<HashMap<String, (Option<String>, CancellationToken)>>,
}

impl Generations {
    /// Register a request. Fails if the id is in use or the conversation is busy.
    pub fn begin(&self, request_id: &str, conversation_id: Option<&str>) -> AppResult<GenerationGuard<'_>> {
        let mut map = self.inner.lock().map_err(|_| AppError::internal("generation registry poisoned"))?;
        if map.contains_key(request_id) {
            return Err(AppError::validation("Duplicate request id."));
        }
        if let Some(cid) = conversation_id {
            if map.values().any(|(c, _)| c.as_deref() == Some(cid)) {
                return Err(AppError::validation("IGRIS is already responding in this conversation."));
            }
        }
        let token = CancellationToken::new();
        map.insert(request_id.to_string(), (conversation_id.map(str::to_string), token.clone()));
        Ok(GenerationGuard { owner: self, request_id: request_id.to_string(), token })
    }

    pub fn cancel(&self, request_id: &str) -> bool {
        match self.inner.lock() {
            Ok(map) => map.get(request_id).map(|(_, t)| t.cancel()).is_some(),
            Err(_) => false,
        }
    }

    pub fn is_busy(&self, conversation_id: &str) -> bool {
        self.inner.lock().map(|m| m.values().any(|(c, _)| c.as_deref() == Some(conversation_id))).unwrap_or(false)
    }

    fn set_conversation(&self, request_id: &str, conversation_id: &str) {
        if let Ok(mut map) = self.inner.lock() {
            if let Some(entry) = map.get_mut(request_id) {
                entry.0 = Some(conversation_id.to_string());
            }
        }
    }
}

/// Removes the registry entry when the generation ends (including on error).
pub struct GenerationGuard<'a> {
    owner: &'a Generations,
    request_id: String,
    pub token: CancellationToken,
}

impl GenerationGuard<'_> {
    /// Bind a newly created conversation to this request.
    pub fn attach(&self, conversation_id: &str) {
        self.owner.set_conversation(&self.request_id, conversation_id);
    }
}

impl Drop for GenerationGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut map) = self.owner.inner.lock() {
            map.remove(&self.request_id);
        }
    }
}

/// Tool approvals waiting for the user, keyed by tool call id.
#[derive(Default)]
pub struct PendingApprovals {
    inner: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl PendingApprovals {
    /// Deliver the user's decision. Returns false if nothing is waiting (expired or unknown).
    pub fn respond(&self, call_id: &str, approved: bool) -> bool {
        let sender = self.inner.lock().ok().and_then(|mut m| m.remove(call_id));
        match sender {
            Some(tx) => tx.send(approved).is_ok(),
            None => false,
        }
    }

    fn register(&self, call_id: &str) -> Option<oneshot::Receiver<bool>> {
        let mut map = self.inner.lock().ok()?;
        if map.contains_key(call_id) {
            return None;
        }
        let (tx, rx) = oneshot::channel();
        map.insert(call_id.to_string(), tx);
        Some(rx)
    }

    fn forget(&self, call_id: &str) {
        if let Ok(mut m) = self.inner.lock() {
            m.remove(call_id);
        }
    }
}

/// Waits for the user to answer an approval prompt in the UI.
pub struct UiApprover {
    pub pending: Arc<PendingApprovals>,
    pub timeout: Duration,
}

pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);

#[async_trait::async_trait]
impl Approver for UiApprover {
    async fn request(&self, activity: &ToolActivity, cancel: &CancellationToken) -> Approval {
        let Some(rx) = self.pending.register(&activity.id) else {
            return Approval::Denied; // duplicate id: fail closed
        };
        let decision = tokio::select! {
            _ = cancel.cancelled() => Approval::Cancelled,
            r = tokio::time::timeout(self.timeout, rx) => match r {
                Ok(Ok(true)) => Approval::Approved,
                Ok(Ok(false)) => Approval::Denied,
                Ok(Err(_)) => Approval::Cancelled,
                Err(_) => Approval::Expired,
            },
        };
        self.pending.forget(&activity.id);
        decision
    }
}

/// Approves everything — used only for actions the user started themselves.
pub struct UserInitiated;

#[async_trait::async_trait]
impl Approver for UserInitiated {
    async fn request(&self, _activity: &ToolActivity, _cancel: &CancellationToken) -> Approval {
        Approval::Approved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::executor::ActivityStatus;
    use crate::tools::PermissionLevel;

    fn activity(id: &str) -> ToolActivity {
        ToolActivity {
            id: id.into(),
            tool: "t".into(),
            title: "t".into(),
            permission: Some(PermissionLevel::Sensitive),
            description: "d".into(),
            status: ActivityStatus::AwaitingApproval,
            result: None,
            duration_ms: None,
            text_offset: None,
            sources: Vec::new(),
            attachments: Vec::new(),
        }
    }

    #[tokio::test]
    async fn ui_approver_waits_for_the_user() {
        let pending = Arc::new(PendingApprovals::default());
        let approver = UiApprover { pending: pending.clone(), timeout: Duration::from_secs(5) };
        let p2 = pending.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(p2.respond("c1", true));
        });
        assert_eq!(approver.request(&activity("c1"), &CancellationToken::new()).await, Approval::Approved);
        assert!(!pending.respond("c1", true), "answered approvals can't be answered again");
    }

    #[tokio::test]
    async fn ui_approver_expires_and_cancels() {
        let pending = Arc::new(PendingApprovals::default());
        let approver = UiApprover { pending: pending.clone(), timeout: Duration::from_millis(50) };
        assert_eq!(approver.request(&activity("c2"), &CancellationToken::new()).await, Approval::Expired);
        assert!(!pending.respond("c2", true), "a late approval does nothing");

        let cancel = CancellationToken::new();
        cancel.cancel();
        let approver = UiApprover { pending, timeout: Duration::from_secs(5) };
        assert_eq!(approver.request(&activity("c3"), &cancel).await, Approval::Cancelled);
    }

    #[test]
    fn unknown_approval_ids_are_rejected() {
        assert!(!PendingApprovals::default().respond("nope", true));
    }

    #[test]
    fn one_generation_per_conversation_and_cleanup_on_drop() {
        let g = Generations::default();
        let a = g.begin("r1", Some("c1")).unwrap();
        assert!(g.begin("r2", Some("c1")).is_err());
        assert!(g.begin("r1", None).is_err());
        assert!(g.is_busy("c1"));
        assert!(g.cancel("r1"));
        assert!(a.token.is_cancelled());
        drop(a);
        assert!(!g.is_busy("c1"));
        assert!(!g.cancel("r1"));
        assert!(g.begin("r2", Some("c1")).is_ok());
    }

    #[test]
    fn attach_binds_new_conversation() {
        let g = Generations::default();
        let a = g.begin("r1", None).unwrap();
        a.attach("c9");
        assert!(g.is_busy("c9"));
    }
}
