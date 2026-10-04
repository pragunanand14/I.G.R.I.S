//! IGRIS core: orchestration of a conversation turn.
//!
//! Current pipeline (Phase 2): receive input → load context → call the model →
//! stream → persist. Intent analysis, memory retrieval, planning, permission
//! checks and tool execution slot in here in later phases.

pub mod chat;
pub mod context;
pub mod prompt;
