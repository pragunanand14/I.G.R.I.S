//! Builds the active provider from configuration.

use std::sync::Arc;

use serde::Serialize;

use super::anthropic::{self, AnthropicProvider};
use super::openai::OpenAiCompatibleProvider;
use super::router::{ModelRole, ModelRouter};
use super::AiProvider;
use crate::config::AppConfig;

/// Provider ids IGRIS knows about.
pub const SUPPORTED: &[&str] = &["anthropic", "openai", "gemini", "local"];

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
    /// Picks the model for each request (chat / vision / fast).
    pub router: Option<Arc<ModelRouter>>,
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
            router: None,
            status: AiStatus { provider: provider_id.clone(), configured_model: configured_model.clone(), ready: false, problem: Some(problem) },
        };

        let Some(id) = provider_id.as_deref() else {
            return fail("No AI provider configured. Set AI_PROVIDER (anthropic, openai, gemini or local) in your .env file.".into());
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
            "gemini" => match &cfg.ai_api_key {
                None => Err("AI_API_KEY is not set for the Gemini provider. Create a free key at https://aistudio.google.com/apikey.".into()),
                Some(k) => OpenAiCompatibleProvider::gemini(k.clone(), cfg.ai_base_url.clone()).map(|p| Arc::new(p) as _).map_err(|e| e.message),
            },
            other => Err(format!("Unknown AI_PROVIDER '{other}'. Supported: {}.", SUPPORTED.join(", "))),
        };
        match built {
            Err(problem) => fail(problem),
            Ok(provider) => match configured_model.clone() {
                None => fail(format!("AI_MODEL must be set for the {id} provider.")),
                Some(model) => {
                    let router = ModelRouter::new(provider, model)
                        .with_role_model(ModelRole::Vision, cfg.ai_vision_model.clone())
                        .with_role_model(ModelRole::Fast, cfg.ai_fast_model.clone())
                        .with_context_window(cfg.ai_context_window);
                    AiRuntime {
                        router: Some(Arc::new(router)),
                        status: AiStatus { provider: provider_id.clone(), configured_model, ready: true, problem: None },
                    }
                }
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
        assert!(rt.router.is_none());
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
    fn gemini_needs_a_key_and_a_model() {
        assert!(AiRuntime::from_config(&cfg(&[("AI_PROVIDER", "gemini")])).status.problem.unwrap().contains("aistudio.google.com"));
        assert!(AiRuntime::from_config(&cfg(&[("AI_PROVIDER", "gemini"), ("AI_API_KEY", "k")])).status.problem.unwrap().contains("AI_MODEL"));
        let rt = AiRuntime::from_config(&cfg(&[("AI_PROVIDER", "gemini"), ("AI_API_KEY", "k"), ("AI_MODEL", "gemini-2.5-flash")]));
        assert!(rt.status.ready);
        assert_eq!(rt.router.unwrap().provider_id(), "gemini");
    }

    #[test]
    fn router_roles_come_from_configuration() {
        let rt = AiRuntime::from_config(&cfg(&[("AI_PROVIDER", "gemini"), ("AI_API_KEY", "k"), ("AI_MODEL", "gemini-3.5-flash-lite")]));
        let r = rt.router.unwrap();
        assert_eq!(r.route(ModelRole::Fast, None).model, "gemini-3.5-flash-lite", "defaults: unchanged behaviour");
        let rt = AiRuntime::from_config(&cfg(&[
            ("AI_PROVIDER", "local"),
            ("AI_MODEL", "qwen2.5:7b"),
            ("AI_VISION_MODEL", "llava"),
            ("AI_FAST_MODEL", "qwen2.5:1.5b"),
            ("AI_CONTEXT_WINDOW", "8_192"),
        ]));
        let r = rt.router.unwrap();
        assert_eq!(r.route(ModelRole::Vision, None).model, "llava");
        assert_eq!(r.route(ModelRole::Fast, None).model, "qwen2.5:1.5b");
        assert_eq!(r.route(ModelRole::Chat, None).caps.context_window, 8_192);
    }

    #[test]
    fn unknown_providers_are_reported() {
        assert!(AiRuntime::from_config(&cfg(&[("AI_PROVIDER", "foo")])).status.problem.unwrap().contains("Unknown"));
    }
}
