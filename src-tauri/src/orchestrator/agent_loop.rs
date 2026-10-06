//! The agent loop: model → tool calls → results → model …, bounded.
//!
//! Extracted from the chat pipeline. Chat prepares the context (Phase 2:
//! routing, budget, compaction) and persists the reply; this loop runs the
//! rounds. When the turn turns out to be a task (see `intent`), a
//! [`TaskSession`] wraps every call with the task lifecycle: state, pause
//! and stop checkpoints, verification, recovery and the completion decision.
//! Every tool call still goes through the one executor (validation,
//! permission, approval, audit).
//!
//! Bounds: rounds ([`MAX_TOOL_ROUNDS`] for chat, [`TASK_MAX_ROUNDS`] for
//! tasks, [`OPERATOR_MAX_ROUNDS`] while operating the computer), the task's
//! failure budget and time limit, tool timeouts, and cancellation.

use std::sync::Arc;
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use super::session::{After, RunEnd, TaskSession};
use super::toolset::Toolset;
use super::{intent, task::Task};
use crate::ai::{AiError, AiErrorKind, ChatRequest, ChatTurn, ResponseDepth, Route, ServerToolEvent, StopReason, StreamEvent, ToolCall, ToolResult, Usage};
use crate::conversations;
use crate::core::budget::{self, estimate_turns, shorten_old_tool_results, Budget};
use crate::core::chat::{ChatEvent, Emit, Tooling};
use crate::db::Database;
use crate::tools::executor::{self, ActivityStatus, Actor, ExecContext, ToolActivity};
use crate::tools::{audit, PermissionLevel};

/// Model ↔ tool round trips for a plain chat turn.
pub const MAX_TOOL_ROUNDS: usize = 8;
/// Round trips for a task (plan → act → check → …).
pub const TASK_MAX_ROUNDS: usize = 30;
/// Round trips while an operator task runs (observe → act → verify loops).
pub const OPERATOR_MAX_ROUNDS: usize = 80;
/// Per-response output cap. Streaming keeps large values safe from HTTP timeouts.
pub const MAX_OUTPUT_TOKENS: u32 = 64_000;
/// Screenshots kept in the request during a long tool loop; older ones are dropped
/// (the model already acted on them) to bound cost.
const KEEP_RECENT_TOOL_IMAGES: usize = 2;

pub enum Outcome {
    Finished(StopReason),
    Failed(AiError),
    Cancelled,
    StepLimit,
}

/// Everything the loop needs, prepared by the chat pipeline.
pub struct LoopInput<'a> {
    pub db: &'a Arc<Database>,
    pub conversation_id: &'a str,
    /// The user's request (a new task's objective).
    pub request: String,
    pub system: String,
    pub route: Route,
    pub budget: Budget,
    pub base_turns: Vec<ChatTurn>,
    pub depth: Option<ResponseDepth>,
    pub tooling: &'a Tooling,
    pub toolset: Toolset,
    pub cancel: &'a CancellationToken,
    /// Continue this stored task (the user resumed it).
    pub resume: Option<String>,
}

pub struct LoopOutput {
    pub outcome: Outcome,
    pub text: String,
    pub new_turns: Vec<ChatTurn>,
    pub activities: Vec<ToolActivity>,
    pub usage: Usage,
    pub served_model: String,
    pub limit: usize,
    pub first_token_ms: Option<u128>,
    /// The task this turn ran, as it ended.
    pub task: Option<Task>,
}

fn exec_context<'a>(input: &'a LoopInput<'a>, allowed: &'a [String], task_id: Option<&'a str>) -> ExecContext<'a> {
    ExecContext {
        registry: &input.tooling.registry,
        db: input.db,
        conversation_id: Some(input.conversation_id),
        actor: Actor::Assistant,
        policy: input.tooling.policy,
        allowed: Some(allowed),
        approver: input.tooling.approver.as_ref(),
        cancel: input.cancel,
        trust: input.tooling.trust.as_deref(),
        operator: input.tooling.operator.as_deref(),
        task_id,
    }
}

/// Re-evaluate the conversation's tools against the registry (task start / resume).
fn refresh_tools(input: &LoopInput<'_>, toolset: &mut Toolset) {
    let current = input.tooling.registry.offered(input.tooling.web_search_mode.as_deref());
    if let Some(fresh) = super::toolset::refreshed(toolset.offered(), current) {
        if let Err(e) = input.db.conn().and_then(|c| conversations::set_tool_specs(&c, input.conversation_id, &fresh)) {
            tracing::warn!(event = "TOOLS_REFRESH_SAVE_FAILED", error = %e);
        }
        tracing::info!(event = "TOOLS_REFRESHED", before = toolset.offered().len(), after = fresh.len());
        toolset.replace(fresh);
    }
}

pub async fn run(input: LoopInput<'_>, emit: Emit<'_>) -> LoopOutput {
    let started = Instant::now();
    let mut first_token_ms: Option<u128> = None;
    let mut reasoning_chars = 0usize;
    let mut text = String::new();
    let mut new_turns: Vec<ChatTurn> = Vec::new();
    let mut activities: Vec<ToolActivity> = Vec::new();
    let mut usage = Usage::default();
    let mut served_model = input.route.model.clone();
    let mut outcome = Outcome::StepLimit;
    let mut toolset = input.toolset.clone();
    let cid = input.conversation_id;
    let hub = input.tooling.orchestrator.clone();

    let mut session: Option<TaskSession> = None;
    if let (Some(id), Some(hub)) = (&input.resume, &hub) {
        match TaskSession::resume(hub, id, cid, input.cancel) {
            Ok(s) => {
                refresh_tools(&input, &mut toolset);
                session = Some(s);
            }
            Err(e) => tracing::warn!(event = "TASK_RESUME_FAILED", error = %e),
        }
    }

    let operator_running = || input.tooling.operator.as_ref().is_some_and(|o| o.covers(Some(cid)));
    let mut limit = MAX_TOOL_ROUNDS;
    let mut round = 0usize;
    loop {
        // Checkpoint: honour pause and stop before doing anything else.
        if let Some(s) = session.as_mut() {
            let allowed = toolset.allowed();
            let id = s.id.clone();
            let ctx = exec_context(&input, &allowed, Some(&id));
            if let Err(why) = s.checkpoint(&ctx).await {
                tracing::info!(event = "TASK_LOOP_STOPPED", reason = %why);
                outcome = Outcome::Cancelled;
                break;
            }
        }
        if input.cancel.is_cancelled() {
            outcome = Outcome::Cancelled;
            break;
        }
        let operating = operator_running();
        if toolset.set_operator_session(operating) {
            tracing::info!(event = "TOOLS_EXPOSURE_CHANGED", operator = operating, exposed = toolset.exposed().len());
        }
        limit = limit.max(if operating {
            OPERATOR_MAX_ROUNDS
        } else if session.is_some() {
            TASK_MAX_ROUNDS
        } else {
            MAX_TOOL_ROUNDS
        });

        let mut turns = input.base_turns.clone();
        turns.extend(new_turns.iter().cloned());
        drop_old_tool_images(&mut turns, KEEP_RECENT_TOOL_IMAGES);
        if let Some(brief) = session.as_mut().and_then(|s| s.brief()) {
            super::brief::inject(&mut turns, &brief);
        }
        let fixed = budget::estimate_fixed(&input.system, toolset.exposed());
        fit_tool_output(&mut turns, fixed, &input.budget);
        let request = ChatRequest {
            model: input.route.model.clone(),
            system: input.system.clone(),
            turns,
            max_tokens: MAX_OUTPUT_TOKENS,
            depth: input.depth,
            tools: toolset.exposed().to_vec(),
        };

        let mut round_has_text = false;
        let mut server_running: Vec<ToolActivity> = Vec::new();
        let result = input
            .route
            .provider
            .stream(&request, input.cancel, &mut |ev| match ev {
                StreamEvent::ServerTool(ServerToolEvent::Started { id, name, input: tool_input }) => {
                    let query = tool_input["query"].as_str().or(tool_input["url"].as_str()).unwrap_or_default();
                    let a = ToolActivity {
                        id,
                        tool: name.clone(),
                        title: if name == "web_search" { "Web search".into() } else { name.clone() },
                        permission: Some(PermissionLevel::Safe),
                        description: if name == "web_search" { format!("Search the web for \"{query}\"") } else { format!("{name} {query}") },
                        status: ActivityStatus::Running,
                        result: None,
                        duration_ms: None,
                        text_offset: Some(text.chars().count()),
                        sources: Vec::new(),
                        attachments: Vec::new(),
                        failure: None,
                    };
                    emit(ChatEvent::Tool { activity: a.clone() });
                    server_running.push(a);
                }
                StreamEvent::ServerTool(ServerToolEvent::Finished { id, ok, summary, sources }) => {
                    if let Some(pos) = server_running.iter().position(|a| a.id == id) {
                        let mut a = server_running.remove(pos);
                        a.status = if ok { ActivityStatus::Completed } else { ActivityStatus::Failed };
                        a.result = Some(summary);
                        a.sources = sources;
                        if let Ok(conn) = input.db.conn() {
                            let _ = audit::record(
                                &conn,
                                &audit::NewAuditEntry {
                                    conversation_id: Some(cid),
                                    tool: &a.tool,
                                    permission: "safe",
                                    actor: "assistant",
                                    description: &a.description,
                                    input: "",
                                    status: if ok { "completed" } else { "failed" },
                                    approval: "auto",
                                    result: a.result.as_deref(),
                                    duration_ms: None,
                                },
                            );
                        }
                        emit(ChatEvent::Tool { activity: a.clone() });
                        activities.push(a);
                    }
                }
                StreamEvent::Reasoning(n) => {
                    reasoning_chars += n;
                    emit(ChatEvent::Reasoning { chars: reasoning_chars });
                }
                StreamEvent::TextDelta(t) => {
                    if first_token_ms.is_none() {
                        first_token_ms = Some(started.elapsed().as_millis());
                    }
                    // Separate text written before and after tool calls.
                    if !round_has_text && round > 0 && !text.is_empty() && !text.ends_with("\n\n") {
                        text.push_str("\n\n");
                        emit(ChatEvent::Delta { text: "\n\n".into() });
                    }
                    round_has_text = true;
                    text.push_str(&t);
                    emit(ChatEvent::Delta { text: t });
                }
            })
            .await;

        // Server tools that never reported a result in this round.
        for mut a in server_running.drain(..) {
            a.status = ActivityStatus::Cancelled;
            emit(ChatEvent::Tool { activity: a.clone() });
            activities.push(a);
        }
        let completion = match result {
            Ok(c) => c,
            Err(e) if e.kind == AiErrorKind::Cancelled => {
                outcome = Outcome::Cancelled;
                break;
            }
            Err(e) => {
                outcome = Outcome::Failed(e);
                break;
            }
        };
        usage.input_tokens = Some(usage.input_tokens.unwrap_or(0) + completion.usage.input_tokens.unwrap_or(0));
        usage.output_tokens = Some(usage.output_tokens.unwrap_or(0) + completion.usage.output_tokens.unwrap_or(0));
        served_model = completion.model.clone();

        // Only act on tool calls when the model actually stopped to use them —
        // a refusal or max_tokens stop can leave a call cut off mid-input.
        let wants_tools = completion.stop_reason == StopReason::ToolUse && !completion.tool_calls.is_empty();
        new_turns.push(ChatTurn {
            extras: completion.extras.clone(),
            tool_calls: completion.tool_calls.clone(),
            ..ChatTurn::assistant(completion.text.clone())
        });
        if completion.stop_reason == StopReason::PauseTurn {
            // A provider-side tool loop paused: re-send as-is and it resumes.
            if round >= limit {
                break; // StepLimit
            }
            round += 1;
            continue;
        }
        if !wants_tools {
            outcome = Outcome::Finished(completion.stop_reason);
            break;
        }
        if round >= limit {
            break; // StepLimit
        }

        let mut results = Vec::new();
        let offset = Some(text.chars().count());
        for call in &completion.tool_calls {
            // Chat or task: the first action (or a declared plan) makes this turn a task.
            if session.is_none() {
                if let (Some(trigger), Some(hub)) = (intent::trigger(&call.name), &hub) {
                    match TaskSession::begin(hub, cid, &input.request, trigger, input.cancel) {
                        Ok(s) => {
                            refresh_tools(&input, &mut toolset);
                            session = Some(s);
                        }
                        Err(e) => tracing::warn!(event = "TASK_START_FAILED", error = %e),
                    }
                }
            }
            let (result, activity) = run_call(&input, &toolset, session.as_mut(), call, offset, &mut *emit).await;
            emit(ChatEvent::Tool { activity: activity.clone() });
            activities.push(activity);
            results.push(result);
        }
        new_turns.push(ChatTurn { tool_results: results, ..ChatTurn::user("") });
        if input.cancel.is_cancelled() {
            outcome = Outcome::Cancelled;
            break;
        }
        round += 1;
    }

    let task = session.map(|s| {
        let end = match &outcome {
            Outcome::Finished(_) => RunEnd::Answered,
            Outcome::Cancelled => RunEnd::Cancelled,
            Outcome::Failed(e) => RunEnd::Error(e.message.clone()),
            Outcome::StepLimit => RunEnd::StepLimit(limit),
        };
        s.finish(&end)
    });
    LoopOutput { outcome, text, new_turns, activities, usage, served_model, limit, first_token_ms, task: task.flatten() }
}

/// One tool call: refused, or run through the executor (once more if the
/// orchestrator retries a read), with the task lifecycle around it.
async fn run_call(
    input: &LoopInput<'_>,
    toolset: &Toolset,
    mut session: Option<&mut TaskSession>,
    call: &ToolCall,
    offset: Option<usize>,
    emit: Emit<'_>,
) -> (ToolResult, ToolActivity) {
    if let Some(why) = session.as_deref().and_then(|s| s.refuse(call, input.cancel)) {
        tracing::info!(event = "TASK_CALL_REFUSED", tool = %call.name);
        let activity = ToolActivity {
            id: call.id.clone(),
            tool: call.name.clone(),
            title: call.name.clone(),
            permission: input.tooling.registry.get(&call.name).map(|t| t.spec().permission),
            description: format!("{} (not run)", call.name),
            status: if input.cancel.is_cancelled() { ActivityStatus::Cancelled } else { ActivityStatus::Failed },
            result: Some("Not run".into()),
            duration_ms: Some(0),
            text_offset: offset,
            sources: Vec::new(),
            attachments: Vec::new(),
            failure: None,
        };
        return (ToolResult { call_id: call.id.clone(), content: why, is_error: true, media: Vec::new() }, activity);
    }
    let allowed = toolset.allowed();
    let task_id = session.as_deref().map(|s| s.id.clone());
    let ctx = exec_context(input, &allowed, task_id.as_deref());
    if let Some(s) = session.as_deref_mut() {
        // A pause holds actions the model already asked for, too; after a
        // resume the task re-checks before acting.
        if super::toolset::capability(&call.name).is_some_and(|c| c.acts() || c == super::toolset::Capability::OperatorSession) {
            let held = match s.checkpoint(&ctx).await {
                Ok(false) => None,
                // Decided before the pause: the computer may have changed since.
                Ok(true) => Some(
                    "IGRIS was paused before this action, so it wasn't run. The task has been resumed: check the current state and decide again.".to_string(),
                ),
                Err(why) => Some(why),
            };
            if let Some(why) = held {
                let activity = ToolActivity {
                    id: call.id.clone(),
                    tool: call.name.clone(),
                    title: call.name.clone(),
                    permission: input.tooling.registry.get(&call.name).map(|t| t.spec().permission),
                    description: format!("{} (not run)", call.name),
                    status: ActivityStatus::Cancelled,
                    result: Some("Not run".into()),
                    duration_ms: Some(0),
                    text_offset: offset,
                    sources: Vec::new(),
                    attachments: Vec::new(),
                    failure: None,
                };
                return (ToolResult { call_id: call.id.clone(), content: why, is_error: true, media: Vec::new() }, activity);
            }
        }
        s.before_call(call);
    }
    let mut attempt = 0u32;
    loop {
        let (mut result, mut activity) = executor::execute(call, &ctx, &mut |a| {
            if let Some(s) = session.as_deref() {
                s.on_activity(a);
            }
            emit(ChatEvent::Tool { activity: ToolActivity { text_offset: offset, ..a.clone() } })
        })
        .await;
        activity.text_offset = offset;
        let Some(s) = session.as_deref_mut() else { return (result, activity) };
        if call.name == "task_plan" && activity.status == ActivityStatus::Completed {
            s.plan_changed();
        }
        match s.after_call(call, &mut result, &activity, attempt, &ctx).await {
            After::RetryNow if attempt == 0 && !input.cancel.is_cancelled() => {
                attempt += 1;
                tracing::info!(event = "TASK_RETRY", tool = %call.name);
                continue;
            }
            _ => return (result, activity),
        }
    }
}

/// Long tool loops (operator tasks, build-fix cycles) grow the request every
/// round. Over budget, older tool output in this request is shortened — more
/// aggressively if needed — while calls, the newest results and the
/// conversation stay intact. Only the request copy changes; nothing stored does.
pub fn fit_tool_output(turns: &mut [ChatTurn], fixed: u32, budget: &Budget) {
    for (keep, max_chars) in [(6, 1_200), (3, 400), (1, 160)] {
        if fixed + estimate_turns(turns) <= budget.input_limit {
            return;
        }
        shorten_old_tool_results(turns, keep, max_chars);
    }
    let size = fixed + estimate_turns(turns);
    if size > budget.input_limit {
        // Still too big (e.g. one huge message): send it and let the provider
        // report a real context error rather than silently dropping content.
        tracing::warn!(event = "CONTEXT_OVER_BUDGET", estimated = size, limit = budget.input_limit);
    }
}

/// Keep only the `keep` most recent tool results that carry images; older
/// ones get a short note instead (the model has already acted on them).
fn drop_old_tool_images(turns: &mut [ChatTurn], keep: usize) {
    let mut seen = 0;
    for t in turns.iter_mut().rev() {
        for r in t.tool_results.iter_mut().rev() {
            if r.media.is_empty() {
                continue;
            }
            seen += 1;
            if seen > keep && r.media.iter().all(|m| m.attachment_id.is_empty()) {
                r.media.clear();
                r.content.push_str("\n[Older screenshot omitted.]");
            }
        }
    }
}
