//! IGRIS relay: the meeting point for one user's devices.
//!
//! Devices connect *out* to the relay (no device opens a port). The relay
//! authenticates each device by its key, routes end-to-end encrypted
//! envelopes between devices that trust each other, reports presence and
//! holds a few messages briefly for an offline device. It cannot read
//! messages, run tasks or approve anything. See docs/CROSS_DEVICE.md.

pub mod proto;
pub mod server;

pub use server::{Config, Relay};
