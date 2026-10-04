//! Internet connectivity detection.
//!
//! A background thread periodically attempts TCP connections to a few
//! well-known anycast resolvers. This measures reachability, not just whether
//! a network adapter is up. Results are cached so UI polling stays cheap.

use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Connectivity {
    Unknown,
    Online,
    Offline,
}

impl Connectivity {
    fn to_u8(self) -> u8 {
        match self {
            Connectivity::Unknown => 0,
            Connectivity::Online => 1,
            Connectivity::Offline => 2,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            1 => Connectivity::Online,
            2 => Connectivity::Offline,
            _ => Connectivity::Unknown,
        }
    }
}

pub const DEFAULT_TARGETS: &[&str] = &["1.1.1.1:443", "8.8.8.8:443", "9.9.9.9:443"];

/// Returns `Online` if any target accepts a TCP connection within `timeout`.
pub fn probe(targets: &[SocketAddr], timeout: Duration) -> Connectivity {
    if targets.iter().any(|addr| TcpStream::connect_timeout(addr, timeout).is_ok()) {
        Connectivity::Online
    } else {
        Connectivity::Offline
    }
}

#[derive(Clone)]
pub struct ConnectivityMonitor {
    state: Arc<AtomicU8>,
}

impl ConnectivityMonitor {
    /// Start the background probe loop.
    pub fn start(interval: Duration) -> Self {
        let state = Arc::new(AtomicU8::new(Connectivity::Unknown.to_u8()));
        let shared = Arc::clone(&state);
        let targets: Vec<SocketAddr> = DEFAULT_TARGETS.iter().filter_map(|t| t.parse().ok()).collect();
        let spawned = std::thread::Builder::new().name("igris-connectivity".into()).spawn(move || {
            let mut last = Connectivity::Unknown;
            loop {
                let now = probe(&targets, Duration::from_secs(2));
                if now != last {
                    tracing::info!(event = "CONNECTIVITY_CHANGED", state = ?now);
                    last = now;
                }
                shared.store(now.to_u8(), Ordering::Relaxed);
                std::thread::sleep(interval);
            }
        });
        if let Err(err) = spawned {
            tracing::error!(event = "CONNECTIVITY_MONITOR_FAILED", error = %err);
        }
        Self { state }
    }

    pub fn current(&self) -> Connectivity {
        Connectivity::from_u8(self.state.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn online_when_a_target_accepts() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        assert_eq!(probe(&[addr], Duration::from_millis(500)), Connectivity::Online);
    }

    #[test]
    fn offline_when_no_target_accepts() {
        // Bind then drop to get a port that is (almost certainly) closed.
        let addr = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        assert_eq!(probe(&[addr], Duration::from_millis(200)), Connectivity::Offline);
        assert_eq!(probe(&[], Duration::from_millis(200)), Connectivity::Offline);
    }

    #[test]
    fn u8_roundtrip() {
        for c in [Connectivity::Unknown, Connectivity::Online, Connectivity::Offline] {
            assert_eq!(Connectivity::from_u8(c.to_u8()), c);
        }
    }
}
