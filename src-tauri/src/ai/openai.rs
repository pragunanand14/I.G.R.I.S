//! OpenAI-compatible Chat Completions provider. Covers OpenAI itself and local
//! servers that implement the same API (Ollama, LM Studio, llama.cpp server).

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::http::{self, cancelled};
use super::sse::SseParser;
use super::{
    extras_for, AiError, AiErrorKind, AiProvider, AiResult, ChatRequest, Completion, EventSink, Media, MediaKind, ProviderExtras, ResponseDepth, Role,
    StopReason, StreamEvent, ToolCall, Usage,
};

pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const LOCAL_BASE_URL: &str = "http://localhost:11434/v1";
pub const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai";
/// Local models on a CPU can be silent for minutes while reading a long prompt.
const LOCAL_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// Separates `<think>…</think>` reasoning, which some local models (e.g. Qwen3 on
/// older Ollama) write inline, from the answer — across chunk boundaries. Only a
/// block at the very start counts, so a literal "<think>" later in an answer stays.
#[derive(Default)]
struct ThinkFilter {
    inside: bool,
    seen_answer: bool,
    pending: String,
}

impl ThinkFilter {
    /// Feed a content delta; returns (answer text, reasoning chars).
    fn push(&mut self, s: &str) -> (String, usize) {
        if self.seen_answer && !self.inside {
            return (s.to_string(), 0);
        }
        self.pending.push_str(s);
        let (mut visible, mut hidden) = (String::new(), 0);
        loop {
            let tag = if self.inside { "</think>" } else { "<think>" };
            if !self.inside
                && !self.pending.trim_start().is_empty()
                && !tag.starts_with(self.pending.trim_start())
                && !self.pending.trim_start().starts_with(tag)
            {
                // Answer text that isn't a think block: pass everything through from now on.
                self.seen_answer = true;
                visible.push_str(&std::mem::take(&mut self.pending));
                break;
            }
            if let Some(i) = self.pending.find(tag) {
                if self.inside {
                    hidden += self.pending[..i].chars().count();
                }
                self.pending = self.pending[i + tag.len()..].to_string();
                self.inside = !self.inside;
                if !self.inside {
                    // Drop the blank lines models put after </think>.
                    self.pending = self.pending.trim_start().to_string();
                }
                continue;
            }
            if self.inside {
                // Keep a possible partial closing tag; the rest is reasoning.
                let keep = (1..tag.len())
                    .rev()
                    .find(|&n| {
                        self.pending.len() >= n
                            && self.pending.is_char_boundary(self.pending.len() - n)
                            && tag.starts_with(&self.pending[self.pending.len() - n..])
                    })
                    .unwrap_or(0);
                let cut = self.pending.len() - keep;
                hidden += self.pending[..cut].chars().count();
                self.pending = self.pending[cut..].to_string();
            }
            break;
        }
        (visible, hidden)
    }

    /// Text held back at the end of the stream (never unfinished reasoning).
    fn finish(&mut self) -> String {
        if self.inside {
            String::new()
        } else {
            std::mem::take(&mut self.pending)
        }
    }
}

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

    /// Google Gemini through its OpenAI-compatible endpoint (free tier via AI Studio keys).
    pub fn gemini(api_key: String, base_url: Option<String>) -> AiResult<Self> {
        Self::build("gemini", "Gemini", Some(api_key), base_url.unwrap_or_else(|| GEMINI_BASE_URL.into()))
    }

    pub fn local(api_key: Option<String>, base_url: Option<String>) -> AiResult<Self> {
        Self::build("local", "The local model server", api_key, base_url.unwrap_or_else(|| LOCAL_BASE_URL.into()))
    }

    fn build(id: &'static str, label: &'static str, api_key: Option<String>, base_url: String) -> AiResult<Self> {
        let client = if id == "local" { http::client_with_read_timeout(LOCAL_READ_TIMEOUT)? } else { http::client()? };
        Ok(Self { id, label, client, api_key, base_url: base_url.trim_end_matches('/').to_string() })
    }
}

/// The provider's own error text. OpenAI-style servers send `{"error":{"message"}}`;
/// Gemini wraps it in an array (`[{"error":{…}}]`); some local servers send `{"message"}`.
pub fn error_message(v: &Value) -> Option<String> {
    let v = v.as_array().and_then(|a| a.first()).unwrap_or(v);
    v["error"]["message"].as_str().or(v["error"].as_str()).or(v["message"].as_str()).map(str::to_string)
}

/// Gemini's OpenAI-compatible endpoint accepts a subset of JSON Schema and
/// identifies function results by name: drop `additionalProperties` (IGRIS still
/// validates every input itself), omit empty parameter objects, and name tool
/// results after the call they answer.
pub fn gemini_compat(body: &mut Value) {
    fn strip(schema: &mut Value) {
        if let Some(obj) = schema.as_object_mut() {
            obj.remove("additionalProperties");
            for v in obj.values_mut() {
                strip(v);
            }
        } else if let Some(arr) = schema.as_array_mut() {
            arr.iter_mut().for_each(strip);
        }
    }
    if let Some(tools) = body["tools"].as_array_mut() {
        for t in tools {
            let f = &mut t["function"];
            // An object schema without properties (missing or empty) is rejected.
            if f["parameters"]["properties"].as_object().map(|p| p.is_empty()).unwrap_or(true) {
                f.as_object_mut().map(|o| o.remove("parameters"));
            } else {
                strip(&mut f["parameters"]);
            }
        }
    }
    let mut names = std::collections::HashMap::new();
    if let Some(msgs) = body["messages"].as_array_mut() {
        for m in msgs.iter_mut() {
            // Gemini 3 requires a thought signature on replayed function calls. Calls
            // stored before signatures were kept (or made by another provider) get
            // Google's documented placeholder that skips validation.
            for c in m["tool_calls"].as_array_mut().into_iter().flatten() {
                if c.get("extra_content").is_none() {
                    c["extra_content"] = json!({ "google": { "thought_signature": "skip_thought_signature_validator" } });
                }
            }
            for c in m["tool_calls"].as_array().into_iter().flatten() {
                if let (Some(id), Some(name)) = (c["id"].as_str(), c["function"]["name"].as_str()) {
                    names.insert(id.to_string(), name.to_string());
                }
            }
            if m["role"] == "tool" {
                if let Some(name) = m["tool_call_id"].as_str().and_then(|id| names.get(id)) {
                    m["name"] = json!(name);
                }
            }
        }
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

/// Response depth → `reasoning_effort`, only where the model has one and only
/// when it differs from the provider default (Medium sends nothing).
fn reasoning_effort(provider: &str, model: &str, depth: Option<ResponseDepth>) -> Option<&'static str> {
    let depth = depth.filter(|d| *d != ResponseDepth::Medium)?;
    let m = model.to_ascii_lowercase();
    let reasoning_model = match provider {
        "gemini" => m.contains("gemini-2.5") || m.contains("gemini-3") || m.contains("-latest"),
        "openai" => m.starts_with('o') || m.starts_with("gpt-5"),
        _ => false, // local servers: no portable setting
    };
    if !reasoning_model {
        return None;
    }
    Some(match depth {
        ResponseDepth::Low => "low",
        ResponseDepth::High => "high",
        ResponseDepth::Medium => "medium",
    })
}

/// Build the request. `provider` decides which stored extras are replayed
/// (Gemini thought signatures only go back to Gemini).
pub fn build_body(provider: &str, req: &ChatRequest, include_usage: bool, with_tools: bool, strict_tools: bool) -> Value {
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
                    .map(|c| {
                        let mut call = json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.input.to_string() } });
                        if let Some(extra) = extras_for(&c.extras, provider) {
                            call["extra_content"] = extra.clone();
                        }
                        call
                    })
                    .collect();
                let content = if t.text.is_empty() { Value::Null } else { Value::String(t.text.clone()) };
                messages.push(json!({ "role": "assistant", "content": content, "tool_calls": calls }));
            }
            Role::Assistant => messages.push(json!({ "role": "assistant", "content": t.text })),
        }
    }
    let mut body = json!({ "model": req.model, "stream": true, "messages": messages });
    if let Some(effort) = reasoning_effort(provider, &req.model, req.depth) {
        body["reasoning_effort"] = json!(effort);
    }
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
    /// Gemini thought signature etc. — must be echoed back verbatim.
    extra: Option<Value>,
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
                e.message =
                    format!("{} This model may not accept images — use a vision-capable model, or start a new conversation without the image.", e.message);
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
        let mut body = build_body(self.id, req, self.id == "openai", with_tools, self.id == "openai");
        if self.id == "gemini" {
            gemini_compat(&mut body);
        }
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
                let body = resp.json::<Value>().await.ok();
                let msg = body.as_ref().and_then(error_message);
                let retry = retry.or_else(|| body.as_ref().and_then(http::retry_hint));
                let daily = msg.as_deref().is_some_and(|m| m.to_ascii_lowercase().contains("per day") || m.contains("PerDay"));
                let mut err = http::status_error(status, msg, if daily { None } else { retry }, label);
                if daily && status.as_u16() == 429 {
                    // A daily quota won't clear by waiting a minute.
                    err.kind = AiErrorKind::PermissionDenied; // not retryable, no fallbacks
                    err.message = format!(
                        "{label}'s daily free quota for this model is used up. It resets tomorrow; or set AI_MODEL to another model. ({})",
                        err.message
                    );
                }
                if label == "Gemini" && status.as_u16() == 404 {
                    err.message.push_str(
                        " Model names change over time — use one listed at https://aistudio.google.com (Models), e.g. the current Flash or Flash-Lite.",
                    );
                }
                err
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

        let mut think = ThinkFilter::default();
        let mut handle = |data: &str| -> AiResult<bool> {
            if data.trim() == "[DONE]" {
                return Ok(true);
            }
            let v: Value = serde_json::from_str(data).map_err(|e| AiError::new(AiErrorKind::Protocol, format!("Malformed stream event from {label}: {e}")))?;
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
                // Reasoning models (Qwen3, DeepSeek-R1 …) stream their thinking separately.
                for field in ["reasoning", "reasoning_content"] {
                    if let Some(t) = choice["delta"][field].as_str().filter(|t| !t.is_empty()) {
                        on_event(StreamEvent::Reasoning(t.chars().count()));
                    }
                }
                if let Some(t) = choice["delta"]["content"].as_str().filter(|t| !t.is_empty()) {
                    let (visible, hidden) = think.push(t);
                    if hidden > 0 {
                        on_event(StreamEvent::Reasoning(hidden));
                    }
                    if !visible.is_empty() {
                        text.push_str(&visible);
                        on_event(StreamEvent::TextDelta(visible));
                    }
                }
                if let Some(t) = choice["delta"]["refusal"].as_str().filter(|t| !t.is_empty()) {
                    text.push_str(t);
                    on_event(StreamEvent::TextDelta(t.to_string()));
                    stop = Some(StopReason::Refusal { category: None });
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
                        if let Some(e) = d.get("extra_content").filter(|e| e.is_object()) {
                            c.extra = Some(e.clone());
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
        let tail = think.finish();
        if !tail.is_empty() {
            text.push_str(&tail);
            on_event(StreamEvent::TextDelta(tail));
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
                ToolCall {
                    id: if c.id.is_empty() { format!("call_{i}") } else { c.id },
                    name: c.name,
                    input,
                    invalid_input,
                    extras: c.extra.map(|e| ProviderExtras::new(self.id, e)),
                }
            })
            .collect();
        let mut stop_reason = stop.unwrap_or(StopReason::EndTurn);
        // Some local servers report "stop" even when they emitted tool calls.
        if !tool_calls.is_empty() && stop_reason == StopReason::EndTurn {
            stop_reason = StopReason::ToolUse;
        }
        Ok(Completion { text, extras: None, model, stop_reason, usage, tool_calls })
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
                ChatTurn { extras: Some(ProviderExtras::new("anthropic", json!([{"type":"text"}]))), ..ChatTurn::assistant("hello") },
                ChatTurn::user("again"),
            ],
            max_tokens: 1000,
            depth: None,
            tools: vec![],
        }
    }

    #[test]
    fn body_puts_system_first_and_sends_text_only() {
        let b = build_body("openai", &req(), true, true, true);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][2]["content"], "hello");
        assert_eq!(b["stream_options"]["include_usage"], true);
        assert!(build_body("openai", &req(), false, true, false).get("stream_options").is_none());
    }

    fn tool_req() -> ChatRequest {
        let mut r = req();
        r.tools = vec![
            ToolDef { name: "calculator".into(), description: "d".into(), input_schema: json!({"type":"object"}), server: None },
            ToolDef { name: "web_search".into(), description: String::new(), input_schema: json!({}), server: Some(json!({"type":"web_search_20260209"})) },
        ];
        r.turns.push(ChatTurn {
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "calculator".into(),
                input: json!({"expression":"1+1"}),
                invalid_input: None,
                extras: None,
            }],
            ..ChatTurn::assistant("")
        });
        r.turns.push(ChatTurn {
            tool_results: vec![ToolResult { call_id: "call_1".into(), content: "2".into(), is_error: false, media: vec![] }],
            ..ChatTurn::user("")
        });
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
        let b = build_body("openai", &r, false, false, false);
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
        let b = build_body("openai", &r, false, true, false);
        let msgs = b["messages"].as_array().unwrap();
        let n = msgs.len();
        assert_eq!(msgs[n - 2]["role"], "tool");
        assert_eq!(msgs[n - 1]["role"], "user");
        assert_eq!(msgs[n - 1]["content"][1]["image_url"]["url"], "data:image/png;base64,QUJD");
    }

    #[test]
    fn tool_turns_map_to_function_calls_and_tool_messages() {
        let b = build_body("openai", &tool_req(), true, true, true);
        let msgs = b["messages"].as_array().unwrap();
        let assistant = &msgs[msgs.len() - 2];
        assert_eq!(assistant["tool_calls"][0]["function"]["arguments"], "{\"expression\":\"1+1\"}");
        assert!(assistant["content"].is_null());
        assert_eq!(msgs.last().unwrap()["role"], "tool");
        assert_eq!(b["tools"][0]["function"]["strict"], true);
        assert_eq!(b["tools"].as_array().unwrap().len(), 1, "server tools are skipped");
        assert!(build_body("openai", &tool_req(), false, true, false)["tools"][0]["function"].get("strict").is_none());
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

    fn filter_all(chunks: &[&str]) -> (String, usize) {
        let mut f = ThinkFilter::default();
        let (mut text, mut hidden) = (String::new(), 0);
        for c in chunks {
            let (v, h) = f.push(c);
            text.push_str(&v);
            hidden += h;
        }
        text.push_str(&f.finish());
        (text, hidden)
    }

    #[tokio::test]
    async fn gemini_thought_signatures_are_kept_and_sent_back() {
        let sig = json!({"google":{"thought_signature":"c2lnbmF0dXJl"}});
        let body = [
            json!({"choices":[{"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_g","type":"function","function":{"name":"system_info","arguments":"{}"},"extra_content":sig}]},"finish_reason":"tool_calls"}]}),
        ]
        .iter()
        .map(|v| format!("data: {v}\n\n"))
        .collect::<String>()
            + "data: [DONE]\n\n";
        let server = MockServer::start(vec![(200, "text/event-stream", body)]).await;
        let p = OpenAiCompatibleProvider::gemini("k".into(), Some(server.url())).unwrap();
        let c = p.stream(&req(), &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(c.tool_calls[0].extras, Some(ProviderExtras::new("gemini", sig.clone())));

        // Replayed on the next request, and it survives being stored and reloaded.
        let stored: ToolCall = serde_json::from_value(serde_json::to_value(&c.tool_calls[0]).unwrap()).unwrap();
        let mut r = req();
        r.turns.push(ChatTurn { tool_calls: vec![stored], ..ChatTurn::assistant("") });
        r.turns.push(ChatTurn {
            tool_results: vec![ToolResult { call_id: "call_g".into(), content: "CPU 7%".into(), is_error: false, media: vec![] }],
            ..ChatTurn::user("")
        });
        let b = build_body("gemini", &r, false, true, false);
        let assistant = b["messages"].as_array().unwrap().iter().find(|m| m["tool_calls"].is_array()).unwrap();
        assert_eq!(assistant["tool_calls"][0]["extra_content"], sig);
        // Another provider never receives Gemini's signature.
        let other = build_body("openai", &r, false, true, false);
        assert!(!other.to_string().contains("c2lnbmF0dXJl"));
    }

    #[test]
    fn response_depth_maps_to_reasoning_effort_only_where_it_exists() {
        let mut r = req();
        let effort = |provider: &str, model: &str, depth| {
            let mut r = r.clone();
            r.model = model.into();
            r.depth = depth;
            build_body(provider, &r, false, false, false).get("reasoning_effort").cloned()
        };
        // Medium = the provider's default: nothing sent, so behaviour is unchanged.
        assert_eq!(effort("gemini", "gemini-3.5-flash-lite", Some(ResponseDepth::Medium)), None);
        assert_eq!(effort("gemini", "gemini-3.5-flash-lite", Some(ResponseDepth::High)), Some(json!("high")));
        assert_eq!(effort("openai", "gpt-5-mini", Some(ResponseDepth::Low)), Some(json!("low")));
        assert_eq!(effort("openai", "gpt-4o", Some(ResponseDepth::High)), None, "not a reasoning model");
        assert_eq!(effort("local", "qwen3:4b", Some(ResponseDepth::High)), None, "no portable setting for local servers");
        r.depth = None;
        assert!(build_body("gemini", &r, false, false, false).get("reasoning_effort").is_none());
    }

    #[test]
    fn provider_error_text_is_found_in_every_shape() {
        assert_eq!(error_message(&json!({"error":{"message":"bad model"}})).as_deref(), Some("bad model"));
        assert_eq!(error_message(&json!([{"error":{"code":404,"message":"models/x is not found"}}])).as_deref(), Some("models/x is not found"));
        assert_eq!(error_message(&json!({"error":"model 'x' not found"})).as_deref(), Some("model 'x' not found"));
        assert_eq!(error_message(&json!({"message":"nope"})).as_deref(), Some("nope"));
        assert_eq!(error_message(&json!({"ok":true})), None);
    }

    #[tokio::test]
    async fn gemini_not_found_shows_googles_reason_and_where_to_find_models() {
        let body = json!([{"error":{"code":404,"message":"models/gemini-old is not found for API version v1beta"}}]).to_string();
        let server = MockServer::start(vec![(404, "application/json", body)]).await;
        let p = OpenAiCompatibleProvider::gemini("k".into(), Some(server.url())).unwrap();
        let e = p.stream(&req(), &CancellationToken::new(), &mut |_| {}).await.unwrap_err();
        assert!(e.message.contains("models/gemini-old is not found"), "{}", e.message);
        assert!(e.message.contains("aistudio.google.com"), "{}", e.message);
    }

    #[test]
    fn gemini_requests_fit_its_schema_subset_and_name_tool_results() {
        let mut r = tool_req();
        r.tools[0].input_schema =
            json!({"type":"object","properties":{"expression":{"type":"string","maxLength":500}},"required":["expression"],"additionalProperties":false});
        r.tools.push(ToolDef {
            name: "get_datetime".into(),
            description: "now".into(),
            input_schema: json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
            server: None,
        });
        let mut b = build_body("gemini", &r, false, true, false);
        gemini_compat(&mut b);
        let tools = b["tools"].as_array().unwrap();
        let calc = tools.iter().find(|t| t["function"]["name"] == "calculator").unwrap();
        assert!(!calc.to_string().contains("additionalProperties"));
        assert!(calc["function"]["parameters"]["properties"].is_object());
        let dt = tools.iter().find(|t| t["function"]["name"] == "get_datetime").unwrap();
        assert!(dt["function"].get("parameters").is_none(), "no empty parameter object");
        let tool_msg = b["messages"].as_array().unwrap().iter().find(|m| m["role"] == "tool").unwrap();
        assert_eq!(tool_msg["name"], "calculator");
        let call = &b["messages"].as_array().unwrap().iter().find(|m| m["tool_calls"].is_array()).unwrap()["tool_calls"][0];
        assert_eq!(call["extra_content"]["google"]["thought_signature"], "skip_thought_signature_validator", "unsigned stored calls get the placeholder");
        assert!(b["tools"].as_array().unwrap().iter().all(|t| t["function"].get("strict").is_none()));
    }

    #[tokio::test]
    async fn gemini_uses_the_compatible_endpoint_with_a_bearer_key() {
        let body = format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":{"content":"Hi"},"finish_reason":"stop"}]}));
        let server = MockServer::start(vec![(200, "text/event-stream", body)]).await;
        let p = OpenAiCompatibleProvider::gemini("AIza-test".into(), Some(server.url())).unwrap();
        assert_eq!(p.id(), "gemini");
        let c = p.stream(&req(), &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(c.text, "Hi");
        let r = &server.requests().await[0];
        assert_eq!(r.header("authorization").as_deref(), Some("Bearer AIza-test"));
        assert!(r.head.starts_with("POST /chat/completions"));
    }

    #[test]
    fn think_blocks_are_hidden_even_when_split_across_chunks() {
        assert_eq!(filter_all(&["<thi", "nk>abc</th", "ink>\n\nHello"]), ("Hello".into(), 3));
        assert_eq!(filter_all(&["<think>", "x", "</think>", "Hi ", "there"]), ("Hi there".into(), 1));
        assert_eq!(filter_all(&["Plain answer"]), ("Plain answer".into(), 0));
        // Only a leading block counts; a literal tag later is part of the answer.
        assert_eq!(filter_all(&["Use ", "<think>", " tags"]), ("Use <think> tags".into(), 0));
        // An unfinished block at the end is dropped, not shown.
        assert_eq!(filter_all(&["<think>still going"]), (String::new(), 11));
        assert_eq!(filter_all(&["", "Hé", "llo"]), ("Héllo".into(), 0));
    }

    #[tokio::test]
    async fn reasoning_fields_report_progress_but_never_reach_the_answer() {
        let body = [
            json!({"choices":[{"delta":{"role":"assistant","content":"","reasoning":"Let me think"}}]}),
            json!({"choices":[{"delta":{"reasoning_content":" more"}}]}),
            json!({"choices":[{"delta":{"content":"Hi!"},"finish_reason":"stop"}]}),
        ]
        .iter()
        .map(|v| format!("data: {v}\n\n"))
        .collect::<String>()
            + "data: [DONE]\n\n";
        let server = MockServer::start(vec![(200, "text/event-stream", body)]).await;
        let p = OpenAiCompatibleProvider::local(None, Some(server.url())).unwrap();
        let mut reasoning = 0;
        let c = p
            .stream(&req(), &CancellationToken::new(), &mut |e| {
                if let StreamEvent::Reasoning(n) = e {
                    reasoning += n
                }
            })
            .await
            .unwrap();
        assert_eq!(c.text, "Hi!");
        assert_eq!(reasoning, "Let me think more".len());
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
