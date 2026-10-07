//! Cross-device IGRIS: one user, several trusted devices (docs/CROSS_DEVICE.md).
//!
//! * [`identity`] — this device's cryptographic identity (keys sealed by the OS).
//! * [`registry`] — the user's other devices, trusted only after explicit pairing.
//! * [`pairing`] — the one-time-code pairing exchange.
//! * [`envelope`] — end-to-end encrypted, signed, replay-protected messages.
//! * [`approval`] — signed, single-use approvals for actions on another device.
//! * [`protocol`] — the typed messages devices exchange (inside envelopes).
//! * [`link`] — the outbound, authenticated connection to the relay.
//! * [`hub`] — routing: remote tasks through the existing orchestrator,
//!   control, approvals, presence, memory sync.
//! * [`remote`] — the record of tasks sent to / received from other devices.
//! * [`sync`] — memory sync (revisions and tombstones; nothing else is synced).
//! * [`target`] — "on my laptop", "here": choosing a device from words.
//!
//! The relay (crate `igris-relay`) only authenticates devices and routes
//! ciphertext. It never runs tasks, decides permissions or sees content.

pub mod approval;
pub mod capability;
pub mod envelope;
pub mod hub;
pub mod identity;
pub mod link;
pub mod pairing;
pub mod protocol;
pub mod registry;
pub mod remote;
pub mod sync;
pub mod target;

pub use capability::Capability;
pub use identity::{Identity, KeyProtector, Platform, PublicDevice, Unprotected};

#[cfg(test)]
mod e2e_tests;
