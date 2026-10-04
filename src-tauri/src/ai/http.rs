//! Shared HTTP plumbing for providers: client construction, status → error
//! mapping, and retry of transient failures before streaming starts.

use std::time::Duration;

use reqwest::{RequestBuilder, Response, StatusCode};
use tokio_util::sync::CancellationToken;

use super::{AiError, AiErrorKind, AiResult};

pub const MAX_ATTEMPTS: u32 = 3;
const MAX_RETRY_DELAY: Duration = Duration::from_secs(20);

pub fn client() -> AiResult<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        // Idle timeout between reads; long generations stream continuously.
        .read_timeout(Duration::from_secs(180))
        .user_agent(concat!("IGRIS/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| AiError::new(AiErrorKind::Network, format!("Could not initialise HTTP client: {e}")))
}

pub fn map_reqwest_error(err: reqwest::Error) -> AiError {
    if err.is_timeout() {
        AiError::new(AiErrorKind::Timeout, "The AI provider took too long to respond.")
    } else if err.is_connect() {
        AiError::new(AiErrorKind::Network, "Couldn't reach the AI provider. Check your internet connection.")
    } else {
        AiError::new(AiErrorKind::Network, format!("Network error while talking to the AI provider: {err}"))
    }
}

/// Map a non-success HTTP response to an actionable error.
/// `body_message` is the provider's own error text, if it could be parsed.
pub fn status_error(status: StatusCode, body_message: Option<String>, retry_after: Option<Duration>, provider: &str) -> AiError {
    let detail = body_message.map(|m| format!(" ({m})")).unwrap_or_default();
    let (kind, msg) = match status.as_u16() {
        400 | 413 | 422 => (AiErrorKind::InvalidRequest, format!("{provider} rejected the request{detail}.")),
        401 => (AiErrorKind::Authentication, format!("{provider} rejected the API key. Check AI_API_KEY in your .env file.")),
        403 => (AiErrorKind::PermissionDenied, format!("The API key doesn't have access to this model or feature{detail}.")),
        404 => (AiErrorKind::NotFound, format!("{provider} couldn't find that model or endpoint{detail}. Check AI_MODEL / AI_BASE_URL.")),
        408 => (AiErrorKind::Timeout, format!("{provider} timed out{detail}.")),
        429 => (AiErrorKind::RateLimited, format!("{provider} rate limit reached{detail}. Try again shortly.")),
        529 => (AiErrorKind::Overloaded, format!("{provider} is temporarily overloaded. Try again shortly.")),
        s if s >= 500 => (AiErrorKind::Server, format!("{provider} had a server error ({s}){detail}.")),
        s => (AiErrorKind::Protocol, format!("Unexpected response from {provider} ({s}){detail}.")),
    };
    AiError { kind, message: msg, retry_after }
}

pub fn retry_after(resp: &Response) -> Option<Duration> {
    resp.headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(|s| Duration::from_secs_f64(s).min(MAX_RETRY_DELAY))
}

/// Send a request, retrying transient failures with backoff. `build` is
/// called per attempt; `on_error` turns a non-2xx response into an error.
pub async fn send_with_retry<B, E, Fut>(cancel: &CancellationToken, build: B, on_error: E) -> AiResult<Response>
where
    B: Fn() -> RequestBuilder,
    E: Fn(Response) -> Fut,
    Fut: std::future::Future<Output = AiError>,
{
    let mut attempt = 0;
    loop {
        attempt += 1;
        let result = tokio::select! {
            _ = cancel.cancelled() => return Err(cancelled()),
            r = build().send() => r,
        };
        let err = match result {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => on_error(resp).await,
            Err(e) => map_reqwest_error(e),
        };
        if !err.is_retryable() || attempt >= MAX_ATTEMPTS {
            return Err(err);
        }
        let delay = err.retry_after.unwrap_or_else(|| Duration::from_millis(800 * 2u64.pow(attempt - 1)));
        tracing::warn!(event = "AI_REQUEST_RETRY", attempt, kind = ?err.kind, delay_ms = delay.as_millis() as u64);
        tokio::select! {
            _ = cancel.cancelled() => return Err(cancelled()),
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

pub fn cancelled() -> AiError {
    AiError::new(AiErrorKind::Cancelled, "Stopped.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_statuses_to_kinds() {
        let k = |s: u16| status_error(StatusCode::from_u16(s).unwrap(), None, None, "P").kind;
        assert_eq!(k(400), AiErrorKind::InvalidRequest);
        assert_eq!(k(401), AiErrorKind::Authentication);
        assert_eq!(k(403), AiErrorKind::PermissionDenied);
        assert_eq!(k(404), AiErrorKind::NotFound);
        assert_eq!(k(429), AiErrorKind::RateLimited);
        assert_eq!(k(529), AiErrorKind::Overloaded);
        assert_eq!(k(503), AiErrorKind::Server);
    }

    #[test]
    fn auth_error_is_actionable_and_not_retryable() {
        let e = status_error(StatusCode::UNAUTHORIZED, None, None, "Anthropic");
        assert!(e.message.contains("AI_API_KEY"));
        assert!(!e.is_retryable());
        assert!(status_error(StatusCode::TOO_MANY_REQUESTS, None, None, "A").is_retryable());
    }
}
