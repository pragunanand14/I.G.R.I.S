//! `tidy_downloads`: sort the Downloads folder into folders by kind, preview
//! first, undo the last tidy. Desktop only.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::db::Database;
use crate::files::tidy;

pub struct TidyDownloadsTool {
    spec: ToolSpec,
    db: Arc<Database>,
    dir: Option<PathBuf>,
}

impl TidyDownloadsTool {
    pub fn new(db: Arc<Database>, dir: Option<PathBuf>) -> Self {
        Self {
            spec: ToolSpec {
                name: "tidy_downloads",
                title: "Tidy Downloads",
                description: "Sort the Downloads folder into Documents, Images, Videos, Music, Archives and Installers folders \
(only loose files; nothing is renamed or overwritten; downloads in progress stay). action: \"preview\" says what would move, \
\"tidy\" moves them, \"undo\" puts the last tidy back.",
                input_schema: json!({
                    "type": "object",
                    "properties": { "action": { "type": "string", "enum": ["preview", "tidy", "undo"] } },
                    "required": ["action"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Sensitive,
            },
            db,
            dir,
        }
    }
}

fn out(content: String, summary: impl Into<String>) -> ToolResultT {
    Ok(ToolOutput { content, summary: summary.into(), sources: vec![], media: Vec::new() })
}

#[async_trait::async_trait]
impl Tool for TidyDownloadsTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn describe(&self, i: &Value) -> String {
        match i["action"].as_str() {
            Some("undo") => "Put the files from the last Downloads tidy back".into(),
            Some("preview") => "Check what tidying Downloads would move".into(),
            _ => match self.dir.as_deref().and_then(|d| tidy::plan(d, SystemTime::now()).ok()) {
                Some(m) if !m.is_empty() => format!("Tidy Downloads — move {}", tidy::describe(&m)),
                _ => "Tidy the Downloads folder".into(),
            },
        }
    }
    async fn execute(&self, i: &Value) -> ToolResultT {
        let dir = self.dir.clone().ok_or_else(|| ToolError::refused("There's no Downloads folder on this device."))?;
        let conn = self.db.conn().map_err(|e| ToolError::failed(e.to_string()))?;
        match i["action"].as_str().unwrap_or("preview") {
            "undo" => {
                let n = tidy::undo_last(&conn).map_err(|e| ToolError::failed(e.to_string()))?;
                out(format!("Put {n} file(s) back in Downloads."), format!("{n} back"))
            }
            "tidy" => {
                let (done, errors) = tidy::run(&conn, &dir).map_err(|e| ToolError::failed(e.to_string()))?;
                if done.is_empty() && errors.is_empty() {
                    return out("Downloads is already tidy.".into(), "Already tidy");
                }
                let mut s = format!("Moved {}. (Undo with action \"undo\".)", tidy::describe(&done));
                if !errors.is_empty() {
                    s.push_str(&format!(" Couldn't move {} (probably open): {}", errors.len(), errors.join("; ")));
                }
                out(s, format!("Tidied {} file(s)", done.len()))
            }
            _ => {
                let moves = tidy::plan(&dir, SystemTime::now()).map_err(|e| ToolError::failed(e.to_string()))?;
                if moves.is_empty() {
                    return out("Nothing to move — Downloads is tidy.".into(), "Already tidy");
                }
                let names: Vec<String> = moves
                    .iter()
                    .take(15)
                    .map(|m| {
                        format!(
                            "{} → {}",
                            m.from.file_name().unwrap_or_default().to_string_lossy(),
                            m.to.parent().and_then(|p| p.file_name()).unwrap_or_default().to_string_lossy()
                        )
                    })
                    .collect();
                out(format!("Would move {}:\n{}", tidy::describe(&moves), names.join("\n")), format!("{} to move", moves.len()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn previews_tidies_and_undoes() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.pdf"), b"x").unwrap();
        let old = SystemTime::now() - tidy::SETTLE * 2;
        std::fs::File::options().write(true).open(d.path().join("a.pdf")).unwrap().set_modified(old).unwrap();
        let db = Arc::new(Database::open_in_memory().unwrap());
        let t = TidyDownloadsTool::new(db, Some(d.path().to_path_buf()));
        assert_eq!(t.spec().permission, PermissionLevel::Sensitive);
        assert!(t.describe(&json!({"action": "tidy"})).contains("1 file: 1 Documents"));
        assert!(t.execute(&json!({"action": "preview"})).await.unwrap().content.contains("a.pdf → Documents"));
        assert!(d.path().join("a.pdf").is_file(), "preview moves nothing");
        t.execute(&json!({"action": "tidy"})).await.unwrap();
        assert!(d.path().join("Documents/a.pdf").is_file());
        assert!(t.execute(&json!({"action": "undo"})).await.unwrap().content.contains("Put 1 file(s) back"));
        assert!(d.path().join("a.pdf").is_file());
        assert!(TidyDownloadsTool::new(Arc::new(Database::open_in_memory().unwrap()), None).execute(&json!({"action": "tidy"})).await.is_err());
    }
}
