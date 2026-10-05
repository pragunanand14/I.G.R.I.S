//! IPC command handlers exposed to the UI.
//!
//! Handlers are deliberately thin: they validate input, delegate to domain
//! modules and map errors. Only commands registered in `lib.rs` are callable.

pub mod ai;
pub mod app;
pub mod attachments;
pub mod chat;
pub mod memory;
pub mod operator;
pub mod productivity;
pub mod settings;
pub mod system;
pub mod tools;
pub mod voice;
pub mod workspace;
