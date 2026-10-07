//! Live operator scenarios: a real AI provider (through the ModelRouter, as in
//! the app), the full tool set, the orchestrator and the real Windows desktop.
//!
//! Needs an interactive Windows desktop and a provider key. Opt in with
//! `IGRIS_LIVE=1` and the usual `AI_PROVIDER` / `AI_API_KEY` / `AI_MODEL`:
//!
//! ```text
//! IGRIS_LIVE=1 cargo test --test operator_live -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The approver here stands in for the user on an unattended machine: it
//! approves ordinary approvals and denies every CRITICAL (consequential)
//! action, and records each request. Results go to the workspace's `target/operator-live-report.md`.
#![cfg(windows)]

use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use igris_lib::ai::AiRuntime;
use igris_lib::config::AppConfig;
use igris_lib::conversations::{self, MessageStatus};
use igris_lib::core::chat::{generate, save_user_message, GenerationParams, Tooling};
use igris_lib::db::Database;
use igris_lib::operator::Operator;
use igris_lib::orchestrator::task::TaskState;
use igris_lib::orchestrator::{store, Orchestrator};
use igris_lib::tools::executor::{Approval, Approver, Policy, ToolActivity};
use igris_lib::tools::PermissionLevel;
use tokio_util::sync::CancellationToken;

struct HarnessApprover(Mutex<Vec<String>>);

#[async_trait::async_trait]
impl Approver for HarnessApprover {
    async fn request(&self, a: &ToolActivity, _c: &CancellationToken) -> Approval {
        let critical = a.permission == Some(PermissionLevel::Critical);
        self.0.lock().unwrap().push(format!("{} {} — {}", if critical { "DENIED" } else { "approved" }, a.tool, a.description));
        if critical {
            Approval::Denied
        } else {
            Approval::Approved
        }
    }
}

struct Ctx {
    db: Arc<Database>,
    params: GenerationParams,
    work: tempfile::TempDir,
    approvals: Arc<HarnessApprover>,
    hub: Arc<Orchestrator>,
}

fn setup() -> Ctx {
    let config = AppConfig::load(None);
    let ai = AiRuntime::from_config(&config);
    let router = ai.router.clone().unwrap_or_else(|| panic!("AI isn't configured: {:?}", ai.status.problem));
    let data = tempfile::tempdir().unwrap().keep();
    let db = Arc::new(Database::open(&data.join("igris.db")).unwrap());
    let work = tempfile::tempdir().unwrap();
    {
        let conn = db.conn().unwrap();
        igris_lib::files::add(&conn, &work.path().display().to_string(), true).unwrap();
        let _ = igris_lib::tools::apps::add(&conn, "Notepad", r"C:\Windows\System32\notepad.exe");
        if let Some(code) = std::process::Command::new("where").arg("code").output().ok().filter(|o| o.status.success()) {
            let p = String::from_utf8_lossy(&code.stdout).lines().find(|l| l.ends_with(".cmd")).unwrap_or_default().replace("bin\\code.cmd", "Code.exe");
            let _ = igris_lib::tools::apps::add(&conn, "VS Code", &p);
        }
    }
    let operator = Arc::new(Operator::new(db.clone(), igris_lib::computer::native()));
    let hub = Orchestrator::new(db.clone(), Some(operator.clone()));
    let config = Arc::new(RwLock::new(config));
    let web = config.read().unwrap().web_search_mode().map(str::to_string);
    let registry = Arc::new(igris_lib::tools::standard::registry(igris_lib::tools::standard::Deps {
        db: db.clone(),
        config,
        system: Arc::new(Mutex::new(igris_lib::system::SystemMonitor::new())),
        connectivity: igris_lib::system::ConnectivityMonitor::start(Duration::from_secs(60)),
        attachments: Arc::new(igris_lib::attachments::AttachmentStore::new(data.join("attachments")).unwrap()),
        operator: operator.clone(),
        orchestrator: hub.clone(),
        browser_profile: data.join("browser-profile"),
        open_path: Arc::new(|p: &std::path::Path| std::process::Command::new("explorer").arg(p).spawn().map(|_| ()).map_err(|e| e.to_string())),
        open_url: Arc::new(|u: &str| std::process::Command::new("cmd").args(["/c", "start", "", u]).spawn().map(|_| ()).map_err(|e| e.to_string())),
        // No other devices in this harness (the device tools report that honestly).
        devices: Arc::new(std::sync::OnceLock::new()),
    }));
    let approvals = Arc::new(HarnessApprover(Mutex::new(Vec::new())));
    let params = GenerationParams {
        router,
        chat_model: None,
        depth: None,
        attachments: None,
        resume_task: None,
        tooling: Tooling {
            registry,
            web_search_mode: web,
            orchestrator: Some(hub.clone()),
            policy: Policy::default(),
            approver: approvals.clone(),
            trust: None,
            operator: Some(operator),
        },
    };
    Ctx { db, params, work, approvals, hub }
}

struct Outcome {
    name: &'static str,
    pass: bool,
    why: String,
    secs: f64,
    reply: String,
    task: String,
}

async fn run(ctx: &Ctx, name: &'static str, prompt: &str, check: impl Fn(&str, Option<&igris_lib::orchestrator::task::Task>) -> (bool, String)) -> Outcome {
    let offered = ctx.params.tooling.registry.offered(ctx.params.tooling.web_search_mode.as_deref());
    let (c, _) = save_user_message(&ctx.db, None, prompt, "Tester", &offered, false).unwrap();
    let t = Instant::now();
    let cancel = CancellationToken::new();
    let m = tokio::time::timeout(Duration::from_secs(600), generate(&ctx.db, &c.id, &ctx.params, &cancel, &mut |_| {})).await;
    let secs = t.elapsed().as_secs_f64();
    let (reply, ok_msg) = match m {
        Ok(Ok(m)) => {
            (format!("[{:?}] {}{}", m.status, m.content, m.error.map(|e| format!(" (error: {e})")).unwrap_or_default()), m.status == MessageStatus::Complete)
        }
        Ok(Err(e)) => (format!("generation error: {e}"), false),
        Err(_) => ("timed out after 10 minutes".into(), false),
    };
    let task = store::for_conversation(&ctx.db.conn().unwrap(), &c.id, 1).unwrap().into_iter().next();
    let (pass, why) = if ok_msg { check(&reply, task.as_ref()) } else { (false, "the reply didn't complete".into()) };
    let task_line = task
        .as_ref()
        .map(|t| {
            format!("{} · {} action(s) · {} failure(s) · {}", t.state.as_str(), t.steps, t.failures, t.result.clone().or(t.error.clone()).unwrap_or_default())
        })
        .unwrap_or_else(|| "no task".into());
    let events = task.as_ref().map(|t| ctx.hub.events(&t.id, 60).unwrap_or_default()).unwrap_or_default();
    eprintln!("FINDING: [{name}] {} in {secs:.0}s — {why}\n  reply: {reply}\n  task: {task_line}", if pass { "PASS" } else { "FAIL" });
    for e in events {
        eprintln!("    {} {} {}", e.kind, e.tool.unwrap_or_default(), e.detail.unwrap_or_default().chars().take(140).collect::<String>());
    }
    let _ = conversations::messages(&ctx.db.conn().unwrap(), &c.id);
    Outcome { name, pass, why, secs, reply, task: task_line }
}

fn notepad_text() -> Option<String> {
    let d = igris_lib::computer::native();
    let w = d.windows().ok()?.into_iter().find(|w| w.process.eq_ignore_ascii_case("notepad.exe"))?;
    let el = d.ui_elements(w.id, 4000).ok()?.into_iter().find(|e| matches!(e.role.as_str(), "edit" | "document"))?;
    d.focus_window(w.id).ok()?;
    let (x, y) = el.rect.center();
    d.click(x, y, igris_lib::computer::MouseButton::Left, 1).ok()?;
    std::thread::sleep(Duration::from_millis(300));
    d.focused_field().and_then(|f| f.value)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real AI provider and an interactive Windows desktop"]
async fn live_operator_scenarios() {
    if std::env::var("IGRIS_LIVE").is_err() {
        eprintln!("Set IGRIS_LIVE=1 (and AI_PROVIDER / AI_API_KEY / AI_MODEL) to run the live scenarios.");
        return;
    }
    let ctx = setup();
    let dir = ctx.work.path().display().to_string();
    let kill = |img: &str| {
        let _ = std::process::Command::new("taskkill").args(["/IM", img, "/F"]).output();
    };
    let completed = |t: Option<&igris_lib::orchestrator::task::Task>| t.is_some_and(|t| t.state == TaskState::Completed);
    let mut results = Vec::new();

    kill("notepad.exe");
    results.push(
        run(&ctx, "1 open application", "Open Notepad.", |_, t| {
            let open = igris_lib::computer::native().windows().unwrap_or_default().iter().any(|w| w.process.eq_ignore_ascii_case("notepad.exe"));
            (open && completed(t), format!("Notepad window open: {open}; task completed: {}", completed(t)))
        })
        .await,
    );
    kill("notepad.exe");

    results.push(
        run(&ctx, "2 type into application", "Open Notepad and type exactly this text into it: IGRIS operator test", |_, t| {
            let text = notepad_text().unwrap_or_default();
            let ok = text.contains("IGRIS operator test");
            (ok && t.is_some_and(|t| t.state != TaskState::Failed), format!("Notepad reads {text:?}"))
        })
        .await,
    );
    kill("notepad.exe");

    let file = ctx.work.path().join("live-test.txt");
    let f2 = file.clone();
    results.push(
        run(&ctx, "3 multi-step file task", &format!("In the folder {dir}, create a text file named live-test.txt containing exactly the sentence 'Phase 4 live test.', then read it back to verify its contents."), move |_, t| {
            let content = std::fs::read_to_string(&f2).unwrap_or_default();
            (content.trim() == "Phase 4 live test." && completed(t), format!("file content {content:?}; task completed: {}", completed(t)))
        })
        .await,
    );

    let hello = ctx.work.path().join("hello.py");
    let h2 = hello.clone();
    results.push(
        run(
            &ctx,
            "4 developer task",
            &format!("In the folder {dir}, create hello.py that prints 'Hello from IGRIS', run it with python, and tell me exactly what it printed."),
            move |reply, t| {
                let exists = h2.exists();
                let ok = exists && reply.contains("Hello from IGRIS") && t.is_some_and(|t| t.state != TaskState::Failed);
                (ok, format!("hello.py exists: {exists}; reply mentions the output: {}", reply.contains("Hello from IGRIS")))
            },
        )
        .await,
    );

    results.push(
        run(&ctx, "5 browser task", "Open https://example.com in the browser and tell me the page's main heading.", |reply, _| {
            (reply.contains("Example Domain"), format!("reply mentions \"Example Domain\": {}", reply.contains("Example Domain")))
        })
        .await,
    );

    kill("notepad.exe");
    let _ = std::process::Command::new("notepad.exe").spawn();
    std::thread::sleep(Duration::from_secs(2));
    results.push(
        run(&ctx, "9 failure and recovery", "In the Notepad window that's open, click the button labelled 'Frobnicate 9000'.", |reply, t| {
            let honest = !completed(t);
            (honest, format!("task not reported as completed: {honest}; reply: {}", reply.chars().take(160).collect::<String>()))
        })
        .await,
    );
    kill("notepad.exe");

    let mut report = String::from("# IGRIS live operator run\n\n");
    report.push_str(&format!(
        "Provider/model: {:?} / {:?}\n\n| Scenario | Result | Time | Task | Notes |\n|---|---|---|---|---|\n",
        std::env::var("AI_PROVIDER").ok(),
        std::env::var("AI_MODEL").ok()
    ));
    for r in &results {
        report.push_str(&format!(
            "| {} | {} | {:.0}s | {} | {} |\n",
            r.name,
            if r.pass { "PASS" } else { "FAIL" },
            r.secs,
            r.task.replace('|', "/"),
            r.why.replace('|', "/")
        ));
    }
    report.push_str("\n## Approvals\n");
    for a in ctx.approvals.0.lock().unwrap().iter() {
        report.push_str(&format!("- {a}\n"));
    }
    report.push_str("\n## Replies\n");
    for r in &results {
        report.push_str(&format!("- **{}**: {}\n", r.name, r.reply.replace('\n', " ")));
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("target").join("operator-live-report.md");
    let _ = std::fs::write(&path, &report);
    eprintln!("{report}");
    let failed: Vec<&str> = results.iter().filter(|r| !r.pass).map(|r| r.name).collect();
    assert!(failed.is_empty(), "scenarios failed: {failed:?} (see the report)");
}
