//! AI provider abstraction.
//!
//! IGRIS talks to models only through [`AiProvider`]. Providers run in the
//! native backend, so API keys never reach the UI. Each provider streams text
//! deltas through a callback and returns a [`Completion`] that the
//! orchestrator persists.

pub mod anthropic;
pub mod http;
pub mod openai;
pub mod registry;
pub mod sse;
#[cfg(test)]
pub mod testutil;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

pub use registry::{AiRuntime, AiStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// One turn of conversation context sent to a provider.
#[derive(Debug, Clone)]
pub struct ChatTurn {
    pub role: Role,
    pub text: String,
    /// Provider-native content (e.g. Anthropic content blocks) to replay
    /// unchanged. Only set when produced by the same provider.
    pub raw: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub system: String,
    pub turns: Vec<ChatTurn>,
    pub max_tokens: u32,
    pub effort: Option<Effort>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    TextDelta(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    Refusal { category: Option<String> },
    Other { reason: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Completion {
    pub text: String,
    pub raw: Option<serde_json::Value>,
    /// The model that actually served the response (may differ on fallback).
    pub model: String,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiErrorKind {
    NotConfigured,
    Authentication,
    PermissionDenied,
    RateLimited,
    Overloaded,
    InvalidRequest,
    NotFound,
    Server,
    Network,
    Timeout,
    Cancelled,
    Protocol,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct AiError {
    pub kind: AiErrorKind,
    pub message: String,
    pub retry_after: Option<Duration>,
}

impl AiError {
    pub fn new(kind: AiErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), retry_after: None }
    }

    /// Transient failures worth retrying before any output has streamed.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self.kind,
            AiErrorKind::RateLimited | AiErrorKind::Overloaded | AiErrorKind::Server | AiErrorKind::Network | AiErrorKind::Timeout
        )
    }
}

pub type AiResult<T> = Result<T, AiError>;

pub type EventSink<'a> = &'a mut (dyn FnMut(StreamEvent) + Send);

#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    /// Stable provider id (`anthropic`, `openai`, `local`).
    fn id(&self) -> &'static str;

    /// Stream a response. Must honour `cancel` promptly and return
    /// `AiErrorKind::Cancelled` when cancelled.
    async fn stream(&self, req: &ChatRequest, cancel: &CancellationToken, on_event: EventSink<'_>) -> AiResult<Completion>;
}
