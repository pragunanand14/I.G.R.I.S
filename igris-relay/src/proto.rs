//! Relay wire protocol (JSON text frames over WebSocket).
//!
//! The relay authenticates each device by a signature over a fresh challenge
//! (no passwords, no bearer tokens), then routes opaque end-to-end encrypted
//! envelopes between devices that have *both* listed each other as peers.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PROTOCOL: &str = "igris-relay/1";
/// Largest frame accepted from a client.
pub const MAX_FRAME: usize = 256 * 1024;
pub const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientFrame {
    /// Answer to the challenge: who I am and proof that I hold the key.
    Auth {
        device: String,
        key: String,
        ts: i64,
        sig: String,
    },
    /// Devices I accept messages from and send to (my trusted devices).
    Peers {
        ids: Vec<String>,
    },
    /// Route an envelope. `id` is the envelope's id (for the `sent` receipt).
    Send {
        id: String,
        to: String,
        env: serde_json::Value,
    },
    /// Open a pairing rendezvous for a code shown on this device.
    PairOpen {
        pid: String,
    },
    PairClose {
        pid: String,
    },
    /// From the device typing the code: deliver to whoever opened `pid`.
    PairSend {
        pid: String,
        blob: String,
    },
    /// From the inviter: answer the device that sent the request.
    PairReply {
        pid: String,
        to: String,
        blob: String,
    },
    Ping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendStatus {
    /// Written to the target's connection.
    Delivered,
    /// Target offline; held (bounded, short expiry).
    Queued,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ServerFrame {
    Challenge {
        nonce: String,
        protocol: String,
    },
    /// Authenticated until `expires_at` (Unix ms); then re-authenticate.
    Ready {
        expires_at: i64,
    },
    Deliver {
        from: String,
        env: serde_json::Value,
    },
    Sent {
        id: String,
        status: SendStatus,
        reason: Option<String>,
    },
    Presence {
        device: String,
        online: bool,
    },
    PairMsg {
        pid: String,
        from: String,
        blob: String,
    },
    PairStatus {
        pid: String,
        ok: bool,
        reason: Option<String>,
    },
    Pong,
    Error {
        reason: String,
    },
}

/// Bytes a device signs to authenticate.
pub fn auth_bytes(nonce: &str, device: &str, ts: i64) -> Vec<u8> {
    let mut b = b"igris-relay-auth-v1".to_vec();
    for p in [nonce.as_bytes(), device.as_bytes(), &ts.to_be_bytes()] {
        b.extend_from_slice(&(p.len() as u32).to_be_bytes());
        b.extend_from_slice(p);
    }
    b
}

/// Device ids are derived from the signing key (same rule as igris-core).
pub fn device_id_for(key: &[u8; 32]) -> String {
    let d = Sha256::digest(key);
    format!("dev_{}", d[..16].iter().map(|b| format!("{b:02x}")).collect::<String>())
}

pub fn valid_device_id(s: &str) -> bool {
    s.len() == 36 && s.starts_with("dev_") && s[4..].bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn valid_pid(s: &str) -> bool {
    s.len() == 10 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_tagged() {
        let f: ClientFrame = serde_json::from_str(r#"{"t":"ping"}"#).unwrap();
        assert_eq!(f, ClientFrame::Ping);
        assert!(serde_json::from_str::<ClientFrame>(r#"{"t":"exec","cmd":"ls"}"#).is_err());
        let s = serde_json::to_string(&ServerFrame::Sent { id: "x".into(), status: SendStatus::Queued, reason: None }).unwrap();
        assert!(s.contains(r#""status":"queued""#));
    }

    #[test]
    fn ids() {
        assert!(valid_device_id(&device_id_for(&[7u8; 32])));
        assert!(!valid_device_id("dev_XYZ"));
        assert!(valid_pid("0123456789") && !valid_pid("012345678g"));
    }
}
