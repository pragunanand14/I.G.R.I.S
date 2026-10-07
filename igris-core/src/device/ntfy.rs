//! The default way devices reach each other: ntfy (https://ntfy.sh), a free,
//! public publish/subscribe service. Nothing to install or host.
//!
//! It plays the same role as the relay and speaks the same internal frames
//! (`ClientFrame` in, `ServerFrame` out), so the hub doesn't care which one is
//! used. ntfy only ever carries what the relay would: envelopes that are
//! already end-to-end encrypted and signed, and pairing blobs encrypted under
//! the one-time code. It never sees a key or any content.
//!
//! * Each device listens on its own inbox topic, derived from the owner id (a
//!   secret shared only by the user's paired devices) and its device id, so
//!   outsiders can't guess it. Anything posted there that isn't a valid
//!   envelope from a trusted device is rejected by the hub (signature, replay).
//! * Pairing uses a topic derived from the code's pairing id.
//! * Presence is approximate: devices say hello when they connect and every
//!   45 minutes; a device not heard from for 100 minutes counts as offline.
//! * ntfy keeps messages for hours, so a device that was closed picks up what
//!   it missed when it reconnects (within the messages' own expiry).
//! * Bodies over ntfy's message size are split into parts and reassembled.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use igris_relay::proto::{ClientFrame, SendStatus, ServerFrame};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use super::identity::{hex, random_hex};
use super::link::LinkEvent;
use crate::error::{AppError, AppResult};

pub const DEFAULT_SERVER: &str = "https://ntfy.sh";
/// ntfy turns bodies over 4096 bytes into attachments; stay well below.
const MAX_BODY: usize = 3500;
const HELLO_EVERY: Duration = Duration::from_secs(45 * 60);
const OFFLINE_AFTER: Duration = Duration::from_secs(100 * 60);
/// No byte at all (ntfy sends keepalives every ~45 s) for this long: reconnect.
const STALL: Duration = Duration::from_secs(120);

/// Check a custom ntfy server address: `https://…`, or `http://` to this machine.
pub fn validate_server(url: &str) -> AppResult<String> {
    let url = url.trim().trim_end_matches('/');
    let bad = || AppError::validation("Enter the server address, like https://ntfy.sh.");
    let (scheme, rest) = url.split_once("://").ok_or_else(bad)?;
    let host_port = rest.split('/').next().unwrap_or_default();
    if host_port.is_empty() || host_port.contains('@') || url.chars().any(|c| c.is_whitespace() || c.is_control()) || url.len() > 300 {
        return Err(bad());
    }
    let host = host_port.rsplit_once(':').map(|(h, _)| h).unwrap_or(host_port);
    let loopback = host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
    match scheme.to_ascii_lowercase().as_str() {
        "https" => Ok(url.to_string()),
        "http" if loopback => Ok(url.to_string()),
        "http" => Err(AppError::validation("Use a secure address (https://).")),
        _ => Err(bad()),
    }
}

fn topic(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update((p.len() as u32).to_be_bytes());
        h.update(p.as_bytes());
    }
    format!("igris_{}", hex(&h.finalize()[..16]))
}

/// A device's inbox: only the user's own devices (who know the owner id) can compute it.
pub fn inbox(owner_id: &str, device_id: &str) -> String {
    topic(&["igris-ntfy-inbox-v1", owner_id, device_id])
}

/// The meeting point for a pairing code.
pub fn pair_topic(pid: &str) -> String {
    topic(&["igris-ntfy-pair-v1", pid])
}

/// Split a body into ntfy-sized parts.
fn split(body: String) -> Vec<String> {
    if body.len() <= MAX_BODY {
        return vec![body];
    }
    let chunk = MAX_BODY - 200;
    let mut parts = Vec::new();
    let mut rest = body.as_str();
    while !rest.is_empty() {
        let mut end = chunk.min(rest.len());
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        parts.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    let id = random_hex(8);
    let n = parts.len();
    parts.into_iter().enumerate().map(|(i, d)| json!({"k": "part", "m": id, "i": i, "n": n, "d": d}).to_string()).collect()
}

/// Reassembles split bodies (bounded).
#[derive(Default)]
struct Parts {
    open: HashMap<String, (Instant, Vec<Option<String>>)>,
}

impl Parts {
    fn add(&mut self, v: &Value) -> Option<String> {
        let (m, i, n, d) = (v["m"].as_str()?, v["i"].as_u64()? as usize, v["n"].as_u64()? as usize, v["d"].as_str()?);
        if n == 0 || n > 200 || i >= n {
            return None;
        }
        self.open.retain(|_, (t, _)| t.elapsed() < Duration::from_secs(600));
        if self.open.len() > 50 && !self.open.contains_key(m) {
            return None;
        }
        let entry = self.open.entry(m.to_string()).or_insert_with(|| (Instant::now(), vec![None; n]));
        if entry.1.len() != n {
            return None;
        }
        entry.1[i] = Some(d.to_string());
        if entry.1.iter().all(Option::is_some) {
            let (_, parts) = self.open.remove(m)?;
            return Some(parts.into_iter().map(Option::unwrap_or_default).collect());
        }
        None
    }
}

struct State {
    me: String,
    owner: String,
    peers: Vec<String>,
    heard: HashMap<String, Instant>,
    /// Pairing ids this device listens on (opened as inviter, or joining).
    pairs: HashSet<String>,
    /// Waiting for the subscription to include their topic: (pid, frame to send after).
    after_open: Vec<(String, Option<Value>)>,
}

/// Run the ntfy link until `cancel`, with the same contract as `link::run`.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    server: String,
    device_id: String,
    owner_id: String,
    mut peers: watch::Receiver<Vec<String>>,
    mut out: mpsc::UnboundedReceiver<ClientFrame>,
    events: mpsc::UnboundedSender<LinkEvent>,
    cancel: CancellationToken,
) {
    let http = match crate::net::client_builder().connect_timeout(Duration::from_secs(15)).build() {
        Ok(c) => c,
        Err(e) => {
            let _ = events.send(LinkEvent::Down { reason: format!("Couldn't start the connection ({e}).") });
            return;
        }
    };
    let mut st = State {
        me: device_id,
        owner: owner_id,
        peers: peers.borrow_and_update().clone(),
        heard: HashMap::new(),
        pairs: HashSet::new(),
        after_open: Vec::new(),
    };
    let mut parts = Parts::default();
    let mut since = "15m".to_string();
    let mut backoff = Duration::from_secs(1);
    let mut up = false;
    let mut last_hello = Instant::now() - HELLO_EVERY;
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    let _ = events.send(LinkEvent::Connecting);

    'connect: loop {
        if cancel.is_cancelled() {
            return;
        }
        let mut topics = vec![inbox(&st.owner, &st.me)];
        topics.extend(st.pairs.iter().map(|p| pair_topic(p)));
        topics.sort();
        let url = format!("{server}/{}/json?since={since}", topics.join(","));
        let resp = tokio::select! {
            _ = cancel.cancelled() => return,
            r = http.get(&url).send() => r,
        };
        let mut stream = match resp.and_then(|r| r.error_for_status()) {
            Ok(r) => r.bytes_stream(),
            Err(e) => {
                let reason = format!("Couldn't reach {} ({}).", host_of(&server), crate::orchestrator::task::clip(&e.to_string(), 120));
                if up {
                    up = false;
                }
                let _ = events.send(LinkEvent::Down { reason });
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        let mut buf: Vec<u8> = Vec::new();
        let mut last_rx = Instant::now();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tick.tick() => {
                    if last_rx.elapsed() > STALL {
                        let _ = events.send(LinkEvent::Down { reason: format!("{} stopped responding.", host_of(&server)) });
                        up = false;
                        continue 'connect;
                    }
                    if up && last_hello.elapsed() >= HELLO_EVERY {
                        last_hello = Instant::now();
                        for p in st.peers.clone() {
                            publish(&http, &server, &inbox(&st.owner, &p), json!({"k": "hi", "f": st.me}).to_string()).await.ok();
                        }
                    }
                    let gone: Vec<String> = st.heard.iter().filter(|(_, t)| t.elapsed() > OFFLINE_AFTER).map(|(d, _)| d.clone()).collect();
                    for d in gone {
                        st.heard.remove(&d);
                        let _ = events.send(LinkEvent::Frame(ServerFrame::Presence { device: d, online: false }));
                    }
                }
                changed = peers.changed() => {
                    if changed.is_err() { return; }
                    let new = peers.borrow_and_update().clone();
                    let added: Vec<String> = new.iter().filter(|p| !st.peers.contains(p)).cloned().collect();
                    for gone in st.peers.iter().filter(|p| !new.contains(p)) {
                        if st.heard.remove(gone).is_some() {
                            let _ = events.send(LinkEvent::Frame(ServerFrame::Presence { device: gone.clone(), online: false }));
                        }
                    }
                    st.peers = new;
                    if up {
                        for p in added {
                            publish(&http, &server, &inbox(&st.owner, &p), json!({"k": "hi", "f": st.me}).to_string()).await.ok();
                        }
                    }
                }
                f = out.recv() => {
                    let Some(f) = f else { return };
                    if handle_out(f, &mut st, &http, &server, &events).await {
                        // The topic set changed: resubscribe (keeping our place).
                        continue 'connect;
                    }
                }
                chunk = stream.next() => {
                    let chunk = match chunk {
                        Some(Ok(c)) => c,
                        Some(Err(e)) => {
                            let _ = events.send(LinkEvent::Down { reason: format!("Connection lost ({}).", crate::orchestrator::task::clip(&e.to_string(), 120)) });
                            up = false;
                            tokio::time::sleep(backoff).await;
                            backoff = (backoff * 2).min(Duration::from_secs(60));
                            continue 'connect;
                        }
                        None => {
                            up = false;
                            let _ = events.send(LinkEvent::Down { reason: format!("{} closed the connection.", host_of(&server)) });
                            tokio::time::sleep(backoff).await;
                            continue 'connect;
                        }
                    };
                    last_rx = Instant::now();
                    buf.extend_from_slice(&chunk);
                    if buf.len() > 1 << 20 {
                        buf.clear();
                    }
                    while let Some(nl) = buf.iter().position(|b| *b == b'\n') {
                        let line: Vec<u8> = buf.drain(..=nl).collect();
                        let Ok(ev) = serde_json::from_slice::<Value>(&line) else { continue };
                        match ev["event"].as_str() {
                            Some("open") => {
                                backoff = Duration::from_secs(1);
                                if !up {
                                    up = true;
                                    let _ = events.send(LinkEvent::Up);
                                    last_hello = Instant::now();
                                    for p in st.peers.clone() {
                                        publish(&http, &server, &inbox(&st.owner, &p), json!({"k": "hi", "f": st.me}).to_string()).await.ok();
                                    }
                                }
                                // Pairing topics now subscribed: finish what was waiting for that.
                                for (pid, then) in std::mem::take(&mut st.after_open) {
                                    match then {
                                        None => { let _ = events.send(LinkEvent::Frame(ServerFrame::PairStatus { pid, ok: true, reason: None })); }
                                        Some(body) => {
                                            let ok = publish(&http, &server, &pair_topic(&pid), body.to_string()).await;
                                            let _ = events.send(LinkEvent::Frame(ServerFrame::PairStatus { pid, ok: ok.is_ok(), reason: ok.err() }));
                                        }
                                    }
                                }
                            }
                            Some("message") => {
                                if let Some(id) = ev["id"].as_str() {
                                    since = id.to_string();
                                }
                                let Some(body) = ev["message"].as_str() else { continue };
                                let Ok(mut v) = serde_json::from_str::<Value>(body) else { continue };
                                if v["k"] == "part" {
                                    match parts.add(&v).and_then(|whole| serde_json::from_str::<Value>(&whole).ok()) {
                                        Some(whole) => v = whole,
                                        None => continue,
                                    }
                                }
                                incoming(v, &mut st, &http, &server, &events).await;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

fn host_of(server: &str) -> String {
    server.split("://").nth(1).unwrap_or(server).split('/').next().unwrap_or(server).to_string()
}

/// Publish a body to a topic (split when large). The error is for the user.
async fn publish(http: &reqwest::Client, server: &str, topic: &str, body: String) -> Result<(), String> {
    for part in split(body) {
        let r = http.post(format!("{server}/{topic}")).header("X-Firebase", "no").body(part).timeout(Duration::from_secs(20)).send().await;
        match r {
            Ok(r) if r.status().is_success() => {}
            Ok(r) if r.status().as_u16() == 429 => {
                return Err(format!("{} is limiting how many messages this device can send; try again later", host_of(server)))
            }
            Ok(r) => return Err(format!("{} answered {}", host_of(server), r.status())),
            Err(e) => return Err(format!("couldn't reach {} ({})", host_of(server), crate::orchestrator::task::clip(&e.to_string(), 100))),
        }
    }
    Ok(())
}

/// Returns true when the subscription must be renewed (pairing topics changed).
async fn handle_out(f: ClientFrame, st: &mut State, http: &reqwest::Client, server: &str, events: &mpsc::UnboundedSender<LinkEvent>) -> bool {
    match f {
        ClientFrame::Send { id, to, env } => {
            let r = publish(http, server, &inbox(&st.owner, &to), json!({"k": "e", "f": st.me, "to": to, "env": env}).to_string()).await;
            let (status, reason) = match r {
                Ok(()) => (SendStatus::Delivered, None),
                Err(e) => (SendStatus::Rejected, Some(e)),
            };
            let _ = events.send(LinkEvent::Frame(ServerFrame::Sent { id, status, reason }));
            false
        }
        ClientFrame::PairOpen { pid } => {
            st.pairs.insert(pid.clone());
            st.after_open.push((pid, None));
            true
        }
        ClientFrame::PairClose { pid } => st.pairs.remove(&pid),
        ClientFrame::PairSend { pid, blob } => {
            // Listen for the answer before asking.
            let body = json!({"k": "pj", "pid": pid, "f": st.me, "blob": blob});
            let renew = st.pairs.insert(pid.clone());
            if renew {
                st.after_open.push((pid, Some(body)));
            } else {
                let r = publish(http, server, &pair_topic(&pid), body.to_string()).await;
                let _ = events.send(LinkEvent::Frame(ServerFrame::PairStatus { pid, ok: r.is_ok(), reason: r.err() }));
            }
            renew
        }
        ClientFrame::PairReply { pid, to, blob } => {
            let r = publish(http, server, &pair_topic(&pid), json!({"k": "pa", "pid": pid, "f": st.me, "to": to, "blob": blob}).to_string()).await;
            let _ = events.send(LinkEvent::Frame(ServerFrame::PairStatus { pid, ok: r.is_ok(), reason: r.err() }));
            false
        }
        ClientFrame::Peers { .. } | ClientFrame::Auth { .. } | ClientFrame::Ping => false,
    }
}

async fn incoming(v: Value, st: &mut State, http: &reqwest::Client, server: &str, events: &mpsc::UnboundedSender<LinkEvent>) {
    let from = v["f"].as_str().unwrap_or_default().to_string();
    if from.is_empty() || from == st.me {
        return;
    }
    let seen = |st: &mut State, events: &mpsc::UnboundedSender<LinkEvent>| {
        if st.peers.contains(&from) && st.heard.insert(from.clone(), Instant::now()).is_none() {
            let _ = events.send(LinkEvent::Frame(ServerFrame::Presence { device: from.clone(), online: true }));
        }
    };
    match v["k"].as_str() {
        Some("e") if v["to"] == st.me.as_str() => {
            seen(st, events);
            let _ = events.send(LinkEvent::Frame(ServerFrame::Deliver { from: from.clone(), env: v["env"].clone() }));
        }
        Some("hi") => {
            let was = st.heard.contains_key(&from);
            seen(st, events);
            // Answer so the other side knows this device is here too.
            if st.peers.contains(&from) && !was {
                publish(http, server, &inbox(&st.owner, &from), json!({"k": "ho", "f": st.me}).to_string()).await.ok();
            }
        }
        Some("ho") => seen(st, events),
        Some("pj") | Some("pa") => {
            let pid = v["pid"].as_str().unwrap_or_default().to_string();
            if !st.pairs.contains(&pid) || (v["k"] == "pa" && v["to"] != st.me.as_str()) {
                return;
            }
            // An inviter only listens for requests, a joiner only for answers to itself.
            if let Some(blob) = v["blob"].as_str() {
                let _ = events.send(LinkEvent::Frame(ServerFrame::PairMsg { pid, from, blob: blob.to_string() }));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_are_unguessable_without_the_owner_id() {
        let a = inbox("own_aaaa", "dev_1");
        assert_ne!(a, inbox("own_bbbb", "dev_1"));
        assert_ne!(a, inbox("own_aaaa", "dev_2"));
        assert!(a.len() <= 64 && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        assert_ne!(pair_topic("0123456789"), pair_topic("0123456788"));
    }

    #[test]
    fn large_bodies_split_and_reassemble() {
        let body = json!({"k": "e", "env": "é".repeat(5000)}).to_string();
        let parts = split(body.clone());
        assert!(parts.len() > 1 && parts.iter().all(|p| p.len() <= 4096));
        let mut r = Parts::default();
        let mut out = None;
        for p in parts.iter().rev() {
            out = r.add(&serde_json::from_str(p).unwrap()).or(out);
        }
        assert_eq!(out.unwrap(), body);
        assert_eq!(split("small".into()), vec!["small".to_string()]);
    }

    #[test]
    fn server_addresses() {
        assert_eq!(validate_server("https://ntfy.sh/").unwrap(), "https://ntfy.sh");
        assert!(validate_server("http://127.0.0.1:2586").is_ok());
        assert!(validate_server("http://ntfy.example.com").is_err());
        assert!(validate_server("ntfy.sh").is_err());
    }
}
