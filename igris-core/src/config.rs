//! Environment-based configuration.
//!
//! Configuration (including secrets such as API keys) is loaded only in the
//! native backend. The UI receives a redacted [`PublicConfig`] that reports
//! *whether* a secret is configured, never its value.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Keys IGRIS reads from the environment / `.env` files.
const KEYS: &[&str] = &[
    "AI_PROVIDER",
    "AI_API_KEY",
    "AI_MODEL",
    "AI_BASE_URL",
    "AI_VISION_MODEL",
    "AI_FAST_MODEL",
    "AI_FALLBACK_MODEL",
    "AI_CONTEXT_WINDOW",
    "DATABASE_URL",
    "SEARCH_PROVIDER",
    "SEARCH_API_KEY",
    "TTS_PROVIDER",
    "STT_PROVIDER",
    "VOICE_API_KEY",
    "VOICE_BASE_URL",
    "STT_MODEL",
    "STT_LANGUAGE",
    "TTS_MODEL",
    "TTS_VOICE",
    "IGRIS_LOG",
];

#[derive(Clone, Default)]
pub struct AppConfig {
    pub ai_provider: Option<String>,
    pub ai_api_key: Option<String>,
    pub ai_model: Option<String>,
    pub ai_base_url: Option<String>,
    /// Model for requests with images (defaults to the chat model).
    pub ai_vision_model: Option<String>,
    /// Model for cheap background work like history compaction (defaults to the chat model).
    pub ai_fast_model: Option<String>,
    /// Backup model, same provider: used when the main one is overloaded or down.
    pub ai_fallback_model: Option<String>,
    /// Context window of the configured models, in tokens (for local servers).
    pub ai_context_window: Option<u32>,
    pub database_url: Option<PathBuf>,
    pub search_provider: Option<String>,
    pub search_api_key: Option<String>,
    pub tts_provider: Option<String>,
    pub stt_provider: Option<String>,
    pub voice_api_key: Option<String>,
    pub voice_base_url: Option<String>,
    pub stt_model: Option<String>,
    /// Spoken language hint for speech recognition, e.g. `en` (ISO 639-1).
    pub stt_language: Option<String>,
    pub tts_model: Option<String>,
    pub tts_voice: Option<String>,
    pub log_filter: Option<String>,
    /// `.env` files that were found and read, in load order.
    pub env_files: Vec<PathBuf>,
}

// Manual Debug so secrets can never leak through `{:?}` in logs.
impl std::fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppConfig")
            .field("ai_provider", &self.ai_provider)
            .field("ai_api_key", &self.ai_api_key.as_ref().map(|_| "<redacted>"))
            .field("ai_model", &self.ai_model)
            .field("ai_base_url", &self.ai_base_url)
            .field("database_url", &self.database_url)
            .field("search_provider", &self.search_provider)
            .field("search_api_key", &self.search_api_key.as_ref().map(|_| "<redacted>"))
            .field("tts_provider", &self.tts_provider)
            .field("stt_provider", &self.stt_provider)
            .field("voice_api_key", &self.voice_api_key.as_ref().map(|_| "<redacted>"))
            .field("voice_base_url", &self.voice_base_url)
            .field("env_files", &self.env_files)
            .finish()
    }
}

/// Redacted view of the configuration that is safe to send to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicConfig {
    pub ai_provider: Option<String>,
    pub ai_model: Option<String>,
    pub ai_base_url: Option<String>,
    pub ai_key_configured: bool,
    pub search_configured: bool,
    /// How web search works: `brave`, `tavily`, `anthropic` (built-in) or none.
    pub web_search: Option<String>,
    pub tts_provider: Option<String>,
    pub stt_provider: Option<String>,
    pub env_files: Vec<String>,
}

impl AppConfig {
    /// Load configuration. Precedence (highest first):
    /// process environment > `<config_dir>/.env` > `./.env`.
    pub fn load(config_dir: Option<&Path>) -> Self {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(dir) = config_dir {
            candidates.push(dir.join(".env"));
        }
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join(".env"));
        }

        let mut merged: HashMap<String, String> = HashMap::new();
        let mut env_files = Vec::new();
        // Earlier candidates win, so insert only if absent.
        for path in candidates {
            if !path.is_file() || env_files.contains(&path) {
                continue;
            }
            match dotenvy::from_path_iter(&path) {
                Ok(iter) => {
                    for (k, v) in iter.flatten() {
                        merged.entry(k).or_insert(v);
                    }
                    env_files.push(path);
                }
                Err(err) => {
                    tracing::warn!(event = "CONFIG_ENV_FILE_INVALID", path = %path.display(), error = %err);
                }
            }
        }

        let process: HashMap<String, String> = KEYS.iter().filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v))).collect();

        let mut cfg = Self::from_map(|key| process.get(key).or_else(|| merged.get(key)).cloned());
        cfg.env_files = env_files;
        cfg
    }

    /// Build a config from a lookup function. Empty values are treated as unset.
    pub fn from_map(get: impl Fn(&str) -> Option<String>) -> Self {
        let read = |k: &str| get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        Self {
            ai_provider: read("AI_PROVIDER").map(|v| v.to_lowercase()),
            ai_api_key: read("AI_API_KEY"),
            ai_model: read("AI_MODEL"),
            ai_base_url: read("AI_BASE_URL"),
            ai_vision_model: read("AI_VISION_MODEL"),
            ai_fast_model: read("AI_FAST_MODEL"),
            ai_fallback_model: read("AI_FALLBACK_MODEL"),
            ai_context_window: read("AI_CONTEXT_WINDOW").and_then(|v| v.replace(['_', ','], "").parse().ok()),
            database_url: read("DATABASE_URL").map(PathBuf::from),
            search_provider: read("SEARCH_PROVIDER").map(|v| v.to_lowercase()),
            search_api_key: read("SEARCH_API_KEY"),
            tts_provider: read("TTS_PROVIDER").map(|v| v.to_lowercase()),
            stt_provider: read("STT_PROVIDER").map(|v| v.to_lowercase()),
            voice_api_key: read("VOICE_API_KEY"),
            voice_base_url: read("VOICE_BASE_URL"),
            stt_model: read("STT_MODEL"),
            stt_language: read("STT_LANGUAGE").map(|v| v.to_lowercase()),
            tts_model: read("TTS_MODEL"),
            tts_voice: read("TTS_VOICE"),
            log_filter: read("IGRIS_LOG"),
            env_files: Vec::new(),
        }
    }

    /// Which web search backend is usable with this configuration.
    /// A Brave/Tavily key wins; otherwise Anthropic's built-in search when Anthropic is the AI provider.
    pub fn web_search_mode(&self) -> Option<&'static str> {
        match (self.search_provider.as_deref(), self.search_api_key.is_some()) {
            (Some("brave"), true) => Some("brave"),
            (Some("tavily"), true) => Some("tavily"),
            (None, true) => Some("brave"),
            _ if self.ai_provider.as_deref() == Some("anthropic") && self.ai_api_key.is_some() => Some("anthropic"),
            _ => None,
        }
    }

    pub fn public(&self) -> PublicConfig {
        PublicConfig {
            ai_provider: self.ai_provider.clone(),
            ai_model: self.ai_model.clone(),
            ai_base_url: self.ai_base_url.clone(),
            ai_key_configured: self.ai_api_key.is_some(),
            search_configured: self.search_api_key.is_some(),
            web_search: self.web_search_mode().map(str::to_string),
            tts_provider: self.tts_provider.clone(),
            stt_provider: self.stt_provider.clone(),
            env_files: self.env_files.iter().map(|p| p.display().to_string()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(pairs: &[(&str, &str)]) -> AppConfig {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        AppConfig::from_map(|k| map.get(k).cloned())
    }

    #[test]
    fn empty_values_are_unset() {
        let c = cfg(&[("AI_API_KEY", "   "), ("AI_PROVIDER", "")]);
        assert!(c.ai_api_key.is_none());
        assert!(c.ai_provider.is_none());
    }

    #[test]
    fn public_config_never_contains_secrets() {
        let c = cfg(&[("AI_API_KEY", "sk-secret-value"), ("SEARCH_API_KEY", "search-secret"), ("VOICE_API_KEY", "voice-secret"), ("AI_PROVIDER", "Anthropic")]);
        let public = serde_json::to_string(&c.public()).unwrap();
        assert!(!public.contains("sk-secret-value"));
        assert!(!public.contains("search-secret"));
        assert!(!public.contains("voice-secret"));
        assert!(!format!("{c:?}").contains("voice-secret"));
        assert!(public.contains("\"aiKeyConfigured\":true"));
        assert!(public.contains("\"aiProvider\":\"anthropic\""));
    }

    #[test]
    fn web_search_mode_selection() {
        assert_eq!(cfg(&[("SEARCH_PROVIDER", "Tavily"), ("SEARCH_API_KEY", "k")]).web_search_mode(), Some("tavily"));
        assert_eq!(cfg(&[("SEARCH_API_KEY", "k")]).web_search_mode(), Some("brave"));
        assert_eq!(cfg(&[("AI_PROVIDER", "anthropic"), ("AI_API_KEY", "k")]).web_search_mode(), Some("anthropic"));
        assert_eq!(cfg(&[("AI_PROVIDER", "openai"), ("AI_API_KEY", "k")]).web_search_mode(), None);
        assert_eq!(cfg(&[("SEARCH_PROVIDER", "bing"), ("SEARCH_API_KEY", "k")]).web_search_mode(), None);
    }

    #[test]
    fn debug_output_redacts_secrets() {
        let c = cfg(&[("AI_API_KEY", "sk-secret-value")]);
        let dbg = format!("{c:?}");
        assert!(!dbg.contains("sk-secret-value"));
        assert!(dbg.contains("<redacted>"));
    }

    #[test]
    fn loads_env_file_from_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "AI_MODEL=test-model-from-file\n").unwrap();
        let c = AppConfig::load(Some(dir.path()));
        // A real process env var would override; in tests AI_MODEL is normally unset.
        if std::env::var("AI_MODEL").is_err() {
            assert_eq!(c.ai_model.as_deref(), Some("test-model-from-file"));
        }
        assert!(c.env_files.iter().any(|p| p.starts_with(dir.path())));
    }
}
