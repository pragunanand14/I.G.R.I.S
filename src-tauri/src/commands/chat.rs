use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;

use crate::conversations::{self, Conversation, Message};
use crate::core::chat::{self, ChatEvent, GenerationParams, Tooling};
use crate::error::{AppError, AppResult};
use crate::settings;
use crate::state::{AppState, UiApprover, APPROVAL_TIMEOUT};
use crate::tools::executor::Policy;

fn validate_request_id(id: &str) -> AppResult<()> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(AppError::validation("Invalid request id."));
    }
    Ok(())
}

/// Resolve provider + model + effort for a new generation.
fn generation_params(state: &AppState) -> AppResult<GenerationParams> {
    let (provider, status) = state.ai_provider()?;
    let settings = settings::load(&*state.db.conn()?)?;
    let model = Some(settings.ai_model.clone())
        .filter(|m| !m.is_empty())
        .or(status.configured_model)
        .ok_or_else(|| AppError::AiUnavailable("No model configured. Set AI_MODEL or choose a model in Settings.".into()))?;
    Ok(GenerationParams {
        attachments: Some(state.attachments.clone()),
        provider,
        model,
        effort: Some(settings.ai_effort),
        tooling: Tooling {
            registry: state.tools.clone(),
            policy: Policy { confirm_low: settings.confirm_low_risk },
            approver: std::sync::Arc::new(UiApprover { pending: state.approvals.clone(), timeout: APPROVAL_TIMEOUT }),
            trust: Some(state.trust.clone()),
            operator: Some(state.operator.clone()),
        },
    })
}

/// Tools for a new conversation (frozen with it).
fn offered_tools(state: &AppState) -> AppResult<Vec<crate::ai::ToolDef>> {
    let mode = state.config.read().map_err(|_| AppError::internal("config lock poisoned"))?.web_search_mode();
    Ok(state.tools.offered(mode))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnResult {
    pub conversation_id: String,
    pub assistant_message: Message,
}

async fn run_generation(
    state: &AppState,
    conversation_id: &str,
    guard_token: &tokio_util::sync::CancellationToken,
    params: GenerationParams,
    on_event: &Channel<ChatEvent>,
) -> AppResult<TurnResult> {
    let mut emit = |ev: ChatEvent| {
        if let Err(e) = on_event.send(ev) {
            tracing::warn!(event = "CHAT_EVENT_DELIVERY_FAILED", error = %e);
        }
    };
    let message = chat::generate(&state.db, conversation_id, &params, guard_token, &mut emit).await;
    // Control always returns to the user when the reply ends, however it ended.
    state.operator.end_turn(conversation_id);
    let message = message?;
    Ok(TurnResult { conversation_id: conversation_id.to_string(), assistant_message: message })
}

/// Send a message (creating a conversation when `conversation_id` is null) and stream the reply.
#[tauri::command]
pub async fn chat_send(
    state: State<'_, AppState>,
    request_id: String,
    conversation_id: Option<String>,
    content: String,
    attachment_ids: Option<Vec<String>>,
    on_event: Channel<ChatEvent>,
) -> AppResult<TurnResult> {
    validate_request_id(&request_id)?;
    let attachment_ids = attachment_ids.unwrap_or_default();
    let content = chat::validate_input_with(&content, !attachment_ids.is_empty())?;
    // Check AI availability before saving anything.
    let params = generation_params(&state)?;
    let guard = state.generations.begin(&request_id, conversation_id.as_deref())?;
    let s = settings::load(&*state.db.conn()?)?;

    let (conversation, message) = chat::save_user_message_with(
        &state.db,
        conversation_id.as_deref(),
        &content,
        &attachment_ids,
        &s.user_name,
        &offered_tools(&state)?,
        s.memory_enabled,
    )?;
    guard.attach(&conversation.id);
    let _ = on_event.send(ChatEvent::UserMessage { conversation: conversation.clone(), message });
    run_generation(&state, &conversation.id, &guard.token, params, &on_event).await
}

/// Discard the last answer and generate a new one.
#[tauri::command]
pub async fn chat_regenerate(state: State<'_, AppState>, request_id: String, conversation_id: String, on_event: Channel<ChatEvent>) -> AppResult<TurnResult> {
    validate_request_id(&request_id)?;
    let params = generation_params(&state)?;
    let guard = state.generations.begin(&request_id, Some(&conversation_id))?;
    chat::prepare_regenerate(&state.db, &conversation_id)?;
    run_generation(&state, &conversation_id, &guard.token, params, &on_event).await
}

/// Edit one of the user's messages, drop everything after it, and respond again.
#[tauri::command]
pub async fn chat_edit(
    state: State<'_, AppState>,
    request_id: String,
    message_id: String,
    content: String,
    on_event: Channel<ChatEvent>,
) -> AppResult<TurnResult> {
    validate_request_id(&request_id)?;
    let original = conversations::get_message(&*state.db.conn()?, &message_id)?.ok_or_else(|| AppError::validation("That message no longer exists."))?;
    let content = chat::validate_input_with(&content, !original.attachments.is_empty())?;
    let params = generation_params(&state)?;
    let conversation_id = original.conversation_id;
    let guard = state.generations.begin(&request_id, Some(&conversation_id))?;
    let memory_enabled = settings::load(&*state.db.conn()?)?.memory_enabled;
    chat::edit_user_message(&state.db, &message_id, &content, memory_enabled)?;
    tracing::info!(event = "MESSAGE_EDITED", conversation_id = %conversation_id);
    run_generation(&state, &conversation_id, &guard.token, params, &on_event).await
}

/// "Allow for this chat": later overwrite / move / close-app actions in this
/// conversation run without asking until IGRIS restarts. Deleting files and
/// screenshots still ask every time.
#[tauri::command]
pub fn trust_conversation(state: State<'_, AppState>, conversation_id: String) -> AppResult<()> {
    conversations::require(&*state.db.conn()?, &conversation_id)?;
    state.trust.trust(&conversation_id);
    tracing::info!(event = "CONVERSATION_TRUSTED");
    Ok(())
}

/// Answer a pending tool approval. Returns false if it already expired or finished.
#[tauri::command]
pub fn respond_tool_approval(state: State<'_, AppState>, call_id: String, approved: bool) -> bool {
    let delivered = state.approvals.respond(&call_id, approved);
    tracing::info!(event = "TOOL_APPROVAL_ANSWERED", approved, delivered);
    delivered
}

/// Stop an in-flight generation. Returns false if it already finished.
#[tauri::command]
pub fn chat_cancel(state: State<'_, AppState>, request_id: String) -> bool {
    let cancelled = state.generations.cancel(&request_id);
    if cancelled {
        tracing::info!(event = "CHAT_CANCEL_REQUESTED");
    }
    cancelled
}

#[tauri::command]
pub fn list_conversations(state: State<'_, AppState>) -> AppResult<Vec<Conversation>> {
    conversations::list(&*state.db.conn()?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDetail {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
    pub busy: bool,
}

#[tauri::command]
pub fn get_conversation(state: State<'_, AppState>, id: String) -> AppResult<ConversationDetail> {
    let conn = state.db.conn()?;
    Ok(ConversationDetail {
        conversation: conversations::require(&conn, &id)?,
        messages: conversations::messages(&conn, &id)?,
        busy: state.generations.is_busy(&id),
    })
}

#[tauri::command]
pub fn rename_conversation(state: State<'_, AppState>, id: String, title: String) -> AppResult<Conversation> {
    conversations::rename(&*state.db.conn()?, &id, &title)
}

#[tauri::command]
pub fn delete_conversation(state: State<'_, AppState>, id: String) -> AppResult<()> {
    if state.generations.is_busy(&id) {
        return Err(AppError::validation("Stop the response before deleting this conversation."));
    }
    let conn = state.db.conn()?;
    if !conversations::delete(&conn, &id)? {
        return Err(AppError::validation("That conversation no longer exists."));
    }
    // Attachment rows cascade; remove their files now.
    if let Err(e) = state.attachments.gc(&conn) {
        tracing::warn!(event = "ATTACHMENT_GC_FAILED", error = %e);
    }
    tracing::info!(event = "CONVERSATION_DELETED", conversation_id = %id);
    Ok(())
}
