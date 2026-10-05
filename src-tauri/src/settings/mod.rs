//! User settings: typed model, validation, and persistence.
//!
//! Settings are stored as one JSON value per key so new settings can be added
//! without a migration. Unknown or invalid stored values fall back to defaults
//! (and are logged) instead of breaking startup.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::ai::Effort;
use crate::error::{AppError, AppResult};

pub const USER_NAME_MAX_CHARS: usize = 48;
pub const TELEMETRY_INTERVAL_MIN_MS: u32 = 1_000;
pub const TELEMETRY_INTERVAL_MAX_MS: u32 = 10_000;
pub const AI_MODEL_MAX_CHARS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    Azure,
    Violet,
    Emerald,
    Amber,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// How IGRIS addresses the user. Empty means "no name".
    pub user_name: String,
    pub theme: Theme,
    pub accent: Accent,
    pub reduced_motion: bool,
    /// How often the UI refreshes system telemetry.
    pub telemetry_interval_ms: u32,
    /// Model override. Empty means "use AI_MODEL or the provider default".
    pub ai_model: String,
    /// Reasoning effort for providers that support it.
    pub ai_effort: Effort,
    /// Ask before LOW-risk tool actions (SENSITIVE/CRITICAL always ask).
    pub confirm_low_risk: bool,
    /// Use and update persistent memory. When off, nothing is attached or saved.
    pub memory_enabled: bool,
    /// Speak replies aloud when the request was spoken.
    pub voice_auto_speak: bool,
    /// Experimental "IGRIS" wake word (needs browser speech recognition).
    pub wake_word_enabled: bool,
    /// Preferred browser voice name; empty = system default.
    pub tts_voice: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            user_name: String::new(),
            theme: Theme::Dark,
            accent: Accent::Azure,
            reduced_motion: false,
            telemetry_interval_ms: 2_000,
            ai_model: String::new(),
            ai_effort: Effort::Medium,
            confirm_low_risk: false,
            memory_enabled: true,
            voice_auto_speak: true,
            wake_word_enabled: false,
            tts_voice: String::new(),
        }
    }
}

/// A partial update. Unknown fields are rejected so typos surface as errors.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    pub user_name: Option<String>,
    pub theme: Option<Theme>,
    pub accent: Option<Accent>,
    pub reduced_motion: Option<bool>,
    pub telemetry_interval_ms: Option<u32>,
    pub ai_model: Option<String>,
    pub ai_effort: Option<Effort>,
    pub confirm_low_risk: Option<bool>,
    pub memory_enabled: Option<bool>,
    pub voice_auto_speak: Option<bool>,
    pub wake_word_enabled: Option<bool>,
    pub tts_voice: Option<String>,
}

impl SettingsPatch {
    /// Validate and normalise the patch.
    pub fn validate(mut self) -> AppResult<Self> {
        if let Some(name) = self.user_name.take() {
            let name = name.trim().to_string();
            if name.chars().count() > USER_NAME_MAX_CHARS {
                return Err(AppError::validation(format!(
                    "Name must be at most {USER_NAME_MAX_CHARS} characters."
                )));
            }
            if name.chars().any(char::is_control) {
                return Err(AppError::validation("Name cannot contain control characters."));
            }
            self.user_name = Some(name);
        }
        if let Some(ms) = self.telemetry_interval_ms {
            if !(TELEMETRY_INTERVAL_MIN_MS..=TELEMETRY_INTERVAL_MAX_MS).contains(&ms) {
                return Err(AppError::validation(format!(
                    "Telemetry interval must be between {TELEMETRY_INTERVAL_MIN_MS} and {TELEMETRY_INTERVAL_MAX_MS} ms."
                )));
            }
        }
        if let Some(model) = self.ai_model.take() {
            let model = model.trim().to_string();
            if model.chars().count() > AI_MODEL_MAX_CHARS {
                return Err(AppError::validation(format!("Model id must be at most {AI_MODEL_MAX_CHARS} characters.")));
            }
            if !model.chars().all(|c| c.is_ascii_alphanumeric() || "._:/@-".contains(c)) {
                return Err(AppError::validation("Model id may only contain letters, digits and . _ : / @ -"));
            }
            self.ai_model = Some(model);
        }
        if let Some(v) = self.tts_voice.take() {
            let v = v.trim().to_string();
            if v.chars().count() > 200 || v.chars().any(char::is_control) {
                return Err(AppError::validation("Invalid voice name."));
            }
            self.tts_voice = Some(v);
        }
        Ok(self)
    }

    /// Field names present in this patch (for audit logging without values).
    pub fn changed_fields(&self) -> Vec<&'static str> {
        let mut f = Vec::new();
        if self.user_name.is_some() { f.push("userName"); }
        if self.theme.is_some() { f.push("theme"); }
        if self.accent.is_some() { f.push("accent"); }
        if self.reduced_motion.is_some() { f.push("reducedMotion"); }
        if self.telemetry_interval_ms.is_some() { f.push("telemetryIntervalMs"); }
        if self.ai_model.is_some() { f.push("aiModel"); }
        if self.ai_effort.is_some() { f.push("aiEffort"); }
        if self.confirm_low_risk.is_some() { f.push("confirmLowRisk"); }
        if self.memory_enabled.is_some() { f.push("memoryEnabled"); }
        if self.voice_auto_speak.is_some() { f.push("voiceAutoSpeak"); }
        if self.wake_word_enabled.is_some() { f.push("wakeWordEnabled"); }
        if self.tts_voice.is_some() { f.push("ttsVoice"); }
        f
    }
}

impl Settings {
    pub fn apply(&mut self, patch: SettingsPatch) {
        if let Some(v) = patch.user_name { self.user_name = v; }
        if let Some(v) = patch.theme { self.theme = v; }
        if let Some(v) = patch.accent { self.accent = v; }
        if let Some(v) = patch.reduced_motion { self.reduced_motion = v; }
        if let Some(v) = patch.telemetry_interval_ms { self.telemetry_interval_ms = v; }
        if let Some(v) = patch.ai_model { self.ai_model = v; }
        if let Some(v) = patch.ai_effort { self.ai_effort = v; }
        if let Some(v) = patch.confirm_low_risk { self.confirm_low_risk = v; }
        if let Some(v) = patch.memory_enabled { self.memory_enabled = v; }
        if let Some(v) = patch.voice_auto_speak { self.voice_auto_speak = v; }
        if let Some(v) = patch.wake_word_enabled { self.wake_word_enabled = v; }
        if let Some(v) = patch.tts_voice { self.tts_voice = v; }
    }
}

/// Load settings, layering stored values over defaults.
pub fn load(conn: &Connection) -> AppResult<Settings> {
    let defaults = serde_json::to_value(Settings::default())?;
    let mut merged = defaults.as_object().cloned().unwrap_or_default();

    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (key, raw) = row?;
        if !merged.contains_key(&key) {
            continue; // obsolete key — ignore
        }
        match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(v) => { merged.insert(key, v); }
            Err(_) => tracing::warn!(event = "SETTINGS_VALUE_CORRUPT", key = %key),
        }
    }

    let candidate = serde_json::Value::Object(merged);
    match serde_json::from_value::<Settings>(candidate) {
        Ok(s) => Ok(s),
        Err(err) => {
            // A stored value has the wrong type; fall back per-field.
            tracing::warn!(event = "SETTINGS_INVALID_FALLBACK", error = %err);
            load_per_field(conn)
        }
    }
}

/// Slow path: apply each stored value individually, skipping invalid ones.
fn load_per_field(conn: &Connection) -> AppResult<Settings> {
    let mut settings = Settings::default();
    let keys = ["userName", "theme", "accent", "reducedMotion", "telemetryIntervalMs", "aiModel", "aiEffort", "confirmLowRisk", "memoryEnabled", "voiceAutoSpeak", "wakeWordEnabled", "ttsVoice"];
    for key in keys {
        let raw: Option<String> = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()?;
        let Some(raw) = raw else { continue };
        let patch_json = format!("{{\"{key}\":{raw}}}");
        match serde_json::from_str::<SettingsPatch>(&patch_json).map_err(AppError::from).and_then(|p| p.validate()) {
            Ok(patch) => settings.apply(patch),
            Err(_) => tracing::warn!(event = "SETTINGS_VALUE_SKIPPED", key = %key),
        }
    }
    Ok(settings)
}

/// Validate, apply and persist a patch atomically. Returns the new settings.
pub fn update(conn: &mut Connection, patch: SettingsPatch) -> AppResult<Settings> {
    let patch = patch.validate()?;
    let fields = patch.changed_fields();
    let mut settings = load(conn)?;
    settings.apply(patch);

    let value = serde_json::to_value(&settings)?;
    let obj = value.as_object().ok_or_else(|| AppError::internal("settings not an object"))?;
    let tx = conn.transaction()?;
    for key in &fields {
        let v = obj.get(*key).ok_or_else(|| AppError::internal(format!("missing field {key}")))?;
        tx.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, v.to_string()],
        )?;
    }
    tx.commit()?;
    tracing::info!(event = "SETTINGS_UPDATED", fields = ?fields);
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn patch(json: &str) -> AppResult<SettingsPatch> {
        Ok(serde_json::from_str::<SettingsPatch>(json)?)
    }

    #[test]
    fn defaults_when_empty() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(load(&db.conn().unwrap()).unwrap(), Settings::default());
    }

    #[test]
    fn update_persists_and_merges() {
        let db = Database::open_in_memory().unwrap();
        let mut conn = db.conn().unwrap();
        update(&mut conn, patch(r#"{"userName":"  Ada  ","accent":"violet"}"#).unwrap()).unwrap();
        update(&mut conn, patch(r#"{"reducedMotion":true}"#).unwrap()).unwrap();
        let s = load(&conn).unwrap();
        assert_eq!(s.user_name, "Ada");
        assert_eq!(s.accent, Accent::Violet);
        assert!(s.reduced_motion);
        assert_eq!(s.theme, Theme::Dark);
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(patch(r#"{"isAdmin":true}"#).is_err());
    }

    #[test]
    fn rejects_invalid_enum() {
        assert!(patch(r#"{"theme":"neon"}"#).is_err());
    }

    #[test]
    fn rejects_out_of_range_interval() {
        assert!(patch(r#"{"telemetryIntervalMs":10}"#).unwrap().validate().is_err());
        assert!(patch(r#"{"telemetryIntervalMs":60000}"#).unwrap().validate().is_err());
        assert!(patch(r#"{"telemetryIntervalMs":3000}"#).unwrap().validate().is_ok());
    }

    #[test]
    fn rejects_bad_names() {
        let long = format!(r#"{{"userName":"{}"}}"#, "a".repeat(USER_NAME_MAX_CHARS + 1));
        assert!(patch(&long).unwrap().validate().is_err());
        assert!(patch(r#"{"userName":"a\u0007b"}"#).unwrap().validate().is_err());
    }

    #[test]
    fn invalid_update_does_not_persist() {
        let db = Database::open_in_memory().unwrap();
        let mut conn = db.conn().unwrap();
        let bad = patch(r#"{"accent":"amber","telemetryIntervalMs":1}"#).unwrap();
        assert!(update(&mut conn, bad).is_err());
        assert_eq!(load(&conn).unwrap().accent, Accent::Azure);
    }

    #[test]
    fn validates_ai_model_and_effort() {
        assert!(patch(r#"{"aiModel":"claude-opus-5-5"}"#).unwrap().validate().is_ok());
        assert!(patch(r#"{"aiModel":"org/model:7b@q4"}"#).unwrap().validate().is_ok());
        assert!(patch(r#"{"aiModel":"bad model; rm -rf"}"#).unwrap().validate().is_err());
        assert!(patch(r#"{"aiEffort":"max"}"#).is_err());
        let db = Database::open_in_memory().unwrap();
        let mut conn = db.conn().unwrap();
        let s = update(&mut conn, patch(r#"{"aiEffort":"high","aiModel":" x "}"#).unwrap()).unwrap();
        assert_eq!(s.ai_effort, Effort::High);
        assert_eq!(s.ai_model, "x");
    }

    #[test]
    fn corrupt_stored_values_fall_back_to_defaults() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn().unwrap();
        conn.execute("INSERT INTO settings (key, value) VALUES ('theme', '\"neon\"')", []).unwrap();
        conn.execute("INSERT INTO settings (key, value) VALUES ('accent', 'not json')", []).unwrap();
        conn.execute("INSERT INTO settings (key, value) VALUES ('userName', '\"Kai\"')", []).unwrap();
        conn.execute("INSERT INTO settings (key, value) VALUES ('legacyKey', '1')", []).unwrap();
        let s = load(&conn).unwrap();
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.accent, Accent::Azure);
        assert_eq!(s.user_name, "Kai");
    }
}
