//! One assistant message: save the user's message → prepare the context →
//! run the agent loop → persist the reply.
//!
//! Chat owns the conversation: transcripts, the Phase 2 context preparation
//! (model routing, budget, compaction) and persistence. The rounds — model,
//! tool calls through the executor, and the task lifecycle when a request is
//! actionable — run in the orchestrator's agent loop
//! (`orchestrator/agent_loop.rs`). Every outcome is persisted with a `status`
//! (complete, truncated, refused, cancelled, error), so the UI never has to
//! guess and a failure is never reported as success.

use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::budget::{self, estimate_text, estimate_turns, Budget};
use super::compaction::{self, Group};
use super::context::encode_turns;
use super::prompt::{self, PromptContext};
use crate::ai::{AiErrorKind, ChatTurn, MediaKind, ModelRole, ModelRouter, Needs, ResponseDepth, Role, StopReason, ToolDef};
use crate::attachments::AttachmentStore;
use crate::conversations::StoredSummary;
use crate::conversations::{self, Conversation, Message, MessageStatus, NewMessage};
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::memory::retrieval::{self, MemoryContext};
use crate::orchestrator::agent_loop::{self, LoopInput, LoopOutput, Outcome};
use crate::orchestrator::toolset::Toolset;
use crate::tools::executor::{self, Approver, Policy, ToolActivity};
use crate::tools::ToolRegistry;

pub const MAX_INPUT_CHARS: usize = 100_000;
pub use crate::orchestrator::agent_loop::{MAX_OUTPUT_TOKENS, MAX_TOOL_ROUNDS, OPERATOR_MAX_ROUNDS, TASK_MAX_ROUNDS};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ChatEvent {
    /// The user's message was saved (and the conversation created if new).
    UserMessage { conversation: Conversation, message: Message },
    /// The provider request is starting.
    Generating { conversation_id: String, model: String },
    /// Older messages are being summarized to fit the model's context (`messages` of them).
    Compacting { messages: usize },
    /// Streamed response text.
    Delta { text: String },
    /// The model is reasoning before it answers; `chars` is the total so far (text not shown).
    Reasoning { chars: usize },
    /// A tool call started, needs approval, or finished (upsert by `activity.id`).
    Tool { activity: ToolActivity },
    /// The assistant turn ended; `message.status` says how.
    Finished { message: Message },
}

pub type Emit<'a> = &'a mut (dyn FnMut(ChatEvent) + Send);

pub struct Tooling {
    pub registry: Arc<ToolRegistry>,
    /// Web search mode for re-evaluating a conversation's tools (see `ToolRegistry::offered`).
    pub web_search_mode: Option<String>,
    /// The task orchestrator (actionable requests become tasks). `None` = plain tool loop.
    pub orchestrator: Option<Arc<crate::orchestrator::Orchestrator>>,
    pub policy: Policy,
    pub approver: Arc<dyn Approver>,
    /// Conversations trusted with "Allow for this chat".
    pub trust: Option<Arc<executor::Trust>>,
    /// Operator mode (computer control) for this app.
    pub operator: Option<Arc<crate::operator::Operator>>,
}

pub struct GenerationParams {
    /// Picks the model per request (chat, vision for images, fast for compaction).
    pub router: Arc<ModelRouter>,
    /// The user's model choice from Settings (applies to the chat role).
    pub chat_model: Option<String>,
    pub depth: Option<ResponseDepth>,
    pub tooling: Tooling,
    /// Where attachment bytes are loaded from (None in tests without files).
    pub attachments: Option<Arc<AttachmentStore>>,
    /// Continue this stored task (the user resumed it) instead of starting fresh.
    pub resume_task: Option<String>,
}

pub fn validate_input(content: &str) -> AppResult<String> {
    validate_input_with(content, false)
}

/// Like [`validate_input`], but an empty message is fine when files are attached.
pub fn validate_input_with(content: &str, has_attachments: bool) -> AppResult<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() && !has_attachments {
        return Err(AppError::validation("Message is empty."));
    }
    if trimmed.chars().count() > MAX_INPUT_CHARS {
        return Err(AppError::validation(format!("Message is too long (max {MAX_INPUT_CHARS} characters).")));
    }
    Ok(trimmed.to_string())
}

/// Memories to attach to a new user message, given earlier messages.
fn memory_for(conn: &rusqlite::Connection, content: &str, earlier: &[Message], enabled: bool) -> AppResult<Option<MemoryContext>> {
    if !enabled {
        return Ok(None);
    }
    let previous: Vec<MemoryContext> = earlier.iter().filter_map(|m| m.memory_context.clone()).collect();
    let ctx = retrieval::select(conn, content, &previous)?;
    if let Some(c) = &ctx {
        tracing::info!(event = "MEMORY_ATTACHED", count = c.items.len());
    }
    Ok(ctx)
}

/// Save a user message, creating the conversation (with its frozen system
/// prompt and tool set) when `conversation_id` is `None`. Relevant memories
/// are attached when `memory_enabled`.
pub fn save_user_message(
    db: &Database,
    conversation_id: Option<&str>,
    content: &str,
    user_name: &str,
    tools: &[ToolDef],
    memory_enabled: bool,
) -> AppResult<(Conversation, Message)> {
    save_user_message_with(db, conversation_id, content, &[], user_name, tools, memory_enabled)
}

/// [`save_user_message`] with staged attachments, linked to the new message.
pub fn save_user_message_with(
    db: &Database,
    conversation_id: Option<&str>,
    content: &str,
    attachment_ids: &[String],
    user_name: &str,
    tools: &[ToolDef],
    memory_enabled: bool,
) -> AppResult<(Conversation, Message)> {
    let mut conn = db.conn()?;
    crate::attachments::check_staged(&conn, attachment_ids)?;
    let conversation = match conversation_id {
        Some(id) => conversations::require(&conn, id)?,
        None => {
            let system = prompt::system_prompt(&PromptContext { user_name, date: &prompt::today(), os: prompt::os_label() });
            let title_source = match (content.trim().is_empty(), attachment_ids.first()) {
                (true, Some(id)) => crate::attachments::get(&conn, id)?.map(|a| a.name).unwrap_or_default(),
                _ => content.to_string(),
            };
            let c = conversations::create(&conn, &conversations::title_from(&title_source), &system, tools)?;
            tracing::info!(event = "CONVERSATION_CREATED", conversation_id = %c.id, tools = tools.len());
            c
        }
    };
    let earlier = conversations::messages(&conn, &conversation.id)?;
    let memory_context = memory_for(&conn, content, &earlier, memory_enabled)?;
    let message = conversations::append(&mut conn, &conversation.id, NewMessage { memory_context, ..NewMessage::user(content) })?;
    let message = if attachment_ids.is_empty() {
        message
    } else {
        let attachments = crate::attachments::link_to_message(&conn, attachment_ids, &conversation.id, &message.id)?;
        tracing::info!(event = "ATTACHMENTS_SENT", count = attachments.len());
        Message { attachments, ..message }
    };
    let conversation = conversations::require(&conn, &conversation.id)?;
    Ok((conversation, message))
}

/// Replace a user message's text, re-select its memories and drop everything after it.
pub fn edit_user_message(db: &Database, message_id: &str, content: &str, memory_enabled: bool) -> AppResult<Message> {
    let mut conn = db.conn()?;
    let msg = conversations::get_message(&conn, message_id)?.ok_or_else(|| AppError::validation("That message no longer exists."))?;
    let earlier: Vec<Message> = conversations::messages(&conn, &msg.conversation_id)?.into_iter().filter(|m| m.seq < msg.seq).collect();
    let memory_context = memory_for(&conn, content, &earlier, memory_enabled)?;
    conversations::edit_user_message(&mut conn, message_id, content, memory_context.as_ref())
}

/// Generate and persist the assistant reply to the conversation's last user message.
pub async fn generate(db: &Arc<Database>, conversation_id: &str, params: &GenerationParams, cancel: &CancellationToken, emit: Emit<'_>) -> AppResult<Message> {
    let (system, history, offered) = {
        let conn = db.conn()?;
        (
            conversations::system_prompt(&conn, conversation_id)?,
            conversations::messages(&conn, conversation_id)?,
            conversations::tool_specs(&conn, conversation_id)?,
        )
    };
    if history.last().map(|m| m.role) != Some(Role::User) {
        return Err(AppError::validation("There is no message to respond to."));
    }

    // Intelligence interface: pick the model, then fit the context to it.
    let chat_override = params.chat_model.as_deref();
    let mut summary = {
        let conn = db.conn()?;
        conversations::latest_summary(&conn, conversation_id)?
    };
    let mut groups: Vec<Group> = history.iter().filter(|m| summary.as_ref().map(|s| m.seq > s.through_seq).unwrap_or(true)).map(Group::from_message).collect();
    if !groups.iter().any(|g| g.role == Role::User) {
        // A summary that leaves no request (shouldn't happen): use the full history.
        summary = None;
        groups = history.iter().map(Group::from_message).collect();
    }
    let has_image = |t: &ChatTurn| t.media.iter().chain(t.tool_results.iter().flat_map(|r| &r.media)).any(|m| m.kind == MediaKind::Image);
    let needs = Needs { vision: groups.iter().flat_map(|g| &g.turns).any(has_image), tools: !offered.is_empty() };
    let route = params.router.route_for(ModelRole::Chat, needs, chat_override).unwrap_or_else(|why| {
        // Keep today's behaviour: the chat model (its adapter reports images it can't read).
        tracing::warn!(event = "MODEL_ROUTE_FALLBACK", reason = %why);
        params.router.route(ModelRole::Chat, chat_override)
    });
    let budget = Budget::for_model(&route.caps, MAX_OUTPUT_TOKENS);
    let fixed = budget::estimate_fixed(&system, &offered);
    let prefix_tokens = summary.as_ref().map(|s| estimate_text(&compaction::render_prefix(s))).unwrap_or(0);
    if let Some(split) = compaction::plan_split(&groups, fixed, prefix_tokens, &budget) {
        emit(ChatEvent::Compacting { messages: split });
        let fast = params.router.route(ModelRole::Fast, chat_override);
        let previous = summary.as_ref().map(|s| s.summary.clone());
        let memory = compaction::merge_memory(summary.as_ref().map(|s| s.memory.as_slice()).unwrap_or(&[]), &groups[..split]);
        let through_seq = groups[split - 1].seq;
        match compaction::summarize(&fast, previous.as_deref(), &groups[..split], cancel).await {
            Ok(text) => {
                let s = StoredSummary { through_seq, summary: text, memory };
                if let Err(e) = db.conn().and_then(|c| conversations::save_summary(&c, conversation_id, &s, fast.provider_id(), &fast.model)) {
                    tracing::warn!(event = "CONTEXT_SUMMARY_SAVE_FAILED", error = %e);
                }
                tracing::info!(event = "CONTEXT_COMPACTED", messages = split, through_seq, model = %fast.model);
                summary = Some(s);
                groups.drain(..split);
            }
            Err(e) if e.kind == AiErrorKind::Cancelled => {} // the request below ends as cancelled
            Err(e) => {
                // Not stored: the next request tries again. The conversation is untouched.
                tracing::warn!(event = "CONTEXT_COMPACTION_FAILED", kind = ?e.kind);
                let digest = compaction::fallback_digest(previous.as_deref(), &groups[..split]);
                summary = Some(StoredSummary { through_seq, summary: digest, memory });
                groups.drain(..split);
            }
        }
    }
    let mut base_turns = compaction::assemble(summary.as_ref(), &groups);
    if let Some(store) = &params.attachments {
        let conn = db.conn()?;
        for t in &mut base_turns {
            for m in t.media.iter_mut().chain(t.tool_results.iter_mut().flat_map(|r| r.media.iter_mut())) {
                store.hydrate(&conn, m);
            }
        }
    }
    let provider_id = route.provider_id();
    let toolset = Toolset::new(offered);

    emit(ChatEvent::Generating { conversation_id: conversation_id.to_string(), model: route.model.clone() });
    tracing::info!(
        event = "CHAT_REQUEST_STARTED",
        provider = provider_id,
        model = %route.model,
        role = ?route.role,
        turns = base_turns.len(),
        tools = toolset.exposed().len(),
        context_tokens = budget::estimate_fixed(&system, toolset.exposed()) + estimate_turns(&base_turns),
        input_limit = budget.input_limit,
    );

    // The agent loop (and, for actionable requests, the task orchestrator) runs the rounds.
    let started = Instant::now();
    let run = agent_loop::run(
        LoopInput {
            db,
            conversation_id,
            request: history.last().map(|m| m.content.clone()).unwrap_or_default(),
            system,
            route,
            budget,
            base_turns,
            depth: params.depth,
            tooling: &params.tooling,
            toolset,
            cancel,
            resume: params.resume_task.clone(),
        },
        &mut *emit,
    )
    .await;
    let LoopOutput { outcome, text, new_turns, activities, usage, served_model, limit, first_token_ms, task } = run;

    // Raw turns are only replayable when every tool call in them was answered.
    let answered = new_turns.last().is_some_and(|t| t.tool_calls.is_empty());
    let base = NewMessage {
        role: Role::Assistant,
        provider: Some(provider_id.to_string()),
        model: Some(served_model.clone()),
        content: text.clone(),
        input_tokens: usage.input_tokens.map(|v| v as i64),
        output_tokens: usage.output_tokens.map(|v| v as i64),
        tool_activity: (!activities.is_empty()).then(|| serde_json::to_string(&activities).unwrap_or_default()),
        ..NewMessage::user("")
    };
    let new_message = match outcome {
        Outcome::Finished(stop) => {
            let (status, error) = match &stop {
                StopReason::EndTurn | StopReason::ToolUse | StopReason::PauseTurn | StopReason::Other { .. } => (MessageStatus::Complete, None),
                StopReason::MaxTokens => (MessageStatus::Truncated, Some("The response hit the output length limit.".to_string())),
                StopReason::Refusal { category } => (
                    MessageStatus::Refused,
                    Some(match category {
                        Some(c) => format!("The model declined this request (category: {c})."),
                        None => "The model declined this request.".to_string(),
                    }),
                ),
            };
            tracing::info!(
                event = "CHAT_COMPLETED",
                status = ?status,
                served_model = %served_model,
                tool_calls = activities.len(),
                task = task.as_ref().map(|t| t.state.as_str()),
                first_token_ms = first_token_ms.map(|v| v as u64),
                total_ms = started.elapsed().as_millis() as u64,
                input_tokens = usage.input_tokens,
                output_tokens = usage.output_tokens,
            );
            let keep_raw = matches!(status, MessageStatus::Complete | MessageStatus::Truncated) && answered;
            NewMessage { status, error, raw: keep_raw.then(|| encode_turns(&new_turns).to_string()), ..base }
        }
        Outcome::Cancelled => {
            tracing::info!(event = "CHAT_CANCELLED", partial_chars = text.chars().count(), tool_calls = activities.len());
            NewMessage { status: MessageStatus::Cancelled, ..base }
        }
        Outcome::Failed(e) => {
            tracing::warn!(event = "CHAT_FAILED", kind = ?e.kind, provider = provider_id);
            NewMessage { status: MessageStatus::Error, error: Some(e.message), ..base }
        }
        Outcome::StepLimit => {
            tracing::warn!(event = "CHAT_TOOL_STEP_LIMIT", rounds = limit);
            NewMessage { status: MessageStatus::Error, error: Some(format!("Stopped after {limit} tool steps without a final answer.")), ..base }
        }
    };

    let message = {
        let mut conn = db.conn()?;
        conversations::append(&mut conn, conversation_id, new_message)?
    };
    emit(ChatEvent::Finished { message: message.clone() });
    Ok(message)
}

/// Prepare a regeneration: drop assistant turns after the last user message.
pub fn prepare_regenerate(db: &Database, conversation_id: &str) -> AppResult<()> {
    let conn = db.conn()?;
    let history = conversations::messages(&conn, conversation_id)?;
    let last_user = history.iter().rev().find(|m| m.role == Role::User).ok_or_else(|| AppError::validation("There is nothing to regenerate."))?;
    conversations::truncate_after(&conn, conversation_id, last_user.seq)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::anthropic::AnthropicProvider;
    use crate::ai::testutil::MockServer;
    use crate::conversations::NewMessage;
    use crate::tools::calculator::CalculatorTool;
    use crate::tools::executor::{ActivityStatus, Approval};
    use crate::tools::{PermissionLevel, Tool, ToolOutput, ToolResultT, ToolSpec};
    use serde_json::json;
    use std::sync::Mutex;

    struct FixedApprover(Approval);

    #[async_trait::async_trait]
    impl Approver for FixedApprover {
        async fn request(&self, _a: &ToolActivity, _c: &CancellationToken) -> Approval {
            self.0
        }
    }

    /// A LOW-risk tool that records whether it ran.
    struct Opener(ToolSpec, Mutex<u32>);

    #[async_trait::async_trait]
    impl Tool for Opener {
        fn spec(&self) -> &ToolSpec {
            &self.0
        }
        fn describe(&self, _i: &serde_json::Value) -> String {
            "Open thing".into()
        }
        async fn execute(&self, _i: &serde_json::Value) -> ToolResultT {
            *self.1.lock().unwrap() += 1;
            Ok(ToolOutput { content: "opened".into(), summary: "opened".into(), sources: vec![], media: Vec::new() })
        }
    }

    fn events(evs: &[serde_json::Value]) -> String {
        evs.iter().map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap())).collect()
    }

    fn sse_reply(text: &str) -> String {
        events(&[
            json!({"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":10}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":text}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}),
            json!({"type":"message_stop"}),
        ])
    }

    fn sse_tool_use(id: &str, name: &str, input: serde_json::Value) -> String {
        events(&[
            json!({"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":10}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Let me check."}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":id,"name":name,"input":{}}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":input.to_string()}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":5}}),
            json!({"type":"message_stop"}),
        ])
    }

    fn registry_with(extra: Option<Arc<dyn Tool>>) -> Arc<ToolRegistry> {
        let mut r = ToolRegistry::default();
        r.register(Arc::new(CalculatorTool::default()));
        if let Some(t) = extra {
            r.register(t);
        }
        Arc::new(r)
    }

    fn params(url: String, registry: Arc<ToolRegistry>, policy: Policy, approval: Approval) -> GenerationParams {
        GenerationParams {
            resume_task: None,
            attachments: None,
            router: Arc::new(ModelRouter::new(Arc::new(AnthropicProvider::new("k".into(), Some(url)).unwrap()), "claude-opus-5-5")),
            chat_model: None,
            depth: Some(ResponseDepth::Medium),
            tooling: Tooling {
                registry,
                web_search_mode: None,
                orchestrator: None,
                policy,
                approver: Arc::new(FixedApprover(approval)),
                trust: None,
                operator: None,
            },
        }
    }

    fn db() -> Arc<Database> {
        Arc::new(Database::open_in_memory().unwrap())
    }

    // --- Intelligence interface: budgeting, compaction, provider-neutral history ---

    fn long_conversation(db: &Arc<Database>, reg: &Arc<ToolRegistry>, exchanges: usize) -> String {
        let filler = "The itinerary needs flights, a hotel near the beach and a budget under thirty thousand rupees. ".repeat(8);
        let (conv, _) = save_user_message(db, None, &format!("question 0: plan a trip to Goa. {filler}"), "Ada", &reg.defs(), false).unwrap();
        let mut conn = db.conn().unwrap();
        conversations::append(
            &mut conn,
            &conv.id,
            NewMessage { role: Role::Assistant, provider: Some("anthropic".into()), ..NewMessage::user(format!("answer 0. {filler}")) },
        )
        .unwrap();
        for i in 1..exchanges {
            conversations::append(&mut conn, &conv.id, NewMessage::user(format!("question {i}. {filler}"))).unwrap();
            conversations::append(
                &mut conn,
                &conv.id,
                NewMessage { role: Role::Assistant, provider: Some("anthropic".into()), ..NewMessage::user(format!("answer {i}. {filler}")) },
            )
            .unwrap();
        }
        drop(conn);
        save_user_message(db, Some(&conv.id), "What did we decide?", "Ada", &reg.defs(), false).unwrap();
        conv.id
    }

    fn small_window(url: String, reg: Arc<ToolRegistry>) -> GenerationParams {
        let mut p = params(url.clone(), reg, Policy::default(), Approval::Approved);
        p.router =
            Arc::new(ModelRouter::new(Arc::new(AnthropicProvider::new("k".into(), Some(url)).unwrap()), "claude-opus-5-5").with_context_window(Some(8_000)));
        p
    }

    #[tokio::test]
    async fn short_conversations_send_everything_without_extra_calls() {
        let db = db();
        let server = MockServer::start(vec![(200, "text/event-stream", sse_reply("Hi."))]).await;
        let reg = registry_with(None);
        let (conv, _) = save_user_message(&db, None, "Hello", "Ada", &reg.defs(), false).unwrap();
        let mut evs = Vec::new();
        generate(&db, &conv.id, &small_window(server.url(), reg), &CancellationToken::new(), &mut |e| evs.push(e)).await.unwrap();
        assert_eq!(server.requests().await.len(), 1, "no summary call");
        assert!(!evs.iter().any(|e| matches!(e, ChatEvent::Compacting { .. })));
        assert!(conversations::latest_summary(&db.conn().unwrap(), &conv.id).unwrap().is_none());
    }

    #[tokio::test]
    async fn long_conversations_are_compacted_once_and_the_summary_is_reused() {
        let db = db();
        let notes = "- Goal: plan a trip to Goa under 30k rupees\n- Decided: beach hotel";
        // The old part is larger than one summarizer call may carry, so it is summarized in two chunks.
        let server = MockServer::start(vec![
            (200, "text/event-stream", sse_reply("- Goal: plan a trip to Goa")),
            (200, "text/event-stream", sse_reply(notes)),
            (200, "text/event-stream", sse_reply("We chose a beach hotel.")),
            (200, "text/event-stream", sse_reply("Flights next.")),
        ])
        .await;
        let reg = registry_with(None);
        let conv = long_conversation(&db, &reg, 14);
        let before = conversations::messages(&db.conn().unwrap(), &conv).unwrap().len();
        let p = small_window(server.url(), reg.clone());
        let mut evs = Vec::new();
        let m = generate(&db, &conv, &p, &CancellationToken::new(), &mut |e| evs.push(e)).await.unwrap();
        assert_eq!(m.content, "We chose a beach hotel.");
        assert!(evs.iter().any(|e| matches!(e, ChatEvent::Compacting { messages } if *messages > 0)));

        let reqs = server.requests().await;
        let (summary_req, merge_req, chat_req) = (reqs[0].json(), reqs[1].json(), reqs[2].json());
        assert!(summary_req["system"].as_str().unwrap().starts_with("You condense"));
        assert!(summary_req["messages"][0]["content"].as_str().unwrap().contains("question 0: plan a trip to Goa"));
        assert!(summary_req.get("tools").is_none(), "the summarizer gets no tools");
        assert!(merge_req["messages"][0]["content"].as_str().unwrap().starts_with("Existing notes"), "chunks build on the notes so far");

        let sent = chat_req["messages"].to_string();
        assert!(sent.contains("<conversation_summary>") && sent.contains("plan a trip to Goa under 30k"));
        assert!(!sent.contains("question 0:"), "old messages are replaced by the summary");
        assert!(sent.contains("What did we decide?"), "the latest request is always sent");
        let size = budget::estimate_fixed(chat_req["system"].as_str().unwrap(), &reg.defs()) + estimate_text(&sent);
        assert!(size <= Budget::for_model(&p.router.route(ModelRole::Chat, None).caps, MAX_OUTPUT_TOKENS).input_limit, "{size}");

        // Stored provider-neutrally; nothing deleted.
        {
            let conn = db.conn().unwrap();
            let stored = conversations::latest_summary(&conn, &conv).unwrap().unwrap();
            assert!(stored.summary.contains("beach hotel"));
            assert_eq!(conversations::messages(&conn, &conv).unwrap().len(), before + 1);
        }

        // The next message reuses the summary: one request, no new compaction.
        save_user_message(&db, Some(&conv), "And flights?", "Ada", &reg.defs(), false).unwrap();
        generate(&db, &conv, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        let reqs = server.requests().await;
        assert_eq!(reqs.len(), 4);
        let third = reqs[3].json()["messages"].to_string();
        assert!(third.contains("<conversation_summary>") && third.contains("And flights?") && !third.contains("question 0:"));
    }

    #[tokio::test]
    async fn failed_compaction_falls_back_without_losing_anything() {
        let db = db();
        let err = json!({"type":"error","error":{"type":"invalid_request_error","message":"summary refused"}}).to_string();
        let server = MockServer::start(vec![(400, "application/json", err), (200, "text/event-stream", sse_reply("Answer."))]).await;
        let reg = registry_with(None);
        let conv = long_conversation(&db, &reg, 14);
        let before = conversations::messages(&db.conn().unwrap(), &conv).unwrap().len();
        let m = generate(&db, &conv, &small_window(server.url(), reg), &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!((m.status, m.content.as_str()), (MessageStatus::Complete, "Answer."));
        let sent = server.requests().await[1].json()["messages"].to_string();
        assert!(sent.contains("A full summary couldn't be made") && sent.contains("question 0: plan a trip"), "extractive digest stands in");
        let conn = db.conn().unwrap();
        assert!(conversations::latest_summary(&conn, &conv).unwrap().is_none(), "a fallback is never stored");
        assert_eq!(conversations::messages(&conn, &conv).unwrap().len(), before + 1);
    }

    #[tokio::test]
    async fn switching_provider_keeps_the_tool_history() {
        use crate::ai::openai::OpenAiCompatibleProvider;
        use crate::ai::{ProviderExtras, ToolCall, ToolResult};
        let db = db();
        let reg = registry_with(None);
        let (conv, _) = save_user_message(&db, None, "What's 2+2?", "Ada", &reg.defs(), false).unwrap();
        // A turn produced by Anthropic, with its native blocks.
        let turns = vec![
            ChatTurn {
                tool_calls: vec![ToolCall {
                    id: "toolu_1".into(),
                    name: "calculator".into(),
                    input: json!({"expression": "2+2"}),
                    invalid_input: None,
                    extras: None,
                }],
                extras: Some(ProviderExtras::new(
                    "anthropic",
                    json!([{"type": "thinking", "thinking": "", "signature": "sig"}, {"type": "tool_use", "id": "toolu_1", "name": "calculator", "input": {"expression": "2+2"}}]),
                )),
                ..ChatTurn::assistant("")
            },
            ChatTurn {
                tool_results: vec![ToolResult { call_id: "toolu_1".into(), content: "2+2 = 4".into(), is_error: false, media: vec![] }],
                ..ChatTurn::user("")
            },
            ChatTurn::assistant("It's 4."),
        ];
        conversations::append(
            &mut db.conn().unwrap(),
            &conv.id,
            NewMessage {
                role: Role::Assistant,
                provider: Some("anthropic".into()),
                raw: Some(encode_turns(&turns).to_string()),
                ..NewMessage::user("It's 4.")
            },
        )
        .unwrap();
        save_user_message(&db, Some(&conv.id), "Times 3?", "Ada", &reg.defs(), false).unwrap();

        let body = format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":{"content":"12."},"finish_reason":"stop"}]}));
        let server = MockServer::start(vec![(200, "text/event-stream", body)]).await;
        let mut p = params(server.url(), reg, Policy::default(), Approval::Approved);
        p.router = Arc::new(ModelRouter::new(Arc::new(OpenAiCompatibleProvider::openai("k".into(), Some(server.url())).unwrap()), "gpt-4o"));
        let m = generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(m.content, "12.");
        let sent = server.requests().await[0].json();
        let msgs = sent["messages"].as_array().unwrap();
        let call = msgs.iter().find(|m| m["tool_calls"].is_array()).expect("the tool call crossed providers");
        assert_eq!(call["tool_calls"][0]["function"]["name"], "calculator");
        assert!(msgs.iter().any(|m| m["role"] == "tool" && m["content"] == "2+2 = 4"));
        assert!(!sent.to_string().contains("signature"), "Anthropic-only data stays with Anthropic");
    }

    #[test]
    fn long_tool_loops_stay_within_budget_without_touching_recent_work() {
        use crate::ai::{ToolCall, ToolResult};
        let budget = Budget::for_model(&crate::ai::capabilities::capabilities_for("local", "m", Some(16_000)), MAX_OUTPUT_TOKENS);
        let mut turns = vec![ChatTurn::user("Fix the failing build")];
        for i in 0..40 {
            turns.push(ChatTurn {
                tool_calls: vec![ToolCall {
                    id: format!("c{i}"),
                    name: "run_command".into(),
                    input: json!({"program": "npm"}),
                    invalid_input: None,
                    extras: None,
                }],
                ..ChatTurn::assistant("")
            });
            turns.push(ChatTurn {
                tool_results: vec![ToolResult {
                    call_id: format!("c{i}"),
                    content: format!("build log {i}\n{}", "error TS2345 at src/app.ts ".repeat(120)),
                    is_error: false,
                    media: vec![],
                }],
                ..ChatTurn::user("")
            });
        }
        let latest = turns.last().unwrap().tool_results[0].content.clone();
        assert!(estimate_turns(&turns) > budget.input_limit);
        crate::orchestrator::agent_loop::fit_tool_output(&mut turns, 3_000, &budget);
        assert!(3_000 + estimate_turns(&turns) <= budget.input_limit);
        assert_eq!(turns.last().unwrap().tool_results[0].content, latest, "the newest result is intact");
        assert_eq!(turns.iter().filter(|t| !t.tool_calls.is_empty()).count(), 40, "every call is still there");
        assert_eq!(turns[0].text, "Fix the failing build");
    }

    #[tokio::test]
    async fn full_turn_persists_and_replays_history_unchanged() {
        let db = db();
        let server = MockServer::start(vec![(200, "text/event-stream", sse_reply("Hello.")), (200, "text/event-stream", sse_reply("Still here."))]).await;
        let reg = registry_with(None);
        let p = params(server.url(), reg.clone(), Policy::default(), Approval::Approved);

        let (conv, _) = save_user_message(&db, None, "Hi IGRIS", "Ada", &reg.defs(), true).unwrap();
        let mut evs = Vec::new();
        let m1 = generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |e| evs.push(e)).await.unwrap();
        assert_eq!(m1.status, MessageStatus::Complete);
        assert_eq!(m1.content, "Hello.");
        assert!(matches!(evs.first(), Some(ChatEvent::Generating { .. })));
        assert!(matches!(evs.last(), Some(ChatEvent::Finished { .. })));

        save_user_message(&db, Some(&conv.id), "Are you there?", "Ada", &reg.defs(), true).unwrap();
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();

        let reqs = server.requests().await;
        let (first, second) = (reqs[0].json(), reqs[1].json());
        assert_eq!(first["system"], second["system"], "system prompt is frozen");
        assert_eq!(first["tools"], second["tools"], "tool set is frozen");
        assert!(first["system"].as_str().unwrap().contains("Ada"));
        let replay = &second["messages"][1]["content"];
        assert_eq!(replay[0]["type"], "thinking");
        assert_eq!(replay[0]["signature"], "sig");
        assert_eq!(replay[1]["text"], "Hello.");
        assert_eq!(second["messages"][2]["content"], "Are you there?");
    }

    #[tokio::test]
    async fn attachments_and_screenshots_reach_the_model_and_replay_from_disk() {
        use crate::attachments::{testdata, AttachmentStore};
        use crate::tools::screen::ScreenshotTool;
        use base64::Engine;

        let db = db();
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(AttachmentStore::new(dir.path().join("attachments")).unwrap());
        let shot: Arc<dyn Tool> = Arc::new(ScreenshotTool::with_capturer(
            db.clone(),
            store.clone(),
            Arc::new(|| Ok(image::RgbaImage::from_pixel(40, 20, image::Rgba([0, 128, 255, 255])))),
        ));
        let server = MockServer::start(vec![
            (200, "text/event-stream", sse_tool_use("toolu_s", "take_screenshot", json!({}))),
            (200, "text/event-stream", sse_reply("I see a blue screen.")),
            (200, "text/event-stream", sse_reply("Still blue.")),
        ])
        .await;
        let reg = registry_with(Some(shot));
        let mut p = params(server.url(), reg.clone(), Policy::default(), Approval::Approved);
        p.attachments = Some(store.clone());

        let png = testdata::png();
        let staged = store.save_upload(&db.conn().unwrap(), "chart.png", &png).unwrap();
        let (conv, m) = save_user_message_with(&db, None, "", std::slice::from_ref(&staged.id), "", &reg.defs(), false).unwrap();
        assert_eq!(conv.title, "chart.png", "attachment-only messages are titled by the file");
        assert_eq!(m.attachments.len(), 1);
        let done = generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(done.content, "Let me check.\n\nI see a blue screen.");
        let shot_id = done.tool_activity.as_ref().unwrap()[0]["attachments"][0].as_str().unwrap().to_string();
        let adopted = crate::attachments::get(&db.conn().unwrap(), &shot_id).unwrap().unwrap();
        assert_eq!(adopted.conversation_id.as_deref(), Some(conv.id.as_str()), "captures belong to the conversation");

        save_user_message(&db, Some(&conv.id), "And now?", "", &reg.defs(), false).unwrap();
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();

        let reqs = server.requests().await;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        let first = reqs[0].json();
        assert_eq!(first["messages"][0]["content"][0]["type"], "image");
        assert_eq!(first["messages"][0]["content"][0]["source"]["data"], b64.as_str());
        assert_eq!(first["messages"][0]["content"].as_array().unwrap().len(), 1, "no empty text block");
        let tool_result = &reqs[1].json()["messages"][2]["content"][0];
        assert_eq!(tool_result["type"], "tool_result");
        assert_eq!(tool_result["content"][1]["type"], "image");
        // The later request replays both images from disk, byte for byte.
        let third = reqs[2].json();
        assert_eq!(third["messages"][0], first["messages"][0]);
        assert_eq!(third["messages"][2]["content"][0]["content"], tool_result["content"]);

        crate::conversations::delete(&db.conn().unwrap(), &conv.id).unwrap();
        assert_eq!(store.gc(&db.conn().unwrap()).unwrap(), 2, "files go with the conversation");
    }

    #[tokio::test]
    async fn runs_a_tool_and_sends_the_result_back() {
        let db = db();
        let server = MockServer::start(vec![
            (200, "text/event-stream", sse_tool_use("toolu_1", "calculator", json!({"expression":"6*7"}))),
            (200, "text/event-stream", sse_reply("It's 42.")),
            (200, "text/event-stream", sse_reply("You're welcome.")),
        ])
        .await;
        let reg = registry_with(None);
        let p = params(server.url(), reg.clone(), Policy::default(), Approval::Denied);
        let (conv, _) = save_user_message(&db, None, "What's 6*7?", "", &reg.defs(), true).unwrap();
        let mut tool_events = Vec::new();
        let mut deltas = String::new();
        let m = generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |e| match e {
            ChatEvent::Tool { activity } => tool_events.push(activity),
            ChatEvent::Delta { text } => deltas.push_str(&text),
            _ => {}
        })
        .await
        .unwrap();

        assert_eq!(m.status, MessageStatus::Complete);
        assert_eq!(m.content, "Let me check.\n\nIt's 42.");
        assert_eq!(deltas, m.content, "streamed text matches the stored text");
        assert_eq!(tool_events.last().unwrap().status, ActivityStatus::Completed);
        assert_eq!(tool_events.last().unwrap().result.as_deref(), Some("= 42"));
        assert_eq!(tool_events.last().unwrap().text_offset, Some("Let me check.".len()), "tool sits after the text written before it");
        assert_eq!(m.tool_activity.as_ref().unwrap()[0]["status"], "completed");

        let second = server.requests().await[1].json();
        let msgs = second["messages"].as_array().unwrap();
        assert_eq!(msgs[1]["content"][1]["type"], "tool_use");
        assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
        assert_eq!(msgs[2]["content"][0]["content"], "6*7 = 42");
        assert_eq!(msgs[2]["content"][0]["is_error"], false);

        // The whole tool exchange is replayed on the next turn.
        save_user_message(&db, Some(&conv.id), "Thanks", "", &reg.defs(), true).unwrap();
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        let third = server.requests().await[2].json();
        let roles: Vec<&str> = third["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, vec!["user", "assistant", "user", "assistant", "user"]);
        assert_eq!(third["messages"][3]["content"][1]["text"], "It's 42.");
    }

    #[tokio::test]
    async fn denied_action_is_not_executed_and_the_model_is_told() {
        let db = db();
        let opener = Arc::new(Opener(
            ToolSpec {
                name: "open_thing",
                title: "Open",
                description: "d",
                input_schema: json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
                permission: PermissionLevel::Low,
            },
            Mutex::new(0),
        ));
        let reg = registry_with(Some(opener.clone()));
        let server = MockServer::start(vec![
            (200, "text/event-stream", sse_tool_use("toolu_2", "open_thing", json!({}))),
            (200, "text/event-stream", sse_reply("Okay, I won't.")),
        ])
        .await;
        let p = params(server.url(), reg.clone(), Policy { confirm_low: true }, Approval::Denied);
        let (conv, _) = save_user_message(&db, None, "Open it", "", &reg.defs(), true).unwrap();
        let mut statuses = Vec::new();
        let m = generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |e| {
            if let ChatEvent::Tool { activity } = e {
                statuses.push(activity.status)
            }
        })
        .await
        .unwrap();
        assert_eq!(*opener.1.lock().unwrap(), 0);
        assert!(statuses.contains(&ActivityStatus::AwaitingApproval));
        assert_eq!(*statuses.last().unwrap(), ActivityStatus::Denied);
        assert_eq!(m.status, MessageStatus::Complete);
        let result = &server.requests().await[1].json()["messages"][2]["content"][0];
        assert_eq!(result["is_error"], true);
        assert!(result["content"].as_str().unwrap().contains("denied"));
    }

    #[tokio::test]
    async fn hallucinated_tool_gets_an_error_result_not_a_fake_success() {
        let db = db();
        let reg = registry_with(None);
        let server = MockServer::start(vec![
            (200, "text/event-stream", sse_tool_use("toolu_3", "delete_all_files", json!({}))),
            (200, "text/event-stream", sse_reply("I can't do that.")),
        ])
        .await;
        let p = params(server.url(), reg.clone(), Policy::default(), Approval::Approved);
        let (conv, _) = save_user_message(&db, None, "Delete everything", "", &reg.defs(), true).unwrap();
        let m = generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(m.tool_activity.unwrap()[0]["status"], "invalid");
        let result = &server.requests().await[1].json()["messages"][2]["content"][0];
        assert_eq!(result["is_error"], true);
        assert!(result["content"].as_str().unwrap().contains("Unknown tool"));
    }

    #[tokio::test]
    async fn stops_after_the_tool_step_limit() {
        let db = db();
        let reg = registry_with(None);
        let responses = (0..=MAX_TOOL_ROUNDS)
            .map(|i| (200u16, "text/event-stream", sse_tool_use(&format!("toolu_{i}"), "calculator", json!({"expression":"1+1"}))))
            .collect();
        let server = MockServer::start(responses).await;
        let p = params(server.url(), reg.clone(), Policy::default(), Approval::Approved);
        let (conv, _) = save_user_message(&db, None, "loop", "", &reg.defs(), true).unwrap();
        let m = generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(m.status, MessageStatus::Error);
        assert!(m.error.unwrap().contains("tool steps"));
        assert_eq!(server.requests().await.len(), MAX_TOOL_ROUNDS + 1);
    }

    #[tokio::test]
    async fn server_web_search_is_shown_audited_and_pause_turn_resumes() {
        let db = db();
        let first = events(&[
            json!({"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":10}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"server_tool_use","id":"srv_1","name":"web_search","input":{}}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"igris news\"}"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"web_search_tool_result","tool_use_id":"srv_1","content":[{"type":"web_search_result","title":"News","url":"https://news.example/","encrypted_content":"e"}]}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_delta","delta":{"stop_reason":"pause_turn"},"usage":{"output_tokens":5}}),
            json!({"type":"message_stop"}),
        ]);
        let server = MockServer::start(vec![(200, "text/event-stream", first), (200, "text/event-stream", sse_reply("Here's the news."))]).await;
        let mut reg = ToolRegistry::default();
        reg.register(Arc::new(CalculatorTool::default()));
        let reg = Arc::new(reg);
        let offered = vec![crate::tools::web::anthropic_server_tool()];
        let p = params(server.url(), reg, Policy::default(), Approval::Approved);
        let (conv, _) = save_user_message(&db, None, "Any IGRIS news?", "", &offered, true).unwrap();
        let mut acts = Vec::new();
        let m = generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |e| {
            if let ChatEvent::Tool { activity } = e {
                acts.push(activity)
            }
        })
        .await
        .unwrap();
        assert_eq!(m.status, MessageStatus::Complete);
        assert_eq!(m.content, "Here's the news.");
        let done = acts.last().unwrap();
        assert_eq!(done.status, ActivityStatus::Completed);
        assert_eq!(done.description, "Search the web for \"igris news\"");
        assert_eq!(done.sources[0].url, "https://news.example/");
        let reqs = server.requests().await;
        assert_eq!(reqs[0].json()["tools"][0]["type"], "web_search_20260209");
        let second = reqs[1].json();
        let msgs = second["messages"].as_array().unwrap();
        assert_eq!(msgs.last().unwrap()["role"], "assistant", "paused turn is re-sent without an extra user message");
        assert_eq!(msgs.last().unwrap()["content"][0]["type"], "server_tool_use");
        let log = crate::tools::audit::list(&db.conn().unwrap(), 5).unwrap();
        assert_eq!(log[0].tool, "web_search");
        assert_eq!(log[0].status, "completed");
    }

    #[tokio::test]
    async fn legacy_conversations_without_a_tool_snapshot_get_no_tools() {
        let db = db();
        let server = MockServer::start(vec![(200, "text/event-stream", sse_reply("Hi."))]).await;
        let reg = registry_with(None);
        let p = params(server.url(), reg, Policy::default(), Approval::Approved);
        let (conv, _) = save_user_message(&db, None, "Hi", "", &[], true).unwrap();
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert!(server.requests().await[0].json().get("tools").is_none());
    }

    #[tokio::test]
    async fn provider_failure_is_persisted_as_error_not_success() {
        let db = db();
        let err = json!({"type":"error","error":{"type":"authentication_error","message":"bad key"}}).to_string();
        let server = MockServer::start(vec![(401, "application/json", err)]).await;
        let reg = registry_with(None);
        let (conv, _) = save_user_message(&db, None, "Hi", "", &reg.defs(), true).unwrap();
        let p = params(server.url(), reg, Policy::default(), Approval::Approved);
        let m = generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(m.status, MessageStatus::Error);
        assert!(m.error.unwrap().contains("API key"));
        assert!(m.content.is_empty());
    }

    #[tokio::test]
    async fn regenerate_replaces_last_answer() {
        let db = db();
        let server = MockServer::start(vec![(200, "text/event-stream", sse_reply("First.")), (200, "text/event-stream", sse_reply("Second."))]).await;
        let reg = registry_with(None);
        let p = params(server.url(), reg.clone(), Policy::default(), Approval::Approved);
        let (conv, _) = save_user_message(&db, None, "Q", "", &reg.defs(), true).unwrap();
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        prepare_regenerate(&db, &conv.id).unwrap();
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        let ms = conversations::messages(&db.conn().unwrap(), &conv.id).unwrap();
        assert_eq!(ms.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(), vec!["Q", "Second."]);
        assert_eq!(server.requests().await[1].json()["messages"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn refuses_to_generate_without_a_pending_user_message() {
        let db = db();
        let server = MockServer::start(vec![(200, "text/event-stream", sse_reply("A."))]).await;
        let reg = registry_with(None);
        let p = params(server.url(), reg.clone(), Policy::default(), Approval::Approved);
        let (conv, _) = save_user_message(&db, None, "Q", "", &reg.defs(), true).unwrap();
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert!(generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.is_err());
    }

    #[tokio::test]
    async fn memories_are_attached_once_and_replayed_unchanged() {
        use crate::memory::{self as mem, MemoryKind, MemorySource};
        let db = db();
        mem::add(&db.conn().unwrap(), MemoryKind::LongTerm, "Main project is SkillTrack", MemorySource::User, None).unwrap();
        let server = MockServer::start(vec![(200, "text/event-stream", sse_reply("SkillTrack.")), (200, "text/event-stream", sse_reply("Sure."))]).await;
        let reg = registry_with(None);
        let p = params(server.url(), reg.clone(), Policy::default(), Approval::Approved);
        let (conv, m1) = save_user_message(&db, None, "What's my main project?", "", &reg.defs(), true).unwrap();
        assert_eq!(m1.memory_context.as_ref().unwrap().items.len(), 1);
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        // The memory changes after it was sent; history must not change with it.
        mem::update(&db.conn().unwrap(), 1, "Main project is Apollo", None).unwrap();
        let (_, m2) = save_user_message(&db, Some(&conv.id), "ok", "", &reg.defs(), true).unwrap();
        assert!(m2.memory_context.unwrap().rendered.contains("Apollo"), "the new version is attached once");
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();

        let reqs = server.requests().await;
        let first_user = reqs[0].json()["messages"][0]["content"].as_str().unwrap().to_string();
        assert!(first_user.starts_with("<memory>"));
        assert!(first_user.ends_with("What's my main project?"));
        assert_eq!(reqs[1].json()["messages"][0]["content"], first_user, "earlier turn replayed byte-for-byte");
    }

    #[test]
    fn memory_switch_attaches_nothing() {
        use crate::memory::{self as mem, MemoryKind, MemorySource};
        let db = db();
        mem::add(&db.conn().unwrap(), MemoryKind::LongTerm, "fact", MemorySource::User, None).unwrap();
        let (_, m) = save_user_message(&db, None, "hi", "", &[], false).unwrap();
        assert!(m.memory_context.is_none());
    }

    #[test]
    fn validates_input() {
        assert!(validate_input("   ").is_err());
        assert!(validate_input(&"x".repeat(MAX_INPUT_CHARS + 1)).is_err());
        assert_eq!(validate_input("  hi \n").unwrap(), "hi");
    }
}
