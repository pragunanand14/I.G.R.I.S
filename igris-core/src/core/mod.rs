//! IGRIS core: orchestration of a conversation turn.
//!
//! Pipeline: receive input → recall memory → build provider-neutral history →
//! budget it for the routed model (compacting older messages when needed) →
//! call the model → run requested tools through the executor → stream →
//! persist.

pub mod budget;
pub mod chat;
pub mod compaction;
pub mod context;
pub mod prompt;
