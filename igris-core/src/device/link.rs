//! The device's connection to the relay.
//!
//! Always outbound (no device listens on a port), always authenticated: the
//! relay sends a fresh challenge and the device answers with a signature from
//! its identity key. There is no password or bearer token to steal. The
//! connection reconnects with backoff and re-authenticates when the relay
//! ends the session. Only `wss://` relays are accepted, except `ws://` to this
//! same machine (development, tests, or a relay on loopback behind a proxy).

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use igris_relay::proto::{self, ClientFrame, ServerFrame};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_util::sync::CancellationToken;

use super::envelope::now_ms;
use crate::error::{AppError, AppResult};

pub const PING_EVERY: Duration = Duration::from_secs(25);
/// No frame at all for this long: the connection is dead.
pub const DEAD_AFTER: Duration = Duration::from_secs(70);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Check a relay URL: `wss://host[:port][/path]`, or `ws://` to loopback.
pub fn validate_url(url: &str) -> AppResult<String> {
    let url = url.trim();
    let bad = || AppError::validation("Enter the relay address, like wss://relay.example.com.");
    let (scheme, rest) = url.split_once("://").ok_or_else(bad)?;
    let host_port = rest.split('/').next().unwrap_or_default();
    if host_port.is_empty() || host_port.contains('@') || url.chars().any(|c| c.is_whitespace() || c.is_control()) || url.len() > 300 {
        return Err(bad());
    }
    let host = if let Some(h) = host_port.strip_prefix('[') {
        h.split(']').next().unwrap_or_default()
    } else {
        host_port.rsplit_once(':').map(|(h, _)| h).unwrap_or(host_port)
    };
    let loopback = host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
    match scheme.to_ascii_lowercase().as_str() {
        "wss" => Ok(url.to_string()),
        "ws" if loopback => Ok(url.to_string()),
        "ws" => Err(AppError::validation("Use a secure relay address (wss://). Unencrypted ws:// is only allowed to this same computer.")),
        _ => Err(bad()),
    }
}

/// Signs with the device key (the key itself stays with the identity).
pub type Signer = Arc<dyn Fn(&[u8]) -> [u8; 64] + Send + Sync>;

/// How this device proves who it is to the relay.
pub struct Credentials {
    pub device_id: String,
    pub signing_key_b64: String,
    pub sign: Signer,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkEvent {
    Connecting,
    /// Authenticated; frames flow.
    Up,
    Down {
        reason: String,
    },
    Frame(ServerFrame),
}

/// Run the link until `cancel`. Sends `peers` right after each authentication
/// (and whenever it changes), then frames from `out`.
pub async fn run(
    url: String,
    creds: Credentials,
    mut peers: watch::Receiver<Vec<String>>,
    mut out: mpsc::UnboundedReceiver<ClientFrame>,
    events: mpsc::UnboundedSender<LinkEvent>,
    cancel: CancellationToken,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        if cancel.is_cancelled() {
            return;
        }
        let _ = events.send(LinkEvent::Connecting);
        let started = std::time::Instant::now();
        let reason = tokio::select! {
            _ = cancel.cancelled() => return,
            r = session(&url, &creds, &mut peers, &mut out, &events, &cancel) => r,
        };
        let _ = events.send(LinkEvent::Down { reason: reason.clone() });
        tracing::info!(event = "DEVICE_LINK_DOWN", reason = %reason);
        // A session that lasted a while resets the backoff.
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        // Frames queued while down are stale (the hub re-sends what matters).
        while out.try_recv().is_ok() {}
        let jitter = Duration::from_millis(now_ms().rem_euclid(1000) as u64);
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(backoff + jitter) => {}
        }
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
}

async fn session(
    url: &str,
    creds: &Credentials,
    peers: &mut watch::Receiver<Vec<String>>,
    out: &mut mpsc::UnboundedReceiver<ClientFrame>,
    events: &mpsc::UnboundedSender<LinkEvent>,
    cancel: &CancellationToken,
) -> String {
    let cfg = WebSocketConfig::default().max_message_size(Some(proto::MAX_FRAME)).max_frame_size(Some(proto::MAX_FRAME));
    let connect = tokio_tungstenite::connect_async_with_config(url, Some(cfg), false);
    let ws = match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Err(_) => return "The relay didn't answer.".into(),
        Ok(Err(e)) => return format!("Couldn't reach the relay ({}).", short_err(&e.to_string())),
        Ok(Ok((ws, _))) => ws,
    };
    let (mut sink, mut source) = ws.split();
    let text = |f: &ClientFrame| WsMessage::text(serde_json::to_string(f).unwrap_or_default());

    // Challenge → signed answer → ready.
    let nonce = match tokio::time::timeout(Duration::from_secs(10), source.next()).await {
        Ok(Some(Ok(WsMessage::Text(t)))) => match serde_json::from_str::<ServerFrame>(&t) {
            Ok(ServerFrame::Challenge { nonce, protocol }) if protocol == proto::PROTOCOL => nonce,
            _ => return "That address isn't an IGRIS relay (or it's a different version).".into(),
        },
        _ => return "That address isn't an IGRIS relay.".into(),
    };
    let ts = now_ms();
    let sig = (creds.sign)(&proto::auth_bytes(&nonce, &creds.device_id, ts));
    let auth = ClientFrame::Auth { device: creds.device_id.clone(), key: creds.signing_key_b64.clone(), ts, sig: proto::B64.encode(sig) };
    if sink.send(text(&auth)).await.is_err() {
        return "Connection lost while signing in.".into();
    }
    match tokio::time::timeout(Duration::from_secs(10), source.next()).await {
        Ok(Some(Ok(WsMessage::Text(t)))) => match serde_json::from_str::<ServerFrame>(&t) {
            Ok(ServerFrame::Ready { .. }) => {}
            Ok(ServerFrame::Error { reason }) => return format!("The relay refused this device: {reason}."),
            _ => return "Unexpected answer from the relay.".into(),
        },
        _ => return "The relay didn't confirm sign-in.".into(),
    }
    let current = peers.borrow_and_update().clone();
    if sink.send(text(&ClientFrame::Peers { ids: current })).await.is_err() {
        return "Connection lost.".into();
    }
    let _ = events.send(LinkEvent::Up);
    tracing::info!(event = "DEVICE_LINK_UP");

    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    let mut last_rx = std::time::Instant::now();
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = sink.close().await;
                return "Disconnected.".into();
            }
            _ = ping.tick() => {
                if last_rx.elapsed() > DEAD_AFTER {
                    return "The relay stopped responding.".into();
                }
                if sink.send(text(&ClientFrame::Ping)).await.is_err() {
                    return "Connection lost.".into();
                }
            }
            changed = peers.changed() => {
                if changed.is_err() {
                    return "Disconnected.".into();
                }
                let ids = peers.borrow_and_update().clone();
                if sink.send(text(&ClientFrame::Peers { ids })).await.is_err() {
                    return "Connection lost.".into();
                }
            }
            f = out.recv() => {
                let Some(f) = f else { return "Disconnected.".into() };
                if sink.send(text(&f)).await.is_err() {
                    return "Connection lost.".into();
                }
            }
            m = source.next() => {
                last_rx = std::time::Instant::now();
                match m {
                    None => return "The relay closed the connection.".into(),
                    Some(Err(e)) => return format!("Connection lost ({}).", short_err(&e.to_string())),
                    Some(Ok(WsMessage::Text(t))) => match serde_json::from_str::<ServerFrame>(&t) {
                        Ok(ServerFrame::Error { reason }) => return format!("The relay ended the session: {reason}."),
                        Ok(ServerFrame::Pong) => {}
                        Ok(f) => { let _ = events.send(LinkEvent::Frame(f)); }
                        Err(_) => tracing::warn!(event = "DEVICE_LINK_BAD_FRAME"),
                    },
                    Some(Ok(WsMessage::Close(_))) => return "The relay closed the connection.".into(),
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

fn short_err(e: &str) -> String {
    crate::orchestrator::task::clip(e, 120)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_secure_or_loopback_relays() {
        assert!(validate_url("wss://relay.example.com").is_ok());
        assert!(validate_url("wss://relay.example.com:8443/igris").is_ok());
        assert!(validate_url("ws://127.0.0.1:8787").is_ok());
        assert!(validate_url("ws://localhost:8787").is_ok());
        assert!(validate_url("ws://[::1]:8787").is_ok());
        assert!(validate_url("ws://192.168.1.20:8787").is_err(), "same network is not trust");
        assert!(validate_url("ws://relay.example.com").is_err());
        assert!(validate_url("http://relay.example.com").is_err());
        assert!(validate_url("wss://user:pw@relay.example.com").is_err());
        assert!(validate_url("relay.example.com").is_err());
        assert!(validate_url("ws://localhost.evil.com").is_err());
    }
}
