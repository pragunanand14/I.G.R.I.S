//! The typed messages devices exchange, always inside an [`Envelope`](super::envelope::Envelope).
//!
//! There is deliberately no "run this tool" or "run this command" message: a
//! device can only ask another to do a *task* in natural language, which the
//! receiving device plans and executes with its own tools, its own permission
//! policy and its own approvals. Everything else is status, control of a task
//! the sender itself requested, approvals, device announcements and memory
//! sync.

use serde::{Deserialize, Serialize};

use super::approval::{ApprovalRequest, SignedApproval};
use super::capability::Capability;
use super::pairing::KnownDevice;
use super::sync::SyncItem;

pub const MAX_OBJECTIVE_CHARS: usize = 2000;
pub const MAX_DETAIL_CHARS: usize = 2000;

/// Where a cross-device task stands, as both devices see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteStatus {
    /// Waiting to be sent (this device has no relay connection right now).
    Pending,
    /// Handed to the relay; the target device is online.
    Sent,
    /// The target is offline; the relay holds it for a limited time.
    Queued,
    /// The target accepted it and is starting.
    Accepted,
    Running,
    WaitingForApproval,
    Paused,
    Completed,
    /// Finished without a verified result; the user needs to look.
    Ended,
    Failed,
    Cancelled,
    /// The target refused it (not allowed, busy, can't do it).
    Rejected,
    /// Nobody answered in time (target offline too long, or no reply).
    TimedOut,
}

impl RemoteStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RemoteStatus::Pending => "pending",
            RemoteStatus::Sent => "sent",
            RemoteStatus::Queued => "queued",
            RemoteStatus::Accepted => "accepted",
            RemoteStatus::Running => "running",
            RemoteStatus::WaitingForApproval => "waiting_for_approval",
            RemoteStatus::Paused => "paused",
            RemoteStatus::Completed => "completed",
            RemoteStatus::Ended => "ended",
            RemoteStatus::Failed => "failed",
            RemoteStatus::Cancelled => "cancelled",
            RemoteStatus::Rejected => "rejected",
            RemoteStatus::TimedOut => "timed_out",
        }
    }

    pub fn parse(s: &str) -> RemoteStatus {
        serde_json::from_value(serde_json::Value::String(s.to_string())).unwrap_or(RemoteStatus::Failed)
    }

    pub fn is_final(self) -> bool {
        matches!(
            self,
            RemoteStatus::Completed | RemoteStatus::Ended | RemoteStatus::Failed | RemoteStatus::Cancelled | RemoteStatus::Rejected | RemoteStatus::TimedOut
        )
    }

    /// From the executing device's orchestrator state.
    pub fn from_task(state: crate::orchestrator::task::TaskState) -> RemoteStatus {
        use crate::orchestrator::task::TaskState::*;
        match state {
            Created | Planning | Executing | Verifying | Recovering => RemoteStatus::Running,
            WaitingForApproval => RemoteStatus::WaitingForApproval,
            Paused => RemoteStatus::Paused,
            Completed => RemoteStatus::Completed,
            Failed => RemoteStatus::Failed,
            Cancelled => RemoteStatus::Cancelled,
            Ended => RemoteStatus::Ended,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlAction {
    Pause,
    Resume,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// Sent when a connection comes up: name and what the device can do now.
    Hello {
        name: String,
        capabilities: Vec<Capability>,
    },
    /// Do this, on your side, with your tools and your rules.
    TaskRequest {
        request_id: String,
        objective: String,
    },
    /// Received and accepted (work starts) or refused, with why.
    TaskAck {
        request_id: String,
        accepted: bool,
        reason: Option<String>,
    },
    /// Progress. `seq` increases per request; older updates are ignored.
    TaskUpdate {
        request_id: String,
        seq: u64,
        status: RemoteStatus,
        step: Option<String>,
        detail: Option<String>,
    },
    /// Pause / resume / stop a task this device requested.
    TaskControl {
        request_id: String,
        action: ControlAction,
    },
    ControlResult {
        request_id: String,
        action: ControlAction,
        ok: bool,
        message: String,
    },
    /// An action in a task you requested needs approval.
    ApprovalNeeded {
        request: ApprovalRequest,
    },
    /// The user's signed answer.
    ApprovalAnswer {
        approval: SignedApproval,
    },
    /// A device the sender just paired (only accepted from devices allowed to manage devices).
    DeviceAnnounce {
        owner_id: String,
        device: KnownDevice,
    },
    /// The sender removed this device from the user's IGRIS.
    DeviceRevoked {
        device_id: String,
    },
    /// Memory changes after the receiver's last acknowledged point.
    SyncBatch {
        items: Vec<SyncItem>,
        upto: i64,
        more: bool,
    },
    SyncAck {
        upto: i64,
    },
}

impl Message {
    pub fn kind(&self) -> &'static str {
        match self {
            Message::Hello { .. } => "hello",
            Message::TaskRequest { .. } => "task_request",
            Message::TaskAck { .. } => "task_ack",
            Message::TaskUpdate { .. } => "task_update",
            Message::TaskControl { .. } => "task_control",
            Message::ControlResult { .. } => "control_result",
            Message::ApprovalNeeded { .. } => "approval_needed",
            Message::ApprovalAnswer { .. } => "approval_answer",
            Message::DeviceAnnounce { .. } => "device_announce",
            Message::DeviceRevoked { .. } => "device_revoked",
            Message::SyncBatch { .. } => "sync_batch",
            Message::SyncAck { .. } => "sync_ack",
        }
    }

    /// How long the relay may hold it for an offline device.
    pub fn ttl_ms(&self) -> i64 {
        match self {
            // Task requests and approvals are only useful soon.
            Message::TaskRequest { .. } | Message::TaskControl { .. } => 10 * 60 * 1000,
            Message::ApprovalNeeded { .. } | Message::ApprovalAnswer { .. } => 5 * 60 * 1000,
            _ => super::envelope::MAX_TTL_MS,
        }
    }
}

/// Request ids are chosen by the requesting device: 32 hex chars.
pub fn valid_request_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_tagged_json_and_unknown_types_fail() {
        let m = Message::TaskRequest { request_id: "a".repeat(32), objective: "open notepad".into() };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains("\"type\":\"task_request\""));
        assert_eq!(serde_json::from_str::<Message>(&j).unwrap(), m);
        assert!(serde_json::from_str::<Message>(r#"{"type":"run_shell","cmd":"rm -rf /"}"#).is_err());
    }

    #[test]
    fn status_round_trips() {
        for s in [RemoteStatus::WaitingForApproval, RemoteStatus::TimedOut, RemoteStatus::Completed] {
            assert_eq!(RemoteStatus::parse(s.as_str()), s);
        }
    }
}
