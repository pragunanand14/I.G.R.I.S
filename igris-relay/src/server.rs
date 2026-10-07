//! The relay server.
//!
//! What it does: authenticate devices by key signature, keep one connection
//! per device, route envelopes between devices that list each other as
//! peers, tell peers who is online (never addresses), hold a few messages for
//! an offline device for a short time, and meet two devices for pairing.
//!
//! What it never does: decrypt anything (it has no keys), run tasks, decide
//! permissions, store conversations or memory, or persist anything to disk.
//! Everything is in memory and gone on restart.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use rand_core::{OsRng, RngCore};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::proto::{self, ClientFrame, SendStatus, ServerFrame, B64};

#[derive(Debug, Clone)]
pub struct Config {
    /// Authenticated sessions last this long, then the device re-authenticates.
    pub session_ttl: Duration,
    /// Messages held per offline device.
    pub queue_per_device: usize,
    /// How long a held message waits.
    pub queue_ttl: Duration,
    /// Time allowed to answer the challenge.
    pub auth_timeout: Duration,
    /// Connections idle (no frame, not even a ping) this long are closed.
    pub idle_timeout: Duration,
    /// Frames per connection per 10 seconds.
    pub rate_per_10s: u32,
    /// Pairing rendezvous lifetime and attempts.
    pub pair_ttl: Duration,
    pub pair_attempts: u32,
    /// Only these devices may connect (empty = any device that proves its key;
    /// routing still needs both sides to list each other).
    pub allow: HashSet<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            session_ttl: Duration::from_secs(12 * 3600),
            queue_per_device: 100,
            queue_ttl: Duration::from_secs(10 * 60),
            auth_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(90),
            rate_per_10s: 200,
            pair_ttl: Duration::from_secs(10 * 60),
            pair_attempts: 5,
            allow: HashSet::new(),
        }
    }
}

struct Conn {
    /// Distinguishes a replaced connection from the current one.
    serial: u64,
    tx: mpsc::UnboundedSender<ServerFrame>,
}

struct Held {
    from: String,
    env: serde_json::Value,
    until: Instant,
}

struct Pairing {
    inviter: String,
    until: Instant,
    attempts: u32,
    /// Devices that sent a request (the only ones the inviter may answer).
    joiners: HashSet<String>,
}

#[derive(Default)]
struct State {
    conns: HashMap<String, Conn>,
    peers: HashMap<String, HashSet<String>>,
    held: HashMap<String, VecDeque<Held>>,
    pairings: HashMap<String, Pairing>,
    serial: u64,
}

impl State {
    /// Both devices list each other.
    fn mutual(&self, a: &str, b: &str) -> bool {
        self.peers.get(a).is_some_and(|p| p.contains(b)) && self.peers.get(b).is_some_and(|p| p.contains(a))
    }

    fn send(&self, to: &str, f: ServerFrame) -> bool {
        self.conns.get(to).is_some_and(|c| c.tx.send(f).is_ok())
    }
}

pub struct Relay {
    cfg: Config,
    state: Mutex<State>,
}

impl Relay {
    pub fn new(cfg: Config) -> Arc<Self> {
        Arc::new(Self { cfg, state: Mutex::new(State::default()) })
    }

    /// Accept connections until the listener fails.
    pub async fn serve(self: Arc<Self>, listener: TcpListener) -> std::io::Result<()> {
        let sweeper = Arc::downgrade(&self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let Some(r) = sweeper.upgrade() else { break };
                r.sweep().await;
            }
        });
        loop {
            let (stream, addr) = listener.accept().await?;
            let me = self.clone();
            tokio::spawn(async move {
                if let Err(e) = me.handle(stream, addr).await {
                    tracing::debug!(event = "RELAY_CONN_ENDED", reason = %e);
                }
            });
        }
    }

    async fn sweep(&self) {
        let now = Instant::now();
        let mut s = self.state.lock().await;
        for q in s.held.values_mut() {
            q.retain(|h| h.until > now);
        }
        s.held.retain(|_, q| !q.is_empty());
        s.pairings.retain(|_, p| p.until > now && p.attempts < self.cfg.pair_attempts);
    }

    async fn handle(self: Arc<Self>, stream: TcpStream, _addr: SocketAddr) -> Result<(), String> {
        // Addresses are never logged or shared with other devices.
        let ws_cfg = WebSocketConfig::default().max_message_size(Some(proto::MAX_FRAME)).max_frame_size(Some(proto::MAX_FRAME));
        let ws = tokio_tungstenite::accept_async_with_config(stream, Some(ws_cfg)).await.map_err(|e| e.to_string())?;
        let (mut sink, mut source) = ws.split();

        // 1. Challenge → signed answer.
        let mut nonce = [0u8; 32];
        OsRng.fill_bytes(&mut nonce);
        let nonce = B64.encode(nonce);
        let hello = ServerFrame::Challenge { nonce: nonce.clone(), protocol: proto::PROTOCOL.into() };
        sink.send(WsMessage::text(serde_json::to_string(&hello).unwrap_or_default())).await.map_err(|e| e.to_string())?;
        let first = tokio::time::timeout(self.cfg.auth_timeout, source.next()).await.map_err(|_| "auth timeout".to_string())?;
        let device = match first {
            Some(Ok(WsMessage::Text(t))) => match serde_json::from_str::<ClientFrame>(&t) {
                Ok(ClientFrame::Auth { device, key, ts, sig }) => self.verify_auth(&nonce, &device, &key, ts, &sig),
                _ => Err("expected auth".into()),
            },
            _ => Err("expected auth".into()),
        };
        let device = match device {
            Ok(d) => d,
            Err(reason) => {
                let _ = sink.send(WsMessage::text(serde_json::to_string(&ServerFrame::Error { reason: reason.clone() }).unwrap_or_default())).await;
                let _ = sink.close().await;
                tracing::info!(event = "RELAY_AUTH_REFUSED", reason = %reason);
                return Err(reason);
            }
        };

        // 2. Register (a newer connection replaces an older one).
        let (tx, mut rx) = mpsc::unbounded_channel::<ServerFrame>();
        let expires = Instant::now() + self.cfg.session_ttl;
        let serial = {
            let mut s = self.state.lock().await;
            s.serial += 1;
            let serial = s.serial;
            if let Some(old) = s.conns.insert(device.clone(), Conn { serial, tx: tx.clone() }) {
                let _ = old.tx.send(ServerFrame::Error { reason: "replaced by a newer connection".into() });
            }
            serial
        };
        let expires_at = now_ms() + self.cfg.session_ttl.as_millis() as i64;
        let _ = tx.send(ServerFrame::Ready { expires_at });
        tracing::info!(event = "RELAY_DEVICE_CONNECTED", device = %short(&device));

        // Writer task.
        let writer = tokio::spawn(async move {
            while let Some(f) = rx.recv().await {
                let closing = matches!(f, ServerFrame::Error { .. });
                if sink.send(WsMessage::text(serde_json::to_string(&f).unwrap_or_default())).await.is_err() {
                    break;
                }
                if closing {
                    let _ = sink.close().await;
                    break;
                }
            }
        });

        // 3. Frames until close, idle, session expiry or replacement.
        let mut window = (Instant::now(), 0u32);
        let result = loop {
            let next = tokio::select! {
                n = tokio::time::timeout(self.cfg.idle_timeout, source.next()) => n,
                _ = tokio::time::sleep_until(expires.into()) => {
                    let _ = tx.send(ServerFrame::Error { reason: "session expired; authenticate again".into() });
                    break Ok(());
                }
            };
            let msg = match next {
                Err(_) => break Err("idle".to_string()),
                Ok(None) => break Ok(()),
                Ok(Some(Err(e))) => break Err(e.to_string()),
                Ok(Some(Ok(m))) => m,
            };
            let text = match msg {
                WsMessage::Text(t) => t,
                WsMessage::Close(_) => break Ok(()),
                WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
                _ => {
                    let _ = tx.send(ServerFrame::Error { reason: "text frames only".into() });
                    break Err("binary".into());
                }
            };
            if window.0.elapsed() > Duration::from_secs(10) {
                window = (Instant::now(), 0);
            }
            window.1 += 1;
            if window.1 > self.cfg.rate_per_10s {
                let _ = tx.send(ServerFrame::Error { reason: "too many messages".into() });
                break Err("rate".into());
            }
            match serde_json::from_str::<ClientFrame>(&text) {
                Ok(f) => {
                    if !self.frame(&device, serial, f, &tx).await {
                        break Ok(());
                    }
                }
                Err(_) => {
                    let _ = tx.send(ServerFrame::Error { reason: "malformed frame".into() });
                    break Err("malformed".into());
                }
            }
        };

        // 4. Unregister (only if still the current connection) and tell peers.
        {
            let mut s = self.state.lock().await;
            if s.conns.get(&device).is_some_and(|c| c.serial == serial) {
                s.conns.remove(&device);
                s.pairings.retain(|_, p| p.inviter != device);
                let watchers: Vec<String> = s.peers.get(&device).map(|p| p.iter().cloned().collect()).unwrap_or_default();
                for w in watchers {
                    if s.mutual(&device, &w) {
                        s.send(&w, ServerFrame::Presence { device: device.clone(), online: false });
                    }
                }
            }
        }
        drop(tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
        tracing::info!(event = "RELAY_DEVICE_DISCONNECTED", device = %short(&device));
        result
    }

    fn verify_auth(&self, nonce: &str, device: &str, key: &str, ts: i64, sig: &str) -> Result<String, String> {
        let key: [u8; 32] = B64.decode(key).ok().and_then(|k| k.try_into().ok()).ok_or("bad key")?;
        if proto::device_id_for(&key) != device {
            return Err("device id doesn't match key".into());
        }
        if (now_ms() - ts).abs() > 5 * 60 * 1000 {
            return Err("clock too far off".into());
        }
        let sig: [u8; 64] = B64.decode(sig).ok().and_then(|s| s.try_into().ok()).ok_or("bad signature")?;
        let vk = VerifyingKey::from_bytes(&key).map_err(|_| "bad key")?;
        vk.verify_strict(&proto::auth_bytes(nonce, device, ts), &Signature::from_bytes(&sig)).map_err(|_| "bad signature")?;
        if !self.cfg.allow.is_empty() && !self.cfg.allow.contains(device) {
            return Err("this relay doesn't accept that device".into());
        }
        Ok(device.to_string())
    }

    /// Handle one frame. Returns false to close the connection.
    async fn frame(&self, me: &str, serial: u64, f: ClientFrame, tx: &mpsc::UnboundedSender<ServerFrame>) -> bool {
        let mut s = self.state.lock().await;
        if s.conns.get(me).map(|c| c.serial) != Some(serial) {
            return false; // replaced
        }
        match f {
            ClientFrame::Auth { .. } => {
                let _ = tx.send(ServerFrame::Error { reason: "already authenticated".into() });
                return false;
            }
            ClientFrame::Ping => {
                let _ = tx.send(ServerFrame::Pong);
            }
            ClientFrame::Peers { ids } => {
                let ids: HashSet<String> = ids.into_iter().filter(|i| proto::valid_device_id(i) && i != me).take(64).collect();
                let before: HashSet<String> = s.peers.get(me).cloned().unwrap_or_default();
                s.peers.insert(me.to_string(), ids.clone());
                // Presence both ways for every mutual peer now online.
                for p in &ids {
                    if s.mutual(me, p) && s.conns.contains_key(p.as_str()) {
                        let _ = tx.send(ServerFrame::Presence { device: p.clone(), online: true });
                        s.send(p, ServerFrame::Presence { device: me.to_string(), online: true });
                    }
                }
                // Removed peers stop seeing this device.
                for gone in before.difference(&ids) {
                    s.send(gone, ServerFrame::Presence { device: me.to_string(), online: false });
                }
                // Deliver anything held for me from devices that are still mutual peers.
                let now = Instant::now();
                if let Some(q) = s.held.remove(me) {
                    for h in q {
                        if h.until > now && s.mutual(me, &h.from) {
                            let _ = tx.send(ServerFrame::Deliver { from: h.from, env: h.env });
                        }
                    }
                }
            }
            ClientFrame::Send { id, to, env } => {
                let reply = |status, reason: Option<&str>| ServerFrame::Sent { id: id.clone(), status, reason: reason.map(str::to_string) };
                if !proto::valid_device_id(&to) || !s.mutual(me, &to) {
                    let _ = tx.send(reply(SendStatus::Rejected, Some("not a peer")));
                } else if s.send(&to, ServerFrame::Deliver { from: me.to_string(), env: env.clone() }) {
                    let _ = tx.send(reply(SendStatus::Delivered, None));
                } else {
                    let cap = self.cfg.queue_per_device;
                    let until = Instant::now() + self.cfg.queue_ttl;
                    let q = s.held.entry(to.clone()).or_default();
                    if q.len() >= cap {
                        let _ = tx.send(reply(SendStatus::Rejected, Some("the device is offline and its queue is full")));
                    } else {
                        q.push_back(Held { from: me.to_string(), env, until });
                        let _ = tx.send(reply(SendStatus::Queued, Some("the device is offline")));
                    }
                }
            }
            ClientFrame::PairOpen { pid } => {
                let open = s.pairings.values().filter(|p| p.inviter == me).count();
                if !proto::valid_pid(&pid) || s.pairings.contains_key(&pid) || open >= 3 {
                    let _ = tx.send(ServerFrame::PairStatus { pid, ok: false, reason: Some("can't open that pairing".into()) });
                } else {
                    s.pairings.insert(
                        pid.clone(),
                        Pairing { inviter: me.to_string(), until: Instant::now() + self.cfg.pair_ttl, attempts: 0, joiners: HashSet::new() },
                    );
                    let _ = tx.send(ServerFrame::PairStatus { pid, ok: true, reason: None });
                }
            }
            ClientFrame::PairClose { pid } => {
                if s.pairings.get(&pid).is_some_and(|p| p.inviter == me) {
                    s.pairings.remove(&pid);
                }
            }
            ClientFrame::PairSend { pid, blob } => {
                let max = self.cfg.pair_attempts;
                let target = match s.pairings.get_mut(&pid) {
                    Some(p) if p.until > Instant::now() && p.attempts < max && p.inviter != me => {
                        p.attempts += 1;
                        p.joiners.insert(me.to_string());
                        Some(p.inviter.clone())
                    }
                    _ => None,
                };
                let ok = target.is_some_and(|inviter| s.send(&inviter, ServerFrame::PairMsg { pid: pid.clone(), from: me.to_string(), blob }));
                let reason = (!ok).then(|| "No device is waiting for that code. Check the code, and that the other device still shows it.".to_string());
                let _ = tx.send(ServerFrame::PairStatus { pid, ok, reason });
            }
            ClientFrame::PairReply { pid, to, blob } => {
                let allowed = s.pairings.get(&pid).is_some_and(|p| p.inviter == me && p.joiners.contains(&to));
                let ok = allowed && s.send(&to, ServerFrame::PairMsg { pid: pid.clone(), from: me.to_string(), blob });
                let _ = tx.send(ServerFrame::PairStatus { pid, ok, reason: (!ok).then(|| "The other device isn't connected.".to_string()) });
            }
        }
        true
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Enough of an id to tell devices apart in logs.
fn short(id: &str) -> &str {
    &id[..id.len().min(12)]
}
