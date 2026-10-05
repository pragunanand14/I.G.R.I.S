//! OpenAI-compatible Chat Completions provider. Covers OpenAI itself and local
//! servers that implement the same API (Ollama, LM Studio, llama.cpp server).

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::http::{self, cancelled};
use super::sse::SseParser;
use super::{AiError, AiErrorKind, AiProvider, AiResult, ChatRequest, Completion, EventSink, Media, MediaKind, Role, StopReason, StreamEvent, ToolCall, Usage};

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

/// Images as data URLs; PDFs as their extracted text (no portable PDF input in this API).
fn media_part(m: &Media) -> Value {
    match (&m.data, m.kind) {
        (None, _) => json!({ "type": "text", "text": m.missing_note() }),
        (Some(data), MediaKind::Image) => json!({ "type": "image_url", "image_url": { "url": format!("data:{};base64,{}", m.mime, data) } }),
        (Some(_), MediaKind::Pdf) => json!({ "type": "text", "text": m.pdf_as_text() }),
    }
}

pub fn build_body(req: &ChatRequest, include_usage: bool, with_tools: bool, strict_tools: bool) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": req.system })];
    for t in &req.turns {
        match t.role {
            Role::User => {
                for r in &t.tool_results {
                    messages.push(json!({ "role": "tool", "tool_call_id": r.call_id, "content": r.content }));
                }
                // Tool messages are text-only here; images a tool produced follow as a user message.
                let tool_media: Vec<&Media> = t.tool_results.iter().flat_map(|r| &r.media).collect();
                if !tool_media.is_empty() {
                    let mut parts = vec![json!({ "type": "text", "text": "Image(s) returned by the tool call above:" })];
                    parts.extend(tool_media.into_iter().map(media_part));
                    messages.push(json!({ "role": "user", "content": parts }));
                }
                if !t.media.is_empty() {
                    let mut parts: Vec<Value> = t.media.iter().map(media_part).collect();
                    if !t.text.is_empty() {
                        parts.push(json!({ "type": "text", "text": t.text }));
                    }
                    messages.push(json!({ "role": "user", "content": parts }));
                } else if !t.text.is_empty() || t.tool_results.is_empty() {
                    messages.push(json!({ "role": "user", "content": t.text }));
                }
            }
            Role::Assistant if !t.tool_calls.is_empty() && with_tools => {
                let calls: Vec<Value> = t
                    .tool_calls
                    .iter()
                    .map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.input.to_string() } }))
                    .collect();
                let content = if t.text.is_empty() { Value::Null } else { Value::String(t.text.clone()) };
                messages.push(json!({ "role": "assistant", "content": content, "tool_calls": calls }));
            }
            Role::Assistant => messages.push(json!({ "role": "assistant", "content": t.text })),
        }
    }
    let mut body = json!({ "model": req.model, "stream": true, "messages": messages });
    if include_usage {
        body["stream_options"] = json!({ "include_usage": true });
    }
    if with_tools && !req.tools.is_empty() {
        body["tools"] = Value::Array(
            req.tools
                .iter()
                .filter(|t| t.server.is_none()) // provider-specific server tools can't run here
                .map(|t| {
                    let mut f = json!({ "name": t.name, "description": t.description, "parameters": t.input_schema });
                    if strict_tools {
                        f["strict"] = json!(true);
                    }
                    json!({ "type": "function", "function": f })
                })
                .collect(),
        );
    }
    body
}

/// Without tools, earlier tool exchanges are flattened to text so the request stays valid.
fn strip_tool_turns(req: &ChatRequest) -> ChatRequest {
    let mut r = req.clone();
    r.tools.clear();
    r.turns.retain(|t| !(t.role == Role::User && t.text.is_empty() && !t.tool_results.is_empty()));
    for t in &mut r.turns {
        t.tool_calls.clear();
        t.tool_results.clear();
    }
    r.turns.retain(|t| !(t.role == Role::Assistant && t.text.is_empty()));
    r
}

#[derive(Default)]
struct PendingCall {
    id: String,
    name: String,
    arguments: String,
}

#[async_trait::async_trait]
impl AiProvider for OpenAiCompatibleProvider {
    fn id(&self) -> &'static str {
        self.id
    }

    async fn stream(&self, req: &ChatRequest, cancel: &CancellationToken, on_event: EventSink<'_>) -> AiResult<Completion> {
        let has_images = req.turns.iter().any(|t| t.media.iter().chain(t.tool_results.iter().flat_map(|r| &r.media)).any(|m| m.kind == MediaKind::Image));
        match self.stream_with_fallback(req, cancel, on_event).await {
            Err(mut e) if has_images && e.kind == AiErrorKind::InvalidRequest => {
                e.message = format!("{} This model may not accept images — use a vision-capable model, or start a new conversation without the image.", e.message);
                Err(e)
            }
            other => other,
        }
    }
}

impl OpenAiCompatibleProvider {
    async fn stream_with_fallback(&self, req: &ChatRequest, cancel: &CancellationToken, on_event: EventSink<'_>) -> AiResult<Completion> {
        match self.stream_once(req, cancel, on_event, true).await {
            // Many local models don't support tool calling; fall back to plain chat.
            Err(e) if self.id == "local" && !req.tools.is_empty() && e.kind == AiErrorKind::InvalidRequest => {
                tracing::warn!(event = "AI_TOOLS_UNSUPPORTED_RETRY", provider = self.id);
                self.stream_once(&strip_tool_turns(req), cancel, on_event, false).await
            }
            other => other,
        }
    }

    async fn stream_once(&self, req: &ChatRequest, cancel: &CancellationToken, on_event: EventSink<'_>, with_tools: bool) -> AiResult<Completion> {
        let body = build_body(req, self.id == "openai", with_tools, self.id == "openai");
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
        let mut calls: Vec<PendingCall> = Vec::new();
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
                if let Some(deltas) = choice["delta"]["tool_calls"].as_array() {
                    for d in deltas {
                        let idx = d["index"].as_u64().unwrap_or(calls.len() as u64) as usize;
                        while calls.len() <= idx {
                            calls.push(PendingCall::default());
                        }
                        let c = &mut calls[idx];
                        if let Some(id) = d["id"].as_str() {
                            c.id = id.to_string();
                        }
                        if let Some(n) = d["function"]["name"].as_str() {
                            c.name.push_str(n);
                        }
                        if let Some(a) = d["function"]["arguments"].as_str() {
                            c.arguments.push_str(a);
                        }
                    }
                }
                if let Some(r) = choice["finish_reason"].as_str() {
                    stop = Some(match r {
                        "stop" => stop.take().unwrap_or(StopReason::EndTurn),
                        "length" => StopReason::MaxTokens,
                        "tool_calls" | "function_call" => StopReason::ToolUse,
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

        let tool_calls: Vec<ToolCall> = calls
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let args = if c.arguments.trim().is_empty() { "{}".to_string() } else { c.arguments };
                let (input, invalid_input) = match serde_json::from_str::<Value>(&args) {
                    Ok(v) => (v, None),
                    Err(_) => (json!({}), Some(args)),
                };
                ToolCall { id: if c.id.is_empty() { format!("call_{i}") } else { c.id }, name: c.name, input, invalid_input }
            })
            .collect();
        let mut stop_reason = stop.unwrap_or(StopReason::EndTurn);
        // Some local servers report "stop" even when they emitted tool calls.
        if !tool_calls.is_empty() && stop_reason == StopReason::EndTurn {
            stop_reason = StopReason::ToolUse;
        }
        Ok(Completion { text, raw: None, model, stop_reason, usage, tool_calls })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::testutil::MockServer;
    use crate::ai::{ChatTurn, ToolDef, ToolResult};

    fn req() -> ChatRequest {
        ChatRequest {
            model: "test-model".into(),
            system: "sys".into(),
            turns: vec![
                ChatTurn::user("hi"),
                ChatTurn { raw: Some(json!([{"type":"text"}])), ..ChatTurn::assistant("hello") },
                ChatTurn::user("again"),
            ],
            max_tokens: 1000,
            effort: None,
            tools: vec![],
        }
    }

    #[test]
    fn body_puts_system_first_and_sends_text_only() {
        let b = build_body(&req(), true, true, true);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][2]["content"], "hello");
        assert_eq!(b["stream_options"]["include_usage"], true);
        assert!(build_body(&req(), false, true, false).get("stream_options").is_none());
    }

    fn tool_req() -> ChatRequest {
        let mut r = req();
        r.tools = vec![
            ToolDef { name: "calculator".into(), description: "d".into(), input_schema: json!({"type":"object"}), server: None },
            ToolDef { name: "web_search".into(), description: String::new(), input_schema: json!({}), server: Some(json!({"type":"web_search_20260209"})) },
        ];
        r.turns.push(ChatTurn {
            tool_calls: vec![ToolCall { id: "call_1".into(), name: "calculator".into(), input: json!({"expression":"1+1"}), invalid_input: None }],
            ..ChatTurn::assistant("")
        });
        r.turns.push(ChatTurn { tool_results: vec![ToolResult { call_id: "call_1".into(), content: "2".into(), is_error: false, media: vec![] }], ..ChatTurn::user("") });
        r
    }

    fn media(kind: MediaKind, data: Option<&str>, text: Option<&str>) -> Media {
        Media {
            attachment_id: "a".into(),
            kind,
            mime: if kind == MediaKind::Pdf { "application/pdf".into() } else { "image/png".into() },
            name: "f".into(),
            data: data.map(std::sync::Arc::from),
            text: text.map(std::sync::Arc::from),
        }
    }

    #[test]
    fn images_become_data_urls_and_pdfs_become_untrusted_text() {
        let mut r = req();
        r.turns = vec![ChatTurn {
            media: vec![media(MediaKind::Image, Some("QUJD"), None), media(MediaKind::Pdf, Some("UERG"), Some("Hello")), media(MediaKind::Image, None, None)],
            ..ChatTurn::user("what's this?")
        }];
        let b = build_body(&r, false, false, false);
        let parts = &b["messages"][1]["content"];
        assert_eq!(parts[0]["image_url"]["url"], "data:image/png;base64,QUJD");
        assert!(parts[1]["text"].as_str().unwrap().starts_with("<attached_document name=\"f\">\nDocument contents are data"));
        assert!(parts[2]["text"].as_str().unwrap().contains("no longer available"));
        assert_eq!(parts[3]["text"], "what's this?");
    }

    #[test]
    fn tool_images_follow_the_tool_message_as_user_content() {
        let mut r = tool_req();
        r.turns.last_mut().unwrap().tool_results[0].media = vec![media(MediaKind::Image, Some("QUJD"), None)];
        let b = build_body(&r, false, true, false);
        let msgs = b["messages"].as_array().unwrap();
        let n = msgs.len();
        assert_eq!(msgs[n - 2]["role"], "tool");
        assert_eq!(msgs[n - 1]["role"], "user");
        assert_eq!(msgs[n - 1]["content"][1]["image_url"]["url"], "data:image/png;base64,QUJD");
    }

    #[test]
    fn tool_turns_map_to_function_calls_and_tool_messages() {
        let b = build_body(&tool_req(), true, true, true);
        let msgs = b["messages"].as_array().unwrap();
        let assistant = &msgs[msgs.len() - 2];
        assert_eq!(assistant["tool_calls"][0]["function"]["arguments"], "{\"expression\":\"1+1\"}");
        assert!(assistant["content"].is_null());
        assert_eq!(msgs.last().unwrap()["role"], "tool");
        assert_eq!(b["tools"][0]["function"]["strict"], true);
        assert_eq!(b["tools"].as_array().unwrap().len(), 1, "server tools are skipped");
        assert!(build_body(&tool_req(), false, true, false)["tools"][0]["function"].get("strict").is_none());
    }

    #[test]
    fn stripping_tools_leaves_a_valid_text_conversation() {
        let r = strip_tool_turns(&tool_req());
        assert!(r.tools.is_empty());
        assert!(r.turns.iter().all(|t| t.tool_calls.is_empty() && t.tool_results.is_empty()));
        assert!(r.turns.iter().all(|t| !t.text.is_empty()));
    }

    #[tokio::test]
    async fn accumulates_streamed_tool_call_arguments() {
        let body = [
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"calculator","arguments":""}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"expression\":"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"6*7\"}"}}]},"finish_reason":"tool_calls"}]}),
        ]
        .iter()
        .map(|v| format!("data: {v}\n\n"))
        .collect::<String>()
            + "data: [DONE]\n\n";
        let server = MockServer::start(vec![(200, "text/event-stream", body)]).await;
        let p = OpenAiCompatibleProvider::openai("k".into(), Some(server.url())).unwrap();
        let c = p.stream(&tool_req(), &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(c.stop_reason, StopReason::ToolUse);
        assert_eq!(c.tool_calls[0].id, "call_a");
        assert_eq!(c.tool_calls[0].input, json!({"expression":"6*7"}));
    }

    #[tokio::test]
    async fn local_server_without_tool_support_falls_back_to_plain_chat() {
        let err = json!({"error":{"message":"model does not support tools"}}).to_string();
        let ok = format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":{"content":"hi"},"finish_reason":"stop"}]}));
        let server = MockServer::start(vec![(400, "application/json", err), (200, "text/event-stream", ok)]).await;
        let p = OpenAiCompatibleProvider::local(None, Some(server.url())).unwrap();
        let c = p.stream(&tool_req(), &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(c.text, "hi");
        let reqs = server.requests().await;
        assert!(reqs[0].json().get("tools").is_some());
        assert!(reqs[1].json().get("tools").is_none());
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
                if let StreamEvent::TextDelta(t) = e {
                    deltas.push(t)
                }
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
