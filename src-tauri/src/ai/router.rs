//! Model routing: IGRIS asks for a *role*, the router picks the model.
//!
//! Roles are IGRIS concepts, not vendor ones:
//! * `Chat` — conversation and tool use (AI_MODEL, or the Settings override);
//! * `Vision` — requests with images (AI_VISION_MODEL; defaults to the chat model);
//! * `Fast` — cheap background work such as compacting history (AI_FAST_MODEL;
//!   defaults to the chat model).
//!
//! With no extra configuration every role resolves to the chat model, so
//! behaviour is exactly what it was before the router existed. All roles use
//! the configured provider; routing to other providers is future work.

use std::sync::Arc;

use serde::Serialize;

use super::capabilities::{capabilities_for, ModelCapabilities};
use super::AiProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelRole {
    Chat,
    Vision,
    Fast,
}

/// What a request needs from its model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Needs {
    pub vision: bool,
    pub tools: bool,
}

/// A resolved model: who to call, which model, and what it can do.
#[derive(Clone)]
pub struct Route {
    pub role: ModelRole,
    pub provider: Arc<dyn AiProvider>,
    pub model: String,
    pub caps: ModelCapabilities,
}

impl Route {
    pub fn provider_id(&self) -> &'static str {
        self.provider.id()
    }

    pub fn satisfies(&self, needs: Needs) -> bool {
        (!needs.vision || self.caps.vision) && (!needs.tools || self.caps.tools)
    }
}

impl std::fmt::Debug for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Route").field("role", &self.role).field("provider", &self.provider_id()).field("model", &self.model).field("caps", &self.caps).finish()
    }
}

pub struct ModelRouter {
    provider: Arc<dyn AiProvider>,
    chat_model: String,
    vision_model: Option<String>,
    fast_model: Option<String>,
    context_window: Option<u32>,
}

impl ModelRouter {
    /// One provider and model for every role (the pre-router behaviour).
    pub fn new(provider: Arc<dyn AiProvider>, chat_model: impl Into<String>) -> Self {
        Self { provider, chat_model: chat_model.into(), vision_model: None, fast_model: None, context_window: None }
    }

    pub fn with_role_model(mut self, role: ModelRole, model: Option<String>) -> Self {
        match role {
            ModelRole::Chat => {
                if let Some(m) = model {
                    self.chat_model = m;
                }
            }
            ModelRole::Vision => self.vision_model = model,
            ModelRole::Fast => self.fast_model = model,
        }
        self
    }

    /// AI_CONTEXT_WINDOW: the context size of this provider's models (local servers).
    pub fn with_context_window(mut self, tokens: Option<u32>) -> Self {
        self.context_window = tokens;
        self
    }

    pub fn provider_id(&self) -> &'static str {
        self.provider.id()
    }

    /// The model for `role`. `chat_override` is the user's model choice in
    /// Settings; roles without their own model follow it.
    pub fn route(&self, role: ModelRole, chat_override: Option<&str>) -> Route {
        let chat = chat_override.filter(|m| !m.trim().is_empty()).unwrap_or(&self.chat_model);
        let model = match role {
            ModelRole::Chat => chat,
            ModelRole::Vision => self.vision_model.as_deref().unwrap_or(chat),
            ModelRole::Fast => self.fast_model.as_deref().unwrap_or(chat),
        }
        .to_string();
        let caps = capabilities_for(self.provider.id(), &model, self.context_window);
        Route { role, provider: self.provider.clone(), model, caps }
    }

    /// A model for `role` that can do what the request needs: the role's model
    /// if it can, else the vision model for image requests. An honest,
    /// actionable error when nothing configured fits.
    pub fn route_for(&self, role: ModelRole, needs: Needs, chat_override: Option<&str>) -> Result<Route, String> {
        let primary = self.route(role, chat_override);
        if primary.satisfies(needs) {
            return Ok(primary);
        }
        if needs.vision {
            let vision = self.route(ModelRole::Vision, chat_override);
            if vision.satisfies(needs) {
                return Ok(vision);
            }
        }
        let missing = if needs.vision && !primary.caps.vision { "read images" } else { "use tools" };
        Err(format!("The model {} isn't known to {missing}. Set AI_VISION_MODEL to a model that can, or choose a different model in Settings.", primary.model))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::openai::OpenAiCompatibleProvider;

    fn local(model: &str) -> ModelRouter {
        ModelRouter::new(Arc::new(OpenAiCompatibleProvider::local(None, None).unwrap()), model)
    }

    #[test]
    fn every_role_defaults_to_the_chat_model() {
        let r = local("qwen2.5:7b");
        for role in [ModelRole::Chat, ModelRole::Vision, ModelRole::Fast] {
            let route = r.route(role, None);
            assert_eq!((route.model.as_str(), route.provider_id()), ("qwen2.5:7b", "local"));
        }
        // The Settings override applies to roles that follow chat.
        assert_eq!(r.route(ModelRole::Fast, Some("llama3.2")).model, "llama3.2");
        assert_eq!(r.route(ModelRole::Chat, Some("  ")).model, "qwen2.5:7b", "blank override is ignored");
    }

    #[test]
    fn roles_use_their_own_models_when_configured() {
        let r = local("qwen2.5:7b").with_role_model(ModelRole::Vision, Some("llava:13b".into())).with_role_model(ModelRole::Fast, Some("qwen2.5:1.5b".into()));
        assert_eq!(r.route(ModelRole::Vision, None).model, "llava:13b");
        assert_eq!(r.route(ModelRole::Fast, Some("llama3.2")).model, "qwen2.5:1.5b", "explicit role models aren't overridden");
        assert_eq!(r.route(ModelRole::Chat, None).model, "qwen2.5:7b");
    }

    #[test]
    fn capability_matching_and_honest_errors() {
        let needs_vision = Needs { vision: true, tools: true };
        // Chat model can't see; a configured vision model is used instead.
        let r = local("qwen2.5:7b").with_role_model(ModelRole::Vision, Some("llava:13b".into()));
        assert_eq!(r.route_for(ModelRole::Chat, needs_vision, None).unwrap().model, "llava:13b");
        // Nothing fits: an actionable error, not a silent success.
        let e = local("qwen2.5:7b").route_for(ModelRole::Chat, needs_vision, None).unwrap_err();
        assert!(e.contains("qwen2.5:7b") && e.contains("AI_VISION_MODEL"), "{e}");
        // No special needs: the chat model.
        assert_eq!(local("qwen2.5:7b").route_for(ModelRole::Chat, Needs::default(), None).unwrap().model, "qwen2.5:7b");
    }

    #[test]
    fn unknown_models_get_conservative_capabilities() {
        let r = ModelRouter::new(Arc::new(OpenAiCompatibleProvider::openai("k".into(), None).unwrap()), "brand-new-model");
        let route = r.route(ModelRole::Chat, None);
        assert!(!route.caps.known);
        assert_eq!(route.caps.context_window, crate::ai::capabilities::FALLBACK_CONTEXT);
        let r = local("qwen2.5:7b").with_context_window(Some(8_192));
        assert_eq!(r.route(ModelRole::Chat, None).caps.context_window, 8_192);
    }
}
