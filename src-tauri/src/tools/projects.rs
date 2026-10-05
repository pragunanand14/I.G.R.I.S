use std::sync::Arc;

use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::db::Database;
use crate::projects;

fn all(db: &Database) -> Result<Vec<projects::Project>, ToolError> {
    projects::list(&*db.conn().map_err(|e| ToolError::failed(e.to_string()))?).map_err(|e| ToolError::failed(e.to_string()))
}

pub struct ListProjectsTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl ListProjectsTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "list_projects",
                title: "List projects",
                description: "List the user's registered projects (name, path, language, stack).",
                input_schema: json!({ "type": "object", "properties": {}, "required": [], "additionalProperties": false }),
                permission: PermissionLevel::Safe,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ListProjectsTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, _i: &Value) -> String {
        "List projects".into()
    }
    async fn execute(&self, _i: &Value) -> ToolResultT {
        let ps = all(&self.db)?;
        if ps.is_empty() {
            return Ok(ToolOutput {
                content: "No projects are registered. The user can add them on the Projects page.".into(),
                summary: "None".into(),
                sources: vec![],
                media: Vec::new(),
            });
        }
        let lines: Vec<String> = ps
            .iter()
            .map(|p| format!("{} — {} [{}{}]", p.name, p.path, p.language, if p.framework.is_empty() { String::new() } else { format!("; {}", p.framework) }))
            .collect();
        Ok(ToolOutput { content: lines.join("\n"), summary: format!("{} projects", ps.len()), sources: vec![], media: Vec::new() })
    }
}

pub struct ProjectContextTool {
    spec: ToolSpec,
    db: Arc<Database>,
}

impl ProjectContextTool {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            spec: ToolSpec {
                name: "get_project_context",
                title: "Load project",
                description: "Load a registered project's context: details, notes, git branch, top-level files and README. Use \
it when the user asks about one of their projects or wants to work on it.",
                input_schema: json!({
                    "type": "object",
                    "properties": { "name": { "type": "string", "minLength": 1, "maxLength": 80 } },
                    "required": ["name"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Safe,
            },
            db,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ProjectContextTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, input: &Value) -> String {
        format!("Load project {}", input["name"].as_str().unwrap_or_default())
    }
    async fn execute(&self, input: &Value) -> ToolResultT {
        let p = projects::resolve(&all(&self.db)?, input["name"].as_str().unwrap_or_default()).map_err(ToolError::not_found)?;
        let ctx = tokio::task::spawn_blocking({
            let p = p.clone();
            move || projects::context(&p)
        })
        .await
        .map_err(|e| ToolError::failed(e.to_string()))?;
        Ok(ToolOutput { content: ctx, summary: format!("Loaded {}", p.name), sources: vec![], media: Vec::new() })
    }
}
