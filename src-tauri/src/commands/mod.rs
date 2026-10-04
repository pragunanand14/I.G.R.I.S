//! IPC command handlers exposed to the UI.
//!
//! Handlers are deliberately thin: they validate input, delegate to domain
//! modules and map errors. Only commands registered in `lib.rs` are callable.

pub mod ai;
pub mod app;
pub mod chat;
pub mod settings;
pub mod system;
pub mod tools;
