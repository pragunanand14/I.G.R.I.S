//! End-to-end: chat turns through the agent loop, the real executor and the
//! orchestrator, against a mock provider.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::task::{StepStatus, TaskKind, TaskState};
use super::{store, Control, Orchestrator, TaskUpdate};
use crate::ai::anthropic::AnthropicProvider;
use crate::ai::testutil::{sse, MockServer};
use crate::ai::{ModelRouter, ResponseDepth};
use crate::conversations::{self, MessageStatus};
use crate::core::chat::{generate, save_user_message, GenerationParams, Tooling};
use crate::db::Database;
use crate::tools::executor::{Approval, Approver, Policy, ToolActivity};
use crate::tools::{PermissionLevel, Tool, ToolCtx, ToolError, ToolErrorKind, ToolOutput, ToolRegistry, ToolResultT, ToolSpec};

fn spec(name: &'static str, permission: PermissionLevel) -> ToolSpec {
    ToolSpec {
        name,
        title: name,
        description: "test tool",
        input_schema: json!({"type": "object", "properties": {"x": {"type": "string"}}, "required": ["x"], "additionalProperties": false}),
        permission,
    }
}

type OnCall = Box<dyn Fn(&ToolCtx) + Send + Sync>;

/// A scripted tool: each call takes the next outcome (the last one repeats).
struct Scripted {
    spec: ToolSpec,
    outcomes: Mutex<Vec<Result<String, ToolErrorKind>>>,
    calls: AtomicU32,
    on_call: Option<OnCall>,
}

impl Scripted {
    fn new(name: &'static str, permission: PermissionLevel, outcomes: Vec<Result<&str, ToolErrorKind>>) -> Arc<Self> {
        Arc::new(Self {
            spec: spec(name, permission),
            outcomes: Mutex::new(outcomes.into_iter().map(|o| o.map(str::to_string)).collect()),
            calls: AtomicU32::new(0),
            on_call: None,
        })
    }
    fn calls(&self) -> u32 {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl Tool for Scripted {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        format!("{} {}", self.spec.name, i["x"].as_str().unwrap_or_default())
    }
    async fn execute(&self, _i: &Value) -> ToolResultT {
        unreachable!()
    }
    async fn execute_in(&self, _i: &Value, ctx: &ToolCtx) -> ToolResultT {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(f) = &self.on_call {
            f(ctx);
        }
        let mut o = self.outcomes.lock().unwrap();
        let next = if o.len() > 1 { o.remove(0) } else { o[0].clone() };
        match next {
            Ok(content) => Ok(ToolOutput { content: content.clone(), summary: "ok".into(), sources: vec![], media: vec![] }),
            Err(kind) => Err(ToolError { kind, message: format!("{} failed ({kind:?}).", self.spec.name) }),
        }
    }
}

/// The existing approval flow, answering `answer` and noting the task's state while it asks.
struct Approve {
    answer: Approval,
    hub: Arc<Orchestrator>,
    asked: Mutex<Vec<(String, Option<TaskState>)>>,
}

#[async_trait::async_trait]
impl Approver for Approve {
    async fn request(&self, a: &ToolActivity, _c: &CancellationToken) -> Approval {
        let state = self.hub.lock().values().next().map(|l| l.task.state);
        self.asked.lock().unwrap().push((a.tool.clone(), state));
        self.answer
    }
}

struct Env {
    db: Arc<Database>,
    hub: Arc<Orchestrator>,
    dir: tempfile::TempDir,
    approve: Arc<Approve>,
    events: Arc<Mutex<Vec<String>>>,
    extra: Vec<Arc<dyn Tool>>,
    operator: Option<Arc<crate::operator::Operator>>,
}

impl Env {
    fn new(answer: Approval) -> Self {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let dir = tempfile::tempdir().unwrap();
        crate::files::add(&db.conn().unwrap(), &dir.path().display().to_string(), true).unwrap();
        let hub = Orchestrator::new(db.clone(), None);
        Self::with(db, hub, dir, answer, None)
    }

    fn with(db: Arc<Database>, hub: Arc<Orchestrator>, dir: tempfile::TempDir, answer: Approval, operator: Option<Arc<crate::operator::Operator>>) -> Self {
        let events = Arc::new(Mutex::new(Vec::new()));
        let e2 = events.clone();
        hub.on_event(Arc::new(move |u: &TaskUpdate| e2.lock().unwrap().push(u.event.name().to_string())));
        let approve = Arc::new(Approve { answer, hub: hub.clone(), asked: Mutex::new(Vec::new()) });
        Self { db, hub, dir, approve, events, extra: Vec::new(), operator }
    }

    fn path(&self, name: &str) -> String {
        self.dir.path().join(name).display().to_string()
    }

    fn registry(&self) -> Arc<ToolRegistry> {
        use crate::tools::files::*;
        let mut r = ToolRegistry::default();
        r.register(Arc::new(crate::tools::calculator::CalculatorTool::default()));
        r.register(Arc::new(ReadFileTool::new(self.db.clone())));
        r.register(Arc::new(ListDirectoryTool::new(self.db.clone())));
        r.register(Arc::new(WriteFileTool::new(self.db.clone())));
        r.register(Arc::new(CreateFileTool::new(self.db.clone())));
        r.register(Arc::new(crate::tools::task::TaskPlanTool::new(self.hub.clone())));
        r.register(Arc::new(crate::tools::task::RequestToolsTool::default()));
        if let Some(op) = &self.operator {
            use crate::tools::computer::*;
            r.register(Arc::new(OperatorStartTool::new(op.clone())));
            r.register(Arc::new(OperatorFinishTool::new(op.clone())));
            r.register(Arc::new(ComputerObserveTool::new(op.clone())));
        }
        for t in &self.extra {
            r.register(t.clone());
        }
        Arc::new(r)
    }

    fn params(&self, url: String) -> GenerationParams {
        GenerationParams {
            resume_task: None,
            attachments: None,
            router: Arc::new(ModelRouter::new(Arc::new(AnthropicProvider::new("k".into(), Some(url)).unwrap()), "claude-opus-5-5")),
            chat_model: None,
            depth: Some(ResponseDepth::Medium),
            tooling: Tooling {
                registry: self.registry(),
                web_search_mode: None,
                orchestrator: Some(self.hub.clone()),
                policy: Policy::default(),
                approver: self.approve.clone(),
                trust: None,
                operator: self.operator.clone(),
            },
        }
    }

    async fn turn(&self, server: &MockServer, text: &str) -> (String, crate::conversations::Message) {
        self.turn_with(self.params(server.url()), None, text, &CancellationToken::new()).await
    }

    async fn turn_with(&self, p: GenerationParams, conv: Option<&str>, text: &str, cancel: &CancellationToken) -> (String, crate::conversations::Message) {
        let (c, _) = save_user_message(&self.db, conv, text, "", &p.tooling.registry.offered(None), false).unwrap();
        let m = generate(&self.db, &c.id, &p, cancel, &mut |_| {}).await.unwrap();
        (c.id, m)
    }

    fn tasks(&self, cid: &str) -> Vec<super::task::Task> {
        store::for_conversation(&self.db.conn().unwrap(), cid, 10).unwrap()
    }

    fn saw(&self, name: &str) -> bool {
        self.events.lock().unwrap().iter().any(|e| e == name)
    }
}

fn tool_names(body: &Value) -> Vec<String> {
    body["tools"].as_array().map(|a| a.iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()).unwrap_or_default()
}

/// The tool_result contents of a request, newest last.
fn tool_results(body: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for m in body["messages"].as_array().unwrap() {
        if let Some(blocks) = m["content"].as_array() {
            for b in blocks.iter().filter(|b| b["type"] == "tool_result") {
                out.push(b["content"].as_str().map(str::to_string).unwrap_or_else(|| b["content"].to_string()));
            }
        }
    }
    out
}

#[tokio::test]
async fn simple_questions_stay_lightweight_chat() {
    let env = Env::new(Approval::Denied);
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tool("t1", "calculator", json!({"expression": "12*7"}))),
        (200, "text/event-stream", sse::text("84.")),
    ])
    .await;
    let (cid, m) = env.turn(&server, "What's 12 * 7?").await;
    assert_eq!((m.status, m.content.as_str()), (MessageStatus::Complete, "84."));
    assert!(env.tasks(&cid).is_empty(), "no task for a question");
    let reqs = server.requests().await;
    assert_eq!(reqs.len(), 2, "no planning or verification calls");
    assert!(!reqs[1].body.contains("<task_state source="), "no task state for a chat turn");
    assert!(env.approve.asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_action_becomes_a_task_that_is_approved_verified_and_completed() {
    let env = Env::new(Approval::Approved);
    let path = env.path("notes.txt");
    std::fs::write(&path, "old").unwrap();
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tool("t1", "write_file", json!({"path": path, "content": "IGRIS was here."}))),
        (200, "text/event-stream", sse::text("Saved.")),
    ])
    .await;
    let project = crate::projects::add(
        &env.db.conn().unwrap(),
        &crate::projects::ProjectInput { name: "Notes".into(), path: env.dir.path().display().to_string(), ..Default::default() },
    )
    .unwrap();
    let (cid, m) = env.turn(&server, "Put 'IGRIS was here.' into notes.txt").await;
    assert_eq!(m.status, MessageStatus::Complete);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "IGRIS was here.");

    let t = &env.tasks(&cid)[0];
    assert_eq!((t.state, t.kind, t.steps, t.failures), (TaskState::Completed, TaskKind::General, 1, 0));
    assert_eq!(t.objective, "Put 'IGRIS was here.' into notes.txt");
    assert_eq!(t.project.as_ref().map(|p| p.id), Some(project.id), "the task is linked to the project it works in");
    assert_eq!(t.context.actions[0].verification, super::task::Verification::Passed);
    // The existing approval flow asked once, while the task waited for it.
    assert_eq!(*env.approve.asked.lock().unwrap(), vec![("write_file".to_string(), Some(TaskState::WaitingForApproval))]);
    // The check went through the executor (audited) and the model saw its result.
    let audit = crate::tools::audit::list(&env.db.conn().unwrap(), 10).unwrap();
    assert!(audit.iter().any(|e| e.tool == "read_file"), "verification probe is audited");
    let second = server.requests().await[1].json();
    let results = tool_results(&second);
    assert!(results[0].contains("[IGRIS check: Confirmed"), "{results:?}");
    assert!(results[0].contains("<task_state source=\"IGRIS\">"), "task state goes to the model");
    for e in ["TASK_CREATED", "TASK_TOOL_REQUESTED", "TASK_APPROVAL_REQUESTED", "TASK_VERIFICATION_STARTED", "TASK_VERIFICATION_PASSED", "TASK_COMPLETED"] {
        assert!(env.saw(e), "{e}");
    }
    // Task state never enters the stored transcript.
    let stored = conversations::messages(&env.db.conn().unwrap(), &cid).unwrap();
    assert!(!stored.iter().any(|m| m.raw.as_deref().unwrap_or_default().contains("<task_state source=")));
    assert!(!env.hub.is_live(&t.id));
}

#[tokio::test]
async fn multi_step_tasks_plan_execute_and_complete() {
    let env = Env::new(Approval::Denied);
    let (a, b) = (env.path("a.txt"), env.path("b.txt"));
    let plan = |s1: &str, s2: &str| json!({"steps": [{"title": "Create a.txt", "status": s1}, {"title": "Create b.txt", "status": s2}]});
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tool("p1", "task_plan", plan("active", "pending"))),
        (200, "text/event-stream", sse::tool("c1", "create_file", json!({"path": a, "content": "one"}))),
        (200, "text/event-stream", sse::tool("c2", "create_file", json!({"path": b, "content": "two"}))),
        (200, "text/event-stream", sse::tool("p2", "task_plan", plan("completed", "completed"))),
        (200, "text/event-stream", sse::text("Both files are there.")),
    ])
    .await;
    let (cid, m) = env.turn(&server, "Create a.txt and b.txt").await;
    assert_eq!(m.status, MessageStatus::Complete);
    let t = &env.tasks(&cid)[0];
    assert_eq!((t.state, t.steps), (TaskState::Completed, 2));
    assert!(t.plan.iter().all(|s| s.status == StepStatus::Completed));
    assert!(env.saw("TASK_PLANNING_STARTED") && env.saw("TASK_PLAN_UPDATED"));
    let reqs = server.requests().await;
    assert_eq!(reqs.len(), 5);
    let third = reqs[2].json();
    assert!(third.to_string().contains("1. [done] Create a.txt") || third.to_string().contains("[in progress] Create a.txt"), "the plan is in context");
    assert!(env.approve.asked.lock().unwrap().is_empty(), "LOW actions don't ask (existing policy)");
}

#[tokio::test]
async fn unfinished_or_unverifiable_work_is_never_called_complete() {
    let mut env = Env::new(Approval::Denied);
    let launch =
        Scripted::new("launch_application", PermissionLevel::Low, vec![Ok("Paint was launched. Its launcher exited normally, which usually means it opened.")]);
    env.extra.push(launch.clone());
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tool("l1", "launch_application", json!({"x": "Paint"}))),
        (200, "text/event-stream", sse::text("Paint is open.")),
        (
            200,
            "text/event-stream",
            sse::tool("p1", "task_plan", json!({"steps": [{"title": "Create a", "status": "active"}, {"title": "Create b", "status": "pending"}]})),
        ),
        (200, "text/event-stream", sse::tool("c1", "create_file", json!({"path": env.path("a"), "content": "a"}))),
        (200, "text/event-stream", sse::text("Done.")),
    ])
    .await;
    let (cid, _) = env.turn(&server, "Open Paint").await;
    let t = &env.tasks(&cid)[0];
    assert_eq!(t.state, TaskState::Ended, "launched, but nothing confirmed it opened");
    assert!(t.result.as_deref().unwrap().contains("couldn't be verified"));

    let (cid, _) = env.turn_with(env.params(server.url()), Some(&cid), "Create a and b", &CancellationToken::new()).await;
    let t = &env.tasks(&cid)[0];
    assert_eq!(t.state, TaskState::Ended);
    // The model never marked "Create a" done or did "Create b": not complete.
    assert!(t.result.as_deref().unwrap().contains("Stopped before finishing") && t.result.as_deref().unwrap().contains("Create b"), "{:?}", t.result);
}

#[tokio::test]
async fn a_failed_check_leads_to_recovery_and_completion_needs_a_passing_one() {
    let mut env = Env::new(Approval::Denied);
    let out = |code: u8| format!("<command_output untrusted=\"true\">\n$ cargo test\n(in /w, 1.0s, exit code {code})\n</command_output>");
    let (fail, pass) = (out(1), out(0));
    let cmd = Scripted::new("run_command", PermissionLevel::Low, vec![Ok(fail.as_str()), Ok(pass.as_str())]);
    env.extra.push(cmd.clone());
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tool("r1", "run_command", json!({"x": "cargo test"}))),
        (200, "text/event-stream", sse::tool("w1", "create_file", json!({"path": env.path("fix.rs"), "content": "fixed"}))),
        (200, "text/event-stream", sse::tool("r2", "run_command", json!({"x": "cargo test"}))),
        (200, "text/event-stream", sse::text("Tests pass now.")),
        (200, "text/event-stream", sse::tool("r3", "run_command", json!({"x": "cargo test"}))),
        (200, "text/event-stream", sse::text("Still failing, sorry.")),
    ])
    .await;
    let (cid, _) = env.turn(&server, "Fix the failing tests").await;
    let reqs = server.requests().await;
    let after_fail = tool_results(&reqs[1].json());
    assert!(
        after_fail[0].contains("[IGRIS check: The command exited with code 1.]") && after_fail[0].contains("more failed action(s) allowed"),
        "{after_fail:?}"
    );
    assert!(env.saw("TASK_VERIFICATION_FAILED") && env.saw("TASK_RECOVERING"));
    let t = &env.tasks(&cid)[0];
    assert_eq!((t.state, t.failures), (TaskState::Completed, 1), "the same command passing later resolves the failure");

    // The run fails again and the model gives up: the task fails, honestly.
    *cmd.outcomes.lock().unwrap() = vec![Ok(out_static(1))];
    let (cid, _) = env.turn_with(env.params(server.url()), Some(&cid), "Run them again", &CancellationToken::new()).await;
    let t = &env.tasks(&cid)[0];
    assert_eq!(t.state, TaskState::Failed);
    assert!(t.error.as_deref().unwrap().contains("exited with code 1"));
}

fn out_static(code: u8) -> String {
    format!("<command_output untrusted=\"true\">\n$ cargo test\n(in /w, 1.0s, exit code {code})\n</command_output>")
}

#[tokio::test]
async fn reads_are_retried_once_and_failing_actions_are_never_repeated_blindly() {
    let mut env = Env::new(Approval::Denied);
    let flaky = Scripted::new("fetch_url", PermissionLevel::Safe, vec![Err(ToolErrorKind::Timeout), Ok("page text")]);
    let folder = Scripted::new("create_folder", PermissionLevel::Low, vec![Err(ToolErrorKind::Failed)]);
    env.extra.push(flaky.clone());
    env.extra.push(folder.clone());
    let same = json!({"x": "/w/new"});
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tool("p1", "task_plan", json!({"steps": [{"title": "Read the docs", "status": "active"}]}))),
        (200, "text/event-stream", sse::tool("f1", "fetch_url", json!({"x": "https://docs"}))),
        (200, "text/event-stream", sse::tool("d1", "create_folder", same.clone())),
        (200, "text/event-stream", sse::tool("d2", "create_folder", same.clone())),
        (200, "text/event-stream", sse::tool("d3", "create_folder", same.clone())),
        (200, "text/event-stream", sse::text("I couldn't create the folder.")),
    ])
    .await;
    let (cid, m) = env.turn(&server, "Read the docs and make a folder").await;
    assert_eq!(m.status, MessageStatus::Complete);
    assert_eq!(flaky.calls(), 2, "a read that timed out is retried once, automatically");
    assert!(tool_results(&server.requests().await[2].json())[1].starts_with("page text"));
    assert!(env.saw("TASK_RETRYING"));
    assert_eq!(folder.calls(), 2, "an action is never retried automatically, and the third identical attempt is refused");
    let last = tool_results(&server.requests().await[5].json());
    assert!(last.last().unwrap().contains("already failed 2 times"), "{last:?}");
    let t = &env.tasks(&cid)[0];
    assert_eq!((t.state, t.failures), (TaskState::Failed, 2));
}

#[tokio::test]
async fn the_failure_budget_stops_the_task() {
    let mut env = Env::new(Approval::Denied);
    let folder = Scripted::new("create_folder", PermissionLevel::Low, vec![Err(ToolErrorKind::Failed)]);
    env.extra.push(folder.clone());
    let mut responses: Vec<(u16, &'static str, String)> =
        (0..6).map(|i| (200, "text/event-stream", sse::tool(&format!("d{i}"), "create_folder", json!({"x": format!("/w/{i}")})))).collect();
    responses.push((200, "text/event-stream", sse::text("Nothing worked.")));
    let server = MockServer::start(responses).await;
    let (cid, m) = env.turn(&server, "Make folders").await;
    assert_eq!(m.status, MessageStatus::Complete, "the model still gets to explain");
    assert_eq!(folder.calls(), super::recovery::MAX_TASK_FAILURES, "nothing runs after the budget is spent");
    let t = &env.tasks(&cid)[0];
    assert_eq!(t.state, TaskState::Failed);
    assert!(t.error.as_deref().unwrap().contains("actions failed"));
    assert!(tool_results(&server.requests().await[6].json()).last().unwrap().contains("The task has ended (failed"));
}

#[tokio::test]
async fn denial_stops_the_operation_and_the_task() {
    let env = Env::new(Approval::Denied);
    let path = env.path("keep.txt");
    std::fs::write(&path, "original").unwrap();
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tool("w1", "write_file", json!({"path": path, "content": "new"}))),
        (200, "text/event-stream", sse::tool("w2", "write_file", json!({"path": path, "content": "newer"}))),
        (200, "text/event-stream", sse::text("Okay, I left it alone.")),
    ])
    .await;
    let (cid, _) = env.turn(&server, "Overwrite keep.txt").await;
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    assert_eq!(env.approve.asked.lock().unwrap().len(), 1, "asked once; the second attempt was refused without running or asking");
    let t = &env.tasks(&cid)[0];
    assert_eq!(t.state, TaskState::Failed);
    assert!(t.error.as_deref().unwrap().starts_with("Not approved"));
    assert!(tool_results(&server.requests().await[2].json())[1].contains("The task has ended"));
}

#[tokio::test]
async fn cancellation_reaches_the_running_action_and_nothing_more_runs() {
    let mut env = Env::new(Approval::Denied);
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    let opener = Arc::new(Scripted {
        on_call: Some(Box::new(move |_| c2.cancel())),
        ..Arc::try_unwrap(Scripted::new("open_url", PermissionLevel::Low, vec![Ok("opened")])).ok().unwrap()
    });
    let folder = Scripted::new("create_folder", PermissionLevel::Low, vec![Ok("Created folder.")]);
    env.extra.push(opener.clone());
    env.extra.push(folder.clone());
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tools(&[("o1", "open_url", json!({"x": "https://a"})), ("d1", "create_folder", json!({"x": "/w/b"}))])),
        (200, "text/event-stream", sse::text("never requested")),
    ])
    .await;
    let (cid, m) = env.turn_with(env.params(server.url()), None, "Open a and make b", &cancel).await;
    assert_eq!(m.status, MessageStatus::Cancelled);
    assert_eq!(folder.calls(), 0, "no action starts after cancellation");
    assert_eq!(server.requests().await.len(), 1);
    assert_eq!(env.tasks(&cid)[0].state, TaskState::Cancelled);
}

#[tokio::test]
async fn pause_holds_the_task_and_resume_reobserves_before_continuing() {
    let mut env = Env::new(Approval::Approved);
    let path = env.path("draft.txt");
    std::fs::write(&path, "x").unwrap();
    let hub = env.hub.clone();
    let pauser = Arc::new(Scripted {
        on_call: Some(Box::new(move |ctx: &ToolCtx| {
            let id = ctx.task_id.clone().unwrap();
            hub.control(&id, Control::Pause).unwrap();
            let hub = hub.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(250)).await;
                hub.control(&id, Control::Resume).unwrap();
            });
        })),
        ..Arc::try_unwrap(Scripted::new("get_datetime", PermissionLevel::Safe, vec![Ok("Monday")])).ok().unwrap()
    });
    env.extra.push(pauser);
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tool("w1", "write_file", json!({"path": path, "content": "draft"}))),
        (200, "text/event-stream", sse::tool("g1", "get_datetime", json!({"x": ""}))),
        (200, "text/event-stream", sse::text("Done.")),
    ])
    .await;
    let started = Instant::now();
    let (cid, _) = env.turn(&server, "Write the draft").await;
    assert!(started.elapsed() >= Duration::from_millis(250), "the next round waited for the resume");
    let last = server.requests().await[2].json().to_string();
    assert!(last.contains("has been resumed"), "the model is told");
    assert!(last.contains("IGRIS re-checked earlier work") && last.contains("still there"), "and IGRIS looked again itself");
    assert!(env.saw("TASK_PAUSED") && env.saw("TASK_RESUMED"));
    assert_eq!(env.tasks(&cid)[0].state, TaskState::Completed);
}

#[tokio::test]
async fn after_a_restart_the_task_waits_for_the_user_and_resumes_with_fresh_tools() {
    let env = Env::new(Approval::Denied);
    // A conversation created before task_plan existed.
    let old_tools: Vec<_> = env.registry().offered(None).into_iter().filter(|d| d.name != "task_plan").collect();
    let (conv, _) = save_user_message(&env.db, None, "Create notes.txt", "", &old_tools, false).unwrap();
    // A task was running when IGRIS closed.
    let t = env.hub.begin(&conv.id, "Create notes.txt", &CancellationToken::new()).unwrap();
    env.hub.transition(&t.id, TaskState::Executing, |t| t.steps = 1).unwrap();

    let hub2 = Orchestrator::new(env.db.clone(), None);
    assert_eq!(hub2.recover().unwrap().len(), 1);
    assert_eq!(hub2.task(&t.id).unwrap().state, TaskState::Paused, "not resumed on its own");
    let env2 = Env::with(env.db.clone(), hub2, tempfile::tempdir().unwrap(), Approval::Denied, None);
    let server = MockServer::start(vec![(200, "text/event-stream", sse::text("The file isn't there yet; shall I create it?"))]).await;
    let mut p = env2.params(server.url());
    p.resume_task = Some(t.id.clone());
    let (_, m) = env2.turn_with(p, Some(&conv.id), "Continue the task: Create notes.txt", &CancellationToken::new()).await;
    assert_eq!(m.status, MessageStatus::Complete);
    let req = server.requests().await[0].json();
    assert!(tool_names(&req).contains(&"task_plan".to_string()), "the tool list was refreshed when the task resumed");
    assert!(conversations::tool_specs(&env.db.conn().unwrap(), &conv.id).unwrap().iter().any(|d| d.name == "task_plan"));
    assert!(req.to_string().contains("has been resumed (or IGRIS was restarted)"));
    let back = env2.tasks(&conv.id)[0].clone();
    assert!(back.context.resumes >= 1 && !back.context.interrupted);
    assert!(back.state.is_final() && !env2.hub.is_live(&t.id));
}

#[tokio::test]
async fn operator_tasks_run_through_the_orchestrator_and_its_tools_appear_when_needed() {
    use crate::computer::fake::{window, FakeDriver};
    let db = Arc::new(Database::open_in_memory().unwrap());
    let op = Arc::new(crate::operator::Operator::new(db.clone(), Arc::new(FakeDriver::with(vec![window(1, "Untitled - Notepad", "notepad.exe")], vec![]))));
    let hub = Orchestrator::new(db.clone(), Some(op.clone()));
    let env = Env::with(db, hub, tempfile::tempdir().unwrap(), Approval::Approved, Some(op.clone()));
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tool("s1", "operator_start", json!({"objective": "Check Notepad", "plan": ["Look at the screen"]}))),
        (200, "text/event-stream", sse::tool("o1", "computer_observe", json!({"screenshot": false, "list_windows": true, "find": "", "ocr": false}))),
        (200, "text/event-stream", sse::tool("f1", "operator_finish", json!({"outcome": "completed", "summary": "Notepad is open."}))),
        (200, "text/event-stream", sse::text("Notepad is open.")),
    ])
    .await;
    let (cid, m) = env.turn(&server, "Is Notepad open? Check on screen.").await;
    assert_eq!(m.status, MessageStatus::Complete);
    let reqs = server.requests().await;
    assert!(!tool_names(&reqs[0].json()).contains(&"computer_observe".to_string()), "hidden until operator mode runs");
    assert!(tool_names(&reqs[1].json()).contains(&"computer_observe".to_string()), "refreshed once it does");
    assert_eq!(*env.approve.asked.lock().unwrap(), vec![("operator_start".to_string(), Some(TaskState::WaitingForApproval))]);
    let tasks = env.tasks(&cid);
    assert_eq!(tasks.len(), 1, "the operator session is the task, not a second record");
    assert_eq!((tasks[0].kind, tasks[0].state), (TaskKind::Operator, TaskState::Completed));
    assert!(!op.is_active());
}

#[tokio::test]
async fn task_turns_use_the_router_budget_and_compaction() {
    let env = Env::new(Approval::Denied);
    let filler = "The project plan has milestones, owners and risks listed for every team in detail. ".repeat(10);
    let reg = env.registry();
    let (conv, _) = save_user_message(&env.db, None, &format!("q0 {filler}"), "", &reg.offered(None), false).unwrap();
    {
        let mut conn = env.db.conn().unwrap();
        for i in 0..12 {
            let role = if i % 2 == 0 { crate::ai::Role::Assistant } else { crate::ai::Role::User };
            conversations::append(
                &mut conn,
                &conv.id,
                crate::conversations::NewMessage { role, ..crate::conversations::NewMessage::user(format!("m{i} {filler}")) },
            )
            .unwrap();
        }
    }
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::text("Summary: a project plan was discussed.")),
        (200, "text/event-stream", sse::tool("c1", "create_file", json!({"path": env.path("plan.md"), "content": "# Plan"}))),
        (200, "text/event-stream", sse::text("Written.")),
    ])
    .await;
    let mut p = env.params(server.url());
    p.router = Arc::new(
        ModelRouter::new(Arc::new(AnthropicProvider::new("k".into(), Some(server.url())).unwrap()), "claude-opus-5-5")
            .with_role_model(crate::ai::ModelRole::Fast, Some("claude-haiku-4-5".into()))
            .with_context_window(Some(6_000)),
    );
    let (cid, m) = env.turn_with(p, Some(&conv.id), "Write the plan to plan.md", &CancellationToken::new()).await;
    let reqs = server.requests().await;
    assert_eq!(m.status, MessageStatus::Complete);
    let summary_calls = reqs.iter().filter(|r| r.json()["model"] == "claude-haiku-4-5").count();
    assert_eq!(summary_calls, 1, "compaction ran once, through the fast role");
    assert_eq!(reqs.len(), 3, "summary, action, answer");
    let task_req = reqs.iter().rev().find(|r| r.json()["model"] == "claude-opus-5-5").unwrap().json();
    assert!(task_req.to_string().contains("conversation_summary"), "the task works on the compacted context");
    assert!(conversations::latest_summary(&env.db.conn().unwrap(), &cid).unwrap().is_some());
    assert_eq!(env.tasks(&cid)[0].state, TaskState::Completed);
}

#[tokio::test]
async fn tasks_are_bounded_by_their_round_limit() {
    let mut env = Env::new(Approval::Denied);
    let opener = Scripted::new("open_url", PermissionLevel::Low, vec![Ok("opened")]);
    env.extra.push(opener.clone());
    let n = crate::core::chat::TASK_MAX_ROUNDS + 1;
    let responses: Vec<(u16, &'static str, String)> =
        (0..n).map(|i| (200, "text/event-stream", sse::tool(&format!("o{i}"), "open_url", json!({"x": format!("https://{i}")})))).collect();
    let server = MockServer::start(responses).await;
    let (cid, m) = env.turn(&server, "Open everything").await;
    assert_eq!(m.status, MessageStatus::Error);
    assert_eq!(opener.calls() as usize, crate::core::chat::TASK_MAX_ROUNDS);
    let t = &env.tasks(&cid)[0];
    assert_eq!(t.state, TaskState::Failed);
    assert!(t.error.as_deref().unwrap().contains("Ran out of steps"));
}

#[tokio::test]
async fn an_action_requested_before_a_pause_is_not_run_after_it() {
    let mut env = Env::new(Approval::Denied);
    let hub = env.hub.clone();
    // Runs first in the response; pauses the task and resumes it shortly after.
    let pauser = Arc::new(Scripted {
        on_call: Some(Box::new(move |ctx: &ToolCtx| {
            let id = ctx.task_id.clone().unwrap();
            hub.control(&id, Control::Pause).unwrap();
            let hub = hub.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                hub.control(&id, Control::Resume).unwrap();
            });
        })),
        ..Arc::try_unwrap(Scripted::new("get_datetime", PermissionLevel::Safe, vec![Ok("Monday")])).ok().unwrap()
    });
    let folder = Scripted::new("create_folder", PermissionLevel::Low, vec![Ok("Created folder.")]);
    env.extra.push(pauser);
    env.extra.push(folder.clone());
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tool("p1", "task_plan", json!({"steps": [{"title": "Make b", "status": "active"}]}))),
        (200, "text/event-stream", sse::tools(&[("g1", "get_datetime", json!({"x": ""})), ("d1", "create_folder", json!({"x": "/w/b"}))])),
        (200, "text/event-stream", sse::text("Paused and resumed; I'll check first.")),
    ])
    .await;
    let (_cid, _) = env.turn(&server, "Make folder b").await;
    assert_eq!(folder.calls(), 0, "the action decided before the pause was not run");
    let last = tool_results(&server.requests().await[2].json());
    assert!(last.last().unwrap().contains("paused before this action"), "{last:?}");
}

#[tokio::test]
async fn questions_get_a_focused_tool_list_and_the_model_can_ask_for_more() {
    let mut env = Env::new(Approval::Denied);
    let add = Scripted::new("add_task", PermissionLevel::Low, vec![Ok("Added.")]);
    env.extra.push(add.clone());
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tool("r1", "request_tools", json!({"groups": ["productivity"]}))),
        (200, "text/event-stream", sse::tool("a1", "add_task", json!({"x": "milk"}))),
        (200, "text/event-stream", sse::text("Added milk.")),
    ])
    .await;
    let (_cid, m) = env.turn(&server, "Hello there, could you help with something?").await;
    assert_eq!(m.status, MessageStatus::Complete);
    let reqs = server.requests().await;
    let first = tool_names(&reqs[0].json());
    assert!(!first.contains(&"add_task".to_string()) && !first.contains(&"write_file".to_string()), "focused: {first:?}");
    assert!(first.contains(&"request_tools".to_string()) && first.contains(&"calculator".to_string()));
    assert!(tool_names(&reqs[1].json()).contains(&"add_task".to_string()), "added after request_tools");
    assert_eq!(add.calls(), 1);
}

/// While a consequential computer action waits for the user, the overlay says so.
#[tokio::test]
async fn the_overlay_shows_waiting_while_an_operator_action_awaits_approval() {
    use crate::computer::fake::{element, window, FakeDriver};
    struct Watch(Arc<crate::operator::Operator>, Mutex<Vec<(crate::operator::Phase, String)>>);
    #[async_trait::async_trait]
    impl Approver for Watch {
        async fn request(&self, _a: &ToolActivity, _c: &CancellationToken) -> Approval {
            if let Some(t) = self.0.snapshot().task {
                self.1.lock().unwrap().push((t.phase, t.status));
            }
            Approval::Approved
        }
    }
    let db = Arc::new(Database::open_in_memory().unwrap());
    let driver = Arc::new(FakeDriver::with(vec![window(1, "Inbox - Mail", "mail.exe")], vec![element("Send", "button", 10, 10)]));
    let op = Arc::new(crate::operator::Operator::new(db.clone(), driver));
    let hub = Orchestrator::new(db.clone(), Some(op.clone()));
    let mut env = Env::with(db, hub, tempfile::tempdir().unwrap(), Approval::Approved, Some(op.clone()));
    env.extra.push(Arc::new(crate::tools::computer::ComputerConfirmedActionTool::new(op.clone())));
    let watch = Arc::new(Watch(op.clone(), Mutex::new(Vec::new())));
    let server = MockServer::start(vec![
        (200, "text/event-stream", sse::tool("s1", "operator_start", json!({"objective": "Send the mail", "plan": []}))),
        (200, "text/event-stream", sse::tool("o1", "computer_observe", json!({"screenshot": false, "list_windows": false, "find": "", "ocr": false}))),
        (
            200,
            "text/event-stream",
            sse::tool("c1", "computer_confirmed_action", json!({"action": "click", "element": 0, "x": -1, "y": -1, "keys": "", "effect": "Sends the email"})),
        ),
        (200, "text/event-stream", sse::text("Sent.")),
    ])
    .await;
    let mut p = env.params(server.url());
    p.tooling.approver = watch.clone();
    env.turn_with(p, None, "Send the mail", &CancellationToken::new()).await;
    let seen = watch.1.lock().unwrap().clone();
    assert!(seen.iter().any(|(phase, status)| *phase == crate::operator::Phase::Waiting && status.contains("WAITING")), "{seen:?}");
}
