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
const GROQ_BASE: &str = "https://api.groq.com/openai/v1";
const GEMINI_NATIVE_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
/// Natural-sounding defaults per service.
const GROQ_TTS_MODEL: &str = "canopylabs/orpheus-v1-english";
const GROQ_TTS_VOICE: &str = "troy";
const GROQ_STT_MODEL: &str = "whisper-large-v3-turbo";
const GEMINI_TTS_MODEL: &str = "gemini-2.5-flash-preview-tts";
const GEMINI_TTS_VOICE: &str = "Charon";
/// Groq's Orpheus voices accept at most 200 characters per request.
const ORPHEUS_MAX_CHARS: usize = 190;

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
        Some("groq") => match &cfg.voice_api_key {
            None => Backend::Misconfigured(format!("{kind}: set VOICE_API_KEY to your Groq API key.")),
            Some(k) => Backend::Http(HttpVoice {
                label: "groq",
                base_url: cfg.voice_base_url.clone().filter(|u| u.contains("groq.com")).unwrap_or_else(|| GROQ_BASE.into()).trim_end_matches('/').into(),
                api_key: Some(k.clone()),
                model: model.cloned().unwrap_or_else(|| (if kind == "Speech output" { GROQ_TTS_MODEL } else { GROQ_STT_MODEL }).into()),
                voice: cfg.tts_voice.clone().unwrap_or_else(|| GROQ_TTS_VOICE.into()),
            }),
        },
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
        Some(other) => Backend::Misconfigured(format!("{kind}: unknown provider '{other}'. Use browser, groq, gemini, openai or local.")),
    }
}

pub fn stt_backend(cfg: &AppConfig) -> Backend {
    if cfg.stt_provider.as_deref() == Some("gemini") {
        return gemini_stt(cfg);
    }
    http_backend(cfg, cfg.stt_provider.as_deref(), "Speech recognition", "whisper-1", cfg.stt_model.as_ref())
}

/// Gemini transcribes via its chat endpoint with audio input. Reuses the chat
/// key and model when the chat provider is Gemini, so one key covers both.
fn gemini_stt(cfg: &AppConfig) -> Backend {
    let chat_is_gemini = cfg.ai_provider.as_deref() == Some("gemini");
    let key = cfg.voice_api_key.clone().or_else(|| chat_is_gemini.then(|| cfg.ai_api_key.clone()).flatten());
    let model = cfg.stt_model.clone().or_else(|| chat_is_gemini.then(|| cfg.ai_model.clone()).flatten());
    match (key, model) {
        (None, _) => Backend::Misconfigured("Speech recognition: set VOICE_API_KEY to a Gemini key (or use AI_PROVIDER=gemini).".into()),
        (_, None) => Backend::Misconfigured("Speech recognition: set STT_MODEL to a Gemini model (e.g. gemini-flash-lite-latest).".into()),
        (Some(k), Some(m)) => {
            Backend::Http(HttpVoice { label: "gemini", base_url: crate::ai::openai::GEMINI_BASE_URL.into(), api_key: Some(k), model: m, voice: String::new() })
        }
    }
}

pub fn tts_backend(cfg: &AppConfig) -> Backend {
    if cfg.tts_provider.as_deref() == Some("gemini") {
        return gemini_tts(cfg);
    }
    http_backend(cfg, cfg.tts_provider.as_deref(), "Speech output", "tts-1", cfg.tts_model.as_ref())
}

/// Gemini's speech models (natural voices such as Charon, Kore, Puck). Uses the
/// chat key when the chat provider is Gemini, otherwise VOICE_API_KEY.
fn gemini_tts(cfg: &AppConfig) -> Backend {
    let key = if cfg.ai_provider.as_deref() == Some("gemini") { cfg.ai_api_key.clone() } else { cfg.voice_api_key.clone() };
    match key {
        None => Backend::Misconfigured("Speech output: Gemini voices need a Gemini key (AI_PROVIDER=gemini, or VOICE_API_KEY).".into()),
        Some(k) => Backend::Http(HttpVoice {
            label: "gemini",
            base_url: GEMINI_NATIVE_BASE.into(),
            api_key: Some(k),
            model: cfg.tts_model.clone().unwrap_or_else(|| GEMINI_TTS_MODEL.into()),
            voice: cfg.tts_voice.clone().unwrap_or_else(|| GEMINI_TTS_VOICE.into()),
        }),
    }
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
    let msg = resp.json::<Value>().await.ok().and_then(|v| crate::ai::openai::error_message(&v));
    let mut e = http::status_error(status, msg, retry, label);
    if e.kind == AiErrorKind::Authentication {
        e.message = format!("{label} rejected the voice API key. Check VOICE_API_KEY.");
    }
    e
}

/// Context makes short commands far more accurate, and the explicit "nothing"
/// rule stops the model inventing text for silence or noise.
fn gemini_transcribe_prompt(language: Option<&str>) -> String {
    let lang = match language {
        Some(l) => format!("The speaker is speaking language code \"{l}\"; write it in that language and never translate."),
        None => "Write it in the language spoken; never translate.".to_string(),
    };
    format!(
        "You are a speech-to-text engine. The audio is someone talking to their desktop assistant (IGRIS): usually a short \
command or question, e.g. about reminders, apps, files or their computer. Transcribe exactly the words spoken. {lang} \
Keep names, numbers and technical terms as spoken. Do not answer, summarise, fix grammar or add anything. Reply with \
only the transcription — no quotes or labels. If the audio has no clear speech (silence, noise, music), reply with \
nothing at all."
    )
}

/// Gemini 3 thinks by default, which only slows transcription down.
fn gemini_reasoning_effort(model: &str) -> &'static str {
    if model.contains("2.5") {
        "none"
    } else {
        "low"
    }
}

/// Transcribe recorded audio. Returns the recognised text (may be empty).
pub async fn transcribe(h: &HttpVoice, audio: Vec<u8>, mime: &str, language: Option<&str>) -> Result<String, AiError> {
    if audio.is_empty() {
        return Err(AiError::new(AiErrorKind::InvalidRequest, "No audio was recorded."));
    }
    if audio.len() > MAX_AUDIO_BYTES {
        return Err(AiError::new(AiErrorKind::InvalidRequest, "The recording is too long."));
    }
    if h.label == "gemini" {
        return transcribe_gemini(h, audio, mime, language).await;
    }
    let mime_clean = mime.split(';').next().unwrap_or("audio/webm").trim().to_string();
    let part = multipart::Part::bytes(audio)
        .file_name(format!("speech.{}", extension_for(mime)))
        .mime_str(&mime_clean)
        .map_err(|e| AiError::new(AiErrorKind::InvalidRequest, e.to_string()))?;
    let mut form = multipart::Form::new().text("model", h.model.clone()).text("response_format", "json").text("temperature", "0");
    if let Some(l) = language {
        // Whisper takes ISO 639-1 ("en"); drop any region ("en-in" → "en").
        form = form.text("language", l.split(['-', '_']).next().unwrap_or(l).to_string());
    }
    let form = form.part("file", part);
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

/// Gemini accepts audio as `input_audio` on its OpenAI-compatible chat endpoint
/// (WAV or MP3; the UI converts recordings to WAV for this backend).
async fn transcribe_gemini(h: &HttpVoice, audio: Vec<u8>, mime: &str, language: Option<&str>) -> Result<String, AiError> {
    use base64::Engine;
    let format = match extension_for(mime) {
        "mp3" => "mp3",
        "wav" => "wav",
        other => return Err(AiError::new(AiErrorKind::InvalidRequest, format!("Gemini needs WAV or MP3 audio, not {other}."))),
    };
    let body = json!({
        "model": h.model,
        "temperature": 0,
        "reasoning_effort": gemini_reasoning_effort(&h.model),
        "messages": [{ "role": "user", "content": [
            { "type": "text", "text": gemini_transcribe_prompt(language) },
            { "type": "input_audio", "input_audio": { "data": base64::engine::general_purpose::STANDARD.encode(&audio), "format": format } }
        ]}]
    });
    let mut rb = client()?.post(format!("{}/chat/completions", h.base_url)).json(&body);
    if let Some(k) = &h.api_key {
        rb = rb.bearer_auth(k);
    }
    let resp = rb.send().await.map_err(http::map_reqwest_error)?;
    if !resp.status().is_success() {
        return Err(error_from(resp, "Speech recognition (Gemini)").await);
    }
    let v: Value = resp.json().await.map_err(|e| AiError::new(AiErrorKind::Protocol, format!("Bad transcription response: {e}")))?;
    let text = v["choices"][0]["message"]["content"].as_str().unwrap_or_default().trim();
    Ok(text.trim_matches('"').trim().to_string())
}

/// Synthesize speech; returns audio bytes (MP3 or WAV — the UI detects which).
pub async fn synthesize(h: &HttpVoice, text: &str) -> Result<Vec<u8>, AiError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AiError::new(AiErrorKind::InvalidRequest, "Nothing to say."));
    }
    let text: String = text.chars().take(MAX_TTS_CHARS).collect();
    if h.label == "gemini" {
        return synthesize_gemini(h, &text).await;
    }
    if is_orpheus(h) {
        // Short requests, joined into one WAV.
        let mut parts = Vec::new();
        for chunk in split_for_tts(&text, ORPHEUS_MAX_CHARS) {
            parts.push(speech_request(h, &chunk, "wav").await?);
        }
        return concat_wavs(&parts).ok_or_else(|| AiError::new(AiErrorKind::Protocol, "The speech service returned audio IGRIS couldn't read."));
    }
    speech_request(h, &text, "mp3").await
}

/// Groq's Orpheus models: WAV only, 200 characters per request.
fn is_orpheus(h: &HttpVoice) -> bool {
    h.model.to_ascii_lowercase().contains("orpheus") || h.base_url.contains("groq.com")
}

async fn speech_request(h: &HttpVoice, text: &str, format: &str) -> Result<Vec<u8>, AiError> {
    let mut rb =
        client()?.post(format!("{}/audio/speech", h.base_url)).json(&json!({ "model": h.model, "input": text, "voice": h.voice, "response_format": format }));
    if let Some(k) = &h.api_key {
        rb = rb.bearer_auth(k);
    }
    let resp = rb.send().await.map_err(http::map_reqwest_error)?;
    if !resp.status().is_success() {
        return Err(error_from(resp, "Speech output").await);
    }
    Ok(resp.bytes().await.map_err(http::map_reqwest_error)?.to_vec())
}

/// Gemini speech: `generateContent` with audio output (24 kHz 16-bit PCM), wrapped as WAV.
async fn synthesize_gemini(h: &HttpVoice, text: &str) -> Result<Vec<u8>, AiError> {
    use base64::Engine;
    let body = json!({
        "contents": [{ "parts": [{ "text": text }] }],
        "generationConfig": {
            "responseModalities": ["AUDIO"],
            "speechConfig": { "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": h.voice } } }
        }
    });
    let mut rb = client()?.post(format!("{}/models/{}:generateContent", h.base_url, h.model)).json(&body);
    if let Some(k) = &h.api_key {
        rb = rb.header("x-goog-api-key", k);
    }
    let resp = rb.send().await.map_err(http::map_reqwest_error)?;
    if !resp.status().is_success() {
        return Err(error_from(resp, "Speech output (Gemini)").await);
    }
    let v: Value = resp.json().await.map_err(|e| AiError::new(AiErrorKind::Protocol, format!("Bad speech response: {e}")))?;
    let part = v["candidates"][0]["content"]["parts"].as_array().and_then(|p| p.iter().find(|p| p["inlineData"]["data"].is_string()));
    let Some(part) = part else {
        return Err(AiError::new(AiErrorKind::Protocol, "Gemini returned no audio for that text."));
    };
    let pcm = base64::engine::general_purpose::STANDARD
        .decode(part["inlineData"]["data"].as_str().unwrap_or_default())
        .map_err(|_| AiError::new(AiErrorKind::Protocol, "Gemini returned invalid audio data."))?;
    let rate = part["inlineData"]["mimeType"]
        .as_str()
        .and_then(|m| m.split(';').find_map(|p| p.trim().strip_prefix("rate=")))
        .and_then(|r| r.parse().ok())
        .unwrap_or(24_000);
    Ok(pcm_to_wav(&pcm, rate, 1, 16))
}

/// Split text into pieces of at most `max` characters, preferring sentence,
/// then clause, then word boundaries.
pub fn split_for_tts(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.trim().to_string();
    while rest.chars().count() > max {
        let head: String = rest.chars().take(max).collect();
        let cut = [". ", "! ", "? ", "; ", ": ", ", ", " "]
            .iter()
            .find_map(|sep| head.rfind(sep).filter(|&i| i >= max / 3).map(|i| i + sep.trim_end().len()))
            .unwrap_or(head.len());
        out.push(head[..cut].trim().to_string());
        rest = rest[cut..].trim().to_string();
    }
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

pub fn pcm_to_wav(pcm: &[u8], rate: u32, channels: u16, bits: u16) -> Vec<u8> {
    let block = channels * bits / 8;
    let mut w = Vec::with_capacity(44 + pcm.len());
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&channels.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * block as u32).to_le_bytes());
    w.extend_from_slice(&block.to_le_bytes());
    w.extend_from_slice(&bits.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    w.extend_from_slice(pcm);
    w
}

/// (sample rate, channels, bits, PCM data) of a PCM WAV file. Tolerates the
/// "unknown length" sizes streaming encoders write.
fn parse_wav(b: &[u8]) -> Option<(u32, u16, u16, &[u8])> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let (mut pos, mut fmt) = (12usize, None);
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let size = u32::from_le_bytes(b[pos + 4..pos + 8].try_into().ok()?) as usize;
        let start = pos + 8;
        let end = start.saturating_add(size).min(b.len());
        if id == b"fmt " && end - start >= 16 {
            let f = &b[start..end];
            fmt = Some((
                u32::from_le_bytes(f[4..8].try_into().ok()?),
                u16::from_le_bytes(f[2..4].try_into().ok()?),
                u16::from_le_bytes(f[14..16].try_into().ok()?),
            ));
        } else if id == b"data" {
            let (rate, ch, bits) = fmt?;
            return Some((rate, ch, bits, &b[start..end]));
        }
        pos = start + size + (size & 1);
    }
    None
}

/// Join WAV files with the same format into one.
pub fn concat_wavs(parts: &[Vec<u8>]) -> Option<Vec<u8>> {
    if parts.len() == 1 {
        return Some(parts[0].clone());
    }
    let mut format = None;
    let mut pcm = Vec::new();
    for p in parts {
        let (rate, ch, bits, data) = parse_wav(p)?;
        if format.is_some_and(|f| f != (rate, ch, bits)) {
            return None;
        }
        format = Some((rate, ch, bits));
        pcm.extend_from_slice(data);
    }
    let (rate, ch, bits) = format?;
    Some(pcm_to_wav(&pcm, rate, ch, bits))
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

    #[test]
    fn gemini_speech_reuses_the_chat_key_and_model() {
        match stt_backend(&cfg(&[("STT_PROVIDER", "gemini"), ("AI_PROVIDER", "gemini"), ("AI_API_KEY", "AIza"), ("AI_MODEL", "gemini-flash-lite-latest")])) {
            Backend::Http(h) => {
                assert_eq!((h.label, h.api_key.as_deref(), h.model.as_str()), ("gemini", Some("AIza"), "gemini-flash-lite-latest"));
                assert!(h.base_url.contains("generativelanguage.googleapis.com"));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(stt_backend(&cfg(&[("STT_PROVIDER", "gemini")])), Backend::Misconfigured(m) if m.contains("VOICE_API_KEY")));
        assert!(matches!(stt_backend(&cfg(&[("STT_PROVIDER", "gemini"), ("VOICE_API_KEY", "k")])), Backend::Misconfigured(m) if m.contains("STT_MODEL")));
    }

    #[tokio::test]
    async fn gemini_transcribes_wav_through_chat_with_input_audio() {
        let reply = json!({"choices":[{"message":{"role":"assistant","content":"\"remind me to stretch\"\n"}}]}).to_string();
        let server = MockServer::start(vec![(200, "application/json", reply)]).await;
        let h = HttpVoice { label: "gemini", base_url: server.url(), api_key: Some("AIza".into()), model: "gemini-x".into(), voice: String::new() };
        let text = transcribe(&h, b"RIFF....WAVE".to_vec(), "audio/wav", Some("en")).await.unwrap();
        assert_eq!(text, "remind me to stretch");
        let req = server.requests().await[0].json();
        assert_eq!(req["model"], "gemini-x");
        assert_eq!(req["messages"][0]["content"][1]["input_audio"]["format"], "wav");
        assert_eq!(req["reasoning_effort"], "low");
        assert!(req["messages"][0]["content"][0]["text"].as_str().unwrap().contains("\"en\""));
        assert_eq!(gemini_reasoning_effort("gemini-2.5-flash"), "none");
        assert!(transcribe(&h, b"x".to_vec(), "audio/webm", None).await.unwrap_err().message.contains("WAV or MP3"));
    }

    #[tokio::test]
    async fn transcribes_via_multipart() {
        let server = MockServer::start(vec![(200, "application/json", json!({"text":"  Open VS Code.  "}).to_string())]).await;
        let text = transcribe(&http(server.url()), b"fake-webm-bytes".to_vec(), "audio/webm;codecs=opus", Some("en-in")).await.unwrap();
        assert_eq!(text, "Open VS Code.");
        let req = &server.requests().await[0];
        assert!(req.head.starts_with("POST /audio/transcriptions"));
        assert_eq!(req.header("authorization").as_deref(), Some("Bearer vk"));
        assert!(req.header("content-type").unwrap().starts_with("multipart/form-data"));
        assert!(req.body.contains("name=\"model\"") && req.body.contains("whisper-1"));
        assert!(req.body.contains("filename=\"speech.webm\""));
        assert!(req.body.contains("fake-webm-bytes"));
        assert!(req.body.contains("name=\"language\"\r\n\r\nen\r\n"), "region stripped for Whisper");
    }

    #[tokio::test]
    async fn rejects_empty_or_huge_audio_and_reports_bad_keys() {
        let h = http("http://127.0.0.1:9".into());
        assert!(transcribe(&h, vec![], "audio/webm", None).await.is_err());
        assert!(transcribe(&h, vec![0; MAX_AUDIO_BYTES + 1], "audio/webm", None).await.is_err());
        let server = MockServer::start(vec![(401, "application/json", "{}".into())]).await;
        let e = transcribe(&http(server.url()), vec![1], "audio/webm", None).await.unwrap_err();
        assert!(e.message.contains("VOICE_API_KEY"));
    }

    #[test]
    fn natural_voice_providers() {
        match tts_backend(&cfg(&[("TTS_PROVIDER", "groq"), ("VOICE_API_KEY", "gsk")])) {
            Backend::Http(h) => {
                assert_eq!((h.label, h.base_url.as_str(), h.model.as_str(), h.voice.as_str()), ("groq", GROQ_BASE, GROQ_TTS_MODEL, GROQ_TTS_VOICE))
            }
            other => panic!("{other:?}"),
        }
        match stt_backend(&cfg(&[("STT_PROVIDER", "groq"), ("VOICE_API_KEY", "gsk")])) {
            Backend::Http(h) => assert_eq!(h.model, GROQ_STT_MODEL),
            other => panic!("{other:?}"),
        }
        assert!(matches!(tts_backend(&cfg(&[("TTS_PROVIDER", "groq")])), Backend::Misconfigured(_)));
        // Gemini voices reuse the chat key even when VOICE_API_KEY is a Groq key.
        match tts_backend(&cfg(&[("TTS_PROVIDER", "gemini"), ("AI_PROVIDER", "gemini"), ("AI_API_KEY", "g-key"), ("VOICE_API_KEY", "gsk")])) {
            Backend::Http(h) => assert_eq!((h.label, h.api_key.as_deref(), h.voice.as_str()), ("gemini", Some("g-key"), GEMINI_TTS_VOICE)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn splits_text_and_joins_audio() {
        let text = "Your email to Professor Sharma is drafted and waiting in Gmail. I added a subject line, a short apology for the delay, and a promise to submit tomorrow morning. It is signed with your name, and nothing has been sent yet. Want me to send it?";
        let parts = split_for_tts(text, ORPHEUS_MAX_CHARS);
        assert!(parts.len() >= 2 && parts.iter().all(|p| p.chars().count() <= ORPHEUS_MAX_CHARS), "{parts:?}");
        assert_eq!(parts.join(" "), text);
        assert_eq!(split_for_tts("Short.", 190), vec!["Short."]);

        let a = pcm_to_wav(&[1, 0, 2, 0], 24_000, 1, 16);
        let mut b = pcm_to_wav(&[3, 0], 24_000, 1, 16);
        b[40..44].copy_from_slice(&u32::MAX.to_le_bytes()); // streamed "unknown length"
        let joined = concat_wavs(&[a, b]).unwrap();
        assert_eq!(parse_wav(&joined).unwrap(), (24_000, 1, 16, &[1u8, 0, 2, 0, 3, 0][..]));
        assert!(concat_wavs(&[pcm_to_wav(&[0, 0], 24_000, 1, 16), pcm_to_wav(&[0, 0], 16_000, 1, 16)]).is_none(), "mixed formats");
        assert!(concat_wavs(&[b"not audio".to_vec(), b"x".to_vec()]).is_none());
    }

    #[tokio::test]
    async fn gemini_voice_returns_wav() {
        use base64::Engine;
        let pcm = base64::engine::general_purpose::STANDARD.encode([5u8, 0, 6, 0]);
        let server = MockServer::start(vec![(
            200,
            "application/json",
            serde_json::json!({ "candidates": [{ "content": { "parts": [{ "inlineData": { "mimeType": "audio/L16;codec=pcm;rate=24000", "data": pcm } }] } }] }).to_string(),
        )])
        .await;
        let h = HttpVoice { label: "gemini", base_url: server.url(), api_key: Some("k".into()), model: GEMINI_TTS_MODEL.into(), voice: "Kore".into() };
        let wav = synthesize(&h, "Hello there.").await.unwrap();
        assert_eq!(parse_wav(&wav).unwrap(), (24_000, 1, 16, &[5u8, 0, 6, 0][..]));
        let req = &server.requests().await[0];
        assert!(req.head.contains(":generateContent"), "{}", req.head);
        assert_eq!(req.header("x-goog-api-key").as_deref(), Some("k"));
        assert!(req.body.contains("\"voiceName\":\"Kore\"") && req.body.contains("AUDIO"));
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
