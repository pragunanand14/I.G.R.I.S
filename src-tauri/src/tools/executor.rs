//! Tool router: validation → permission policy → execution → audit.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::{audit, schema, PermissionLevel, ToolErrorKind, ToolRegistry};
use crate::ai::{Source, ToolCall, ToolResult};
use crate::db::Database;

pub const TOOL_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESULT_CHARS: usize = 16_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityStatus {
    Running,
    AwaitingApproval,
    Completed,
    Failed,
    Denied,
    Invalid,
    Cancelled,
}

impl ActivityStatus {
    fn as_str(self) -> &'static str {
        match self {
            ActivityStatus::Running => "running",
            ActivityStatus::AwaitingApproval => "awaiting_approval",
            ActivityStatus::Completed => "completed",
            ActivityStatus::Failed => "failed",
            ActivityStatus::Denied => "denied",
            ActivityStatus::Invalid => "invalid",
            ActivityStatus::Cancelled => "cancelled",
        }
    }
}

/// What the UI shows for one tool call. Persisted with the assistant message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolActivity {
    pub id: String,
    pub tool: String,
    pub title: String,
    /// `None` when the call didn't resolve to a registered tool.
    pub permission: Option<PermissionLevel>,
    pub description: String,
    pub status: ActivityStatus,
    pub result: Option<String>,
    pub duration_ms: Option<u64>,
    /// Characters of response text written before this call (for interleaved display).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_offset: Option<usize>,
    /// Links the result came from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
    /// Attachment ids of images the tool produced (e.g. a screenshot).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approval {
    Approved,
    Denied,
    /// Nobody answered in time.
    Expired,
    Cancelled,
}

/// Asks the user to approve an action. Implemented by the IPC layer.
#[async_trait::async_trait]
pub trait Approver: Send + Sync {
    async fn request(&self, activity: &ToolActivity, cancel: &CancellationToken) -> Approval;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Policy {
    /// Ask before LOW-risk actions too (SENSITIVE and CRITICAL always ask).
    pub confirm_low: bool,
}

impl Policy {
    pub fn requires_approval(&self, level: PermissionLevel) -> bool {
        match level {
            PermissionLevel::Safe => false,
            PermissionLevel::Low => self.confirm_low,
            PermissionLevel::Sensitive | PermissionLevel::Critical => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Assistant,
    User,
}

/// Tools the user may approve once for a whole conversation ("Allow for this
/// chat"). Deleting files and screenshots are deliberately excluded: they
/// always ask.
pub const TRUSTABLE: &[&str] = &["write_file", "move_path", "close_application"];

/// Conversations the user has trusted this session (in memory only).
#[derive(Default)]
pub struct Trust {
    inner: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl Trust {
    pub fn trust(&self, conversation_id: &str) {
        if let Ok(mut s) = self.inner.lock() {
            s.insert(conversation_id.to_string());
        }
    }

    pub fn covers(&self, conversation_id: Option<&str>, tool: &str) -> bool {
        match conversation_id {
            Some(c) if TRUSTABLE.contains(&tool) => self.inner.lock().map(|s| s.contains(c)).unwrap_or(false),
            _ => false,
        }
    }
}

pub struct ExecContext<'a> {
    pub registry: &'a ToolRegistry,
    pub db: &'a Arc<Database>,
    pub conversation_id: Option<&'a str>,
    pub actor: Actor,
    pub policy: Policy,
    /// Tools offered in this conversation; calls to anything else are rejected.
    pub allowed: Option<&'a [String]>,
    pub approver: &'a dyn Approver,
    pub cancel: &'a CancellationToken,
    /// Conversations trusted for [`TRUSTABLE`] tools; `None` = always ask.
    pub trust: Option<&'a Trust>,
}

fn clip(s: String) -> String {
    if s.chars().count() <= MAX_RESULT_CHARS {
        s
    } else {
        s.chars().take(MAX_RESULT_CHARS).collect::<String>() + "\n[output truncated]"
    }
}

/// Run one tool call through the full pipeline. Never panics and never
/// reports success unless the tool actually succeeded.
pub async fn execute(call: &ToolCall, ctx: &ExecContext<'_>, on_update: &mut (dyn FnMut(&ToolActivity) + Send)) -> (ToolResult, ToolActivity) {
    let started = Instant::now();
    let mut activity = ToolActivity {
        id: call.id.clone(),
        tool: call.name.clone(),
        title: call.name.clone(),
        permission: None,
        description: call.name.clone(),
        status: ActivityStatus::Running,
        result: None,
        duration_ms: None,
        text_offset: None,
        sources: Vec::new(),
        attachments: Vec::new(),
    };
    let mut approval = "auto";

    let finish = |activity: &mut ToolActivity, status: ActivityStatus, model_text: String, ui_text: String, is_error: bool, approval: &str| {
        activity.status = status;
        activity.result = Some(ui_text);
        activity.duration_ms = Some(started.elapsed().as_millis() as u64);
        if let Ok(conn) = ctx.db.conn() {
            let input = call.invalid_input.clone().unwrap_or_else(|| call.input.to_string());
            let r = audit::record(
                &conn,
                &audit::NewAuditEntry {
                    conversation_id: ctx.conversation_id,
                    tool: &call.name,
                    permission: activity.permission.map(PermissionLevel::as_str).unwrap_or("unknown"),
                    actor: if ctx.actor == Actor::User { "user" } else { "assistant" },
                    description: &activity.description,
                    input: &input,
                    status: status.as_str(),
                    approval,
                    result: activity.result.as_deref(),
                    duration_ms: activity.duration_ms.map(|d| d as i64),
                },
            );
            if let Err(e) = r {
                tracing::error!(event = "TOOL_AUDIT_WRITE_FAILED", error = %e);
            }
        }
        tracing::info!(event = "TOOL_FINISHED", tool = %call.name, status = status.as_str(), approval, duration_ms = activity.duration_ms);
        (ToolResult { call_id: call.id.clone(), content: model_text, is_error, media: Vec::new() }, activity.clone())
    };

    // 1. Malformed arguments from the provider.
    if let Some(raw) = &call.invalid_input {
        let content = json!({ "INVALID_JSON": raw }).to_string();
        return finish(&mut activity, ActivityStatus::Invalid, content, "The model sent malformed input.".into(), true, approval);
    }

    // 2. Unknown tool, or one not offered in this conversation (hallucinated names).
    let offered = ctx.allowed.map(|a| a.iter().any(|n| n == &call.name)).unwrap_or(true);
    let tool = match ctx.registry.get(&call.name).filter(|_| offered) {
        Some(t) => t,
        None => {
            let names: Vec<String> = match ctx.allowed {
                Some(a) => a.to_vec(),
                None => ctx.registry.specs().iter().map(|s| s.name.to_string()).collect(),
            };
            let msg = format!("Unknown tool '{}'. Available tools: {}.", call.name, names.join(", "));
            return finish(&mut activity, ActivityStatus::Invalid, msg.clone(), msg, true, approval);
        }
    };
    let spec = tool.spec();
    activity.title = spec.title.to_string();
    activity.permission = Some(spec.permission);

    // 3. Input validation (schema + tool-specific).
    if let Err(e) = schema::validate(&spec.input_schema, &call.input).and_then(|_| tool.validate(&call.input).map_err(|e| e.message)) {
        let msg = format!("Invalid input for {}: {e}", spec.name);
        return finish(&mut activity, ActivityStatus::Invalid, msg.clone(), msg, true, approval);
    }
    activity.description = tool.describe(&call.input);

    // 4. Permission policy.
    if ctx.policy.requires_approval(spec.permission) && ctx.trust.is_some_and(|t| t.covers(ctx.conversation_id, spec.name)) {
        approval = "trusted";
        tracing::info!(event = "TOOL_APPROVAL_TRUSTED", tool = spec.name);
    } else if ctx.policy.requires_approval(spec.permission) {
        activity.status = ActivityStatus::AwaitingApproval;
        on_update(&activity);
        tracing::info!(event = "TOOL_APPROVAL_REQUESTED", tool = spec.name, permission = spec.permission.as_str());
        match ctx.approver.request(&activity, ctx.cancel).await {
            Approval::Approved => approval = "approved",
            Approval::Denied => {
                return finish(
                    &mut activity,
                    ActivityStatus::Denied,
                    "The user denied this action. It was not performed. Don't retry it unless the user asks.".into(),
                    "Denied".into(),
                    true,
                    "denied",
                );
            }
            Approval::Expired => {
                return finish(
                    &mut activity,
                    ActivityStatus::Denied,
                    "The user didn't respond to the confirmation request, so the action was not performed.".into(),
                    "No response — not performed".into(),
                    true,
                    "expired",
                );
            }
            Approval::Cancelled => {
                return finish(&mut activity, ActivityStatus::Cancelled, "Cancelled by the user.".into(), "Cancelled".into(), true, "cancelled");
            }
        }
    }

    // 5. Execute with timeout and cancellation.
    activity.status = ActivityStatus::Running;
    on_update(&activity);
    tracing::info!(event = "TOOL_STARTED", tool = spec.name);
    let outcome = tokio::select! {
        _ = ctx.cancel.cancelled() => None,
        r = tokio::time::timeout(TOOL_TIMEOUT, tool.execute(&call.input)) => Some(r),
    };
    match outcome {
        None => finish(&mut activity, ActivityStatus::Cancelled, "Cancelled by the user.".into(), "Cancelled".into(), true, approval),
        Some(Err(_)) => {
            let msg = format!("{} timed out after {}s.", spec.title, TOOL_TIMEOUT.as_secs());
            finish(&mut activity, ActivityStatus::Failed, msg.clone(), msg, true, approval)
        }
        Some(Ok(Ok(out))) => {
            activity.sources = out.sources;
            activity.attachments = out.media.iter().map(|m| m.attachment_id.clone()).collect();
            if let (Some(cid), false) = (ctx.conversation_id, activity.attachments.is_empty()) {
                // Captures belong to the conversation they were taken in (deleted with it).
                if let Err(e) = ctx.db.conn().and_then(|c| crate::attachments::adopt(&c, &activity.attachments, cid)) {
                    tracing::warn!(event = "ATTACHMENT_ADOPT_FAILED", error = %e);
                }
            }
            let (mut result, activity) = finish(&mut activity, ActivityStatus::Completed, clip(out.content), out.summary, false, approval);
            result.media = out.media;
            (result, activity)
        }
        Some(Ok(Err(e))) => {
            let status = if e.kind == ToolErrorKind::InvalidInput { ActivityStatus::Invalid } else { ActivityStatus::Failed };
            finish(&mut activity, status, e.message.clone(), e.message, true, approval)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::calculator::CalculatorTool;
    use crate::tools::{Tool, ToolOutput, ToolResultT, ToolSpec};
    use std::sync::Mutex;

    struct FixedApprover(Approval, Mutex<u32>);

    #[async_trait::async_trait]
    impl Approver for FixedApprover {
        async fn request(&self, _a: &ToolActivity, _c: &CancellationToken) -> Approval {
            *self.1.lock().unwrap() += 1;
            self.0
        }
    }

    struct Dangerous(ToolSpec, Mutex<u32>);

    #[async_trait::async_trait]
    impl Tool for Dangerous {
        fn spec(&self) -> &ToolSpec {
            &self.0
        }
        fn describe(&self, _i: &serde_json::Value) -> String {
            "Do something dangerous".into()
        }
        async fn execute(&self, _i: &serde_json::Value) -> ToolResultT {
            *self.1.lock().unwrap() += 1;
            Ok(ToolOutput { content: "done".into(), summary: "done".into(), sources: vec![], media: Vec::new() })
        }
    }

    fn dangerous(level: PermissionLevel) -> Arc<Dangerous> {
        Arc::new(Dangerous(
            ToolSpec {
                name: "danger",
                title: "Danger",
                description: "d",
                input_schema: json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
                permission: level,
            },
            Mutex::new(0),
        ))
    }

    fn call(name: &str, input: serde_json::Value) -> ToolCall {
        ToolCall { id: "c1".into(), name: name.into(), input, invalid_input: None, provider_data: None }
    }

    async fn run(
        registry: &ToolRegistry,
        db: &Arc<Database>,
        c: &ToolCall,
        approver: &dyn Approver,
        policy: Policy,
        allowed: Option<&[String]>,
    ) -> (ToolResult, ToolActivity) {
        let cancel = CancellationToken::new();
        let ctx = ExecContext { registry, db, conversation_id: Some("conv"), actor: Actor::Assistant, policy, allowed, approver, cancel: &cancel, trust: None };
        execute(c, &ctx, &mut |_| {}).await
    }

    fn setup() -> (ToolRegistry, Arc<Database>) {
        let mut r = ToolRegistry::default();
        r.register(Arc::new(CalculatorTool::default()));
        (r, Arc::new(Database::open_in_memory().unwrap()))
    }

    #[tokio::test]
    async fn safe_tool_runs_without_approval_and_is_audited() {
        let (r, db) = setup();
        let approver = FixedApprover(Approval::Denied, Mutex::new(0));
        let (res, act) = run(&r, &db, &call("calculator", json!({"expression":"6*7"})), &approver, Policy::default(), None).await;
        assert!(!res.is_error);
        assert_eq!(res.content, "6*7 = 42");
        assert_eq!(act.status, ActivityStatus::Completed);
        assert_eq!(*approver.1.lock().unwrap(), 0);
        let log = audit::list(&db.conn().unwrap(), 10).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].status, "completed");
        assert_eq!(log[0].approval, "auto");
        assert_eq!(log[0].conversation_id.as_deref(), Some("conv"));
    }

    #[tokio::test]
    async fn hallucinated_and_unoffered_tools_are_rejected() {
        let (r, db) = setup();
        let approver = FixedApprover(Approval::Approved, Mutex::new(0));
        let (res, act) = run(&r, &db, &call("shell_exec", json!({"cmd":"rm -rf /"})), &approver, Policy::default(), None).await;
        assert!(res.is_error);
        assert!(res.content.contains("Unknown tool 'shell_exec'"));
        assert_eq!(act.status, ActivityStatus::Invalid);
        assert_eq!(act.permission, None, "unknown tools have no permission level");
        assert_eq!(audit::list(&db.conn().unwrap(), 1).unwrap()[0].permission, "unknown");
        // Registered but not offered in this conversation.
        let allowed = vec!["system_info".to_string()];
        let (res, _) = run(&r, &db, &call("calculator", json!({"expression":"1"})), &approver, Policy::default(), Some(&allowed)).await;
        assert!(res.is_error);
    }

    #[tokio::test]
    async fn invalid_arguments_never_reach_the_tool() {
        let (r, db) = setup();
        let approver = FixedApprover(Approval::Approved, Mutex::new(0));
        for bad in [json!({}), json!({"expression": 5}), json!({"expression":"1", "extra": true})] {
            let (res, act) = run(&r, &db, &call("calculator", bad), &approver, Policy::default(), None).await;
            assert!(res.is_error);
            assert_eq!(act.status, ActivityStatus::Invalid);
        }
        let mut c = call("calculator", json!({}));
        c.invalid_input = Some("{\"expression\": \"1".into());
        let (res, _) = run(&r, &db, &c, &approver, Policy::default(), None).await;
        assert!(res.content.contains("INVALID_JSON"));
    }

    #[tokio::test]
    async fn trusted_conversations_skip_approval_only_for_trustable_tools() {
        let named = |name: &'static str| {
            Arc::new(Dangerous(
                ToolSpec {
                    name,
                    title: "t",
                    description: "d",
                    input_schema: json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
                    permission: PermissionLevel::Sensitive,
                },
                Mutex::new(0),
            ))
        };
        let (write, trash) = (named("write_file"), named("trash_path"));
        let mut r = ToolRegistry::default();
        r.register(write.clone());
        r.register(trash.clone());
        let db = Arc::new(Database::open_in_memory().unwrap());
        let trust = Trust::default();
        let deny = FixedApprover(Approval::Denied, Mutex::new(0));
        let cancel = CancellationToken::new();
        let ctx = |cid| ExecContext {
            registry: &r,
            db: &db,
            conversation_id: Some(cid),
            actor: Actor::Assistant,
            policy: Policy::default(),
            allowed: None,
            approver: &deny,
            cancel: &cancel,
            trust: Some(&trust),
        };

        // Not trusted yet: asks (and the approver denies).
        execute(&call("write_file", json!({})), &ctx("c1"), &mut |_| {}).await;
        assert_eq!(*write.1.lock().unwrap(), 0);

        trust.trust("c1");
        let (res, act) = execute(&call("write_file", json!({})), &ctx("c1"), &mut |_| {}).await;
        assert!(!res.is_error, "{:?}", act.result);
        assert_eq!(*write.1.lock().unwrap(), 1);
        // Deleting still asks, and other conversations aren't trusted.
        execute(&call("trash_path", json!({})), &ctx("c1"), &mut |_| {}).await;
        execute(&call("write_file", json!({})), &ctx("c2"), &mut |_| {}).await;
        assert_eq!((*trash.1.lock().unwrap(), *write.1.lock().unwrap()), (0, 1));
        let approvals: Vec<String> = audit::list(&db.conn().unwrap(), 10).unwrap().into_iter().map(|e| e.approval).collect();
        assert_eq!(approvals, vec!["denied", "denied", "trusted", "denied"]);
    }

    #[tokio::test]
    async fn sensitive_and_critical_always_need_approval() {
        for level in [PermissionLevel::Sensitive, PermissionLevel::Critical] {
            let tool = dangerous(level);
            let mut r = ToolRegistry::default();
            r.register(tool.clone());
            let db = Arc::new(Database::open_in_memory().unwrap());

            let deny = FixedApprover(Approval::Denied, Mutex::new(0));
            let (res, act) = run(&r, &db, &call("danger", json!({})), &deny, Policy { confirm_low: false }, None).await;
            assert!(res.is_error);
            assert_eq!(act.status, ActivityStatus::Denied);
            assert_eq!(*tool.1.lock().unwrap(), 0, "denied action must not execute");

            let expire = FixedApprover(Approval::Expired, Mutex::new(0));
            run(&r, &db, &call("danger", json!({})), &expire, Policy::default(), None).await;
            assert_eq!(*tool.1.lock().unwrap(), 0, "unanswered request must not execute");

            let approve = FixedApprover(Approval::Approved, Mutex::new(0));
            let (res, _) = run(&r, &db, &call("danger", json!({})), &approve, Policy::default(), None).await;
            assert!(!res.is_error);
            assert_eq!(*tool.1.lock().unwrap(), 1);
            let statuses: Vec<String> = audit::list(&db.conn().unwrap(), 10).unwrap().into_iter().map(|e| e.approval).collect();
            assert_eq!(statuses, vec!["approved", "expired", "denied"]);
        }
    }

    #[tokio::test]
    async fn low_risk_follows_policy() {
        let tool = dangerous(PermissionLevel::Low);
        let mut r = ToolRegistry::default();
        r.register(tool.clone());
        let db = Arc::new(Database::open_in_memory().unwrap());
        let deny = FixedApprover(Approval::Denied, Mutex::new(0));
        run(&r, &db, &call("danger", json!({})), &deny, Policy { confirm_low: false }, None).await;
        assert_eq!(*tool.1.lock().unwrap(), 1, "low runs automatically by default");
        run(&r, &db, &call("danger", json!({})), &deny, Policy { confirm_low: true }, None).await;
        assert_eq!(*tool.1.lock().unwrap(), 1, "low asks when configured");
        assert_eq!(*deny.1.lock().unwrap(), 1);
    }

    #[test]
    fn policy_matrix() {
        let p = Policy { confirm_low: false };
        assert!(!p.requires_approval(PermissionLevel::Safe));
        assert!(!p.requires_approval(PermissionLevel::Low));
        assert!(p.requires_approval(PermissionLevel::Sensitive));
        assert!(p.requires_approval(PermissionLevel::Critical));
        assert!(Policy { confirm_low: true }.requires_approval(PermissionLevel::Low));
        assert!(!Policy { confirm_low: true }.requires_approval(PermissionLevel::Safe));
    }
}
