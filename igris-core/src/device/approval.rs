//! Signed approvals: approving, on one device, an action that runs on another.
//!
//! The executing device never trusts "the other UI said yes". It sends an
//! [`ApprovalRequest`] describing the exact pending action; the user's answer
//! comes back as a [`SignedApproval`] that binds the decision to that task,
//! tool call, tool, argument digest and target device, with an expiry, signed
//! by the approving device's key. The executing device verifies the signature
//! against its trusted registry, checks every binding against the action it
//! is actually about to run, and accepts each approval id once.

use base64::Engine;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::envelope::now_ms;
use super::identity::{hex, random_hex, verify_sig, Identity, PublicDevice, B64};
use crate::error::{AppError, AppResult};

/// How long an approval stays valid once given.
pub const APPROVAL_TTL_MS: i64 = 5 * 60 * 1000;

/// SHA-256 of the tool input in canonical JSON (object keys sorted), hex.
pub fn input_digest(input: &serde_json::Value) -> String {
    fn canonical(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::Value::String(k.clone()).to_string());
                    out.push(':');
                    canonical(&m[k], out);
                }
                out.push('}');
            }
            serde_json::Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    canonical(x, out);
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut s = String::new();
    canonical(input, &mut s);
    hex(&Sha256::digest(s.as_bytes()))
}

/// The action waiting for approval, as the executing device describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    /// The remote task request this belongs to.
    pub request_id: String,
    /// The executing device's task id.
    pub task_id: String,
    /// Tool call id.
    pub call_id: String,
    pub tool: String,
    pub title: String,
    pub description: String,
    pub permission: String,
    pub input_digest: String,
    /// The device that will run the action.
    pub target_device: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedApproval {
    pub approval_id: String,
    pub request_id: String,
    pub task_id: String,
    pub call_id: String,
    pub tool: String,
    pub input_digest: String,
    pub target_device: String,
    pub approver_device: String,
    pub decision: Decision,
    pub issued_at: i64,
    pub expires_at: i64,
    pub sig: String,
}

impl SignedApproval {
    fn signed_bytes(&self) -> Vec<u8> {
        let d = match self.decision {
            Decision::Approve => "approve",
            Decision::Deny => "deny",
        };
        let fields: [&[u8]; 11] = [
            self.approval_id.as_bytes(),
            self.request_id.as_bytes(),
            self.task_id.as_bytes(),
            self.call_id.as_bytes(),
            self.tool.as_bytes(),
            self.input_digest.as_bytes(),
            self.target_device.as_bytes(),
            self.approver_device.as_bytes(),
            d.as_bytes(),
            &self.issued_at.to_be_bytes(),
            &self.expires_at.to_be_bytes(),
        ];
        let mut out = b"igris-approval-v1".to_vec();
        for f in fields {
            out.extend_from_slice(&(f.len() as u32).to_be_bytes());
            out.extend_from_slice(f);
        }
        out
    }

    /// The user's answer on this device, signed.
    pub fn sign(me: &Identity, req: &ApprovalRequest, decision: Decision) -> Self {
        let now = now_ms();
        let mut a = SignedApproval {
            approval_id: random_hex(16),
            request_id: req.request_id.clone(),
            task_id: req.task_id.clone(),
            call_id: req.call_id.clone(),
            tool: req.tool.clone(),
            input_digest: req.input_digest.clone(),
            target_device: req.target_device.clone(),
            approver_device: me.device_id.clone(),
            decision,
            issued_at: now,
            expires_at: now + APPROVAL_TTL_MS,
            sig: String::new(),
        };
        a.sig = B64.encode(me.sign(&a.signed_bytes()));
        a
    }
}

/// The action actually about to run on this device.
pub struct Pending<'a> {
    pub request_id: &'a str,
    pub task_id: &'a str,
    pub call_id: &'a str,
    pub tool: &'a str,
    pub input_digest: &'a str,
    pub me: &'a str,
}

/// Verify a signed approval for `pending`, from `approver` (looked up in the
/// trusted registry by the caller; `allowed` says whether that device may
/// approve this task). Consumes the approval id. Returns the decision.
pub fn verify(conn: &Connection, a: &SignedApproval, approver: &PublicDevice, allowed: bool, pending: &Pending<'_>) -> AppResult<Decision> {
    let reject = |why: &str| Err(AppError::validation(format!("Remote approval refused: {why}.")));
    if !allowed {
        return reject("that device can't approve this task");
    }
    if a.approver_device != approver.device_id {
        return reject("wrong approver");
    }
    verify_sig(approver, &a.signed_bytes(), &a.sig).map_err(|_| AppError::validation("Remote approval refused: bad signature."))?;
    let now = now_ms();
    if a.expires_at <= now || a.expires_at - a.issued_at > APPROVAL_TTL_MS || a.issued_at > now + super::envelope::MAX_SKEW_MS {
        return reject("expired");
    }
    if a.target_device != pending.me {
        return reject("it was for another device");
    }
    if a.request_id != pending.request_id || a.task_id != pending.task_id || a.call_id != pending.call_id {
        return reject("it was for another action");
    }
    if a.tool != pending.tool || a.input_digest != pending.input_digest {
        return reject("the action changed since it was approved");
    }
    let _ = conn.execute("DELETE FROM device_used_approvals WHERE expires_at < ?1", [now]);
    let fresh = conn.execute(
        "INSERT OR IGNORE INTO device_used_approvals (approval_id, expires_at) VALUES (?1, ?2)",
        params![a.approval_id, a.expires_at + super::envelope::MAX_SKEW_MS],
    )?;
    if fresh == 0 {
        return reject("it was already used");
    }
    Ok(a.decision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::device::identity::Platform;
    use serde_json::json;

    fn setup() -> (Identity, Identity, Database, ApprovalRequest) {
        let pc = Identity::generate("PC", Platform::Windows);
        let phone = Identity::generate("Phone", Platform::Android);
        let req = ApprovalRequest {
            request_id: "r1".into(),
            task_id: "t1".into(),
            call_id: "c1".into(),
            tool: "trash_path".into(),
            title: "Move to Recycle Bin".into(),
            description: "Delete report.docx".into(),
            permission: "critical".into(),
            input_digest: input_digest(&json!({"path": "C:/report.docx"})),
            target_device: pc.device_id.clone(),
            expires_at: now_ms() + 60_000,
        };
        (pc, phone, Database::open_in_memory().unwrap(), req)
    }

    fn pending<'a>(req: &'a ApprovalRequest, me: &'a str) -> Pending<'a> {
        Pending { request_id: &req.request_id, task_id: &req.task_id, call_id: &req.call_id, tool: &req.tool, input_digest: &req.input_digest, me }
    }

    #[test]
    fn digest_is_canonical() {
        assert_eq!(input_digest(&json!({"a": 1, "b": [1, {"y": 2, "x": 1}]})), input_digest(&json!({"b": [1, {"x": 1, "y": 2}], "a": 1})));
        assert_ne!(input_digest(&json!({"path": "a"})), input_digest(&json!({"path": "b"})));
    }

    #[test]
    fn a_valid_approval_is_accepted_once() {
        let (pc, phone, db, req) = setup();
        let conn = db.conn().unwrap();
        let a = SignedApproval::sign(&phone, &req, Decision::Approve);
        assert_eq!(verify(&conn, &a, &phone.public(), true, &pending(&req, &pc.device_id)).unwrap(), Decision::Approve);
        assert!(verify(&conn, &a, &phone.public(), true, &pending(&req, &pc.device_id)).is_err(), "single use");
        let d = SignedApproval::sign(&phone, &req, Decision::Deny);
        assert_eq!(verify(&conn, &d, &phone.public(), true, &pending(&req, &pc.device_id)).unwrap(), Decision::Deny);
    }

    #[test]
    fn approvals_are_bound_to_the_exact_action() {
        let (pc, phone, db, req) = setup();
        let conn = db.conn().unwrap();
        let a = SignedApproval::sign(&phone, &req, Decision::Approve);
        let other_args = input_digest(&json!({"path": "C:/everything"}));
        let cases: Vec<Pending<'_>> = vec![
            Pending { input_digest: &other_args, ..pending(&req, &pc.device_id) },
            Pending { tool: "write_file", ..pending(&req, &pc.device_id) },
            Pending { call_id: "c2", ..pending(&req, &pc.device_id) },
            Pending { task_id: "t2", ..pending(&req, &pc.device_id) },
            Pending { me: "dev_other", ..pending(&req, &pc.device_id) },
        ];
        for p in cases {
            assert!(verify(&conn, &a, &phone.public(), true, &p).is_err());
        }
    }

    #[test]
    fn forged_tampered_unauthorized_or_expired_approvals_are_refused() {
        let (pc, phone, db, req) = setup();
        let conn = db.conn().unwrap();
        let p = pending(&req, &pc.device_id);
        // Flipping a deny into an approve breaks the signature.
        let mut flipped = SignedApproval::sign(&phone, &req, Decision::Deny);
        flipped.decision = Decision::Approve;
        assert!(verify(&conn, &flipped, &phone.public(), true, &p).is_err());
        // Signed by a device that isn't the claimed approver.
        let mallory = Identity::generate("M", Platform::Linux);
        let mut forged = SignedApproval::sign(&mallory, &req, Decision::Approve);
        forged.approver_device = phone.device_id.clone();
        assert!(verify(&conn, &forged, &phone.public(), true, &p).is_err());
        // A trusted device that isn't allowed to approve this task.
        let a = SignedApproval::sign(&phone, &req, Decision::Approve);
        assert!(verify(&conn, &a, &phone.public(), false, &p).is_err());
        // Expired.
        let mut old = SignedApproval::sign(&phone, &req, Decision::Approve);
        old.issued_at -= 2 * APPROVAL_TTL_MS;
        old.expires_at -= 2 * APPROVAL_TTL_MS;
        old.sig = B64.encode(phone.sign(&old.signed_bytes()));
        assert!(verify(&conn, &old, &phone.public(), true, &p).is_err());
    }
}
