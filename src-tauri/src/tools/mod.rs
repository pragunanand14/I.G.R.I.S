//! The controlled tool layer.
//!
//! Every action IGRIS takes goes through a [`Tool`] registered here. Each tool
//! declares a JSON input schema and a [`PermissionLevel`]; the
//! [`executor`] validates input, enforces the permission policy, runs the
//! tool with a timeout and writes an audit record. The model never gets
//! direct access to the OS — it can only ask for a registered tool by name.

pub mod apps;
pub mod audit;
pub mod calculator;
pub mod executor;
pub mod schema;
pub mod system_info;

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ai::ToolDef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    /// Read-only, no side effects. Never asks.
    Safe,
    /// Changes local state in a contained way (e.g. opening an allowlisted app).
    Low,
    /// Requires confirmation every time.
    Sensitive,
    /// Always requires explicit confirmation; can never be auto-approved.
    Critical,
}

impl PermissionLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionLevel::Safe => "safe",
            PermissionLevel::Low => "low",
            PermissionLevel::Sensitive => "sensitive",
            PermissionLevel::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub name: &'static str,
    /// Short human label for the UI.
    pub title: &'static str,
    /// Description sent to the model.
    pub description: &'static str,
    pub input_schema: Value,
    pub permission: PermissionLevel,
}

impl ToolSpec {
    pub fn to_def(&self) -> ToolDef {
        ToolDef { name: self.name.to_string(), description: self.description.to_string(), input_schema: self.input_schema.clone() }
    }
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// Returned to the model.
    pub content: String,
    /// One-line result for the UI and audit log.
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolErrorKind {
    InvalidInput,
    NotFound,
    Failed,
    Timeout,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ToolError {
    pub kind: ToolErrorKind,
    pub message: String,
}

impl ToolError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self { kind: ToolErrorKind::InvalidInput, message: msg.into() }
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self { kind: ToolErrorKind::NotFound, message: msg.into() }
    }
    pub fn failed(msg: impl Into<String>) -> Self {
        Self { kind: ToolErrorKind::Failed, message: msg.into() }
    }
}

pub type ToolResultT = Result<ToolOutput, ToolError>;

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> &ToolSpec;

    /// Human-readable description of what this call will do, shown in the UI
    /// and in confirmation prompts. Input has already passed schema validation.
    fn describe(&self, input: &Value) -> String;

    /// Tool-specific validation beyond the JSON schema.
    fn validate(&self, _input: &Value) -> Result<(), ToolError> {
        Ok(())
    }

    async fn execute(&self, input: &Value) -> ToolResultT;
}

#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: BTreeMap<&'static str, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.spec().name;
        assert!(schema::is_strict_compatible(&tool.spec().input_schema), "tool {name} schema must be strict-compatible");
        self.tools.insert(name, tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|t| t.spec().clone()).collect()
    }

    /// Definitions offered to the model, in a deterministic order (stable prompt prefix).
    pub fn defs(&self) -> Vec<ToolDef> {
        self.tools.values().map(|t| t.spec().to_def()).collect()
    }
}
