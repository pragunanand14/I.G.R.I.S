//! Builds the active provider from configuration.

use std::sync::Arc;

use serde::Serialize;

use super::anthropic::{self, AnthropicProvider};
use super::openai::OpenAiCompatibleProvider;
use super::AiProvider;
use crate::config::AppConfig;

/// Provider ids IGRIS knows about. Gemini is planned but not implemented.
pub const SUPPORTED: &[&str] = &["anthropic", "openai", "local"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub provider: Option<String>,
    /// Model from configuration (settings may override it per request).
    pub configured_model: Option<String>,
    pub ready: bool,
    /// Why the provider isn't usable, phrased as an action for the user.
    pub problem: Option<String>,
}

pub struct AiRuntime {
    pub provider: Option<Arc<dyn AiProvider>>,
    pub status: AiStatus,
}

/// Model used when neither settings nor AI_MODEL name one.
pub fn default_model(provider: &str) -> Option<&'static str> {
    match provider {
        "anthropic" => Some(anthropic::DEFAULT_MODEL),
        _ => None, // never guess model names for other providers
    }
}

impl AiRuntime {
    pub fn from_config(cfg: &AppConfig) -> Self {
        let provider_id = cfg.ai_provider.clone();
        let configured_model = cfg.ai_model.clone().or_else(|| provider_id.as_deref().and_then(default_model).map(str::to_string));
        let fail = |problem: String| AiRuntime {
            provider: None,
            status: AiStatus { provider: provider_id.clone(), configured_model: configured_model.clone(), ready: false, problem: Some(problem) },
        };

        let Some(id) = provider_id.as_deref() else {
            return fail("No AI provider configured. Set AI_PROVIDER (anthropic, openai or local) in your .env file.".into());
        };
        let built: Result<Arc<dyn AiProvider>, String> = match id {
            "anthropic" => match &cfg.ai_api_key {
                None => Err("AI_API_KEY is not set for the Anthropic provider.".into()),
                Some(k) => AnthropicProvider::new(k.clone(), cfg.ai_base_url.clone()).map(|p| Arc::new(p) as _).map_err(|e| e.message),
            },
            "openai" => match &cfg.ai_api_key {
                None => Err("AI_API_KEY is not set for the OpenAI provider.".into()),
                Some(k) => OpenAiCompatibleProvider::openai(k.clone(), cfg.ai_base_url.clone()).map(|p| Arc::new(p) as _).map_err(|e| e.message),
            },
            "local" => OpenAiCompatibleProvider::local(cfg.ai_api_key.clone(), cfg.ai_base_url.clone()).map(|p| Arc::new(p) as _).map_err(|e| e.message),
            "gemini" => Err("The Gemini provider is not implemented yet. Use anthropic, openai or local.".into()),
            other => Err(format!("Unknown AI_PROVIDER '{other}'. Supported: {}.", SUPPORTED.join(", "))),
        };
        match built {
            Err(problem) => fail(problem),
            Ok(provider) if configured_model.is_none() => {
                let _ = provider;
                fail(format!("AI_MODEL must be set for the {id} provider."))
            }
            Ok(provider) => AiRuntime {
                provider: Some(provider),
                status: AiStatus { provider: provider_id.clone(), configured_model, ready: true, problem: None },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> AppConfig {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        AppConfig::from_map(|k| m.get(k).cloned())
    }

    #[test]
    fn not_ready_without_provider() {
        let rt = AiRuntime::from_config(&cfg(&[]));
        assert!(!rt.status.ready);
        assert!(rt.provider.is_none());
        assert!(rt.status.problem.unwrap().contains("AI_PROVIDER"));
    }

    #[test]
    fn anthropic_requires_key_and_defaults_model() {
        assert!(!AiRuntime::from_config(&cfg(&[("AI_PROVIDER", "anthropic")])).status.ready);
        let rt = AiRuntime::from_config(&cfg(&[("AI_PROVIDER", "Anthropic"), ("AI_API_KEY", "k")]));
        assert!(rt.status.ready);
        assert_eq!(rt.status.configured_model.as_deref(), Some("claude-opus-5-5"));
    }

    #[test]
    fn openai_and_local_require_explicit_model() {
        let rt = AiRuntime::from_config(&cfg(&[("AI_PROVIDER", "openai"), ("AI_API_KEY", "k")]));
        assert!(!rt.status.ready);
        assert!(rt.status.problem.unwrap().contains("AI_MODEL"));
        let rt = AiRuntime::from_config(&cfg(&[("AI_PROVIDER", "local"), ("AI_MODEL", "llama3.2")]));
        assert!(rt.status.ready, "local servers need no key");
    }

    #[test]
    fn unknown_and_unimplemented_providers_are_reported() {
        assert!(AiRuntime::from_config(&cfg(&[("AI_PROVIDER", "gemini"), ("AI_API_KEY", "k")])).status.problem.unwrap().contains("not implemented"));
        assert!(AiRuntime::from_config(&cfg(&[("AI_PROVIDER", "foo")])).status.problem.unwrap().contains("Unknown"));
    }
}
