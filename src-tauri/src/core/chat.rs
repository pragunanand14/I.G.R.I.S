//! One assistant turn: load context → stream from the provider → persist.
//!
//! Every outcome is persisted as an assistant message whose `status` says what
//! happened (complete, truncated, refused, cancelled, error), so the UI never
//! has to guess and a failure is never reported as success.

use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::context::build_turns;
use super::prompt::{self, PromptContext};
use crate::ai::{AiErrorKind, AiProvider, ChatRequest, Effort, Role, StopReason, StreamEvent};
use crate::conversations::{self, Conversation, Message, MessageStatus, NewMessage};
use crate::db::Database;
use crate::error::{AppError, AppResult};

pub const MAX_INPUT_CHARS: usize = 100_000;
/// Per-response output cap. Streaming keeps large values safe from HTTP timeouts.
pub const MAX_OUTPUT_TOKENS: u32 = 64_000;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ChatEvent {
    /// The user's message was saved (and the conversation created if new).
    UserMessage { conversation: Conversation, message: Message },
    /// The provider request is starting.
    Generating { conversation_id: String, model: String },
    /// Streamed response text.
    Delta { text: String },
    /// The assistant turn ended; `message.status` says how.
    Finished { message: Message },
}

pub type Emit<'a> = &'a mut (dyn FnMut(ChatEvent) + Send);

pub struct GenerationParams {
    pub provider: Arc<dyn AiProvider>,
    pub model: String,
    pub effort: Option<Effort>,
}

pub fn validate_input(content: &str) -> AppResult<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation("Message is empty."));
    }
    if trimmed.chars().count() > MAX_INPUT_CHARS {
        return Err(AppError::validation(format!("Message is too long (max {MAX_INPUT_CHARS} characters).")));
    }
    Ok(trimmed.to_string())
}

/// Save a user message, creating the conversation (with its frozen system
/// prompt) when `conversation_id` is `None`.
pub fn save_user_message(db: &Database, conversation_id: Option<&str>, content: &str, user_name: &str) -> AppResult<(Conversation, Message)> {
    let mut conn = db.conn()?;
    let conversation = match conversation_id {
        Some(id) => conversations::require(&conn, id)?,
        None => {
            let system = prompt::system_prompt(&PromptContext { user_name, date: &prompt::today(), os: prompt::os_label() });
            let c = conversations::create(&conn, &conversations::title_from(content), &system)?;
            tracing::info!(event = "CONVERSATION_CREATED", conversation_id = %c.id);
            c
        }
    };
    let message = conversations::append(&mut conn, &conversation.id, NewMessage::user(content))?;
    let conversation = conversations::require(&conn, &conversation.id)?;
    Ok((conversation, message))
}

/// Generate and persist the assistant reply to the conversation's last user message.
pub async fn generate(db: &Database, conversation_id: &str, params: &GenerationParams, cancel: &CancellationToken, emit: Emit<'_>) -> AppResult<Message> {
    let (system, history) = {
        let conn = db.conn()?;
        (conversations::system_prompt(&conn, conversation_id)?, conversations::messages(&conn, conversation_id)?)
    };
    if history.last().map(|m| m.role) != Some(Role::User) {
        return Err(AppError::validation("There is no message to respond to."));
    }

    let provider_id = params.provider.id();
    let request = ChatRequest {
        model: params.model.clone(),
        system,
        turns: build_turns(&history, provider_id),
        max_tokens: MAX_OUTPUT_TOKENS,
        effort: params.effort,
    };

    emit(ChatEvent::Generating { conversation_id: conversation_id.to_string(), model: params.model.clone() });
    tracing::info!(event = "CHAT_REQUEST_STARTED", provider = provider_id, model = %params.model, turns = request.turns.len());

    let started = Instant::now();
    let mut first_token_ms: Option<u128> = None;
    let mut partial = String::new();
    let result = params
        .provider
        .stream(&request, cancel, &mut |ev| match ev {
            StreamEvent::TextDelta(text) => {
                if first_token_ms.is_none() {
                    first_token_ms = Some(started.elapsed().as_millis());
                }
                partial.push_str(&text);
                emit(ChatEvent::Delta { text });
            }
        })
        .await;

    let base = NewMessage { role: Role::Assistant, provider: Some(provider_id.to_string()), model: Some(params.model.clone()), ..NewMessage::user("") };
    let new_message = match result {
        Ok(c) => {
            let (status, error) = match &c.stop_reason {
                StopReason::EndTurn | StopReason::Other { .. } => (MessageStatus::Complete, None),
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
                served_model = %c.model,
                first_token_ms = first_token_ms.map(|v| v as u64),
                total_ms = started.elapsed().as_millis() as u64,
                input_tokens = c.usage.input_tokens,
                output_tokens = c.usage.output_tokens,
            );
            let keep_raw = matches!(status, MessageStatus::Complete | MessageStatus::Truncated);
            NewMessage {
                content: c.text,
                status,
                error,
                model: Some(c.model),
                raw: if keep_raw { c.raw.map(|r| r.to_string()) } else { None },
                input_tokens: c.usage.input_tokens.map(|v| v as i64),
                output_tokens: c.usage.output_tokens.map(|v| v as i64),
                ..base
            }
        }
        Err(e) if e.kind == AiErrorKind::Cancelled => {
            tracing::info!(event = "CHAT_CANCELLED", partial_chars = partial.chars().count());
            NewMessage { content: partial, status: MessageStatus::Cancelled, ..base }
        }
        Err(e) => {
            tracing::warn!(event = "CHAT_FAILED", kind = ?e.kind, provider = provider_id);
            NewMessage { content: partial, status: MessageStatus::Error, error: Some(e.message), ..base }
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
    use serde_json::json;

    fn sse_reply(text: &str) -> String {
        [
            json!({"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":10}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":text}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}),
            json!({"type":"message_stop"}),
        ]
        .iter()
        .map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap()))
        .collect()
    }

    fn params(url: String) -> GenerationParams {
        GenerationParams {
            provider: Arc::new(AnthropicProvider::new("k".into(), Some(url)).unwrap()),
            model: "claude-opus-5-5".into(),
            effort: Some(Effort::Medium),
        }
    }

    #[tokio::test]
    async fn full_turn_persists_and_replays_history_unchanged() {
        let db = Database::open_in_memory().unwrap();
        let server = MockServer::start(vec![
            (200, "text/event-stream", sse_reply("Hello.")),
            (200, "text/event-stream", sse_reply("Still here.")),
        ])
        .await;
        let p = params(server.url());

        let (conv, _) = save_user_message(&db, None, "Hi IGRIS", "Ada").unwrap();
        let mut events = Vec::new();
        let m1 = generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |e| events.push(e)).await.unwrap();
        assert_eq!(m1.status, MessageStatus::Complete);
        assert_eq!(m1.content, "Hello.");
        assert!(matches!(events.first(), Some(ChatEvent::Generating { .. })));
        assert!(matches!(events.last(), Some(ChatEvent::Finished { .. })));

        save_user_message(&db, Some(&conv.id), "Are you there?", "Ada").unwrap();
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();

        let reqs = server.requests().await;
        let (first, second) = (reqs[0].json(), reqs[1].json());
        // System prompt is frozen across turns.
        assert_eq!(first["system"], second["system"]);
        assert!(first["system"].as_str().unwrap().contains("Ada"));
        // Prior assistant turn replayed with its thinking block intact.
        let replay = &second["messages"][1]["content"];
        assert_eq!(replay[0]["type"], "thinking");
        assert_eq!(replay[0]["signature"], "sig");
        assert_eq!(replay[1]["text"], "Hello.");
        assert_eq!(second["messages"][2]["content"], "Are you there?");
    }

    #[tokio::test]
    async fn provider_failure_is_persisted_as_error_not_success() {
        let db = Database::open_in_memory().unwrap();
        let err = json!({"type":"error","error":{"type":"authentication_error","message":"bad key"}}).to_string();
        let server = MockServer::start(vec![(401, "application/json", err)]).await;
        let (conv, _) = save_user_message(&db, None, "Hi", "").unwrap();
        let m = generate(&db, &conv.id, &params(server.url()), &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(m.status, MessageStatus::Error);
        assert!(m.error.unwrap().contains("API key"));
        assert!(m.content.is_empty());
    }

    #[tokio::test]
    async fn regenerate_replaces_last_answer() {
        let db = Database::open_in_memory().unwrap();
        let server = MockServer::start(vec![
            (200, "text/event-stream", sse_reply("First.")),
            (200, "text/event-stream", sse_reply("Second.")),
        ])
        .await;
        let p = params(server.url());
        let (conv, _) = save_user_message(&db, None, "Q", "").unwrap();
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        prepare_regenerate(&db, &conv.id).unwrap();
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        let ms = conversations::messages(&db.conn().unwrap(), &conv.id).unwrap();
        assert_eq!(ms.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(), vec!["Q", "Second."]);
        // The regenerated request must not include the discarded answer.
        assert_eq!(server.requests().await[1].json()["messages"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn refuses_to_generate_without_a_pending_user_message() {
        let db = Database::open_in_memory().unwrap();
        let server = MockServer::start(vec![(200, "text/event-stream", sse_reply("A."))]).await;
        let p = params(server.url());
        let (conv, _) = save_user_message(&db, None, "Q", "").unwrap();
        generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert!(generate(&db, &conv.id, &p, &CancellationToken::new(), &mut |_| {}).await.is_err());
    }

    #[test]
    fn validates_input() {
        assert!(validate_input("   ").is_err());
        assert!(validate_input(&"x".repeat(MAX_INPUT_CHARS + 1)).is_err());
        assert_eq!(validate_input("  hi \n").unwrap(), "hi");
    }
}
