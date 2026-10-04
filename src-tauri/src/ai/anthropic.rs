//! Anthropic Messages API provider (raw HTTP + SSE; there is no official Rust SDK).

use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::http::{self, cancelled};
use super::sse::SseParser;
use super::{AiError, AiErrorKind, AiProvider, AiResult, ChatRequest, ChatTurn, Completion, EventSink, Role, StopReason, StreamEvent, ToolCall, Usage};

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
const API_VERSION: &str = "2023-06-01";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(api_key: String, base_url: Option<String>) -> AiResult<Self> {
        Ok(Self {
            client: http::client()?,
            api_key,
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()).trim_end_matches('/').to_string(),
        })
    }

    fn is_first_party(&self) -> bool {
        self.base_url == DEFAULT_BASE_URL
    }
}

/// Models that accept the server-side refusal fallback (`fallbacks: "default"`).
fn supports_default_fallback(model: &str) -> bool {
    matches!(model, "claude-fable-5-1" | "claude-opus-5-5" | "claude-opus-5" | "claude-sonnet-5-5")
}

/// `output_config.effort` errors on Haiku 4.5, Sonnet 4.5 and older models.
fn supports_effort(model: &str) -> bool {
    !(model.contains("haiku") || model.contains("sonnet-4-5") || model.starts_with("claude-3"))
}

/// Prepare stored assistant content blocks for replay. After a mid-output
/// fallback, model-internal blocks before the final `fallback` boundary must be
/// omitted; text blocks and everything after the boundary echo unchanged.
pub fn sanitize_replay_blocks(blocks: &[Value]) -> Vec<Value> {
    let last_fallback = blocks.iter().rposition(|b| b["type"] == "fallback");
    match last_fallback {
        None => blocks.to_vec(),
        Some(idx) => blocks
            .iter()
            .enumerate()
            .filter(|(i, b)| *i > idx || (*i < idx && b["type"] == "text"))
            .map(|(_, b)| b.clone())
            .collect(),
    }
}

fn tool_result_blocks(turn: &ChatTurn) -> Vec<Value> {
    let mut blocks: Vec<Value> = turn
        .tool_results
        .iter()
        .map(|r| json!({ "type": "tool_result", "tool_use_id": r.call_id, "content": r.content, "is_error": r.is_error }))
        .collect();
    if !turn.text.is_empty() {
        blocks.push(json!({ "type": "text", "text": turn.text }));
    }
    blocks
}

fn message_for(turn: &ChatTurn) -> Value {
    match turn.role {
        Role::User if !turn.tool_results.is_empty() => json!({ "role": "user", "content": tool_result_blocks(turn) }),
        Role::User => json!({ "role": "user", "content": turn.text }),
        Role::Assistant => match &turn.raw {
            Some(Value::Array(blocks)) if !blocks.is_empty() => json!({ "role": "assistant", "content": sanitize_replay_blocks(blocks) }),
            _ if !turn.tool_calls.is_empty() => {
                let mut blocks = Vec::new();
                if !turn.text.is_empty() {
                    blocks.push(json!({ "type": "text", "text": turn.text }));
                }
                blocks.extend(turn.tool_calls.iter().map(|c| json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": c.input })));
                json!({ "role": "assistant", "content": blocks })
            }
            _ => json!({ "role": "assistant", "content": turn.text }),
        },
    }
}

pub fn build_body(req: &ChatRequest, first_party: bool) -> Value {
    let messages: Vec<Value> = req.turns.iter().map(message_for).collect();

    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "stream": true,
        "system": req.system,
        // Automatic prompt caching of the conversation prefix.
        "cache_control": { "type": "ephemeral" },
        "messages": messages,
    });
    if let Some(effort) = req.effort.filter(|_| supports_effort(&req.model)) {
        body["output_config"] = json!({ "effort": effort.as_str() });
    }
    if first_party && supports_default_fallback(&req.model) {
        body["fallbacks"] = json!("default");
    }
    if !req.tools.is_empty() {
        // Strict mode guarantees schema-valid arguments; inputs are small, so
        // eager (unbuffered) input streaming isn't worth losing that guarantee.
        body["tools"] = Value::Array(
            req.tools
                .iter()
                .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.input_schema, "strict": true }))
                .collect(),
        );
    }
    body
}

fn error_kind_from_type(t: &str) -> AiErrorKind {
    match t {
        "authentication_error" => AiErrorKind::Authentication,
        "permission_error" => AiErrorKind::PermissionDenied,
        "not_found_error" => AiErrorKind::NotFound,
        "rate_limit_error" => AiErrorKind::RateLimited,
        "overloaded_error" => AiErrorKind::Overloaded,
        "invalid_request_error" | "request_too_large" => AiErrorKind::InvalidRequest,
        "timeout_error" => AiErrorKind::Timeout,
        _ => AiErrorKind::Server,
    }
}

/// Accumulates streamed content blocks so the full assistant turn can be
/// replayed unchanged on the next request.
#[derive(Default)]
struct BlockAccumulator {
    blocks: Vec<Value>,
    partial_json: Vec<String>,
    /// Raw tool-input text that failed to parse, by block index.
    invalid_inputs: Vec<Option<String>>,
    /// Set when we see a delta we don't know how to apply; raw replay is then unsafe.
    unreliable: bool,
}

impl BlockAccumulator {
    fn start(&mut self, index: usize, block: Value) {
        while self.blocks.len() <= index {
            self.blocks.push(Value::Null);
            self.partial_json.push(String::new());
            self.invalid_inputs.push(None);
        }
        self.blocks[index] = block;
    }

    /// Apply a delta. Returns streamed visible text, if any.
    fn delta(&mut self, index: usize, delta: &Value) -> Option<String> {
        let Some(block) = self.blocks.get_mut(index) else {
            self.unreliable = true;
            return None;
        };
        let append = |block: &mut Value, field: &str, s: &str| {
            let cur = block[field].as_str().unwrap_or_default().to_string();
            block[field] = Value::String(cur + s);
        };
        match delta["type"].as_str() {
            Some("text_delta") => {
                let s = delta["text"].as_str().unwrap_or_default();
                append(block, "text", s);
                Some(s.to_string())
            }
            Some("thinking_delta") => {
                append(block, "thinking", delta["thinking"].as_str().unwrap_or_default());
                None
            }
            Some("signature_delta") => {
                block["signature"] = delta["signature"].clone();
                None
            }
            Some("input_json_delta") => {
                self.partial_json[index].push_str(delta["partial_json"].as_str().unwrap_or_default());
                None
            }
            Some("citations_delta") => {
                if !block["citations"].is_array() {
                    block["citations"] = json!([]);
                }
                if let Some(arr) = block["citations"].as_array_mut() {
                    arr.push(delta["citation"].clone());
                }
                None
            }
            _ => {
                self.unreliable = true;
                None
            }
        }
    }

    fn stop(&mut self, index: usize) {
        if let (Some(block), Some(pj)) = (self.blocks.get_mut(index), self.partial_json.get(index)) {
            if !pj.is_empty() {
                match serde_json::from_str::<Value>(pj) {
                    Ok(v) => block["input"] = v,
                    Err(_) => {
                        // Keep the block replayable; the call is answered with an INVALID_JSON error.
                        block["input"] = json!({});
                        self.invalid_inputs[index] = Some(pj.clone());
                    }
                }
            }
        }
    }

    fn text(&self) -> String {
        self.blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect()
    }

    /// Tool calls after the last fallback boundary (earlier ones belong to a declined attempt).
    fn tool_calls(&self) -> Vec<ToolCall> {
        let start = self.blocks.iter().rposition(|b| b["type"] == "fallback").map(|i| i + 1).unwrap_or(0);
        self.blocks
            .iter()
            .enumerate()
            .skip(start)
            .filter(|(_, b)| b["type"] == "tool_use")
            .map(|(i, b)| ToolCall {
                id: b["id"].as_str().unwrap_or_default().to_string(),
                name: b["name"].as_str().unwrap_or_default().to_string(),
                input: b["input"].clone(),
                invalid_input: self.invalid_inputs.get(i).cloned().flatten(),
            })
            .collect()
    }

    fn raw(&self) -> Option<Value> {
        (!self.unreliable && !self.blocks.iter().any(Value::is_null)).then(|| Value::Array(self.blocks.clone()))
    }
}

/// Parse one SSE event into accumulator state. Returns `Ok(true)` on `message_stop`.
struct StreamState {
    acc: BlockAccumulator,
    model: String,
    stop_reason: Option<StopReason>,
    usage: Usage,
}

impl StreamState {
    fn handle(&mut self, data: &str, on_event: &mut (dyn FnMut(StreamEvent) + Send)) -> AiResult<bool> {
        let v: Value = serde_json::from_str(data)
            .map_err(|e| AiError::new(AiErrorKind::Protocol, format!("Malformed stream event from Anthropic: {e}")))?;
        match v["type"].as_str().unwrap_or_default() {
            "message_start" => {
                if let Some(m) = v["message"]["model"].as_str() {
                    self.model = m.to_string();
                }
                self.usage.input_tokens = v["message"]["usage"]["input_tokens"].as_u64();
            }
            "content_block_start" => {
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                self.acc.start(idx, v["content_block"].clone());
                // A block may start with text already present.
                if v["content_block"]["type"] == "text" {
                    if let Some(t) = v["content_block"]["text"].as_str().filter(|t| !t.is_empty()) {
                        on_event(StreamEvent::TextDelta(t.to_string()));
                    }
                }
            }
            "content_block_delta" => {
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                if let Some(text) = self.acc.delta(idx, &v["delta"]) {
                    if !text.is_empty() {
                        on_event(StreamEvent::TextDelta(text));
                    }
                }
            }
            "content_block_stop" => self.acc.stop(v["index"].as_u64().unwrap_or(0) as usize),
            "message_delta" => {
                if let Some(r) = v["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(match r {
                        "end_turn" | "stop_sequence" => StopReason::EndTurn,
                        "max_tokens" => StopReason::MaxTokens,
                        "tool_use" => StopReason::ToolUse,
                        "refusal" => StopReason::Refusal {
                            category: v["delta"]["stop_details"]["category"].as_str().map(str::to_string),
                        },
                        other => StopReason::Other { reason: other.to_string() },
                    });
                }
                if let Some(o) = v["usage"]["output_tokens"].as_u64() {
                    self.usage.output_tokens = Some(o);
                }
            }
            "message_stop" => return Ok(true),
            "error" => {
                let t = v["error"]["type"].as_str().unwrap_or_default();
                let msg = v["error"]["message"].as_str().unwrap_or("stream error");
                return Err(AiError::new(error_kind_from_type(t), format!("Anthropic: {msg}")));
            }
            _ => {} // ping and future event types
        }
        Ok(false)
    }
}

#[async_trait::async_trait]
impl AiProvider for AnthropicProvider {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    async fn stream(&self, req: &ChatRequest, cancel: &CancellationToken, on_event: EventSink<'_>) -> AiResult<Completion> {
        let first_party = self.is_first_party();
        let body = build_body(req, first_party);
        let url = format!("{}/v1/messages", self.base_url);
        let send_fallback_beta = body.get("fallbacks").is_some();

        let resp = http::send_with_retry(
            cancel,
            || {
                let mut rb = self
                    .client
                    .post(&url)
                    .header("x-api-key", &self.api_key)
                    .header("anthropic-version", API_VERSION)
                    .json(&body);
                if send_fallback_beta {
                    rb = rb.header("anthropic-beta", FALLBACK_BETA);
                }
                rb
            },
            |resp| async move {
                let status = resp.status();
                let retry = http::retry_after(&resp);
                let msg = resp.json::<Value>().await.ok().and_then(|v| v["error"]["message"].as_str().map(str::to_string));
                http::status_error(status, msg, retry, "Anthropic")
            },
        )
        .await?;

        let mut state = StreamState { acc: BlockAccumulator::default(), model: req.model.clone(), stop_reason: None, usage: Usage::default() };
        let mut parser = SseParser::new();
        let mut bytes = resp.bytes_stream();
        let mut finished = false;

        'outer: loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(cancelled()),
                c = bytes.next() => c,
            };
            let Some(chunk) = chunk else { break };
            let chunk = chunk.map_err(http::map_reqwest_error)?;
            for ev in parser.push(&chunk) {
                if state.handle(&ev.data, on_event)? {
                    finished = true;
                    break 'outer;
                }
            }
        }
        if !finished {
            if let Some(ev) = parser.finish() {
                finished = state.handle(&ev.data, on_event)?;
            }
        }
        if !finished {
            return Err(AiError::new(AiErrorKind::Network, "The connection closed before the response finished."));
        }

        Ok(Completion {
            text: state.acc.text(),
            tool_calls: state.acc.tool_calls(),
            raw: state.acc.raw(),
            model: state.model,
            stop_reason: state.stop_reason.unwrap_or(StopReason::EndTurn),
            usage: state.usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::testutil::MockServer;
    use crate::ai::{Effort, ToolDef, ToolResult};

    fn req(model: &str) -> ChatRequest {
        ChatRequest {
            model: model.into(),
            system: "sys".into(),
            turns: vec![ChatTurn::user("hi")],
            max_tokens: 1000,
            effort: Some(Effort::Medium),
            tools: vec![],
        }
    }

    fn sse(events: &[(&str, Value)]) -> String {
        events.iter().map(|(e, d)| format!("event: {e}\ndata: {d}\n\n")).collect()
    }

    fn happy_stream() -> String {
        sse(&[
            ("message_start", json!({"type":"message_start","message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":12}}})),
            ("content_block_start", json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}})),
            ("content_block_delta", json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig123"}})),
            ("content_block_stop", json!({"type":"content_block_stop","index":0})),
            ("content_block_start", json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}})),
            ("ping", json!({"type":"ping"})),
            ("content_block_delta", json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hello"}})),
            ("content_block_delta", json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":", Ada."}})),
            ("content_block_stop", json!({"type":"content_block_stop","index":1})),
            ("message_delta", json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7}})),
            ("message_stop", json!({"type":"message_stop"})),
        ])
    }

    #[test]
    fn body_has_fallbacks_effort_and_caching_for_first_party_opus() {
        let b = build_body(&req("claude-opus-5-5"), true);
        assert_eq!(b["fallbacks"], "default");
        assert_eq!(b["output_config"]["effort"], "medium");
        assert_eq!(b["cache_control"]["type"], "ephemeral");
        assert_eq!(b["stream"], true);
        assert!(b.get("thinking").is_none(), "thinking must be omitted (adaptive by default)");
        assert!(b.get("temperature").is_none());
    }

    #[test]
    fn body_omits_unsupported_params() {
        let b = build_body(&req("claude-opus-5-5"), false);
        assert!(b.get("fallbacks").is_none(), "fallbacks only on the first-party API");
        let b = build_body(&req("claude-haiku-4-5"), true);
        assert!(b.get("output_config").is_none());
        assert!(b.get("fallbacks").is_none());
    }

    #[test]
    fn assistant_turns_replay_raw_blocks_unchanged() {
        let mut r = req("claude-opus-5-5");
        let blocks = json!([{"type":"thinking","thinking":"","signature":"s"},{"type":"text","text":"Hi"}]);
        r.turns.push(ChatTurn { raw: Some(blocks.clone()), ..ChatTurn::assistant("Hi") });
        r.turns.push(ChatTurn::user("again"));
        let b = build_body(&r, true);
        assert_eq!(b["messages"][1]["content"], blocks);
        assert_eq!(b["messages"][2]["content"], "again");
    }

    #[test]
    fn replay_drops_internal_blocks_before_fallback_boundary() {
        let blocks = vec![
            json!({"type":"thinking","thinking":"","signature":"a"}),
            json!({"type":"text","text":"Part one. "}),
            json!({"type":"fallback","from":{"model":"x"},"to":{"model":"y"}}),
            json!({"type":"thinking","thinking":"","signature":"b"}),
            json!({"type":"text","text":"Part two."}),
        ];
        let out = sanitize_replay_blocks(&blocks);
        let types: Vec<&str> = out.iter().map(|b| b["type"].as_str().unwrap()).collect();
        assert_eq!(types, vec!["text", "thinking", "text"]);
    }

    #[tokio::test]
    async fn streams_text_and_keeps_raw_blocks() {
        let server = MockServer::start(vec![(200, "text/event-stream", happy_stream())]).await;
        let p = AnthropicProvider::new("test-key".into(), Some(server.url())).unwrap();
        let mut deltas = Vec::new();
        let c = p
            .stream(&req("claude-opus-5-5"), &CancellationToken::new(), &mut |e| {
                let StreamEvent::TextDelta(t) = e;
                deltas.push(t)
            })
            .await
            .unwrap();
        assert_eq!(deltas, vec!["Hello", ", Ada."]);
        assert_eq!(c.text, "Hello, Ada.");
        assert_eq!(c.stop_reason, StopReason::EndTurn);
        assert_eq!(c.usage, Usage { input_tokens: Some(12), output_tokens: Some(7) });
        let raw = c.raw.unwrap();
        assert_eq!(raw[0]["signature"], "sig123");
        assert_eq!(raw[1]["text"], "Hello, Ada.");

        let reqs = server.requests().await;
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].header("x-api-key").as_deref(), Some("test-key"));
        assert_eq!(reqs[0].header("anthropic-version").as_deref(), Some(API_VERSION));
    }

    #[test]
    fn tools_are_strict_and_results_become_tool_result_blocks() {
        let mut r = req("claude-opus-5-5");
        r.tools = vec![ToolDef { name: "calculator".into(), description: "d".into(), input_schema: json!({"type":"object"}) }];
        r.turns.push(ChatTurn {
            tool_calls: vec![ToolCall { id: "toolu_1".into(), name: "calculator".into(), input: json!({"expression":"1+1"}), invalid_input: None }],
            ..ChatTurn::assistant("Let me check.")
        });
        r.turns.push(ChatTurn {
            tool_results: vec![ToolResult { call_id: "toolu_1".into(), content: "2".into(), is_error: false }],
            ..ChatTurn::user("")
        });
        let b = build_body(&r, true);
        assert_eq!(b["tools"][0]["strict"], true);
        assert!(b["tools"][0].get("eager_input_streaming").is_none());
        assert_eq!(b["messages"][1]["content"][1]["type"], "tool_use");
        assert_eq!(b["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(b["messages"][2]["content"][0]["tool_use_id"], "toolu_1");
        assert_eq!(b["messages"][2]["content"].as_array().unwrap().len(), 1, "no empty text block");
        assert!(build_body(&req("claude-opus-5-5"), true).get("tools").is_none());
    }

    #[tokio::test]
    async fn parses_tool_use_blocks() {
        let body = sse(&[
            ("message_start", json!({"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":5}}})),
            ("content_block_start", json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})),
            ("content_block_delta", json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Checking."}})),
            ("content_block_stop", json!({"type":"content_block_stop","index":0})),
            ("content_block_start", json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_9","name":"calculator","input":{}}})),
            ("content_block_delta", json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"expression\": "}})),
            ("content_block_delta", json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"2*21\"}"}})),
            ("content_block_stop", json!({"type":"content_block_stop","index":1})),
            ("content_block_start", json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_10","name":"calculator","input":{}}})),
            ("content_block_delta", json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"expression\": \"oops"}})),
            ("content_block_stop", json!({"type":"content_block_stop","index":2})),
            ("message_delta", json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}})),
            ("message_stop", json!({"type":"message_stop"})),
        ]);
        let server = MockServer::start(vec![(200, "text/event-stream", body)]).await;
        let p = AnthropicProvider::new("k".into(), Some(server.url())).unwrap();
        let c = p.stream(&req("claude-opus-5-5"), &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(c.stop_reason, StopReason::ToolUse);
        assert_eq!(c.tool_calls.len(), 2);
        assert_eq!(c.tool_calls[0].input, json!({"expression":"2*21"}));
        assert!(c.tool_calls[0].invalid_input.is_none());
        assert!(c.tool_calls[1].invalid_input.as_deref().unwrap().contains("oops"));
        let raw = c.raw.expect("raw stays replayable");
        assert_eq!(raw[1]["input"]["expression"], "2*21");
        assert_eq!(raw[2]["input"], json!({}));
    }

    #[test]
    fn tool_calls_before_a_fallback_boundary_are_not_executed() {
        let mut acc = BlockAccumulator::default();
        acc.start(0, json!({"type":"tool_use","id":"a","name":"calculator","input":{}}));
        acc.start(1, json!({"type":"fallback","from":{"model":"x"},"to":{"model":"y"}}));
        acc.start(2, json!({"type":"tool_use","id":"b","name":"calculator","input":{}}));
        let ids: Vec<String> = acc.tool_calls().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["b"]);
    }

    #[tokio::test]
    async fn reports_refusal_with_category() {
        let body = sse(&[
            ("message_start", json!({"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":3}}})),
            ("message_delta", json!({"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber"}},"usage":{"output_tokens":0}})),
            ("message_stop", json!({"type":"message_stop"})),
        ]);
        let server = MockServer::start(vec![(200, "text/event-stream", body)]).await;
        let p = AnthropicProvider::new("k".into(), Some(server.url())).unwrap();
        let c = p.stream(&req("claude-opus-5-5"), &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(c.stop_reason, StopReason::Refusal { category: Some("cyber".into()) });
        assert_eq!(c.text, "");
    }

    #[tokio::test]
    async fn auth_failure_is_not_retried() {
        let err = json!({"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}).to_string();
        let server = MockServer::start(vec![(401, "application/json", err)]).await;
        let p = AnthropicProvider::new("bad".into(), Some(server.url())).unwrap();
        let e = p.stream(&req("claude-opus-5-5"), &CancellationToken::new(), &mut |_| {}).await.unwrap_err();
        assert_eq!(e.kind, AiErrorKind::Authentication);
        assert_eq!(server.requests().await.len(), 1);
    }

    #[tokio::test]
    async fn retries_overloaded_then_succeeds() {
        let err = json!({"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}).to_string();
        let server = MockServer::start(vec![(529, "application/json", err), (200, "text/event-stream", happy_stream())]).await;
        let p = AnthropicProvider::new("k".into(), Some(server.url())).unwrap();
        let c = p.stream(&req("claude-opus-5-5"), &CancellationToken::new(), &mut |_| {}).await.unwrap();
        assert_eq!(c.text, "Hello, Ada.");
        assert_eq!(server.requests().await.len(), 2);
    }

    #[tokio::test]
    async fn mid_stream_error_event_fails() {
        let body = sse(&[
            ("message_start", json!({"type":"message_start","message":{"model":"m","usage":{}}})),
            ("error", json!({"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}})),
        ]);
        let server = MockServer::start(vec![(200, "text/event-stream", body)]).await;
        let p = AnthropicProvider::new("k".into(), Some(server.url())).unwrap();
        let e = p.stream(&req("claude-opus-5-5"), &CancellationToken::new(), &mut |_| {}).await.unwrap_err();
        assert_eq!(e.kind, AiErrorKind::Overloaded);
    }

    #[tokio::test]
    async fn truncated_stream_is_an_error_not_a_success() {
        let body = sse(&[
            ("message_start", json!({"type":"message_start","message":{"model":"m","usage":{}}})),
            ("content_block_start", json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}})),
            ("content_block_delta", json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Half"}})),
        ]);
        let server = MockServer::start(vec![(200, "text/event-stream", body)]).await;
        let p = AnthropicProvider::new("k".into(), Some(server.url())).unwrap();
        let e = p.stream(&req("claude-opus-5-5"), &CancellationToken::new(), &mut |_| {}).await.unwrap_err();
        assert_eq!(e.kind, AiErrorKind::Network);
    }

    #[tokio::test]
    async fn cancellation_stops_promptly() {
        let server = MockServer::start_stalling().await;
        let p = AnthropicProvider::new("k".into(), Some(server.url())).unwrap();
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            c2.cancel();
        });
        let started = std::time::Instant::now();
        let e = p.stream(&req("claude-opus-5-5"), &cancel, &mut |_| {}).await.unwrap_err();
        assert_eq!(e.kind, AiErrorKind::Cancelled);
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
    }
}
