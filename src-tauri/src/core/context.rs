//! Builds model context from stored conversation history.
//!
//! Assistant messages store their provider turns in `raw`:
//! * legacy (Phase 2): a JSON array of Anthropic content blocks;
//! * current: `{"v": 2, "turns": [ChatTurn, ...]}` — the full sequence of
//!   assistant turns, tool calls and tool results that produced the message.

use serde_json::{json, Value};

use crate::ai::{ChatTurn, Role};
use crate::conversations::{Message, MessageStatus};

pub const RAW_VERSION: u64 = 2;

/// Serialize the provider turns produced while generating one message.
pub fn encode_turns(turns: &[ChatTurn]) -> Value {
    json!({ "v": RAW_VERSION, "turns": turns })
}

fn decode_turns(raw: &Value) -> Option<Vec<ChatTurn>> {
    if raw["v"].as_u64() != Some(RAW_VERSION) {
        return None;
    }
    serde_json::from_value(raw["turns"].clone()).ok()
}

/// Convert stored messages into provider turns.
///
/// * Failed, cancelled and refused assistant turns are excluded — they aren't
///   real answers and partial provider content can't be replayed safely.
/// * Provider-native content (thinking blocks, tool exchanges) is only
///   replayed to the provider that produced it; otherwise the plain text is sent.
pub fn build_turns(messages: &[Message], provider_id: &str) -> Vec<ChatTurn> {
    let mut out = Vec::new();
    for m in messages {
        match m.role {
            // Attached memories are replayed exactly as first sent.
            Role::User => out.push(ChatTurn {
                media: m.attachments.iter().map(|a| a.media()).collect(),
                ..ChatTurn::user(match &m.memory_context {
                    Some(mc) => format!("{}\n\n{}", mc.rendered, m.content),
                    None => m.content.clone(),
                })
            }),
            Role::Assistant => {
                if !matches!(m.status, MessageStatus::Complete | MessageStatus::Truncated) {
                    continue;
                }
                let same_provider = m.provider.as_deref() == Some(provider_id);
                let raw: Option<Value> = m.raw.as_deref().and_then(|r| serde_json::from_str(r).ok());
                match raw {
                    Some(v) if same_provider && v.is_array() => out.push(ChatTurn { raw: Some(v), ..ChatTurn::assistant(m.content.clone()) }),
                    Some(v) if same_provider => match decode_turns(&v) {
                        Some(turns) if !turns.is_empty() => out.extend(turns),
                        _ if !m.content.is_empty() => out.push(ChatTurn::assistant(m.content.clone())),
                        _ => {}
                    },
                    _ if !m.content.is_empty() => out.push(ChatTurn::assistant(m.content.clone())),
                    _ => {}
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{ToolCall, ToolResult};

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
    fn skips_failed_turns_and_scopes_raw_to_provider() {
        let ms = vec![
            msg(Role::User, "q1", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "", MessageStatus::Error, Some("anthropic"), None),
            msg(Role::User, "q1 again", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "a1", MessageStatus::Complete, Some("anthropic"), Some(r#"[{"type":"text","text":"a1"}]"#.into())),
            msg(Role::User, "q2", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "partial", MessageStatus::Cancelled, Some("anthropic"), None),
            msg(Role::User, "q3", MessageStatus::Complete, None, None),
        ];
        let turns = build_turns(&ms, "anthropic");
        let texts: Vec<&str> = turns.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec!["q1", "q1 again", "a1", "q2", "q3"]);
        assert!(turns[2].raw.is_some());
        assert!(build_turns(&ms, "openai").iter().all(|t| t.raw.is_none()), "raw blocks never cross providers");
    }

    #[test]
    fn expands_tool_exchanges_for_the_same_provider_only() {
        let turns = vec![
            ChatTurn {
                tool_calls: vec![ToolCall { id: "t1".into(), name: "calculator".into(), input: json!({"expression":"1+1"}), invalid_input: None }],
                raw: Some(json!([{"type":"tool_use","id":"t1","name":"calculator","input":{"expression":"1+1"}}])),
                ..ChatTurn::assistant("")
            },
            ChatTurn { tool_results: vec![ToolResult { call_id: "t1".into(), content: "1+1 = 2".into(), is_error: false, media: vec![] }], ..ChatTurn::user("") },
            ChatTurn::assistant("It's 2."),
        ];
        let raw = encode_turns(&turns).to_string();
        let ms = vec![
            msg(Role::User, "1+1?", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "It's 2.", MessageStatus::Complete, Some("anthropic"), Some(raw)),
        ];
        let same = build_turns(&ms, "anthropic");
        assert_eq!(same.len(), 4);
        assert_eq!(same[1].tool_calls[0].id, "t1");
        assert_eq!(same[2].tool_results[0].content, "1+1 = 2");

        let other = build_turns(&ms, "openai");
        assert_eq!(other.len(), 2);
        assert_eq!(other[1].text, "It's 2.");
        assert!(other[1].tool_calls.is_empty());
    }

    #[test]
    fn user_turns_carry_their_stored_memory_block() {
        use crate::memory::retrieval::MemoryContext;
        let mut u = msg(Role::User, "What's my project?", MessageStatus::Complete, None, None);
        u.memory_context = Some(MemoryContext { items: vec![], rendered: "<memory>\n- fact\n</memory>".into() });
        let turns = build_turns(&[u], "anthropic");
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
        let turns = build_turns(&ms, "anthropic");
        assert_eq!(turns.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), vec!["q", "a", "q2", "b"]);
        assert!(turns.iter().all(|t| t.raw.is_none()));
    }
}
