//! Context budgeting: how big a request is, and how big it may be.
//!
//! Estimation is provider-neutral and deterministic — no tokenizer. English
//! text averages about 4 characters per token across current tokenizers; other
//! scripts are denser, so non-ASCII characters count more. Images and PDFs get
//! fixed costs. Estimates lean high, and the budget keeps a margin, so a
//! request that fits by this measure fits the real tokenizer too.

use crate::ai::capabilities::ModelCapabilities;
use crate::ai::{ChatTurn, Media, MediaKind, ToolDef};

/// Cost of the role / message framing around each turn or tool block.
const FRAMING: u32 = 8;
/// A typical screenshot or photo after provider downscaling.
pub const IMAGE_TOKENS: u32 = 1_600;
/// A PDF without extracted text (sent natively; size unknown).
const PDF_TOKENS: u32 = 3_000;
/// Above this, a bigger context only adds cost, latency and rate-limit
/// pressure for a personal assistant, so the budget stops growing (unless
/// AI_CONTEXT_WINDOW sets the window explicitly).
pub const PRACTICAL_CONTEXT: u32 = 128_000;
/// Output room reserved in the window.
const MAX_OUTPUT_RESERVE: u32 = 16_384;

/// Estimated tokens of a piece of text.
pub fn estimate_text(s: &str) -> u32 {
    let (mut ascii, mut other) = (0u32, 0u32);
    for c in s.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
    }
    // ASCII ≈ 4 chars/token; other scripts ≈ 1.3 chars/token.
    ascii.div_ceil(4) + (other * 3).div_ceil(4)
}

fn estimate_media(m: &Media) -> u32 {
    match m.kind {
        // Counted whether or not the bytes are loaded yet, so estimates don't
        // depend on hydration order.
        MediaKind::Image => IMAGE_TOKENS,
        MediaKind::Pdf => m.text.as_deref().map(estimate_text).unwrap_or(PDF_TOKENS).max(500),
    }
}

/// Estimated tokens of one turn, including tool calls, results, media and any
/// provider extras (counted even though only one provider replays them, so
/// the estimate errs high).
pub fn estimate_turn(t: &ChatTurn) -> u32 {
    let mut n = FRAMING + estimate_text(&t.text);
    for c in &t.tool_calls {
        n += FRAMING + estimate_text(&c.name) + estimate_text(&c.input.to_string());
    }
    for r in &t.tool_results {
        n += FRAMING + estimate_text(&r.content) + r.media.iter().map(estimate_media).sum::<u32>();
    }
    n += t.media.iter().map(estimate_media).sum::<u32>();
    if let Some(e) = &t.extras {
        n += estimate_text(&e.data.to_string()) / 2;
    }
    n
}

pub fn estimate_turns(turns: &[ChatTurn]) -> u32 {
    turns.iter().map(estimate_turn).sum()
}

/// The parts of every request that don't change: system prompt and tool definitions.
pub fn estimate_fixed(system: &str, tools: &[ToolDef]) -> u32 {
    FRAMING
        + estimate_text(system)
        + tools.iter().map(|t| FRAMING + estimate_text(&t.name) + estimate_text(&t.description) + estimate_text(&t.input_schema.to_string())).sum::<u32>()
}

/// How much input a request to a model may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// The window the budget works within (possibly capped at [`PRACTICAL_CONTEXT`]).
    pub window: u32,
    /// Room left for the reply.
    pub reserve_output: u32,
    /// Maximum estimated input tokens.
    pub input_limit: u32,
}

impl Budget {
    pub fn for_model(caps: &ModelCapabilities, max_output: u32) -> Self {
        let window = if caps.context_configured { caps.context_window } else { caps.context_window.min(PRACTICAL_CONTEXT) };
        let reserve_output = (window / 4).min(MAX_OUTPUT_RESERVE).min(max_output.max(1));
        let margin = window / 20; // estimation error
        Self { window, reserve_output, input_limit: window.saturating_sub(reserve_output + margin) }
    }
}

/// Shorten tool results in all but the most recent `keep_recent` turns that
/// carry results. Calls stay intact (the model still sees what it did); old
/// outputs keep their first lines. Returns whether anything changed.
pub fn shorten_old_tool_results(turns: &mut [ChatTurn], keep_recent: usize, max_chars: usize) -> bool {
    let mut seen = 0;
    let mut changed = false;
    for t in turns.iter_mut().rev() {
        if t.tool_results.is_empty() {
            continue;
        }
        seen += 1;
        if seen <= keep_recent {
            continue;
        }
        for r in &mut t.tool_results {
            // Already-shortened output can be shortened further on a tighter pass.
            let base = r.content.strip_suffix(SHORTENED_NOTE).map(|b| b.trim_end_matches('…')).unwrap_or(&r.content);
            if base.chars().count() > max_chars {
                r.content = format!("{}…{SHORTENED_NOTE}", base.chars().take(max_chars).collect::<String>());
                changed = true;
            }
            if !r.media.is_empty() {
                r.media.clear();
                r.content.push_str(" [Image omitted.]");
                changed = true;
            }
        }
    }
    changed
}

const SHORTENED_NOTE: &str = "\n[Earlier tool output shortened to save context.]";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::capabilities::capabilities_for;
    use crate::ai::{ProviderExtras, ToolCall, ToolResult};
    use serde_json::json;

    #[test]
    fn estimates_are_deterministic_and_lean_high() {
        assert_eq!(estimate_text(""), 0);
        assert_eq!(estimate_text("abcd"), 1);
        assert_eq!(estimate_text("Hello, IGRIS!"), 4);
        // Non-Latin scripts are denser.
        assert!(estimate_text("नमस्ते दुनिया") > estimate_text("hello world"));
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(100);
        assert_eq!(estimate_text(&text), estimate_text(&text), "deterministic");
        // ~4.5k chars of English ≈ 1.1k tokens.
        assert!((1_000..1_300).contains(&estimate_text(&text)));
    }

    #[test]
    fn turns_count_tools_media_and_extras() {
        let plain = estimate_turn(&ChatTurn::user("hi"));
        let call = ChatTurn {
            tool_calls: vec![ToolCall { id: "c".into(), name: "calculator".into(), input: json!({"expression": "2+2"}), invalid_input: None, extras: None }],
            extras: Some(ProviderExtras::new("anthropic", json!([{"type": "thinking", "signature": "x".repeat(400)}]))),
            ..ChatTurn::assistant("")
        };
        assert!(estimate_turn(&call) > plain + 50);
        let shot = Media {
            attachment_id: String::new(),
            kind: MediaKind::Image,
            mime: "image/jpeg".into(),
            name: "Screen".into(),
            data: Some("QUJD".into()),
            text: None,
        };
        let result =
            ChatTurn { tool_results: vec![ToolResult { call_id: "c".into(), content: "ok".into(), is_error: false, media: vec![shot] }], ..ChatTurn::user("") };
        assert!(estimate_turn(&result) >= IMAGE_TOKENS);
    }

    #[test]
    fn budgets_follow_the_model() {
        let claude = Budget::for_model(&capabilities_for("anthropic", "claude-opus-5-5", None), 64_000);
        assert_eq!(claude.window, PRACTICAL_CONTEXT, "large windows are capped for cost");
        assert_eq!(claude.reserve_output, 16_384);
        let local = Budget::for_model(&capabilities_for("local", "qwen2.5:7b", None), 64_000);
        assert_eq!(local.window, 16_384);
        assert_eq!(local.input_limit, 16_384 - 4_096 - 819);
        let explicit = Budget::for_model(&capabilities_for("anthropic", "claude-opus-5-5", Some(200_000)), 64_000);
        assert_eq!(explicit.window, 200_000, "an explicit AI_CONTEXT_WINDOW isn't capped");
    }

    #[test]
    fn shortens_only_older_tool_results() {
        let result = |s: &str| ChatTurn {
            tool_results: vec![ToolResult { call_id: "c".into(), content: s.into(), is_error: false, media: vec![] }],
            ..ChatTurn::user("")
        };
        let long = "x".repeat(2_000);
        let mut turns = vec![result(&long), ChatTurn::assistant("a"), result(&long), result(&long)];
        assert!(shorten_old_tool_results(&mut turns, 2, 300));
        assert!(turns[0].tool_results[0].content.len() < 400);
        assert_eq!(turns[2].tool_results[0].content.len(), 2_000);
        assert_eq!(turns[3].tool_results[0].content.len(), 2_000);
        assert!(!shorten_old_tool_results(&mut turns, 2, 300), "idempotent");
        assert!(shorten_old_tool_results(&mut turns, 2, 100), "a tighter pass shortens further");
        assert!(turns[0].tool_results[0].content.chars().count() < 200);
    }
}
