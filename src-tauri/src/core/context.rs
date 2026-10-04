//! Builds model context from stored conversation history.

use crate::ai::{ChatTurn, Role};
use crate::conversations::{Message, MessageStatus};

/// Convert stored messages into provider turns.
///
/// * Failed, cancelled and refused assistant turns are excluded — they aren't
///   real answers and partial provider content can't be replayed safely.
/// * Provider-native `raw` content is only replayed to the provider that
///   produced it; otherwise the plain text is sent.
pub fn build_turns(messages: &[Message], provider_id: &str) -> Vec<ChatTurn> {
    messages
        .iter()
        .filter(|m| match m.role {
            Role::User => true,
            Role::Assistant => matches!(m.status, MessageStatus::Complete | MessageStatus::Truncated) && !m.content.is_empty(),
        })
        .map(|m| ChatTurn {
            role: m.role,
            text: m.content.clone(),
            raw: (m.role == Role::Assistant && m.provider.as_deref() == Some(provider_id))
                .then(|| m.raw.as_deref().and_then(|r| serde_json::from_str(r).ok()))
                .flatten(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: Role, content: &str, status: MessageStatus, provider: Option<&str>, raw: Option<&str>) -> Message {
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
            raw: raw.map(Into::into),
            input_tokens: None,
            output_tokens: None,
            created_at: String::new(),
        }
    }

    #[test]
    fn skips_failed_turns_and_scopes_raw_to_provider() {
        let ms = vec![
            msg(Role::User, "q1", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "", MessageStatus::Error, Some("anthropic"), None),
            msg(Role::User, "q1 again", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "a1", MessageStatus::Complete, Some("anthropic"), Some(r#"[{"type":"text","text":"a1"}]"#)),
            msg(Role::User, "q2", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "partial", MessageStatus::Cancelled, Some("anthropic"), None),
            msg(Role::User, "q3", MessageStatus::Complete, None, None),
        ];
        let turns = build_turns(&ms, "anthropic");
        let texts: Vec<&str> = turns.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec!["q1", "q1 again", "a1", "q2", "q3"]);
        assert!(turns[2].raw.is_some());

        let other = build_turns(&ms, "openai");
        assert!(other.iter().all(|t| t.raw.is_none()), "raw blocks never cross providers");
    }

    #[test]
    fn corrupt_raw_falls_back_to_text() {
        let ms = vec![
            msg(Role::User, "q", MessageStatus::Complete, None, None),
            msg(Role::Assistant, "a", MessageStatus::Complete, Some("anthropic"), Some("not json")),
        ];
        let turns = build_turns(&ms, "anthropic");
        assert!(turns[1].raw.is_none());
        assert_eq!(turns[1].text, "a");
    }
}
