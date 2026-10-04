//! OpenAI-compatible Chat Completions provider. Covers OpenAI itself and local
//! servers that implement the same API (Ollama, LM Studio, llama.cpp server).

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::http::{self, cancelled};
use super::sse::SseParser;
use super::{AiError, AiErrorKind, AiProvider, AiResult, ChatRequest, Completion, EventSink, Role, StopReason, StreamEvent, Usage};

pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const LOCAL_BASE_URL: &str = "http://localhost:11434/v1";

pub struct OpenAiCompatibleProvider {
    id: &'static str,
    label: &'static str,
    client: reqwest::Client,
    api_key: Option<String>,
    base_url: String,
}

impl OpenAiCompatibleProvider {
    pub fn openai(api_key: String, base_url: Option<String>) -> AiResult<Self> {
        Self::build("openai", "OpenAI", Some(api_key), base_url.unwrap_or_else(|| OPENAI_BASE_URL.into()))
    }

    pub fn local(api_key: Option<String>, base_url: Option<String>) -> AiResult<Self> {
        Self::build("local", "The local model server", api_key, base_url.unwrap_or_else(|| LOCAL_BASE_URL.into()))
    }

    fn build(id: &'static str, label: &'static str, api_key: Option<String>, base_url: String) -> AiResult<Self> {
        Ok(Self { id, label, client: http::client()?, api_key, base_url: base_url.trim_end_matches('/').to_string() })
    }
}

pub fn build_body(req: &ChatRequest, include_usage: bool) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": req.system })];
    messages.extend(req.turns.iter().map(|t| {
        let role = match t.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        json!({ "role": role, "content": t.text })
    }));
    let mut body = json!({ "model": req.model, "stream": true, "messages": messages });
    if include_usage {
        body["stream_options"] = json!({ "include_usage": true });
    }
    body
}

#[async_trait::async_trait]
impl AiProvider for OpenAiCompatibleProvider {
    fn id(&self) -> &'static str {
        self.id
    }

    async fn stream(&self, req: &ChatRequest, cancel: &CancellationToken, on_event: EventSink<'_>) -> AiResult<Completion> {
        let body = build_body(req, self.id == "openai");
        let url = format!("{}/chat/completions", self.base_url);
        let label = self.label;

        let resp = http::send_with_retry(
            cancel,
            || {
                let rb = self.client.post(&url).json(&body);
                match &self.api_key {
                    Some(k) => rb.bearer_auth(k),
                    None => rb,
                }
            },
            |resp| async move {
                let status = resp.status();
                let retry = http::retry_after(&resp);
                let msg = resp.json::<Value>().await.ok().and_then(|v| v["error"]["message"].as_str().map(str::to_string));
                http::status_error(status, msg, retry, label)
            },
        )
        .await
        .map_err(|e| {
            if e.kind == AiErrorKind::Network && self.id == "local" {
                AiError::new(AiErrorKind::Network, format!("Couldn't reach the local model server at {}. Is it running?", self.base_url))
            } else {
                e
            }
        })?;

        let mut text = String::new();
        let mut model = req.model.clone();
        let mut stop: Option<StopReason> = None;
        let mut usage = Usage::default();
        let mut done = false;
        let mut parser = SseParser::new();
        let mut bytes = resp.bytes_stream();

        let mut handle = |data: &str| -> AiResult<bool> {
            if data.trim() == "[DONE]" {
                return Ok(true);
            }
            let v: Value = serde_json::from_str(data)
                .map_err(|e| AiError::new(AiErrorKind::Protocol, format!("Malformed stream event from {label}: {e}")))?;
            if let Some(msg) = v["error"]["message"].as_str() {
                return Err(AiError::new(AiErrorKind::Server, format!("{label}: {msg}")));
            }
            if let Some(m) = v["model"].as_str() {
                model = m.to_string();
            }
            if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
                usage.input_tokens = u["prompt_tokens"].as_u64();
                usage.output_tokens = u["completion_tokens"].as_u64();
            }
            if let Some(choice) = v["choices"].get(0) {
                for field in ["content", "refusal"] {
                    if let Some(t) = choice["delta"][field].as_str().filter(|t| !t.is_empty()) {
                        text.push_str(t);
                        on_event(StreamEvent::TextDelta(t.to_string()));
                        if field == "refusal" {
                            stop = Some(StopReason::Refusal { category: None });
                        }
                    }
                }
                if let Some(r) = choice["finish_reason"].as_str() {
                    stop = Some(match r {
                        "stop" => stop.take().unwrap_or(StopReason::EndTurn),
                        "length" => StopReason::MaxTokens,
                        "content_filter" => StopReason::Refusal { category: Some("content_filter".into()) },
                        other => StopReason::Other { reason: other.to_string() },
                    });
                }
            }
            Ok(false)
        };

        'outer: loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(cancelled()),
                c = bytes.next() => c,
            };
            let Some(chunk) = chunk else { break };
            for ev in parser.push(&chunk.map_err(http::map_reqwest_error)?) {
                if handle(&ev.data)? {
                    done = true;
                    break 'outer;
                }
            }
        }
        if !done {
            if let Some(ev) = parser.finish() {
                done = handle(&ev.data)?;
            }
        }
        // Some servers close without [DONE] but did send a finish_reason.
        if !done && stop.is_none() {
            return Err(AiError::new(AiErrorKind::Network, "The connection closed before the response finished."));
        }

        Ok(Completion { text, raw: None, model, stop_reason: stop.unwrap_or(StopReason::EndTurn), usage })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::testutil::MockServer;
    use crate::ai::ChatTurn;

    fn req() -> ChatRequest {
        ChatRequest {
            model: "test-model".into(),
            system: "sys".into(),
            turns: vec![
                ChatTurn { role: Role::User, text: "hi".into(), raw: None },
                ChatTurn { role: Role::Assistant, text: "hello".into(), raw: Some(json!([{"type":"text"}])) },
                ChatTurn { role: Role::User, text: "again".into(), raw: None },
            ],
            max_tokens: 1000,
            effort: None,
        }
    }

    #[test]
    fn body_puts_system_first_and_sends_text_only() {
        let b = build_body(&req(), true);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][2]["content"], "hello");
        assert_eq!(b["stream_options"]["include_usage"], true);
        assert!(build_body(&req(), false).get("stream_options").is_none());
    }

    #[tokio::test]
    async fn streams_chat_completion_chunks() {
        let body = [
            json!({"model":"test-model-2","choices":[{"delta":{"role":"assistant","content":"Hel"}}]}),
            json!({"choices":[{"delta":{"content":"lo"},"finish_reason":"stop"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":2}}),
        ]
        .iter()
        .map(|v| format!("data: {v}\n\n"))
        .collect::<String>()
            + "data: [DONE]\n\n";
        let server = MockServer::start(vec![(200, "text/event-stream", body)]).await;
        let p = OpenAiCompatibleProvider::openai("sk-test".into(), Some(server.url())).unwrap();
        let mut deltas = Vec::new();
        let c = p
            .stream(&req(), &CancellationToken::new(), &mut |e| {
                let StreamEvent::TextDelta(t) = e;
                deltas.push(t)
            })
            .await
            .unwrap();
        assert_eq!(deltas.concat(), "Hello");
        assert_eq!(c.text, "Hello");
        assert_eq!(c.model, "test-model-2");
        assert_eq!(c.stop_reason, StopReason::EndTurn);
        assert_eq!(c.usage.output_tokens, Some(2));
        let r = &server.requests().await[0];
        assert_eq!(r.header("authorization").as_deref(), Some("Bearer sk-test"));
        assert!(r.head.starts_with("POST /chat/completions"));
    }

    #[tokio::test]
    async fn local_provider_sends_no_auth_and_explains_unreachable_server() {
        let addr = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let p = OpenAiCompatibleProvider::local(None, Some(format!("http://{addr}"))).unwrap();
        let e = p.stream(&req(), &CancellationToken::new(), &mut |_| {}).await.unwrap_err();
        assert_eq!(e.kind, AiErrorKind::Network);
        assert!(e.message.contains("Is it running?"), "{}", e.message);
    }

    #[tokio::test]
    async fn length_finish_reports_max_tokens() {
        let body = format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":{"content":"x"},"finish_reason":"length"}]}));
        let server = MockServer::start(vec![(200, "text/event-stream", body)]).await;
        let p = OpenAiCompatibleProvider::local(None, Some(server.url())).unwrap();
        let c = p.stream(&req(), &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(c.stop_reason, StopReason::MaxTokens);
        assert!(server.requests().await[0].header("authorization").is_none());
    }
}
