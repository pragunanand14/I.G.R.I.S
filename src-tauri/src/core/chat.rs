//! One assistant message: load context → (stream → run tools)* → persist.
//!
//! The model may request tools; each request goes through the tool executor
//! (validation, permission policy, audit) and the results are sent back until
//! the model produces a final answer or the step limit is reached. Every
//! outcome is persisted with a `status` (complete, truncated, refused,
//! cancelled, error), so the UI never has to guess and a failure is never
//! reported as success.

use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::context::{build_turns, encode_turns};
use super::prompt::{self, PromptContext};
use crate::ai::{AiError, AiErrorKind, AiProvider, ChatRequest, ChatTurn, Effort, Role, ServerToolEvent, StopReason, StreamEvent, ToolDef, Usage};
use crate::conversations::{self, Conversation, Message, MessageStatus, NewMessage};
use crate::attachments::AttachmentStore;
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::memory::retrieval::{self, MemoryContext};
use crate::tools::executor::{self, ActivityStatus, Actor, Approver, ExecContext, Policy, ToolActivity};
use crate::tools::{audit, PermissionLevel, ToolRegistry};

pub const MAX_INPUT_CHARS: usize = 100_000;
/// Per-response output cap. Streaming keeps large values safe from HTTP timeouts.
pub const MAX_OUTPUT_TOKENS: u32 = 64_000;
/// Maximum model ↔ tool round trips for one message.
pub const MAX_TOOL_ROUNDS: usize = 8;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ChatEvent {
    /// The user's message was saved (and the conversation created if new).
    UserMessage { conversation: Conversation, message: Message },
    /// The provider request is starting.
    Generating { conversation_id: String, model: String },
    /// Streamed response text.
    Delta { text: String },
    /// A tool call started, needs approval, or finished (upsert by `activity.id`).
    Tool { activity: ToolActivity },
    /// The assistant turn ended; `message.status` says how.
    Finished { message: Message },
}

pub type Emit<'a> = &'a mut (dyn FnMut(ChatEvent) + Send);

pub struct Tooling {
    pub registry: Arc<ToolRegistry>,
    pub policy: Policy,
    pub approver: Arc<dyn Approver>,
}

pub struct GenerationParams {
    pub provider: Arc<dyn AiProvider>,
    pub model: String,
    pub effort: Option<Effort>,
    pub tooling: Tooling,
    /// Where attachment bytes are loaded from (None in tests without files).
    pub attachments: Option<Arc<AttachmentStore>>,
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

enum Outcome {
    Finished(StopReason),
    Failed(AiError),
    Cancelled,
    StepLimit,
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

    let provider_id = params.provider.id();
    let mut base_turns = build_turns(&history, provider_id);
    if let Some(store) = &params.attachments {
        let conn = db.conn()?;
        for t in &mut base_turns {
            for m in t.media.iter_mut().chain(t.tool_results.iter_mut().flat_map(|r| r.media.iter_mut())) {
                store.hydrate(&conn, m);
            }
        }
    }
    let allowed: Vec<String> = offered.iter().map(|t| t.name.clone()).collect();

    emit(ChatEvent::Generating { conversation_id: conversation_id.to_string(), model: params.model.clone() });
    tracing::info!(event = "CHAT_REQUEST_STARTED", provider = provider_id, model = %params.model, turns = base_turns.len(), tools = offered.len());

    let started = Instant::now();
    let mut first_token_ms: Option<u128> = None;
    let mut text = String::new();
    let mut new_turns: Vec<ChatTurn> = Vec::new();
    let mut activities: Vec<ToolActivity> = Vec::new();
    let mut usage = Usage::default();
    let mut served_model = params.model.clone();
    let mut outcome = Outcome::StepLimit;

    for round in 0..=MAX_TOOL_ROUNDS {
        let mut turns = base_turns.clone();
        turns.extend(new_turns.iter().cloned());
        let request = ChatRequest {
            model: params.model.clone(),
            system: system.clone(),
            turns,
            max_tokens: MAX_OUTPUT_TOKENS,
            effort: params.effort,
            tools: offered.clone(),
        };

        let mut round_has_text = false;
        let mut server_running: Vec<ToolActivity> = Vec::new();
        let result = params
            .provider
            .stream(&request, cancel, &mut |ev| match ev {
                StreamEvent::ServerTool(ServerToolEvent::Started { id, name, input }) => {
                    let query = input["query"].as_str().or(input["url"].as_str()).unwrap_or_default();
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
                        if let Ok(conn) = db.conn() {
                            let _ = audit::record(
                                &conn,
                                &audit::NewAuditEntry {
                                    conversation_id: Some(conversation_id),
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
            raw: completion.raw.clone(),
            tool_calls: completion.tool_calls.clone(),
            ..ChatTurn::assistant(completion.text.clone())
        });
        if completion.stop_reason == StopReason::PauseTurn {
            // A provider-side tool loop paused: re-send as-is and it resumes.
            if round == MAX_TOOL_ROUNDS {
                break; // StepLimit
            }
            continue;
        }
        if !wants_tools {
            outcome = Outcome::Finished(completion.stop_reason);
            break;
        }
        if round == MAX_TOOL_ROUNDS {
            break; // StepLimit
        }

        let ctx = ExecContext {
            registry: &params.tooling.registry,
            db,
            conversation_id: Some(conversation_id),
            actor: Actor::Assistant,
            policy: params.tooling.policy,
            allowed: Some(&allowed),
            approver: params.tooling.approver.as_ref(),
            cancel,
        };
        let mut results = Vec::new();
        let offset = Some(text.chars().count());
        for call in &completion.tool_calls {
            let (result, mut activity) =
                executor::execute(call, &ctx, &mut |a| emit(ChatEvent::Tool { activity: ToolActivity { text_offset: offset, ..a.clone() } })).await;
            activity.text_offset = offset;
            emit(ChatEvent::Tool { activity: activity.clone() });
            activities.push(activity);
            results.push(result);
        }
        new_turns.push(ChatTurn { tool_results: results, ..ChatTurn::user("") });
        if cancel.is_cancelled() {
            outcome = Outcome::Cancelled;
            break;
        }
    }

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
            tracing::warn!(event = "CHAT_TOOL_STEP_LIMIT", rounds = MAX_TOOL_ROUNDS);
            NewMessage {
                status: MessageStatus::Error,
                error: Some(format!("Stopped after {MAX_TOOL_ROUNDS} tool steps without a final answer.")),
                ..base
            }
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
            attachments: None,
            provider: Arc::new(AnthropicProvider::new("k".into(), Some(url)).unwrap()),
            model: "claude-opus-5-5".into(),
            effort: Some(Effort::Medium),
            tooling: Tooling { registry, policy, approver: Arc::new(FixedApprover(approval)) },
        }
    }

    fn db() -> Arc<Database> {
        Arc::new(Database::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn full_turn_persists_and_replays_history_unchanged() {
        let db = db();
        let server = MockServer::start(vec![
            (200, "text/event-stream", sse_reply("Hello.")),
            (200, "text/event-stream", sse_reply("Still here.")),
        ])
        .await;
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
        let log = audit::list(&db.conn().unwrap(), 5).unwrap();
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
        let server = MockServer::start(vec![
            (200, "text/event-stream", sse_reply("First.")),
            (200, "text/event-stream", sse_reply("Second.")),
        ])
        .await;
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
        let server = MockServer::start(vec![
            (200, "text/event-stream", sse_reply("SkillTrack.")),
            (200, "text/event-stream", sse_reply("Sure.")),
        ])
        .await;
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
