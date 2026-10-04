use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tokio_util::sync::CancellationToken;

use crate::ai::{AiProvider, AiRuntime, AiStatus};
use crate::config::AppConfig;
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::system::{ConnectivityMonitor, SystemMonitor};

/// Shared application state, managed by Tauri and injected into commands.
pub struct AppState {
    /// Reloadable via the `reload_config` command.
    pub config: RwLock<AppConfig>,
    pub ai: RwLock<AiRuntime>,
    pub db: Database,
    pub system: Mutex<SystemMonitor>,
    pub connectivity: ConnectivityMonitor,
    pub generations: Generations,
    pub paths: AppPaths,
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub log_dir: PathBuf,
    pub db_path: PathBuf,
}

impl AppState {
    pub fn ai_provider(&self) -> AppResult<(Arc<dyn AiProvider>, AiStatus)> {
        let rt = self.ai.read().map_err(|_| AppError::internal("AI runtime lock poisoned"))?;
        match &rt.provider {
            Some(p) => Ok((Arc::clone(p), rt.status.clone())),
            None => Err(AppError::AiUnavailable(rt.status.problem.clone().unwrap_or_else(|| "AI is not configured.".into()))),
        }
    }
}

/// In-flight generations, keyed by request id. At most one per conversation.
#[derive(Default)]
pub struct Generations {
    inner: Mutex<HashMap<String, (Option<String>, CancellationToken)>>,
}

impl Generations {
    /// Register a request. Fails if the id is in use or the conversation is busy.
    pub fn begin(&self, request_id: &str, conversation_id: Option<&str>) -> AppResult<GenerationGuard<'_>> {
        let mut map = self.inner.lock().map_err(|_| AppError::internal("generation registry poisoned"))?;
        if map.contains_key(request_id) {
            return Err(AppError::validation("Duplicate request id."));
        }
        if let Some(cid) = conversation_id {
            if map.values().any(|(c, _)| c.as_deref() == Some(cid)) {
                return Err(AppError::validation("IGRIS is already responding in this conversation."));
            }
        }
        let token = CancellationToken::new();
        map.insert(request_id.to_string(), (conversation_id.map(str::to_string), token.clone()));
        Ok(GenerationGuard { owner: self, request_id: request_id.to_string(), token })
    }

    pub fn cancel(&self, request_id: &str) -> bool {
        match self.inner.lock() {
            Ok(map) => map.get(request_id).map(|(_, t)| t.cancel()).is_some(),
            Err(_) => false,
        }
    }

    pub fn is_busy(&self, conversation_id: &str) -> bool {
        self.inner.lock().map(|m| m.values().any(|(c, _)| c.as_deref() == Some(conversation_id))).unwrap_or(false)
    }

    fn set_conversation(&self, request_id: &str, conversation_id: &str) {
        if let Ok(mut map) = self.inner.lock() {
            if let Some(entry) = map.get_mut(request_id) {
                entry.0 = Some(conversation_id.to_string());
            }
        }
    }
}

/// Removes the registry entry when the generation ends (including on error).
pub struct GenerationGuard<'a> {
    owner: &'a Generations,
    request_id: String,
    pub token: CancellationToken,
}

impl GenerationGuard<'_> {
    /// Bind a newly created conversation to this request.
    pub fn attach(&self, conversation_id: &str) {
        self.owner.set_conversation(&self.request_id, conversation_id);
    }
}

impl Drop for GenerationGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut map) = self.owner.inner.lock() {
            map.remove(&self.request_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_generation_per_conversation_and_cleanup_on_drop() {
        let g = Generations::default();
        let a = g.begin("r1", Some("c1")).unwrap();
        assert!(g.begin("r2", Some("c1")).is_err());
        assert!(g.begin("r1", None).is_err());
        assert!(g.is_busy("c1"));
        assert!(g.cancel("r1"));
        assert!(a.token.is_cancelled());
        drop(a);
        assert!(!g.is_busy("c1"));
        assert!(!g.cancel("r1"));
        assert!(g.begin("r2", Some("c1")).is_ok());
    }

    #[test]
    fn attach_binds_new_conversation() {
        let g = Generations::default();
        let a = g.begin("r1", None).unwrap();
        a.attach("c9");
        assert!(g.is_busy("c9"));
    }
}
