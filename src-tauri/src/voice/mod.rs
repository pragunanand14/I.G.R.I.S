//! Speech-to-text and text-to-speech backends.
//!
//! Cloud/local speech services are called from the native backend so keys
//! never reach the UI. "browser" means the interface uses the webview's
//! built-in speech APIs instead (Windows: WebView2 speech recognition and
//! SAPI voices); the UI reports when those aren't available.

use std::time::Duration;

use reqwest::multipart;
use serde::Serialize;
use serde_json::{json, Value};

use crate::ai::http;
use crate::ai::{AiError, AiErrorKind};
use crate::config::AppConfig;

pub const MAX_AUDIO_BYTES: usize = 15 * 1024 * 1024;
pub const MAX_TTS_CHARS: usize = 4096;
const OPENAI_BASE: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpVoice {
    pub label: &'static str,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub voice: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// The webview's built-in speech API.
    Browser,
    Http(HttpVoice),
    /// Configured but unusable; the string says how to fix it.
    Misconfigured(String),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackendStatus {
    /// `browser`, `openai`, `local`
    pub mode: String,
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    pub stt: BackendStatus,
    pub tts: BackendStatus,
}

fn status(b: &Backend, configured: Option<&str>) -> BackendStatus {
    match b {
        Backend::Browser => BackendStatus { mode: "browser".into(), problem: None },
        Backend::Http(h) => BackendStatus { mode: h.label.into(), problem: None },
        Backend::Misconfigured(p) => BackendStatus { mode: configured.unwrap_or("unknown").into(), problem: Some(p.clone()) },
    }
}

fn http_backend(cfg: &AppConfig, provider: Option<&str>, kind: &str, default_model: &str, model: Option<&String>) -> Backend {
    let key = cfg.voice_api_key.clone().or_else(|| (cfg.ai_provider.as_deref() == Some("openai")).then(|| cfg.ai_api_key.clone()).flatten());
    let voice = cfg.tts_voice.clone().unwrap_or_else(|| "alloy".into());
    match provider {
        None | Some("browser") | Some("") => Backend::Browser,
        Some("openai") => match key {
            None => Backend::Misconfigured(format!("{kind}: set VOICE_API_KEY (an OpenAI key) or use the browser voice.")),
            Some(k) => Backend::Http(HttpVoice {
                label: "openai",
                base_url: cfg.voice_base_url.clone().unwrap_or_else(|| OPENAI_BASE.into()).trim_end_matches('/').into(),
                api_key: Some(k),
                model: model.cloned().unwrap_or_else(|| default_model.into()),
                voice,
            }),
        },
        Some("local") => match &cfg.voice_base_url {
            None => Backend::Misconfigured(format!("{kind}: set VOICE_BASE_URL to your local speech server (e.g. http://localhost:8000/v1).")),
            Some(base) => Backend::Http(HttpVoice {
                label: "local",
                base_url: base.trim_end_matches('/').into(),
                api_key: cfg.voice_api_key.clone(),
                model: model.cloned().unwrap_or_else(|| default_model.into()),
                voice,
            }),
        },
        Some(other) => Backend::Misconfigured(format!("{kind}: unknown provider '{other}'. Use browser, openai or local.")),
    }
}

pub fn stt_backend(cfg: &AppConfig) -> Backend {
    http_backend(cfg, cfg.stt_provider.as_deref(), "Speech recognition", "whisper-1", cfg.stt_model.as_ref())
}

pub fn tts_backend(cfg: &AppConfig) -> Backend {
    http_backend(cfg, cfg.tts_provider.as_deref(), "Speech output", "tts-1", cfg.tts_model.as_ref())
}

pub fn voice_status(cfg: &AppConfig) -> VoiceStatus {
    VoiceStatus { stt: status(&stt_backend(cfg), cfg.stt_provider.as_deref()), tts: status(&tts_backend(cfg), cfg.tts_provider.as_deref()) }
}

fn client() -> Result<reqwest::Client, AiError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| AiError::new(AiErrorKind::Network, e.to_string()))
}

fn extension_for(mime: &str) -> &'static str {
    let m = mime.split(';').next().unwrap_or_default().trim();
    match m {
        "audio/webm" => "webm",
        "audio/ogg" => "ogg",
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => "m4a",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        _ => "webm",
    }
}

async fn error_from(resp: reqwest::Response, label: &str) -> AiError {
    let status = resp.status();
    let retry = http::retry_after(&resp);
    let msg = resp.json::<Value>().await.ok().and_then(|v| v["error"]["message"].as_str().map(str::to_string));
    let mut e = http::status_error(status, msg, retry, label);
    if e.kind == AiErrorKind::Authentication {
        e.message = format!("{label} rejected the voice API key. Check VOICE_API_KEY.");
    }
    e
}

/// Transcribe recorded audio. Returns the recognised text (may be empty).
pub async fn transcribe(h: &HttpVoice, audio: Vec<u8>, mime: &str) -> Result<String, AiError> {
    if audio.is_empty() {
        return Err(AiError::new(AiErrorKind::InvalidRequest, "No audio was recorded."));
    }
    if audio.len() > MAX_AUDIO_BYTES {
        return Err(AiError::new(AiErrorKind::InvalidRequest, "The recording is too long."));
    }
    let mime_clean = mime.split(';').next().unwrap_or("audio/webm").trim().to_string();
    let part = multipart::Part::bytes(audio)
        .file_name(format!("speech.{}", extension_for(mime)))
        .mime_str(&mime_clean)
        .map_err(|e| AiError::new(AiErrorKind::InvalidRequest, e.to_string()))?;
    let form = multipart::Form::new().text("model", h.model.clone()).text("response_format", "json").part("file", part);
    let mut rb = client()?.post(format!("{}/audio/transcriptions", h.base_url)).multipart(form);
    if let Some(k) = &h.api_key {
        rb = rb.bearer_auth(k);
    }
    let resp = rb.send().await.map_err(http::map_reqwest_error)?;
    if !resp.status().is_success() {
        return Err(error_from(resp, "Speech recognition").await);
    }
    let v: Value = resp.json().await.map_err(|e| AiError::new(AiErrorKind::Protocol, format!("Bad transcription response: {e}")))?;
    Ok(v["text"].as_str().unwrap_or_default().trim().to_string())
}

/// Synthesize speech; returns MP3 bytes.
pub async fn synthesize(h: &HttpVoice, text: &str) -> Result<Vec<u8>, AiError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AiError::new(AiErrorKind::InvalidRequest, "Nothing to say."));
    }
    let text: String = text.chars().take(MAX_TTS_CHARS).collect();
    let mut rb =
        client()?.post(format!("{}/audio/speech", h.base_url)).json(&json!({ "model": h.model, "input": text, "voice": h.voice, "response_format": "mp3" }));
    if let Some(k) = &h.api_key {
        rb = rb.bearer_auth(k);
    }
    let resp = rb.send().await.map_err(http::map_reqwest_error)?;
    if !resp.status().is_success() {
        return Err(error_from(resp, "Speech output").await);
    }
    Ok(resp.bytes().await.map_err(http::map_reqwest_error)?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::testutil::MockServer;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> AppConfig {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        AppConfig::from_map(|k| m.get(k).cloned())
    }

    #[test]
    fn backend_resolution() {
        assert_eq!(stt_backend(&cfg(&[])), Backend::Browser);
        assert!(matches!(stt_backend(&cfg(&[("STT_PROVIDER", "openai")])), Backend::Misconfigured(m) if m.contains("VOICE_API_KEY")));
        match stt_backend(&cfg(&[("STT_PROVIDER", "OpenAI"), ("AI_PROVIDER", "openai"), ("AI_API_KEY", "sk")])) {
            Backend::Http(h) => {
                assert_eq!(h.base_url, OPENAI_BASE);
                assert_eq!(h.model, "whisper-1");
                assert_eq!(h.api_key.as_deref(), Some("sk"), "reuses the OpenAI chat key");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(tts_backend(&cfg(&[("TTS_PROVIDER", "local")])), Backend::Misconfigured(m) if m.contains("VOICE_BASE_URL")));
        match tts_backend(&cfg(&[("TTS_PROVIDER", "local"), ("VOICE_BASE_URL", "http://localhost:8000/v1/"), ("TTS_VOICE", "nova")])) {
            Backend::Http(h) => {
                assert_eq!(h.base_url, "http://localhost:8000/v1");
                assert_eq!(h.voice, "nova");
                assert!(h.api_key.is_none());
            }
            other => panic!("{other:?}"),
        }
        let st = voice_status(&cfg(&[("STT_PROVIDER", "whisperx")]));
        assert!(st.stt.problem.unwrap().contains("unknown provider"));
        assert_eq!(st.tts.mode, "browser");
    }

    fn http(url: String) -> HttpVoice {
        HttpVoice { label: "openai", base_url: url, api_key: Some("vk".into()), model: "whisper-1".into(), voice: "alloy".into() }
    }

    #[tokio::test]
    async fn transcribes_via_multipart() {
        let server = MockServer::start(vec![(200, "application/json", json!({"text":"  Open VS Code.  "}).to_string())]).await;
        let text = transcribe(&http(server.url()), b"fake-webm-bytes".to_vec(), "audio/webm;codecs=opus").await.unwrap();
        assert_eq!(text, "Open VS Code.");
        let req = &server.requests().await[0];
        assert!(req.head.starts_with("POST /audio/transcriptions"));
        assert_eq!(req.header("authorization").as_deref(), Some("Bearer vk"));
        assert!(req.header("content-type").unwrap().starts_with("multipart/form-data"));
        assert!(req.body.contains("name=\"model\"") && req.body.contains("whisper-1"));
        assert!(req.body.contains("filename=\"speech.webm\""));
        assert!(req.body.contains("fake-webm-bytes"));
    }

    #[tokio::test]
    async fn rejects_empty_or_huge_audio_and_reports_bad_keys() {
        let h = http("http://127.0.0.1:9".into());
        assert!(transcribe(&h, vec![], "audio/webm").await.is_err());
        assert!(transcribe(&h, vec![0; MAX_AUDIO_BYTES + 1], "audio/webm").await.is_err());
        let server = MockServer::start(vec![(401, "application/json", "{}".into())]).await;
        let e = transcribe(&http(server.url()), vec![1], "audio/webm").await.unwrap_err();
        assert!(e.message.contains("VOICE_API_KEY"));
    }

    #[tokio::test]
    async fn synthesizes_mp3() {
        let server = MockServer::start(vec![(200, "audio/mpeg", "ID3fake".into())]).await;
        let mut h = http(server.url());
        h.model = "tts-1".into();
        let bytes = synthesize(&h, "Opening VS Code.").await.unwrap();
        assert_eq!(bytes, b"ID3fake");
        let body = server.requests().await[0].json();
        assert_eq!(body["input"], "Opening VS Code.");
        assert_eq!(body["voice"], "alloy");
        assert_eq!(body["response_format"], "mp3");
        assert!(synthesize(&h, "   ").await.is_err());
    }
}
