//! A stand-in "PC" for testing cross-device IGRIS against a real phone or
//! emulator: a real device hub (real identity, relay link, pairing, envelopes,
//! signed approvals) whose task runner, instead of an AI model, asks the phone
//! to approve one action and then reports what happened.
//!
//! ```text
//! cargo run -p igris-core --example device_peer -- ws://127.0.0.1:8787 OUT_DIR
//! ```
//! Writes `OUT_DIR/code.txt` (the pairing code) and `OUT_DIR/events.log`
//! (one line per event). The pairing request is allowed automatically, as a
//! user clicking "Allow" would — use only in tests.

use std::io::Write;
use std::sync::{Arc, Mutex};

use igris_core::db::Database;
use igris_core::device::hub::{DeviceHub, HubEvent, TaskRunner};
use igris_core::device::registry::Device;
use igris_core::device::{Capability, Identity, Platform, Unprotected};
use igris_core::error::{AppError, AppResult};
use igris_core::tools::executor::{ActivityStatus, Approval, Approver, ToolActivity};
use igris_core::tools::PermissionLevel;
use tokio_util::sync::CancellationToken;

struct Log(Mutex<std::fs::File>);

impl Log {
    fn line(&self, s: &str) {
        println!("{s}");
        if let Ok(mut f) = self.0.lock() {
            let _ = writeln!(f, "{s}");
            let _ = f.flush();
        }
    }
}

/// Asks the requesting phone to approve a (harmless, never executed) action.
struct ApprovalRunner(Arc<Log>);

#[async_trait::async_trait]
impl TaskRunner for ApprovalRunner {
    fn open(&self, from: &Device, objective: &str) -> AppResult<String> {
        self.0.line(&format!("TASK_RECEIVED from={} objective={objective}", from.name));
        Ok(format!("conv-{}", uuid::Uuid::new_v4().simple()))
    }

    async fn run(&self, _conversation_id: &str, approver: Arc<dyn Approver>, cancel: CancellationToken) -> AppResult<String> {
        let input = serde_json::json!({"path": "C:/IGRIS-test/hello.txt", "content": "hello from the phone"});
        let activity = ToolActivity {
            id: format!("call_{}", uuid::Uuid::new_v4().simple()),
            tool: "write_file".into(),
            title: "Write file".into(),
            permission: Some(PermissionLevel::Sensitive),
            description: "Write C:/IGRIS-test/hello.txt (test — nothing is written)".into(),
            status: ActivityStatus::AwaitingApproval,
            result: None,
            duration_ms: None,
            text_offset: None,
            sources: vec![],
            attachments: vec![],
            failure: None,
            input_digest: Some(igris_core::device::approval::input_digest(&input)),
        };
        let decision = approver.request(&activity, &cancel).await;
        self.0.line(&format!("APPROVAL {decision:?}"));
        match decision {
            Approval::Approved => Ok("The phone approved it (signed approval verified on the PC).".into()),
            other => Err(AppError::validation(format!("Not approved: {other:?}"))),
        }
    }
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let url = args.next().expect("relay url");
    let out = std::path::PathBuf::from(args.next().expect("output dir"));
    std::fs::create_dir_all(&out).unwrap();
    let log = Arc::new(Log(Mutex::new(std::fs::File::create(out.join("events.log")).unwrap())));

    let db = Arc::new(Database::open(&out.join("peer.db")).unwrap());
    let identity = Identity::load_or_create(&db.conn().unwrap(), &Unprotected, "CI PC", Platform::Windows).unwrap();
    // Present as a Windows PC (a computer may invite devices).
    log.line(&format!("PEER device={}", identity.device_id));
    let orchestrator = igris_core::orchestrator::Orchestrator::new(db.clone(), None);
    let hub = DeviceHub::new(db.clone(), identity, "none", Some(orchestrator));
    hub.set_capabilities(vec![Capability::Tasks, Capability::Memory, Capability::Files]);
    hub.set_runner(Arc::new(ApprovalRunner(log.clone())));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<HubEvent>();
    hub.on_event(Arc::new(move |e: &HubEvent| {
        let _ = tx.send(e.clone());
    }));
    hub.start();
    hub.configure(Some(&url), true, true).unwrap();

    let mut code_written = false;
    while let Some(e) = rx.recv().await {
        log.line(&format!("EVENT {}", serde_json::to_string(&e).unwrap_or_default()));
        match e {
            HubEvent::Status(s) if s.state == "connected" && !code_written => match hub.start_pairing().await {
                Ok(c) => {
                    std::fs::write(out.join("code.txt"), &c.code).unwrap();
                    log.line(&format!("CODE {}", c.code));
                    code_written = true;
                }
                Err(e) => log.line(&format!("PAIRING_FAILED {e}")),
            },
            HubEvent::PairingRequest { pairing_id, device_name, platform } => {
                log.line(&format!("ALLOWING {device_name} ({})", platform.as_str()));
                match hub.confirm_pairing(&pairing_id, true) {
                    Ok(d) => log.line(&format!("PAIRED {} {}", d.name, d.platform.as_str())),
                    Err(e) => log.line(&format!("PAIRING_FAILED {e}")),
                }
            }
            HubEvent::Task(t) if t.task.status.is_final() => {
                log.line(&format!("TASK_FINISHED status={} detail={}", t.task.status.as_str(), t.task.detail.unwrap_or_default()));
            }
            _ => {}
        }
    }
}
