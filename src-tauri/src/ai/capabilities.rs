//! What a model can do, as far as IGRIS needs to know.
//!
//! Providers don't expose these reliably, so IGRIS keeps a small table of
//! model families with conservative fallbacks. `AI_CONTEXT_WINDOW` overrides
//! the context window (needed for local servers, whose window is whatever the
//! server was started with).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapabilities {
    /// Total tokens the model accepts (input + output).
    pub context_window: u32,
    pub tools: bool,
    /// Accepts images. Unknown models are assumed not to.
    pub vision: bool,
    /// Has a reasoning / thinking phase that response depth can steer.
    pub reasoning: bool,
    /// Whether the values came from the table (false = fallback guesses).
    pub known: bool,
    /// The context window was set explicitly (AI_CONTEXT_WINDOW).
    pub context_configured: bool,
}

/// Context assumed for models IGRIS doesn't recognise.
pub const FALLBACK_CONTEXT: u32 = 32_768;
/// Local servers (Ollama, LM Studio): IGRIS's setup guide uses 16k; override with AI_CONTEXT_WINDOW.
pub const LOCAL_CONTEXT: u32 = 16_384;

fn caps(context_window: u32, vision: bool, reasoning: bool) -> ModelCapabilities {
    ModelCapabilities { context_window, tools: true, vision, reasoning, known: true, context_configured: false }
}

/// Capabilities of `model` on `provider`. `context_override` (AI_CONTEXT_WINDOW) wins.
pub fn capabilities_for(provider: &str, model: &str, context_override: Option<u32>) -> ModelCapabilities {
    let m = model.to_ascii_lowercase();
    let mut c = match provider {
        "anthropic" => caps(200_000, true, !m.starts_with("claude-3")),
        "gemini" => caps(1_048_576, true, m.contains("2.5") || m.contains("gemini-3") || m.contains("latest")),
        "openai" => {
            if m.starts_with("gpt-4.1") {
                caps(1_047_576, true, false)
            } else if m.starts_with("gpt-5") {
                caps(400_000, true, true)
            } else if m.starts_with("gpt-4o") || m.starts_with("gpt-4-turbo") {
                caps(128_000, true, false)
            } else if m.starts_with("o1-mini") || m.starts_with("o3-mini") {
                caps(128_000, false, true)
            } else if m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4") {
                caps(200_000, true, true)
            } else if m.starts_with("gpt-3.5") {
                caps(16_385, false, false)
            } else {
                ModelCapabilities { context_window: FALLBACK_CONTEXT, tools: true, vision: false, reasoning: false, known: false, context_configured: false }
            }
        }
        "local" => {
            let vision = ["llava", "vision", "-vl", "vl:", "gemma3", "minicpm-v", "moondream", "bakllava"].iter().any(|v| m.contains(v));
            let reasoning = ["qwen3", "deepseek-r1", "qwq", "magistral"].iter().any(|r| m.contains(r));
            ModelCapabilities { context_window: LOCAL_CONTEXT, tools: true, vision, reasoning, known: false, context_configured: false }
        }
        _ => ModelCapabilities { context_window: FALLBACK_CONTEXT, tools: true, vision: false, reasoning: false, known: false, context_configured: false },
    };
    if let Some(n) = context_override.filter(|n| *n >= 2_048) {
        c.context_window = n;
        c.context_configured = true;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_families_and_fallbacks() {
        let c = capabilities_for("anthropic", "claude-opus-5-5", None);
        assert_eq!((c.context_window, c.vision, c.reasoning, c.known), (200_000, true, true, true));
        let g = capabilities_for("gemini", "gemini-3.5-flash-lite", None);
        assert!(g.vision && g.context_window > 1_000_000);
        assert!(!capabilities_for("openai", "o3-mini", None).vision);
        assert!(capabilities_for("openai", "gpt-4o-mini", None).vision);
        let unknown = capabilities_for("openai", "some-new-model", None);
        assert_eq!((unknown.context_window, unknown.vision, unknown.known), (FALLBACK_CONTEXT, false, false));
        let local = capabilities_for("local", "qwen2.5:7b", None);
        assert_eq!((local.context_window, local.vision), (LOCAL_CONTEXT, false));
        assert!(capabilities_for("local", "llava:13b", None).vision);
    }

    #[test]
    fn context_override_wins_when_sensible() {
        assert_eq!(capabilities_for("local", "qwen2.5:7b", Some(4_096)).context_window, 4_096);
        assert_eq!(capabilities_for("local", "qwen2.5:7b", Some(10)).context_window, LOCAL_CONTEXT, "nonsense values are ignored");
    }
}
