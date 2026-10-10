//! "Always allow": actions the user approved once and for all.
//!
//! When IGRIS asks before an action, the prompt can offer to remember it. A
//! rule is a tool plus a scope chosen by the tool (e.g. the app being closed,
//! the folder being written to; empty = the tool in general), so "always"
//! covers that kind of action and nothing broader. Critical actions and calls
//! a tool marks as always-confirm are never remembered (see the executor).
//! Rules live on this device only and can be removed in Settings.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// What "Always allow" would remember for one call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowRule {
    /// Narrows the rule within the tool; empty means the whole tool.
    pub scope: String,
    /// What's allowed, in plain words ("Close Spotify").
    pub label: String,
}

/// A remembered rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedRule {
    pub id: String,
    pub tool: String,
    pub scope: String,
    pub label: String,
    pub created_at: String,
}

const MAX_SCOPE_CHARS: usize = 1024;

/// Is this call covered by a remembered rule?
pub fn allows(conn: &Connection, tool: &str, scope: &str) -> bool {
    conn.query_row("SELECT 1 FROM permission_rules WHERE tool = ?1 AND scope = ?2", params![tool, scope], |_| Ok(())).optional().ok().flatten().is_some()
}

/// Remember a rule (again is fine: the existing one is kept).
pub fn add(conn: &Connection, tool: &str, rule: &AllowRule) -> AppResult<SavedRule> {
    if tool.is_empty() || rule.scope.chars().count() > MAX_SCOPE_CHARS {
        return Err(AppError::validation("That action can't be remembered."));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO permission_rules (id, tool, scope, label, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (tool, scope) DO NOTHING",
        params![id, tool, rule.scope, rule.label, now],
    )?;
    tracing::info!(event = "PERMISSION_RULE_ADDED", tool);
    Ok(conn.query_row("SELECT id, tool, scope, label, created_at FROM permission_rules WHERE tool = ?1 AND scope = ?2", params![tool, rule.scope], row)?)
}

pub fn list(conn: &Connection) -> AppResult<Vec<SavedRule>> {
    let mut stmt = conn.prepare("SELECT id, tool, scope, label, created_at FROM permission_rules ORDER BY created_at DESC")?;
    let rules = stmt.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rules)
}

/// Forget one rule; IGRIS asks again next time.
pub fn remove(conn: &Connection, id: &str) -> AppResult<()> {
    if conn.execute("DELETE FROM permission_rules WHERE id = ?1", params![id])? == 0 {
        return Err(AppError::validation("That permission was already removed."));
    }
    tracing::info!(event = "PERMISSION_RULE_REMOVED");
    Ok(())
}

/// Forget every rule.
pub fn clear(conn: &Connection) -> AppResult<usize> {
    let n = conn.execute("DELETE FROM permission_rules", [])?;
    tracing::info!(event = "PERMISSION_RULES_CLEARED", count = n);
    Ok(n)
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<SavedRule> {
    Ok(SavedRule { id: r.get(0)?, tool: r.get(1)?, scope: r.get(2)?, label: r.get(3)?, created_at: r.get(4)? })
}

/// A scope for actions on a file or folder: the folder it's in, so "always"
/// covers that folder (not its parent, not the whole disk).
pub fn folder_scope(path: &str) -> Option<String> {
    let p = std::path::Path::new(path.trim());
    let dir = p.parent().filter(|d| !d.as_os_str().is_empty())?;
    Some(dir.to_string_lossy().trim_end_matches(['/', '\\']).to_string()).filter(|d| !d.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn rule(scope: &str) -> AllowRule {
        AllowRule { scope: scope.into(), label: format!("Close {scope}") }
    }

    #[test]
    fn a_remembered_rule_covers_only_its_own_scope() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        assert!(!allows(&c, "close_application", "spotify"));
        let saved = add(&c, "close_application", &rule("spotify")).unwrap();
        assert!(allows(&c, "close_application", "spotify"));
        assert!(!allows(&c, "close_application", "chrome"));
        assert!(!allows(&c, "trash_path", "spotify"));
        // Remembering again keeps one rule.
        assert_eq!(add(&c, "close_application", &rule("spotify")).unwrap().id, saved.id);
        assert_eq!(list(&c).unwrap().len(), 1);
        remove(&c, &saved.id).unwrap();
        assert!(!allows(&c, "close_application", "spotify"));
        assert!(remove(&c, &saved.id).is_err());
    }

    #[test]
    fn clearing_forgets_everything() {
        let db = Database::open_in_memory().unwrap();
        let c = db.conn().unwrap();
        add(&c, "a", &rule("")).unwrap();
        add(&c, "b", &rule("x")).unwrap();
        assert_eq!(clear(&c).unwrap(), 2);
        assert!(list(&c).unwrap().is_empty());
    }

    #[test]
    fn folder_scopes() {
        assert_eq!(folder_scope(r"C:\Users\me\notes\todo.txt").filter(|_| cfg!(windows)), cfg!(windows).then(|| r"C:\Users\me\notes".to_string()));
        assert_eq!(folder_scope("/home/me/notes/todo.txt").as_deref(), Some("/home/me/notes"));
        assert_eq!(folder_scope("todo.txt"), None);
        assert_eq!(folder_scope("/todo.txt"), None);
    }
}
