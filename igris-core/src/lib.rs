//! The IGRIS core: the platform-independent part of IGRIS.
//!
//! Everything here is shared by every IGRIS app (today the Windows desktop
//! app; later others): AI providers and model routing, context budgeting and
//! compaction, conversations, memory, the orchestrator and its tasks, the
//! operator's task engine, the tool framework (registry, permissions,
//! approvals, audit) with the platform-neutral tools, productivity, projects,
//! settings, voice backends and persistence.
//!
//! Nothing in this crate depends on Tauri, a window system or an OS API. A
//! platform app supplies what is device-specific through narrow seams:
//! a [`computer::Driver`] for seeing and operating the screen, its own tools
//! registered into [`tools::ToolRegistry`], an [`tools::executor::Approver`]
//! for asking the user, and event callbacks for its UI.

pub mod ai;
pub mod attachments;
pub mod computer;
pub mod config;
pub mod conversations;
pub mod core;
pub mod db;
pub mod error;
pub mod files;
pub mod memory;
pub mod operator;
pub mod orchestrator;
pub mod productivity;
pub mod projects;
pub mod settings;
pub mod tools;
pub mod voice;
