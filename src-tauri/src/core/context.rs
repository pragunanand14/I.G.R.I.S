//! Builds model context from stored conversation history.
//!
//! History is IGRIS's canonical, provider-neutral form ([`ChatTurn`]): text,
//! tool calls, tool results and media. Provider-specific data travels as
//! tagged [`ProviderExtras`] that only the producing adapter replays, so a
//! conversation can continue on another provider with its tool history intact.
//!
//! Assistant messages store their turns in `raw`:
//! * legacy (Phase 2): a JSON array of Anthropic content blocks;
//! * v2: `{"v": 2, "turns": [ChatTurn, ...]}` — every assistant turn, tool call
//!   and tool result that produced the message. Turns stored before extras
//!   were tagged carry an untagged `raw` / `provider_data`; they are read as
//!   extras of the message's provider. Stored data is never rewritten.

use serde_json::{json, Value};

use crate::ai::{ChatTurn, ProviderExtras, Role};
use crate::conversations::{Message, MessageStatus};

pub const RAW_VERSION: u64 = 2;

/// Serialize the provider turns produced while generating one message.
pub fn encode_turns(turns: &[ChatTurn]) -> Value {
    json!({ "v": RAW_VERSION, "turns": turns })
}

/// Older stored turns kept provider data untagged (`raw` on turns,
/// `provider_data` on tool calls); tag it with the message's provider.
fn upgrade_legacy_fields(turns: &mut Value, provider: Option<&str>) {
    let tag = |v: Value| match provider {
        Some(p) if !v.is_null() => Some(json!({ "provider": p, "data": v })),
        _ => None,
    };
    for t in turns.as_array_mut().into_iter().flatten() {
        let Some(obj) = t.as_object_mut() else { continue };
        if let Some(raw) = obj.remove("raw") {
            if let Some(e) = tag(raw) {
                obj.entry("extras").or_insert(e);
            }
        }
        for c in obj.get_mut("tool_calls").and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(data) = c.as_object_mut().and_then(|c| c.remove("provider_data")) {
                if let (Some(e), Some(c)) = (tag(data), c.as_object_mut()) {
                    c.entry("extras").or_insert(e);
                }
            }
        }
    }
}

fn decode_turns(raw: &Value, provider: Option<&str>) -> Option<Vec<ChatTurn>> {
    if raw["v"].as_u64() != Some(RAW_VERSION) {
        return None;
    }
    let mut turns = raw["turns"].clone();
    upgrade_legacy_fields(&mut turns, provider);
    serde_json::from_value(turns).ok()
}

/// The canonical turns one stored message contributes (empty for failed,
/// cancelled and refused assistant messages — not real answers, and partial
/// content can't be replayed safely).
pub fn message_turns(m: &Message) -> Vec<ChatTurn> {
    match m.role {
        // Attached memories are replayed exactly as first sent.
        Role::User => vec![ChatTurn {
            media: m.attachments.iter().map(|a| a.media()).collect(),
            ..ChatTurn::user(match &m.memory_context {
                Some(mc) => format!("{}\n\n{}", mc.rendered, m.content),
                None => m.content.clone(),
            })
        }],
        Role::Assistant => {
            if !matches!(m.status, MessageStatus::Complete | MessageStatus::Truncated) {
                return Vec::new();
            }
            let raw: Option<Value> = m.raw.as_deref().and_then(|r| serde_json::from_str(r).ok());
            let text_only = || if m.content.is_empty() { Vec::new() } else { vec![ChatTurn::assistant(m.content.clone())] };
            match raw {
                // Legacy: Anthropic content blocks of a text-only answer.
                Some(v) if v.is_array() => {
                    let extras = m.provider.as_deref().map(|p| ProviderExtras::new(p, v));
                    vec![ChatTurn { extras, ..ChatTurn::assistant(m.content.clone()) }]
                }
                Some(v) => match decode_turns(&v, m.provider.as_deref()) {
                    Some(turns) if !turns.is_empty() => turns,
                    _ => text_only(),
                },
                None => text_only(),
            }
        }
    }
}

/// Convert stored messages into canonical turns for any provider.
pub fn build_turns(messages: &[Message]) -> Vec<ChatTurn> {
    messages.iter().flat_map(message_turns).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{ProviderExtras, ToolCall, ToolResult};

    fn msg(role: Role, content: &str, status: MessageStatus, provider: Option<&str>, raw: Option<String>) -> Message {
        Message {
            id: "id".into(),
            conversation_id: "c".into(),
            seq: 0,
            role,
            content: content.into(),
            status,
            error: None,
            provider: provider.map(Into::into),
            model: None,
            raw,
            input_tokens: None,
            output_tokens: None,
            created_at: String::new(),
            tool_activity: None,
            memory_context: None,
            attachments: vec![],
        }
    }

    #[test]
    fn skips_failed_turns_and_tags_legacy_blocks_with_their_provider() {
        let ms = vec![
            msg(Role::User, "q1", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "", MessageStatus::Error, Some("anthropic"), None),
            msg(Role::User, "q1 again", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "a1", MessageStatus::Complete, Some("anthropic"), Some(r#"[{"type":"text","text":"a1"}]"#.into())),
            msg(Role::User, "q2", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "partial", MessageStatus::Cancelled, Some("anthropic"), None),
            msg(Role::User, "q3", MessageStatus::Complete, None, None),
        ];
        let turns = build_turns(&ms);
        let texts: Vec<&str> = turns.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec!["q1", "q1 again", "a1", "q2", "q3"]);
        assert_eq!(turns[2].extras.as_ref().map(|e| e.provider.as_str()), Some("anthropic"), "native blocks stay tagged for their adapter");
    }

    fn calc_exchange(extras: Option<ProviderExtras>) -> Vec<ChatTurn> {
        vec![
            ChatTurn {
                tool_calls: vec![ToolCall {
                    id: "t1".into(),
                    name: "calculator".into(),
                    input: json!({"expression":"1+1"}),
                    invalid_input: None,
                    extras: None,
                }],
                extras,
                ..ChatTurn::assistant("")
            },
            ChatTurn {
                tool_results: vec![ToolResult { call_id: "t1".into(), content: "1+1 = 2".into(), is_error: false, media: vec![] }],
                ..ChatTurn::user("")
            },
            ChatTurn::assistant("It's 2."),
        ]
    }

    #[test]
    fn tool_exchanges_are_canonical_for_every_provider() {
        let blocks = json!([{"type":"tool_use","id":"t1","name":"calculator","input":{"expression":"1+1"}}]);
        let raw = encode_turns(&calc_exchange(Some(ProviderExtras::new("anthropic", blocks)))).to_string();
        let ms = vec![
            msg(Role::User, "1+1?", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "It's 2.", MessageStatus::Complete, Some("anthropic"), Some(raw)),
        ];
        let turns = build_turns(&ms);
        assert_eq!(turns.len(), 4, "the tool exchange survives a provider switch");
        assert_eq!(turns[1].tool_calls[0].id, "t1");
        assert_eq!(turns[2].tool_results[0].content, "1+1 = 2");
        assert_eq!(turns[3].text, "It's 2.");
    }

    #[test]
    fn old_untagged_provider_data_is_read_as_tagged_extras() {
        // Exactly what earlier versions stored: `raw` on turns, `provider_data` on calls.
        let stored = json!({"v": 2, "turns": [
            {"role": "assistant", "text": "", "raw": [{"type": "tool_use"}],
             "tool_calls": [{"id": "c", "name": "system_info", "input": {}, "provider_data": {"google": {"thought_signature": "s"}}}]},
            {"role": "user", "text": "", "tool_results": [{"call_id": "c", "content": "ok", "is_error": false}]},
            {"role": "assistant", "text": "Done."}
        ]})
        .to_string();
        let ms = vec![
            msg(Role::User, "q", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "Done.", MessageStatus::Complete, Some("gemini"), Some(stored)),
        ];
        let turns = build_turns(&ms);
        assert_eq!(turns[1].extras, Some(ProviderExtras::new("gemini", json!([{"type": "tool_use"}]))));
        assert_eq!(turns[1].tool_calls[0].extras, Some(ProviderExtras::new("gemini", json!({"google": {"thought_signature": "s"}}))));
        assert_eq!(turns[2].tool_results[0].content, "ok");
        // Re-encoding writes only the new, tagged form.
        let again = encode_turns(&turns[1..]).to_string();
        assert!(!again.contains("provider_data") && !again.contains("\"raw\""));
    }

    #[test]
    fn user_turns_carry_their_stored_memory_block() {
        use crate::memory::retrieval::MemoryContext;
        let mut u = msg(Role::User, "What's my project?", MessageStatus::Complete, None, None);
        u.memory_context = Some(MemoryContext { items: vec![], rendered: "<memory>\n- fact\n</memory>".into() });
        let turns = build_turns(&[u]);
        assert_eq!(turns[0].text, "<memory>\n- fact\n</memory>\n\nWhat's my project?");
    }

    #[test]
    fn corrupt_raw_falls_back_to_text() {
        let ms = vec![
            msg(Role::User, "q", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "a", MessageStatus::Complete, Some("anthropic"), Some("not json".into())),
            msg(Role::User, "q2", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "b", MessageStatus::Complete, Some("anthropic"), Some(r#"{"v":99}"#.into())),
        ];
        let turns = build_turns(&ms);
        assert_eq!(turns.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), vec!["q", "a", "q2", "b"]);
        assert!(turns.iter().all(|t| t.extras.is_none()));
    }
}
