use tauri::State;

use crate::error::AppResult;
use crate::memory::{self, AddOutcome, Memory, MemoryKind, MemorySource};
use crate::state::AppState;

#[tauri::command]
pub fn list_memories(state: State<'_, AppState>, kind: Option<MemoryKind>, query: Option<String>) -> AppResult<Vec<Memory>> {
    memory::list(&*state.db.conn()?, kind, query.as_deref())
}

/// Add a memory from the Memory page. Duplicates return the existing memory.
#[tauri::command]
pub fn add_memory(state: State<'_, AppState>, kind: MemoryKind, content: String) -> AppResult<Memory> {
    match memory::add(&*state.db.conn()?, kind, &content, MemorySource::User, None)? {
        AddOutcome::Added(m) | AddOutcome::Duplicate(m) => Ok(m),
    }
}

#[tauri::command]
pub fn update_memory(state: State<'_, AppState>, id: i64, content: String, kind: Option<MemoryKind>) -> AppResult<Memory> {
    memory::update(&*state.db.conn()?, id, &content, kind)
}

#[tauri::command]
pub fn delete_memory(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    memory::delete(&*state.db.conn()?, id).map(|_| ())
}
