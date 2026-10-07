//! The device hub: this device's side of cross-device IGRIS.
//!
//! It owns the relay link and turns envelopes into actions *through the
//! existing paths*:
//!
//! * a task request from another device becomes an ordinary conversation on
//!   this device, run by the app's [`TaskRunner`] through the normal chat →
//!   orchestrator → tool executor pipeline, with this device's own permission
//!   policy. Its orchestrator events are reported back as task updates;
//! * pause / resume / stop from the requesting device go through
//!   [`Orchestrator::control`], the same call the local UI makes;
//! * an action that needs approval asks the requesting device for a signed
//!   approval (verified here) while the local UI can answer too;
//! * memories sync in the background (revisions and tombstones).
//!
//! The hub never runs a tool itself and has no "remote command" path.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use igris_relay::proto::{ClientFrame, SendStatus, ServerFrame};
use serde::Serialize;
use tokio::sync::{mpsc, oneshot, watch, Notify};
use tokio_util::sync::CancellationToken;

use super::approval::{self, ApprovalRequest, Decision, SignedApproval};
use super::capability::Capability;
use super::envelope::{self, now_ms, Envelope};
use super::identity::{Identity, Platform};
use super::link::{self, Credentials, LinkEvent};
use super::pairing::{Invite, JoinRequest, Joining, KnownDevice};
use super::protocol::{valid_request_id, ControlAction, Message, RemoteStatus, MAX_OBJECTIVE_CHARS};
use super::registry::{self, Device, TrustSource};
use super::remote::{self, Direction, RemoteTask};
use super::sync;
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::orchestrator::{Control, Orchestrator, TaskUpdate};
use crate::tools::executor::{Approval, Approver, ToolActivity};

/// Outgoing tasks nobody picked up in this time are reported as timed out.
pub const PICKUP_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// How long a remote approval waits.
pub const APPROVAL_WAIT: Duration = Duration::from_secs(5 * 60);
/// Tasks from other devices that may run at once.
pub const MAX_INCOMING: usize = 2;
const OUTBOX_MAX: usize = 200;
const JOIN_WAIT: Duration = Duration::from_secs(3 * 60);
const SYNC_RETRY: Duration = Duration::from_secs(600);

/// Runs a task another device asked for, through the app's normal chat path.
#[async_trait::async_trait]
pub trait TaskRunner: Send + Sync {
    /// Create the conversation the task runs in (shown on this device).
    fn open(&self, from: &Device, objective: &str) -> AppResult<String>;
    /// Generate the reply (and run any task) with `approver` for approvals.
    /// Returns the reply text.
    async fn run(&self, conversation_id: &str, approver: Arc<dyn Approver>, cancel: CancellationToken) -> AppResult<String>;
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkStatus {
    /// "off", "connecting", "connected", "offline".
    pub state: &'static str,
    pub detail: Option<String>,
    pub relay_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSettings {
    pub relay_url: Option<String>,
    pub enabled: bool,
    pub sync_memory: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    #[serde(flatten)]
    pub device: Device,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThisDevice {
    pub device_id: String,
    pub name: String,
    pub platform: Platform,
    pub capabilities: Vec<Capability>,
    /// How the private keys are protected at rest ("dpapi", "android-keystore", "none").
    pub key_protection: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    #[serde(flatten)]
    pub task: RemoteTask,
    pub peer_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalView {
    pub request: ApprovalRequest,
    pub device_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingCode {
    pub code: String,
    pub expires_at: i64,
}

/// What the UI needs to know, as it happens.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum HubEvent {
    Status(LinkStatus),
    Presence {
        device_id: String,
        online: bool,
    },
    DevicesChanged,
    Task(TaskView),
    /// Another device asks this one (the requester) to approve an action.
    ApprovalNeeded(ApprovalView),
    ApprovalClosed {
        call_id: String,
    },
    /// A task from another device is waiting for approval here (the local UI may answer).
    LocalApproval {
        call_id: String,
        request_id: String,
        device_name: String,
        title: String,
        description: String,
    },
    /// A device typed this device's code and waits for the user's OK.
    PairingRequest {
        pairing_id: String,
        device_name: String,
        platform: Platform,
    },
    PairingDone {
        ok: bool,
        message: String,
    },
    /// A short, meaningful notification.
    Notice {
        title: String,
        body: String,
    },
}

pub type HubListener = Arc<dyn Fn(&HubEvent) + Send + Sync>;

struct LinkCtl {
    tx: mpsc::UnboundedSender<ClientFrame>,
    peers: watch::Sender<Vec<String>>,
    cancel: CancellationToken,
    up: bool,
}

struct InviteState {
    invite: Invite,
    pending: Option<JoinRequest>,
    /// Told when the relay confirms the code is open.
    opened: Option<oneshot::Sender<Result<(), String>>>,
}

struct JoinWait {
    joining: Joining,
    tx: oneshot::Sender<Result<(String, String), String>>,
}

struct Awaiting {
    request_id: String,
    task_id: String,
    tool: String,
    digest: String,
    peer: String,
    tx: Option<oneshot::Sender<Decision>>,
}

struct Incoming {
    request_id: String,
    peer: String,
    conversation_id: String,
    cancel: CancellationToken,
    /// Last (status, step) reported, to send only changes.
    last: Option<(RemoteStatus, Option<String>)>,
}

struct Asked {
    request: ApprovalRequest,
    peer: String,
}

pub struct DeviceHub {
    db: Arc<Database>,
    identity: Arc<RwLock<Identity>>,
    protector: String,
    orchestrator: Option<Arc<Orchestrator>>,
    runner: RwLock<Option<Arc<dyn TaskRunner>>>,
    /// Races remote approvals on this (executing) device: the local UI.
    local_approver: RwLock<Option<Arc<dyn Approver>>>,
    caps: RwLock<Vec<Capability>>,
    link: Mutex<Option<LinkCtl>>,
    status: Mutex<LinkStatus>,
    online: Mutex<HashSet<String>>,
    invites: Mutex<HashMap<String, InviteState>>,
    joining: Mutex<Option<JoinWait>>,
    awaiting: Mutex<HashMap<String, Awaiting>>,
    asked: Mutex<HashMap<String, Asked>>,
    incoming: Mutex<HashMap<String, Incoming>>,
    receipts: Mutex<HashMap<String, String>>,
    outbox: Mutex<VecDeque<(String, Message, i64)>>,
    sync_inflight: Mutex<HashMap<String, Instant>>,
    changed: Notify,
    listeners: RwLock<Vec<HubListener>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn clip(s: &str, n: usize) -> String {
    crate::orchestrator::task::clip(s, n)
}

impl DeviceHub {
    pub fn new(db: Arc<Database>, identity: Identity, protector: &str, orchestrator: Option<Arc<Orchestrator>>) -> Arc<Self> {
        let hub = Arc::new(Self {
            db,
            identity: Arc::new(RwLock::new(identity)),
            protector: protector.to_string(),
            orchestrator: orchestrator.clone(),
            runner: RwLock::new(None),
            local_approver: RwLock::new(None),
            caps: RwLock::new(vec![Capability::Tasks]),
            link: Mutex::new(None),
            status: Mutex::new(LinkStatus { state: "off", detail: None, relay_url: None }),
            online: Mutex::new(HashSet::new()),
            invites: Mutex::new(HashMap::new()),
            joining: Mutex::new(None),
            awaiting: Mutex::new(HashMap::new()),
            asked: Mutex::new(HashMap::new()),
            incoming: Mutex::new(HashMap::new()),
            receipts: Mutex::new(HashMap::new()),
            outbox: Mutex::new(VecDeque::new()),
            sync_inflight: Mutex::new(HashMap::new()),
            changed: Notify::new(),
            listeners: RwLock::new(Vec::new()),
        });
        if let Some(o) = orchestrator {
            let weak = Arc::downgrade(&hub);
            o.on_event(Arc::new(move |u: &TaskUpdate| {
                if let Some(h) = weak.upgrade() {
                    h.on_task_event(u);
                }
            }));
        }
        hub
    }

    pub fn set_runner(&self, runner: Arc<dyn TaskRunner>) {
        *self.runner.write().unwrap_or_else(|p| p.into_inner()) = Some(runner);
    }

    pub fn set_local_approver(&self, approver: Arc<dyn Approver>) {
        *self.local_approver.write().unwrap_or_else(|p| p.into_inner()) = Some(approver);
    }

    /// What this device can do (from its registered tools).
    pub fn set_capabilities(&self, caps: Vec<Capability>) {
        *self.caps.write().unwrap_or_else(|p| p.into_inner()) = caps;
    }

    pub fn on_event(&self, l: HubListener) {
        self.listeners.write().unwrap_or_else(|p| p.into_inner()).push(l);
    }

    fn emit(&self, ev: HubEvent) {
        let ls = self.listeners.read().map(|g| g.clone()).unwrap_or_default();
        for l in ls {
            l(&ev);
        }
    }

    fn notice(&self, title: impl Into<String>, body: impl Into<String>) {
        self.emit(HubEvent::Notice { title: title.into(), body: body.into() });
    }

    fn me_id(&self) -> String {
        self.identity.read().map(|i| i.device_id.clone()).unwrap_or_default()
    }

    fn me_name(&self) -> String {
        self.identity.read().map(|i| i.name.clone()).unwrap_or_default()
    }

    fn me_platform(&self) -> Platform {
        self.identity.read().map(|i| i.platform).unwrap_or(Platform::Other)
    }

    fn caps(&self) -> Vec<Capability> {
        self.caps.read().map(|c| c.clone()).unwrap_or_default()
    }

    /// Computers may add and remove devices for the whole IGRIS; phones only for themselves.
    fn is_manager(p: Platform) -> bool {
        matches!(p, Platform::Windows | Platform::Linux | Platform::Macos)
    }

    // ── Settings and connection ────────────────────────────────────────────

    pub fn settings(&self) -> AppResult<DeviceSettings> {
        let conn = self.db.conn()?;
        Ok(conn.query_row("SELECT relay_url, enabled, sync_memory FROM device_settings WHERE id = 1", [], |r| {
            Ok(DeviceSettings { relay_url: r.get(0)?, enabled: r.get::<_, i64>(1)? != 0, sync_memory: r.get::<_, i64>(2)? != 0 })
        })?)
    }

    /// Set (or clear) the relay and connect or disconnect accordingly.
    pub fn configure(self: &Arc<Self>, relay_url: Option<&str>, enabled: bool, sync_memory: bool) -> AppResult<DeviceSettings> {
        // Empty: the free ntfy.sh service. ws(s):// is your own relay; http(s):// your own ntfy server.
        let url = match relay_url.map(str::trim).filter(|u| !u.is_empty()) {
            Some(u) if u.to_ascii_lowercase().starts_with("http") => Some(super::ntfy::validate_server(u)?),
            Some(u) => Some(link::validate_url(u)?),
            None => None,
        };
        self.db.conn()?.execute(
            "UPDATE device_settings SET relay_url = ?1, enabled = ?2, sync_memory = ?3 WHERE id = 1",
            rusqlite::params![url, enabled as i64, sync_memory as i64],
        )?;
        tracing::info!(event = "DEVICE_SETTINGS_CHANGED", enabled, sync_memory);
        self.restart();
        self.settings()
    }

    pub fn status(&self) -> LinkStatus {
        lock(&self.status).clone()
    }

    fn set_status(&self, state: &'static str, detail: Option<String>) {
        let s = {
            let mut g = lock(&self.status);
            g.state = state;
            g.detail = detail;
            g.clone()
        };
        self.emit(HubEvent::Status(s));
    }

    fn link_up(&self) -> bool {
        lock(&self.link).as_ref().is_some_and(|l| l.up)
    }

    /// Start: recover state left by a previous run, connect if configured,
    /// and run the background ticker. Call once, inside the async runtime.
    pub fn start(self: &Arc<Self>) {
        self.recover();
        self.restart();
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            loop {
                tick.tick().await;
                let Some(h) = weak.upgrade() else { break };
                h.tick();
            }
        });
    }

    /// Tasks from other devices that were running when IGRIS closed are
    /// reported as interrupted (the orchestrator marks the local task the same way).
    fn recover(&self) {
        let rows = self.db.conn().and_then(|c| remote::unfinished(&c, Direction::Incoming)).unwrap_or_default();
        for t in rows {
            let detail = format!("IGRIS on {} closed while working on this.", self.me_name());
            self.finish_incoming(&t.request_id, &t.peer_device_id, RemoteStatus::Failed, &detail);
        }
    }

    fn restart(self: &Arc<Self>) {
        if let Some(old) = lock(&self.link).take() {
            old.cancel.cancel();
        }
        lock(&self.online).clear();
        let s = match self.settings() {
            Ok(s) => s,
            Err(e) => {
                self.set_status("off", Some(e.to_string()));
                return;
            }
        };
        lock(&self.status).relay_url = s.relay_url.clone();
        if !s.enabled {
            self.set_status("off", None);
            return;
        }
        let (tx, out) = mpsc::unbounded_channel();
        let (peers_tx, peers_rx) = watch::channel(self.peer_ids());
        let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        *lock(&self.link) = Some(LinkCtl { tx, peers: peers_tx, cancel: cancel.clone(), up: false });
        match s.relay_url.filter(|u| u.to_ascii_lowercase().starts_with("ws")) {
            // Your own relay.
            Some(url) => {
                let creds = {
                    let id = self.identity.clone();
                    let me = self.identity.read().unwrap_or_else(|p| p.into_inner());
                    Credentials {
                        device_id: me.device_id.clone(),
                        signing_key_b64: me.public().signing_key,
                        sign: Arc::new(move |msg: &[u8]| id.read().unwrap_or_else(|p| p.into_inner()).sign(msg)),
                    }
                };
                tokio::spawn(link::run(url, creds, peers_rx, out, ev_tx, cancel.clone()));
            }
            // The default: ntfy.sh (or your own ntfy server).
            None => {
                let server = self.settings().ok().and_then(|s| s.relay_url).unwrap_or_else(|| super::ntfy::DEFAULT_SERVER.to_string());
                let (device_id, owner_id) = self.identity.read().map(|i| (i.device_id.clone(), i.owner_id.clone())).unwrap_or_default();
                tokio::spawn(super::ntfy::run(server, device_id, owner_id, peers_rx, out, ev_tx, cancel.clone()));
            }
        }
        let hub = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    ev = ev_rx.recv() => match ev {
                        Some(ev) => hub.on_link(ev),
                        None => break,
                    },
                }
            }
        });
    }

    /// Stop talking to the relay (local use is unaffected).
    pub fn shutdown(&self) {
        if let Some(old) = lock(&self.link).take() {
            old.cancel.cancel();
        }
    }

    fn peer_ids(&self) -> Vec<String> {
        self.db.conn().and_then(|c| registry::active(&c)).map(|v| v.into_iter().map(|d| d.device_id).collect()).unwrap_or_default()
    }

    fn refresh_peers(&self) {
        let ids = self.peer_ids();
        if let Some(l) = lock(&self.link).as_ref() {
            let _ = l.peers.send(ids);
        }
        self.emit(HubEvent::DevicesChanged);
    }

    fn frame(&self, f: ClientFrame) -> bool {
        lock(&self.link).as_ref().filter(|l| l.up).is_some_and(|l| l.tx.send(f).is_ok())
    }

    fn on_link(self: &Arc<Self>, ev: LinkEvent) {
        match ev {
            LinkEvent::Connecting => {
                if lock(&self.status).state != "offline" {
                    self.set_status("connecting", None);
                }
            }
            LinkEvent::Up => {
                if let Some(l) = lock(&self.link).as_mut() {
                    l.up = true;
                }
                self.set_status("connected", None);
                // A code shown before a reconnect keeps working: listen for it again.
                let open: Vec<String> = lock(&self.invites).iter().filter(|(_, s)| !s.invite.expired() && s.opened.is_none()).map(|(p, _)| p.clone()).collect();
                for pid in open {
                    self.frame(ClientFrame::PairOpen { pid });
                }
                self.flush_outbox();
            }
            LinkEvent::Down { reason } => {
                if let Some(l) = lock(&self.link).as_mut() {
                    l.up = false;
                }
                let was: Vec<String> = lock(&self.online).drain().collect();
                for d in was {
                    self.emit(HubEvent::Presence { device_id: d, online: false });
                }
                lock(&self.sync_inflight).clear();
                self.set_status("offline", Some(reason));
            }
            LinkEvent::Frame(f) => self.on_frame(f),
        }
    }

    fn on_frame(self: &Arc<Self>, f: ServerFrame) {
        match f {
            ServerFrame::Deliver { from, env } => self.receive(&from, env),
            ServerFrame::Sent { id, status, reason } => {
                let Some(request_id) = lock(&self.receipts).remove(&id) else { return };
                let name = self.peer_name_for(&request_id);
                let (st, detail) = match status {
                    SendStatus::Delivered => (RemoteStatus::Sent, format!("Sent to {name}.")),
                    SendStatus::Queued => (RemoteStatus::Queued, format!("{name} is offline. It will get this if it comes online in the next 10 minutes.")),
                    SendStatus::Rejected => (RemoteStatus::Rejected, format!("The relay couldn't deliver this to {name}: {}.", reason.unwrap_or_default())),
                };
                self.update_outgoing(
                    &request_id,
                    |cur| matches!(cur, RemoteStatus::Pending | RemoteStatus::Sent | RemoteStatus::Queued),
                    st,
                    Some(&detail),
                    None,
                );
            }
            ServerFrame::Presence { device, online } => {
                let changed = if online { lock(&self.online).insert(device.clone()) } else { lock(&self.online).remove(&device) };
                if let Ok(c) = self.db.conn() {
                    let _ = registry::touch(&c, &device);
                }
                if changed {
                    self.emit(HubEvent::Presence { device_id: device.clone(), online });
                }
                if online {
                    let hello = Message::Hello { name: self.me_name(), capabilities: self.caps() };
                    let _ = self.send(&device, hello);
                    lock(&self.sync_inflight).remove(&device);
                    self.sync_to(&device);
                }
            }
            ServerFrame::PairMsg { pid, from, blob } => self.on_pair_msg(&pid, &from, &blob),
            ServerFrame::PairStatus { pid, ok, reason } => {
                if let Some(tx) = lock(&self.invites).get_mut(&pid).and_then(|s| s.opened.take()) {
                    let _ = tx.send(if ok { Ok(()) } else { Err(reason.clone().unwrap_or_else(|| "The relay refused the code.".into())) });
                    if !ok {
                        lock(&self.invites).remove(&pid);
                    }
                    return;
                }
                if ok {
                    return;
                }
                let reason = reason.unwrap_or_else(|| "Pairing failed.".into());
                let mut j = lock(&self.joining);
                if j.as_ref().is_some_and(|w| w.joining.pairing_id == pid) {
                    if let Some(w) = j.take() {
                        let _ = w.tx.send(Err(reason));
                    }
                } else if lock(&self.invites).remove(&pid).is_some() {
                    drop(j);
                    self.emit(HubEvent::PairingDone { ok: false, message: reason });
                }
            }
            ServerFrame::Challenge { .. } | ServerFrame::Ready { .. } | ServerFrame::Pong | ServerFrame::Error { .. } => {}
        }
    }

    // ── Sending ────────────────────────────────────────────────────────────

    /// Encrypt `msg` for `peer` and hand it to the relay (or keep it until the
    /// connection is back). Returns the envelope id when it went out now.
    fn send(&self, peer: &str, msg: Message) -> AppResult<Option<String>> {
        let device =
            self.db.conn().and_then(|c| registry::trusted(&c, peer))?.ok_or_else(|| AppError::validation("That device isn't paired with this one."))?;
        if !self.link_up() {
            let mut ob = lock(&self.outbox);
            if ob.len() >= OUTBOX_MAX {
                ob.pop_front();
            }
            ob.push_back((peer.to_string(), msg.clone(), now_ms() + msg.ttl_ms()));
            return Ok(None);
        }
        let bytes = serde_json::to_vec(&msg)?;
        let env = {
            let me = self.identity.read().map_err(|_| AppError::internal("identity lock poisoned"))?;
            envelope::seal(&me, &device.public(), &bytes, msg.ttl_ms())?
        };
        let id = env.id.clone();
        let sent = self.frame(ClientFrame::Send { id: id.clone(), to: peer.to_string(), env: serde_json::to_value(&env)? });
        tracing::debug!(event = "DEVICE_MESSAGE_SENT", kind = msg.kind(), sent);
        Ok(sent.then_some(id))
    }

    fn flush_outbox(&self) {
        let items: Vec<(String, Message, i64)> = lock(&self.outbox).drain(..).collect();
        let now = now_ms();
        for (peer, msg, exp) in items {
            if exp <= now {
                continue;
            }
            let request = match &msg {
                Message::TaskRequest { request_id, .. } => Some(request_id.clone()),
                _ => None,
            };
            if let Ok(Some(id)) = self.send(&peer, msg) {
                if let Some(r) = request {
                    lock(&self.receipts).insert(id, r);
                }
            }
        }
    }

    // ── Receiving ──────────────────────────────────────────────────────────

    fn receive(self: &Arc<Self>, relay_from: &str, env: serde_json::Value) {
        let Ok(env) = serde_json::from_value::<Envelope>(env) else {
            tracing::warn!(event = "DEVICE_MESSAGE_MALFORMED");
            return;
        };
        let opened = (|| -> Result<(Device, Message), String> {
            if env.from != relay_from {
                return Err("sender mismatch".into());
            }
            let conn = self.db.conn().map_err(|e| e.to_string())?;
            let dev = registry::trusted(&conn, &env.from).map_err(|e| e.to_string())?.ok_or("not a trusted device")?;
            let me = self.identity.read().map_err(|_| "identity lock".to_string())?;
            if dev.owner_id != me.owner_id {
                return Err("belongs to another owner".into());
            }
            let bytes = envelope::open(&me, &dev.public(), &env, &conn).map_err(|e| e.to_string())?;
            let msg: Message = serde_json::from_slice(&bytes).map_err(|_| "unknown message".to_string())?;
            let _ = registry::touch(&conn, &dev.device_id);
            Ok((dev, msg))
        })();
        match opened {
            Ok((dev, msg)) => {
                tracing::debug!(event = "DEVICE_MESSAGE_RECEIVED", kind = msg.kind());
                self.dispatch(dev, msg);
            }
            // Re-delivered after a reconnect: already handled, nothing to report.
            Err(why) if why.contains("replay") => tracing::debug!(event = "DEVICE_MESSAGE_DUPLICATE"),
            Err(why) => {
                tracing::warn!(event = "DEVICE_MESSAGE_REFUSED", reason = %why);
                if let Ok(c) = self.db.conn() {
                    registry::audit(&c, Some(relay_from), "message_refused", Some(&why));
                }
            }
        }
    }

    fn dispatch(self: &Arc<Self>, dev: Device, msg: Message) {
        match msg {
            Message::Hello { capabilities, .. } => {
                if let Ok(c) = self.db.conn() {
                    let _ = registry::set_capabilities(&c, &dev.device_id, &capabilities);
                }
                self.emit(HubEvent::DevicesChanged);
            }
            Message::TaskRequest { request_id, objective } => self.on_task_request(dev, request_id, objective),
            Message::TaskAck { request_id, accepted, reason } => {
                if !self.is_outgoing_to(&request_id, &dev.device_id) {
                    return;
                }
                if accepted {
                    self.update_outgoing(&request_id, |s| !s.is_final(), RemoteStatus::Accepted, Some(&format!("{} accepted the task.", dev.name)), None);
                    self.notice(format!("{} accepted the task", dev.name), self.objective_of(&request_id));
                } else {
                    let why = reason.unwrap_or_else(|| "No reason given.".into());
                    self.update_outgoing(
                        &request_id,
                        |s| !s.is_final(),
                        RemoteStatus::Rejected,
                        Some(&format!("{} didn't take the task: {why}", dev.name)),
                        None,
                    );
                    self.notice(format!("{} didn't take the task", dev.name), why);
                }
            }
            Message::TaskUpdate { request_id, seq, status, step, detail } => self.on_task_update(&dev, &request_id, seq, status, step, detail),
            Message::TaskControl { request_id, action } => self.on_task_control(&dev, &request_id, action),
            Message::ControlResult { request_id, ok, message, .. } => {
                if self.is_outgoing_to(&request_id, &dev.device_id) && !ok {
                    self.notice(format!("{} couldn't do that", dev.name), message);
                }
            }
            Message::ApprovalNeeded { request } => self.on_approval_needed(&dev, request),
            Message::ApprovalAnswer { approval } => self.on_approval_answer(&dev, approval),
            Message::DeviceAnnounce { owner_id, device } => self.on_announce(&dev, &owner_id, device),
            Message::DeviceRevoked { device_id } => self.on_revoked(&dev, &device_id),
            Message::SyncBatch { items, upto, .. } => {
                if !self.settings().map(|s| s.sync_memory).unwrap_or(false) {
                    return;
                }
                let me = self.me_id();
                let applied = self.db.conn().and_then(|c| sync::apply(&c, &me, &items));
                match applied {
                    Ok(a) => {
                        if a.added + a.updated + a.deleted > 0 {
                            self.emit(HubEvent::DevicesChanged);
                        }
                        let _ = self.send(&dev.device_id, Message::SyncAck { upto });
                    }
                    Err(e) => tracing::warn!(event = "MEMORY_SYNC_FAILED", error = %e),
                }
            }
            Message::SyncAck { upto } => {
                if let Ok(c) = self.db.conn() {
                    let _ = sync::set_acked(&c, &dev.device_id, upto);
                }
                lock(&self.sync_inflight).remove(&dev.device_id);
                self.sync_to(&dev.device_id);
            }
        }
    }

    // ── Tasks this device sends ────────────────────────────────────────────

    /// Ask `device_id` to do `objective`. Returns at once; follow the task
    /// with [`HubEvent::Task`] events, [`DeviceHub::task`] or [`DeviceHub::wait_for_pickup`].
    pub fn send_task(&self, device_id: &str, objective: &str) -> AppResult<TaskView> {
        let objective = objective.trim();
        if objective.is_empty() || objective.chars().count() > MAX_OBJECTIVE_CHARS {
            return Err(AppError::validation(format!("Describe the task in 1–{MAX_OBJECTIVE_CHARS} characters.")));
        }
        let dev =
            self.db.conn().and_then(|c| registry::trusted(&c, device_id))?.ok_or_else(|| AppError::validation("That device isn't paired with this one."))?;
        if !dev.capabilities.is_empty() && !dev.capabilities.contains(&Capability::Tasks) {
            return Err(AppError::validation(format!("{} can't run tasks.", dev.name)));
        }
        let request_id = super::identity::random_hex(16);
        {
            let c = self.db.conn()?;
            remote::insert(&c, &request_id, Direction::Outgoing, &dev.device_id, objective, None, RemoteStatus::Pending)?;
            registry::audit(&c, Some(&dev.device_id), "task_sent", Some(objective));
        }
        let msg = Message::TaskRequest { request_id: request_id.clone(), objective: objective.to_string() };
        match self.send(&dev.device_id, msg)? {
            Some(id) => {
                lock(&self.receipts).insert(id, request_id.clone());
            }
            None => {
                let detail = match self.status().state {
                    "off" => "Cross-device isn't connected (Settings → Devices). It will be sent when it is, within 10 minutes.".to_string(),
                    _ => "Waiting for the connection to the relay. It will be sent when it's back, within 10 minutes.".to_string(),
                };
                self.update_outgoing(&request_id, |_| true, RemoteStatus::Pending, Some(&detail), None);
            }
        }
        tracing::info!(event = "REMOTE_TASK_SENT");
        self.task(&request_id)?.ok_or_else(|| AppError::internal("task vanished"))
    }

    /// Wait until the target picked the task up (accepted / refused / failed)
    /// or `limit` passes. Returns the task as it stands.
    pub async fn wait_for_pickup(&self, request_id: &str, limit: Duration) -> AppResult<TaskView> {
        let deadline = tokio::time::Instant::now() + limit;
        loop {
            let notified = self.changed.notified();
            let t = self.task(request_id)?.ok_or_else(|| AppError::validation("No such task."))?;
            if !matches!(t.task.status, RemoteStatus::Pending | RemoteStatus::Sent) {
                return Ok(t);
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Ok(t);
            }
        }
    }

    pub fn task(&self, request_id: &str) -> AppResult<Option<TaskView>> {
        let c = self.db.conn()?;
        Ok(remote::get(&c, request_id)?.map(|t| self.view(&c, t)))
    }

    pub fn tasks(&self, limit: u32) -> AppResult<Vec<TaskView>> {
        let c = self.db.conn()?;
        Ok(remote::recent(&c, limit)?.into_iter().map(|t| self.view(&c, t)).collect())
    }

    fn view(&self, c: &rusqlite::Connection, t: RemoteTask) -> TaskView {
        let peer_name = registry::get(c, &t.peer_device_id).ok().flatten().map(|d| d.name).unwrap_or_else(|| "another device".into());
        TaskView { task: t, peer_name }
    }

    fn peer_name_for(&self, request_id: &str) -> String {
        self.task(request_id).ok().flatten().map(|t| t.peer_name).unwrap_or_else(|| "the other device".into())
    }

    fn objective_of(&self, request_id: &str) -> String {
        self.task(request_id).ok().flatten().map(|t| clip(&t.task.objective, 120)).unwrap_or_default()
    }

    fn is_outgoing_to(&self, request_id: &str, peer: &str) -> bool {
        let ok = self
            .db
            .conn()
            .ok()
            .and_then(|c| remote::get(&c, request_id).ok().flatten())
            .is_some_and(|t| t.direction == Direction::Outgoing && t.peer_device_id == peer);
        if !ok {
            tracing::warn!(event = "REMOTE_TASK_MESSAGE_REFUSED", reason = "not a task this device sent to that device");
        }
        ok
    }

    fn update_outgoing(&self, request_id: &str, when: impl Fn(RemoteStatus) -> bool, status: RemoteStatus, detail: Option<&str>, seq: Option<i64>) {
        let updated = (|| -> AppResult<Option<RemoteTask>> {
            let c = self.db.conn()?;
            match remote::get(&c, request_id)? {
                Some(t) if when(t.status) => remote::set_status(&c, request_id, status, detail, None, seq),
                _ => Ok(None),
            }
        })();
        if let Ok(Some(_)) = updated {
            if let Ok(Some(v)) = self.task(request_id) {
                self.emit(HubEvent::Task(v));
            }
            self.changed.notify_waiters();
        }
    }

    /// Pause, resume or stop a task this device sent.
    pub fn control_task(&self, request_id: &str, action: ControlAction) -> AppResult<TaskView> {
        let t = self.task(request_id)?.ok_or_else(|| AppError::validation("No such task."))?;
        if t.task.direction != Direction::Outgoing {
            return Err(AppError::validation("That task was sent from another device; control it there or in its conversation here."));
        }
        if t.task.status.is_final() {
            return Err(AppError::validation("That task has already ended."));
        }
        self.send(&t.task.peer_device_id, Message::TaskControl { request_id: request_id.to_string(), action })?;
        if !self.link_up() {
            return Err(AppError::validation(format!("Not connected right now; {} will get this when the connection is back.", t.peer_name)));
        }
        Ok(t)
    }

    fn on_task_update(&self, dev: &Device, request_id: &str, seq: u64, status: RemoteStatus, step: Option<String>, detail: Option<String>) {
        if !self.is_outgoing_to(request_id, &dev.device_id) {
            return;
        }
        let Ok(Some(cur)) = self.task(request_id) else { return };
        // Ordering and duplicates: only newer updates count.
        if (seq as i64) <= cur.task.last_seq {
            return;
        }
        let text = match (status, &detail, &step) {
            (RemoteStatus::Completed, Some(d), _) => format!("Done — completed on {}. {}", dev.name, clip(d, 1500)),
            (RemoteStatus::Completed, None, _) => format!("Done — completed on {}.", dev.name),
            (_, Some(d), _) => clip(d, 1500),
            (_, None, Some(s)) => clip(s, 300),
            _ => String::new(),
        };
        self.update_outgoing(request_id, |s| !s.is_final(), status, (!text.is_empty()).then_some(text.as_str()), Some(seq as i64));
        // The prompt for an approval goes away once the task moves on.
        if status != RemoteStatus::WaitingForApproval {
            let closed: Vec<String> = {
                let mut a = lock(&self.asked);
                let ids: Vec<String> = a.iter().filter(|(_, x)| x.request.request_id == request_id).map(|(k, _)| k.clone()).collect();
                for k in &ids {
                    a.remove(k);
                }
                ids
            };
            for call_id in closed {
                self.emit(HubEvent::ApprovalClosed { call_id });
            }
        }
        if status.is_final() && cur.task.status != status {
            let title = match status {
                RemoteStatus::Completed => format!("Done — completed on {}", dev.name),
                RemoteStatus::Cancelled => format!("Stopped on {}", dev.name),
                RemoteStatus::Ended => format!("{} needs you to finish this", dev.name),
                _ => format!("The task failed on {}", dev.name),
            };
            self.notice(title, if text.is_empty() { cur.task.objective.clone() } else { text });
        }
    }

    fn on_approval_needed(&self, dev: &Device, request: ApprovalRequest) {
        if !self.is_outgoing_to(&request.request_id, &dev.device_id) || request.target_device != dev.device_id {
            return;
        }
        if request.expires_at <= now_ms() {
            return;
        }
        let view = ApprovalView { request: request.clone(), device_name: dev.name.clone() };
        lock(&self.asked).insert(request.call_id.clone(), Asked { request: request.clone(), peer: dev.device_id.clone() });
        if let Ok(c) = self.db.conn() {
            registry::audit(&c, Some(&dev.device_id), "approval_requested", Some(&format!("{}: {}", request.title, request.description)));
        }
        self.update_outgoing(
            &request.request_id,
            |s| !s.is_final(),
            RemoteStatus::WaitingForApproval,
            Some(&format!("Waiting for your OK: {}", request.description)),
            None,
        );
        self.emit(HubEvent::ApprovalNeeded(view));
        self.notice(format!("{} needs your OK", dev.name), request.description);
    }

    /// Approvals other devices are waiting for from the user here.
    pub fn pending_approvals(&self) -> Vec<ApprovalView> {
        let c = self.db.conn().ok();
        let now = now_ms();
        lock(&self.asked)
            .values()
            .filter(|a| a.request.expires_at > now)
            .map(|a| ApprovalView {
                request: a.request.clone(),
                device_name: c.as_ref().and_then(|c| registry::get(c, &a.peer).ok().flatten()).map(|d| d.name).unwrap_or_default(),
            })
            .collect()
    }

    /// The user's answer to another device's approval request: signed here, verified there.
    pub fn answer_approval(&self, call_id: &str, approve: bool) -> AppResult<()> {
        let asked = lock(&self.asked).remove(call_id).ok_or_else(|| AppError::validation("That request is no longer waiting."))?;
        if asked.request.expires_at <= now_ms() {
            return Err(AppError::validation("That request expired."));
        }
        let decision = if approve { Decision::Approve } else { Decision::Deny };
        let signed = {
            let me = self.identity.read().map_err(|_| AppError::internal("identity lock poisoned"))?;
            SignedApproval::sign(&me, &asked.request, decision)
        };
        if let Ok(c) = self.db.conn() {
            registry::audit(&c, Some(&asked.peer), if approve { "approval_given" } else { "approval_denied" }, Some(&asked.request.description));
        }
        self.send(&asked.peer, Message::ApprovalAnswer { approval: signed })?;
        self.emit(HubEvent::ApprovalClosed { call_id: call_id.to_string() });
        Ok(())
    }

    // ── Tasks other devices send here ──────────────────────────────────────

    fn on_task_request(self: &Arc<Self>, dev: Device, request_id: String, objective: String) {
        let reject = |hub: &Self, why: &str| {
            let _ = hub.send(&dev.device_id, Message::TaskAck { request_id: request_id.clone(), accepted: false, reason: Some(why.to_string()) });
            if let Ok(c) = hub.db.conn() {
                registry::audit(&c, Some(&dev.device_id), "task_refused", Some(why));
            }
        };
        if !valid_request_id(&request_id) || objective.trim().is_empty() || objective.chars().count() > MAX_OBJECTIVE_CHARS {
            return reject(self, "The request was malformed.");
        }
        // Idempotent: a repeated request gets the same answer, never a second run.
        if let Ok(Some(existing)) = self.db.conn().and_then(|c| remote::get(&c, &request_id)) {
            if existing.direction == Direction::Incoming && existing.peer_device_id == dev.device_id {
                let _ = self.send(&dev.device_id, Message::TaskAck { request_id: request_id.clone(), accepted: true, reason: None });
            }
            return;
        }
        let Some(runner) = self.runner.read().ok().and_then(|r| r.clone()) else {
            return reject(self, &format!("IGRIS on {} isn't ready to take tasks right now.", self.me_name()));
        };
        if lock(&self.incoming).len() >= MAX_INCOMING {
            return reject(self, &format!("{} is busy with other tasks from your devices. Try again in a moment.", self.me_name()));
        }
        let conversation_id = match runner.open(&dev, objective.trim()) {
            Ok(c) => c,
            Err(e) => return reject(self, &e.to_string()),
        };
        if let Ok(c) = self.db.conn() {
            if let Err(e) =
                remote::insert(&c, &request_id, Direction::Incoming, &dev.device_id, objective.trim(), Some(&conversation_id), RemoteStatus::Accepted)
            {
                tracing::warn!(event = "REMOTE_TASK_SAVE_FAILED", error = %e);
            }
            registry::audit(&c, Some(&dev.device_id), "task_received", Some(objective.trim()));
        }
        let cancel = CancellationToken::new();
        lock(&self.incoming).insert(
            request_id.clone(),
            Incoming {
                request_id: request_id.clone(),
                peer: dev.device_id.clone(),
                conversation_id: conversation_id.clone(),
                cancel: cancel.clone(),
                last: None,
            },
        );
        let _ = self.send(&dev.device_id, Message::TaskAck { request_id: request_id.clone(), accepted: true, reason: None });
        if let Ok(Some(v)) = self.task(&request_id) {
            self.emit(HubEvent::Task(v));
        }
        self.notice(format!("Task from {}", dev.name), clip(objective.trim(), 200));
        tracing::info!(event = "REMOTE_TASK_ACCEPTED");

        let hub = self.clone();
        let approver: Arc<dyn Approver> = Arc::new(RemoteApprover {
            hub: Arc::downgrade(self),
            request_id: request_id.clone(),
            peer: dev.device_id.clone(),
            peer_name: dev.name.clone(),
            conversation_id: conversation_id.clone(),
        });
        tokio::spawn(async move {
            let started = crate::orchestrator::task::now();
            let outcome = runner.run(&conversation_id, approver, cancel.clone()).await;
            let (status, detail) = hub.outcome(&conversation_id, &started, &outcome, cancel.is_cancelled());
            lock(&hub.incoming).remove(&request_id);
            hub.finish_incoming(&request_id, &dev.device_id, status, &detail);
        });
    }

    /// The honest outcome of a finished run: the orchestrator task's own final
    /// state when there was one, otherwise how the reply ended.
    fn outcome(&self, conversation_id: &str, started: &str, run: &AppResult<String>, cancelled: bool) -> (RemoteStatus, String) {
        let task = self
            .orchestrator
            .as_ref()
            .and_then(|o| o.for_conversation(conversation_id, 1).ok())
            .and_then(|v| v.into_iter().next())
            .filter(|t| t.task.created_at.as_str() >= started);
        if let Some(t) = task {
            let status = if cancelled && !t.task.state.is_final() { RemoteStatus::Cancelled } else { RemoteStatus::from_task(t.task.state) };
            let status = if status.is_final() { status } else { RemoteStatus::Ended };
            let detail = t.task.result.clone().or(t.task.error.clone()).or_else(|| run.as_ref().ok().cloned()).unwrap_or_default();
            return (status, detail);
        }
        match run {
            _ if cancelled => (RemoteStatus::Cancelled, "Stopped.".into()),
            Ok(reply) => (RemoteStatus::Completed, reply.clone()),
            Err(e) => (RemoteStatus::Failed, e.to_string()),
        }
    }

    fn finish_incoming(&self, request_id: &str, peer: &str, status: RemoteStatus, detail: &str) {
        let seq = (|| -> AppResult<i64> {
            let c = self.db.conn()?;
            let seq = remote::next_seq(&c, request_id)?;
            remote::set_status(&c, request_id, status, Some(detail), Some(detail), Some(seq))?;
            registry::audit(&c, Some(peer), "task_finished", Some(&format!("{}: {}", status.as_str(), clip(detail, 200))));
            Ok(seq)
        })();
        let Ok(seq) = seq else { return };
        let _ = self.send(
            peer,
            Message::TaskUpdate {
                request_id: request_id.to_string(),
                seq: seq as u64,
                status,
                step: None,
                detail: Some(clip(detail, super::protocol::MAX_DETAIL_CHARS)),
            },
        );
        if let Ok(Some(v)) = self.task(request_id) {
            self.emit(HubEvent::Task(v));
        }
        tracing::info!(event = "REMOTE_TASK_FINISHED", status = status.as_str());
    }

    /// Orchestrator events for tasks other devices asked for → progress updates.
    fn on_task_event(&self, u: &TaskUpdate) {
        let Some(cid) = u.task.task.conversation_id.as_deref() else { return };
        let status = RemoteStatus::from_task(u.task.task.state);
        if status.is_final() {
            return; // sent once the run has really finished
        }
        let step = u.task.task.current_step.and_then(|i| u.task.task.plan.get(i)).map(|p| p.title.clone()).or_else(|| u.task.task.activity.clone());
        let (request_id, peer) = {
            let mut g = lock(&self.incoming);
            let Some(inc) = g.values_mut().find(|i| i.conversation_id == cid) else { return };
            let now = (status, step.clone());
            if inc.last.as_ref() == Some(&now) {
                return;
            }
            inc.last = Some(now);
            (inc.request_id.clone(), inc.peer.clone())
        };
        let seq = self.db.conn().and_then(|c| {
            let _ = remote::set_task_id(&c, &request_id, &u.task.task.id);
            let seq = remote::next_seq(&c, &request_id)?;
            remote::set_status(&c, &request_id, status, step.as_deref(), None, Some(seq))?;
            Ok(seq)
        });
        if let Ok(seq) = seq {
            let _ = self.send(&peer, Message::TaskUpdate { request_id, seq: seq as u64, status, step, detail: None });
        }
    }

    fn on_task_control(&self, dev: &Device, request_id: &str, action: ControlAction) {
        let found = lock(&self.incoming).get(request_id).filter(|i| i.peer == dev.device_id).map(|i| (i.conversation_id.clone(), i.cancel.clone()));
        let reply = |ok: bool, message: String| {
            if let Ok(c) = self.db.conn() {
                registry::audit(&c, Some(&dev.device_id), "task_control", Some(&format!("{action:?}: {message}")));
            }
            let _ = self.send(&dev.device_id, Message::ControlResult { request_id: request_id.to_string(), action, ok, message });
        };
        let Some((cid, cancel)) = found else {
            return reply(false, "That task isn't running here.".into());
        };
        let live = self.orchestrator.as_ref().and_then(|o| o.live_tasks().into_iter().find(|t| t.conversation_id.as_deref() == Some(cid.as_str())));
        let result = match (action, live, &self.orchestrator) {
            (ControlAction::Stop, Some(t), Some(o)) => {
                cancel.cancel();
                o.control(&t.id, Control::Stop).map(|_| "Stopped.".to_string())
            }
            (ControlAction::Stop, _, _) => {
                cancel.cancel();
                Ok("Stopped.".to_string())
            }
            (ControlAction::Pause, Some(t), Some(o)) => o.control(&t.id, Control::Pause).map(|_| "Paused.".to_string()),
            (ControlAction::Resume, Some(t), Some(o)) => {
                o.control(&t.id, Control::Resume).map(|_| "Resumed; it will check the current state before acting.".to_string())
            }
            (_, _, _) => Err(AppError::validation("There's no step running that can be paused or resumed right now.")),
        };
        match result {
            Ok(m) => reply(true, m),
            Err(e) => reply(false, e.to_string()),
        }
    }

    fn on_approval_answer(&self, dev: &Device, a: SignedApproval) {
        let mut g = lock(&self.awaiting);
        let Some(w) = g.get_mut(&a.call_id) else {
            tracing::warn!(event = "REMOTE_APPROVAL_UNEXPECTED");
            return;
        };
        let me = self.me_id();
        let pending =
            approval::Pending { request_id: &w.request_id, task_id: &w.task_id, call_id: &a.call_id, tool: &w.tool, input_digest: &w.digest, me: &me };
        // Only the device that requested the task can approve its actions.
        let allowed = dev.device_id == w.peer;
        let verdict = self.db.conn().and_then(|c| approval::verify(&c, &a, &dev.public(), allowed, &pending));
        match verdict {
            Ok(decision) => {
                if let Some(tx) = w.tx.take() {
                    let _ = tx.send(decision);
                }
                tracing::info!(event = "REMOTE_APPROVAL_VERIFIED", approved = decision == Decision::Approve);
            }
            Err(e) => {
                tracing::warn!(event = "REMOTE_APPROVAL_REFUSED", reason = %e);
                drop(g);
                if let Ok(c) = self.db.conn() {
                    registry::audit(&c, Some(&dev.device_id), "approval_refused", Some(&e.to_string()));
                }
            }
        }
    }

    // ── Devices ────────────────────────────────────────────────────────────

    pub fn this_device(&self) -> ThisDevice {
        let me = self.identity.read().unwrap_or_else(|p| p.into_inner());
        ThisDevice {
            device_id: me.device_id.clone(),
            name: me.name.clone(),
            platform: me.platform,
            capabilities: self.caps(),
            key_protection: self.protector.clone(),
        }
    }

    pub fn rename_this_device(&self, name: &str) -> AppResult<ThisDevice> {
        {
            let c = self.db.conn()?;
            self.identity.write().map_err(|_| AppError::internal("identity lock poisoned"))?.rename(&c, name)?;
        }
        let online: Vec<String> = lock(&self.online).iter().cloned().collect();
        for d in online {
            let _ = self.send(&d, Message::Hello { name: self.me_name(), capabilities: self.caps() });
        }
        Ok(self.this_device())
    }

    pub fn devices(&self) -> AppResult<Vec<DeviceView>> {
        let online = lock(&self.online).clone();
        Ok(registry::list(&*self.db.conn()?)?.into_iter().map(|d| DeviceView { online: online.contains(&d.device_id) && d.active(), device: d }).collect())
    }

    pub fn is_online(&self, device_id: &str) -> bool {
        lock(&self.online).contains(device_id)
    }

    pub fn rename_device(&self, device_id: &str, name: &str) -> AppResult<Device> {
        let d = registry::rename(&*self.db.conn()?, device_id, name)?;
        self.emit(HubEvent::DevicesChanged);
        Ok(d)
    }

    /// Remove a device from this IGRIS: it can no longer send anything here,
    /// its running tasks here stop, and (from a computer) the user's other
    /// devices are told to drop it too.
    pub fn revoke(&self, device_id: &str) -> AppResult<()> {
        let dev = self.db.conn().and_then(|c| registry::get(&c, device_id))?.ok_or_else(|| AppError::validation("That device isn't paired with this one."))?;
        if dev.active() {
            // Tell it, and the others, while it is still trusted.
            let _ = self.send(device_id, Message::DeviceRevoked { device_id: device_id.to_string() });
            if Self::is_manager(self.me_platform()) {
                for other in self.db.conn().and_then(|c| registry::active(&c))?.into_iter().filter(|d| d.device_id != device_id) {
                    let _ = self.send(&other.device_id, Message::DeviceRevoked { device_id: device_id.to_string() });
                }
            }
        }
        self.revoke_local(device_id, "removed on this device")?;
        Ok(())
    }

    fn revoke_local(&self, device_id: &str, why: &str) -> AppResult<()> {
        {
            let c = self.db.conn()?;
            registry::revoke(&c, device_id)?;
            registry::audit(&c, Some(device_id), "device_revoked", Some(why));
            for t in remote::unfinished(&c, Direction::Outgoing)?.into_iter().filter(|t| t.peer_device_id == device_id) {
                remote::set_status(&c, &t.request_id, RemoteStatus::Failed, Some("That device was removed."), None, None)?;
            }
        }
        // In-flight work from that device stops.
        for inc in lock(&self.incoming).values().filter(|i| i.peer == device_id) {
            inc.cancel.cancel();
        }
        lock(&self.online).remove(device_id);
        tracing::info!(event = "DEVICE_REVOKED");
        self.refresh_peers();
        Ok(())
    }

    /// Forget a removed device completely.
    pub fn forget(&self, device_id: &str) -> AppResult<()> {
        registry::forget(&*self.db.conn()?, device_id)?;
        self.emit(HubEvent::DevicesChanged);
        Ok(())
    }

    fn on_announce(&self, dev: &Device, owner_id: &str, known: KnownDevice) {
        let me = self.identity.read().map(|i| (i.device_id.clone(), i.owner_id.clone())).unwrap_or_default();
        let refuse = |why: &str| {
            if let Ok(c) = self.db.conn() {
                registry::audit(&c, Some(&dev.device_id), "announce_refused", Some(why));
            }
        };
        if !Self::is_manager(dev.platform) {
            return refuse("only computers can add devices");
        }
        if owner_id != me.1 || known.device.device_id == me.0 {
            return refuse("not for this IGRIS");
        }
        match self.db.conn().and_then(|c| registry::trust(&c, &known.device, owner_id, &known.capabilities, TrustSource::Announcement)) {
            Ok(_) => {
                if let Ok(c) = self.db.conn() {
                    registry::audit(&c, Some(&dev.device_id), "device_added", Some(&format!("{} (announced by {})", known.device.name, dev.name)));
                }
                self.refresh_peers();
                self.notice("New device added", format!("{} added {} to your IGRIS.", dev.name, known.device.name));
            }
            Err(e) => refuse(&e.to_string()),
        }
    }

    fn on_revoked(&self, dev: &Device, device_id: &str) {
        let me = self.me_id();
        if device_id == me || device_id == dev.device_id {
            // The sender removed this device (or itself): stop trusting it.
            let _ = self.revoke_local(&dev.device_id, if device_id == me { "it removed this device" } else { "it left" });
            self.notice("Device removed", format!("{} is no longer connected to this IGRIS.", dev.name));
        } else if Self::is_manager(dev.platform) {
            if self.db.conn().ok().and_then(|c| registry::trusted(&c, device_id).ok().flatten()).is_some() {
                let _ = self.revoke_local(device_id, &format!("removed by {}", dev.name));
            }
        } else if let Ok(c) = self.db.conn() {
            registry::audit(&c, Some(&dev.device_id), "revoke_refused", Some("phones can't remove other devices"));
        }
    }

    // ── Pairing ────────────────────────────────────────────────────────────

    /// Turn cross-device on (if it isn't) and wait for the connection.
    async fn ensure_connected(self: &Arc<Self>) -> AppResult<()> {
        if self.link_up() {
            return Ok(());
        }
        let s = self.settings()?;
        if !s.enabled || lock(&self.link).is_none() {
            self.db.conn()?.execute("UPDATE device_settings SET enabled = 1 WHERE id = 1", [])?;
            self.restart();
        }
        for _ in 0..100 {
            if self.link_up() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let why = self.status().detail.unwrap_or_else(|| "no answer".into());
        Err(AppError::validation(format!("Couldn't connect: {why} Check the internet connection and try again.")))
    }

    /// Show a one-time code for a new device (connects first if needed).
    /// Returns once the code is open, so it works as soon as it's shown.
    pub async fn start_pairing(self: &Arc<Self>) -> AppResult<PairingCode> {
        self.ensure_connected().await?;
        let has_devices = !self.db.conn().and_then(|c| registry::active(&c))?.is_empty();
        if !Self::is_manager(self.me_platform()) && has_devices {
            return Err(AppError::validation("Add new devices from your computer."));
        }
        let (invite, code) = Invite::new();
        let pid = invite.pairing_id.clone();
        let expires_at = invite.expires_at;
        let (tx, rx) = oneshot::channel();
        {
            let mut g = lock(&self.invites);
            g.retain(|_, s| !s.invite.expired());
            g.insert(pid.clone(), InviteState { invite, pending: None, opened: Some(tx) });
        }
        self.frame(ClientFrame::PairOpen { pid: pid.clone() });
        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(reason))) => return Err(AppError::validation(format!("Couldn't start pairing: {reason}."))),
            _ => {
                lock(&self.invites).remove(&pid);
                return Err(AppError::validation("The relay didn't answer. Try again."));
            }
        }
        tracing::info!(event = "PAIRING_STARTED");
        Ok(PairingCode { code, expires_at })
    }

    pub fn cancel_pairing(&self) {
        let pids: Vec<String> = lock(&self.invites).drain().map(|(k, _)| k).collect();
        for pid in pids {
            self.frame(ClientFrame::PairClose { pid });
        }
        if let Some(w) = lock(&self.joining).take() {
            let _ = w.tx.send(Err("Cancelled.".into()));
        }
    }

    fn on_pair_msg(self: &Arc<Self>, pid: &str, from: &str, blob: &str) {
        // Joiner side: the inviter's answer.
        {
            let mut j = lock(&self.joining);
            if j.as_ref().is_some_and(|w| w.joining.pairing_id == pid) {
                if let Some(w) = j.take() {
                    let _ = w.tx.send(Ok((from.to_string(), blob.to_string())));
                }
                return;
            }
        }
        // Inviter side: a join request.
        let (result, expired) = {
            let mut g = lock(&self.invites);
            let Some(st) = g.get_mut(pid) else { return };
            if st.pending.is_some() {
                return; // one request at a time; the user is deciding
            }
            let r = st.invite.open_join(blob, from);
            if let Ok(req) = &r {
                st.pending = Some(req.clone());
            }
            (r, st.invite.expired())
        };
        match result {
            Ok(req) => {
                if let Ok(c) = self.db.conn() {
                    registry::audit(&c, Some(from), "pairing_requested", Some(&req.device.name));
                }
                tracing::info!(event = "PAIRING_REQUESTED", platform = req.device.platform.as_str());
                self.emit(HubEvent::Notice { title: "A device wants to join".into(), body: format!("Allow \"{}\" in IGRIS to connect it.", req.device.name) });
                self.emit(HubEvent::PairingRequest { pairing_id: pid.to_string(), device_name: req.device.name.clone(), platform: req.device.platform });
            }
            Err(e) => {
                if let Ok(c) = self.db.conn() {
                    registry::audit(&c, Some(from), "pairing_attempt_failed", Some(&e.to_string()));
                }
                tracing::warn!(event = "PAIRING_ATTEMPT_FAILED", error = %e);
                if !expired {
                    self.emit(HubEvent::Notice {
                        title: "A device couldn't join".into(),
                        body: "A device tried to join with this code, but the code didn't match. Check it and try again.".into(),
                    });
                }
                if expired {
                    lock(&self.invites).remove(pid);
                    self.frame(ClientFrame::PairClose { pid: pid.to_string() });
                    self.emit(HubEvent::PairingDone { ok: false, message: "Too many wrong attempts; that code no longer works.".into() });
                }
            }
        }
    }

    /// The user's answer to "Allow this device to join?".
    pub fn confirm_pairing(&self, pairing_id: &str, allow: bool) -> AppResult<Device> {
        let st = lock(&self.invites).remove(pairing_id).ok_or_else(|| AppError::validation("That pairing request is no longer waiting."))?;
        let req = st.pending.ok_or_else(|| AppError::validation("No device has asked to join yet."))?;
        if !allow || st.invite.expired() {
            self.frame(ClientFrame::PairClose { pid: pairing_id.to_string() });
            if let Ok(c) = self.db.conn() {
                registry::audit(&c, Some(&req.device.device_id), "pairing_denied", Some(&req.device.name));
            }
            return Err(AppError::validation(if allow { "That pairing code expired." } else { "Not allowed." }));
        }
        let owner = self.identity.read().map(|i| i.owner_id.clone()).map_err(|_| AppError::internal("identity lock poisoned"))?;
        let (dev, others) = {
            let c = self.db.conn()?;
            let others: Vec<Device> = registry::active(&c)?.into_iter().filter(|d| d.device_id != req.device.device_id).collect();
            let dev = registry::trust(&c, &req.device, &owner, &req.capabilities, TrustSource::Pairing)?;
            registry::audit(&c, Some(&dev.device_id), "device_paired", Some(&dev.name));
            (dev, others)
        };
        let known: Vec<KnownDevice> = others.iter().map(|d| KnownDevice { device: d.public(), capabilities: d.capabilities.clone() }).collect();
        let blob = {
            let me = self.identity.read().map_err(|_| AppError::internal("identity lock poisoned"))?;
            st.invite.accept(&me, &self.caps(), &req, known)?
        };
        self.frame(ClientFrame::PairReply { pid: pairing_id.to_string(), to: dev.device_id.clone(), blob });
        self.frame(ClientFrame::PairClose { pid: pairing_id.to_string() });
        self.refresh_peers();
        // The user's other devices learn about the new one.
        for o in &others {
            let _ = self.send(
                &o.device_id,
                Message::DeviceAnnounce { owner_id: owner.clone(), device: KnownDevice { device: dev.public(), capabilities: dev.capabilities.clone() } },
            );
        }
        tracing::info!(event = "DEVICE_PAIRED", role = "inviter");
        self.emit(HubEvent::PairingDone { ok: true, message: format!("{} is now connected to your IGRIS.", dev.name) });
        Ok(dev)
    }

    /// Join another device's IGRIS with the code it shows.
    pub async fn join(self: &Arc<Self>, code: &str) -> AppResult<Device> {
        self.ensure_connected().await?;
        if !self.db.conn().and_then(|c| registry::active(&c))?.is_empty() {
            return Err(AppError::validation("This device is already paired with other devices. Remove them first to join a different IGRIS."));
        }
        let joining = Joining::from_code(code)?;
        let blob = {
            let me = self.identity.read().map_err(|_| AppError::internal("identity lock poisoned"))?;
            joining.request(&me, &self.caps())?
        };
        let pid = joining.pairing_id.clone();
        let (tx, rx) = oneshot::channel();
        *lock(&self.joining) = Some(JoinWait { joining, tx });
        self.frame(ClientFrame::PairSend { pid: pid.clone(), blob });
        let answer = tokio::time::timeout(JOIN_WAIT, rx).await;
        let w = lock(&self.joining).take();
        let (from, blob) = match answer {
            Ok(Ok(Ok(x))) => x,
            Ok(Ok(Err(reason))) => return Err(AppError::validation(reason)),
            _ => return Err(AppError::validation("The other device didn't allow it, or didn't answer in time.")),
        };
        // `w` is None here (the answer took it); re-derive from the code for opening.
        drop(w);
        let joining = Joining::from_code(code)?;
        let acc = {
            let me = self.identity.read().map_err(|_| AppError::internal("identity lock poisoned"))?;
            joining.open_accept(&me, &blob, &from)?
        };
        let inviter = {
            let c = self.db.conn()?;
            self.identity.write().map_err(|_| AppError::internal("identity lock poisoned"))?.adopt_owner(&c, &acc.owner_id)?;
            let inviter = registry::trust(&c, &acc.inviter, &acc.owner_id, &acc.inviter_caps, TrustSource::Pairing)?;
            let me = self.me_id();
            for k in acc.devices.iter().filter(|k| k.device.device_id != me) {
                if let Err(e) = registry::trust(&c, &k.device, &acc.owner_id, &k.capabilities, TrustSource::Announcement) {
                    tracing::warn!(event = "PAIRING_KNOWN_DEVICE_SKIPPED", error = %e);
                }
            }
            registry::audit(&c, Some(&inviter.device_id), "device_paired", Some(&inviter.name));
            inviter
        };
        // The inbox (ntfy) depends on the owner id just adopted: reconnect.
        self.restart();
        tracing::info!(event = "DEVICE_PAIRED", role = "joiner");
        self.emit(HubEvent::PairingDone { ok: true, message: format!("Connected to {}.", inviter.name) });
        Ok(inviter)
    }

    // ── Memory sync ────────────────────────────────────────────────────────

    fn sync_to(&self, peer: &str) {
        if !self.settings().map(|s| s.sync_memory).unwrap_or(false) || !self.link_up() || !self.is_online(peer) {
            return;
        }
        {
            let mut f = lock(&self.sync_inflight);
            if f.get(peer).is_some_and(|t| t.elapsed() < SYNC_RETRY) {
                return;
            }
            f.remove(peer);
        }
        let me = self.me_id();
        let batch = self.db.conn().and_then(|c| {
            let after = sync::acked(&c, peer)?;
            sync::changes_since(&c, &me, after, sync::BATCH)
        });
        let Ok((items, upto, more)) = batch else { return };
        if items.is_empty() {
            return;
        }
        lock(&self.sync_inflight).insert(peer.to_string(), Instant::now());
        let _ = self.send(peer, Message::SyncBatch { items, upto, more });
    }

    fn tick(&self) {
        // Outgoing tasks nobody picked up.
        let stale = self
            .db
            .conn()
            .and_then(|c| remote::unfinished(&c, Direction::Outgoing))
            .unwrap_or_default()
            .into_iter()
            .filter(|t| matches!(t.status, RemoteStatus::Pending | RemoteStatus::Sent | RemoteStatus::Queued))
            .filter(|t| age_ms(&t.created_at) > PICKUP_TIMEOUT.as_millis() as i64);
        for t in stale {
            let name = self.peer_name_for(&t.request_id);
            self.update_outgoing(
                &t.request_id,
                |s| matches!(s, RemoteStatus::Pending | RemoteStatus::Sent | RemoteStatus::Queued),
                RemoteStatus::TimedOut,
                Some(&format!("{name} didn't pick this up within 10 minutes, so it wasn't done.")),
                None,
            );
            self.notice(format!("{name} didn't respond"), clip(&t.objective, 120));
        }
        // Expired outbox entries.
        let now = now_ms();
        lock(&self.outbox).retain(|(_, _, exp)| *exp > now);
        // Memory changes for online peers.
        let online: Vec<String> = lock(&self.online).iter().cloned().collect();
        for p in online {
            self.sync_to(&p);
        }
    }
}

fn age_ms(ts: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(ts).map(|t| now_ms() - t.timestamp_millis()).unwrap_or(0)
}

/// Approver for a task another device requested: the requesting device can
/// approve with a signed approval (verified here), and this device's own UI
/// can answer too. Whichever answers first decides; nothing is approved
/// without one of them.
struct RemoteApprover {
    hub: Weak<DeviceHub>,
    request_id: String,
    peer: String,
    peer_name: String,
    conversation_id: String,
}

#[async_trait::async_trait]
impl Approver for RemoteApprover {
    async fn request(&self, activity: &ToolActivity, cancel: &CancellationToken) -> Approval {
        let Some(hub) = self.hub.upgrade() else { return Approval::Denied };
        let local = hub.local_approver.read().ok().and_then(|l| l.clone());
        let task_id = hub
            .orchestrator
            .as_ref()
            .and_then(|o| o.live_tasks().into_iter().find(|t| t.conversation_id.as_deref() == Some(self.conversation_id.as_str())))
            .map(|t| t.id)
            .unwrap_or_default();
        let (tx, rx) = oneshot::channel();
        // Without an argument digest a remote approval can't be bound: local only.
        let remote = activity.input_digest.clone().map(|digest| {
            let req = ApprovalRequest {
                request_id: self.request_id.clone(),
                task_id: task_id.clone(),
                call_id: activity.id.clone(),
                tool: activity.tool.clone(),
                title: activity.title.clone(),
                description: activity.description.clone(),
                permission: activity.permission.map(|p| p.as_str()).unwrap_or("unknown").to_string(),
                input_digest: digest.clone(),
                target_device: hub.me_id(),
                expires_at: now_ms() + APPROVAL_WAIT.as_millis() as i64,
            };
            lock(&hub.awaiting).insert(
                activity.id.clone(),
                Awaiting {
                    request_id: self.request_id.clone(),
                    task_id: task_id.clone(),
                    tool: activity.tool.clone(),
                    digest,
                    peer: self.peer.clone(),
                    tx: Some(tx),
                },
            );
            let _ = hub.send(&self.peer, Message::ApprovalNeeded { request: req });
        });
        hub.emit(HubEvent::LocalApproval {
            call_id: activity.id.clone(),
            request_id: self.request_id.clone(),
            device_name: self.peer_name.clone(),
            title: activity.title.clone(),
            description: activity.description.clone(),
        });
        let local_answer = async {
            match &local {
                Some(l) => l.request(activity, cancel).await,
                None => std::future::pending().await,
            }
        };
        let remote_answer = async {
            if remote.is_some() {
                match rx.await {
                    Ok(Decision::Approve) => Approval::Approved,
                    Ok(Decision::Deny) => Approval::Denied,
                    Err(_) => std::future::pending().await,
                }
            } else {
                std::future::pending().await
            }
        };
        let decision = tokio::select! {
            _ = cancel.cancelled() => Approval::Cancelled,
            a = local_answer => a,
            a = remote_answer => a,
            _ = tokio::time::sleep(APPROVAL_WAIT) => Approval::Expired,
        };
        lock(&hub.awaiting).remove(&activity.id);
        hub.emit(HubEvent::ApprovalClosed { call_id: activity.id.clone() });
        if let Ok(c) = hub.db.conn() {
            let what = match decision {
                Approval::Approved => "approved",
                Approval::Denied => "denied",
                Approval::Expired => "expired",
                Approval::Cancelled => "cancelled",
            };
            registry::audit(&c, Some(&self.peer), "remote_task_approval", Some(&format!("{what}: {}", activity.description)));
        }
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialize_for_the_ui() {
        let e = serde_json::to_value(HubEvent::PairingRequest { pairing_id: "p".into(), device_name: "Pixel".into(), platform: Platform::Android }).unwrap();
        assert_eq!(e, serde_json::json!({"type": "pairing_request", "pairingId": "p", "deviceName": "Pixel", "platform": "android"}));
        let s = serde_json::to_value(HubEvent::Status(LinkStatus { state: "connected", detail: None, relay_url: None })).unwrap();
        assert_eq!(s["type"], "status");
        assert_eq!(s["state"], "connected");
    }
}
