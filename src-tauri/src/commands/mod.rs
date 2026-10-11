//! IPC command handlers exposed to the UI.
//!
//! Handlers are deliberately thin: they validate input, delegate to domain
//! modules and map errors. Only commands registered in `lib.rs` are callable.

pub mod ai;
pub mod app;
pub mod attachments;
pub mod briefing;
pub mod chat;
pub mod devices;
pub mod google;
pub mod home;
pub mod life;
pub mod media;
pub mod memory;
pub mod operator;
pub mod phone;
pub mod productivity;
pub mod routines;
pub mod settings;
pub mod system;
pub mod tidy;
pub mod tools;
pub mod voice;
pub mod widgets;
pub mod workspace;
