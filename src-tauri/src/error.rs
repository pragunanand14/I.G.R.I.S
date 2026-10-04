//! Application-wide error type.
//!
//! Every error that crosses the IPC boundary is converted into a stable,
//! serializable shape (`{ kind, message }`) so the UI can render actionable
//! messages without parsing strings.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Validation(String),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The AI provider isn't configured or usable; the message says how to fix it.
    #[error("{0}")]
    AiUnavailable(String),

    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::Validation(_) => "validation",
            AppError::Database(_) => "database",
            AppError::Serialization(_) => "serialization",
            AppError::Io(_) => "io",
            AppError::AiUnavailable(_) => "ai_unavailable",
            AppError::Internal(_) => "internal",
        }
    }

    pub fn validation(msg: impl Into<String>) -> Self {
        AppError::Validation(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        AppError::Internal(msg.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_kind_and_message() {
        let err = AppError::validation("name too long");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "validation");
        assert_eq!(json["message"], "name too long");
    }
}
