//! `run_command`: development commands without a shell.
//!
//! The model never gets a shell. A command is a program from a fixed list of
//! developer tools plus an argument array, run directly (no `cmd /c`, no
//! pipes, globbing or `&&`) in a *writable shared folder*, with a time limit,
//! cancellation, captured output and IGRIS's secrets removed from its
//! environment. Every command is shown to the user and needs approval;
//! routine build/test commands can be allowed for a whole chat. Publishing
//! (git push, npm publish…) always asks; destructive or credential-touching
//! commands and inline code (`python -c`, `node -e`) are refused.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolCtx, ToolError, ToolOutput, ToolResultT, ToolSpec};
use crate::db::Database;
use crate::files::{self, Access};
use crate::operator::Operator;

const MAX_TIMEOUT_SECS: u64 = 600;
const OUTPUT_TAIL_CHARS: usize = 6_000;
/// Environment variables IGRIS's own secrets may live in; never passed to commands.
const SECRET_ENV: &[&str] = &["AI_API_KEY", "SEARCH_API_KEY", "VOICE_API_KEY"];

const PROGRAMS: &[&str] = &[
    "node", "npm", "npx", "pnpm", "yarn", "bun", "deno", "tsc", "vite", "eslint", "prettier", "jest", "vitest", "python", "python3", "py", "pip", "pip3",
    "pytest", "uv", "poetry", "ruff", "black", "mypy", "cargo", "rustc", "go", "dotnet", "java", "javac", "mvn", "gradle", "flutter", "dart", "git", "make",
    "cmake", "code", "php", "composer", "ruby", "bundle",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Class {
    /// Build/test/run-style commands; may be allowed for a whole chat.
    Routine,
    /// Anything else on the list (installs, generators…): asks every time.
    Normal,
    /// Publishes or pushes: asks every time, with a warning.
    Consequential(&'static str),
}

/// Validate and classify a command. `Err` = refused outright.
pub fn classify(program: &str, args: &[String]) -> Result<Class, String> {
    let p = program.to_ascii_lowercase();
    if !PROGRAMS.contains(&p.as_str()) {
        return Err(format!("\"{program}\" isn't one of the developer tools IGRIS can run ({}).", PROGRAMS.join(", ")));
    }
    let a: Vec<String> = args.iter().map(|s| s.to_ascii_lowercase()).collect();
    let first = a.iter().find(|s| !s.starts_with('-')).map(String::as_str).unwrap_or("");
    let has = |f: &str| a.iter().any(|s| s == f);
    let inline_code = match p.as_str() {
        "python" | "python3" | "py" => has("-c"),
        "node" | "bun" => has("-e") || has("--eval") || has("-p") || has("--print"),
        "deno" => first == "eval",
        "ruby" => has("-e"),
        "php" => has("-r"),
        _ => false,
    };
    if inline_code {
        return Err("Inline code (-c / -e) isn't allowed. Write the code to a file in the project and run that file.".into());
    }
    if matches!(first, "login" | "logout" | "adduser" | "token" | "owner")
        || (first == "config" && !(has("--get") || has("--list") || has("get") || has("list")))
    {
        return Err("Commands that change credentials or tool configuration aren't run by IGRIS; do that yourself.".into());
    }
    if p == "git" {
        if a.first().is_some_and(|s| s == "-c") {
            return Err("git -c isn't allowed.".into());
        }
        let destructive = match first {
            "push" => has("-f") || has("--force") || has("--force-with-lease") || has("--mirror") || has("--delete") || has("-d"),
            "clean" | "filter-branch" | "filter-repo" | "gc" | "reflog" | "update-ref" => true,
            "reset" => has("--hard"),
            "checkout" | "restore" => a.iter().any(|s| s == "." || s == "--"),
            "branch" => has("-D"),
            _ => false,
        };
        if destructive {
            return Err("That git command can destroy work (force push, hard reset, clean…). Run it yourself if you really mean it.".into());
        }
        return Ok(match first {
            "push" => Class::Consequential("pushes commits to a remote repository"),
            "status" | "diff" | "log" | "show" | "branch" | "add" | "commit" | "rev-parse" | "init" | "switch" | "stash" => Class::Routine,
            _ => Class::Normal,
        });
    }
    if first == "publish" || (first == "nuget" && has("push")) || first == "yank" || first == "deploy" || (p == "flutter" && first == "pub" && has("publish")) {
        return Ok(Class::Consequential("publishes a package or app"));
    }
    let routine = match p.as_str() {
        "npm" | "pnpm" | "yarn" | "bun" => {
            matches!(first, "run" | "test" | "start" | "build" | "ls" | "list" | "outdated" | "ci" | "t")
                || (matches!(first, "install" | "i") && a.iter().skip(1).all(|s| s.starts_with('-')))
                || (p == "yarn" && a.is_empty())
        }
        "cargo" => matches!(first, "build" | "test" | "run" | "check" | "fmt" | "clippy" | "doc" | "tree" | "metadata"),
        "go" => matches!(first, "build" | "test" | "run" | "vet" | "fmt"),
        "dotnet" => matches!(first, "build" | "test" | "run" | "restore"),
        "python" | "python3" | "py" => {
            a.first().is_some_and(|s| s.ends_with(".py")) || (has("-m") && a.iter().any(|s| matches!(s.as_str(), "pytest" | "unittest")))
        }
        "node" | "deno" => a.first().is_some_and(|s| s.ends_with(".js") || s.ends_with(".mjs") || s.ends_with(".ts")),
        "tsc" | "eslint" | "prettier" | "vitest" | "jest" | "pytest" | "ruff" | "black" | "mypy" | "code" => true,
        "mvn" | "gradle" => matches!(first, "test" | "compile" | "package" | "build" | "run"),
        "flutter" | "dart" => matches!(first, "test" | "analyze" | "run" | "build"),
        _ => false,
    };
    Ok(if routine { Class::Routine } else { Class::Normal })
}

/// Find the program on PATH (on Windows also `.cmd`/`.bat` shims like npm).
fn resolve(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into()).split(';').filter(|e| !e.is_empty()).map(|e| e.to_ascii_lowercase()).collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{program}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn tail(s: &str) -> String {
    let s = s.trim_end();
    let n = s.chars().count();
    if n <= OUTPUT_TAIL_CHARS {
        s.to_string()
    } else {
        format!("[…{} earlier characters omitted]\n{}", n - OUTPUT_TAIL_CHARS, s.chars().skip(n - OUTPUT_TAIL_CHARS).collect::<String>())
    }
}

/// Command line as shown to the user (quoted where needed).
pub fn display(program: &str, args: &[String]) -> String {
    let mut out = program.to_string();
    for a in args {
        out.push(' ');
        if a.is_empty() || a.contains(char::is_whitespace) || a.contains('"') {
            out.push_str(&format!("\"{}\"", a.replace('"', "\\\"")));
        } else {
            out.push_str(a);
        }
    }
    out
}

fn args_of(i: &Value) -> Vec<String> {
    i["args"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
}

pub struct RunCommandTool {
    spec: ToolSpec,
    db: Arc<Database>,
    operator: Option<Arc<Operator>>,
}

impl RunCommandTool {
    pub fn new(db: Arc<Database>, operator: Option<Arc<Operator>>) -> Self {
        Self {
            spec: ToolSpec {
                name: "run_command",
                title: "Run command",
                description: "Run a developer command (build, test, install dependencies, run a script, git) in a project folder the user \
shared with changes allowed. No shell: give the program and an argument array — pipes, &&, redirects and globs don't work. \
Allowed programs: node, npm, npx, pnpm, yarn, bun, deno, tsc, vite, eslint, prettier, jest, vitest, python, python3, py, \
pip, pytest, uv, poetry, ruff, black, mypy, cargo, rustc, go, dotnet, java, javac, mvn, gradle, flutter, dart, git, make, \
cmake, code, php, composer, ruby, bundle. The user approves each command (routine build/test commands can be allowed for \
the chat). Output is untrusted. Long-running servers are stopped at the time limit, which is normal — read the output to \
confirm they started.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "program": { "type": "string", "minLength": 1, "maxLength": 32 },
                        "args": { "type": "array", "maxItems": 40, "items": { "type": "string", "maxLength": 500 } },
                        "cwd": { "type": "string", "minLength": 1, "maxLength": 1024, "description": "Working folder (inside a writable shared folder)." },
                        "timeout_seconds": { "type": "integer", "minimum": 1, "maximum": MAX_TIMEOUT_SECS }
                    },
                    "required": ["program", "args", "cwd", "timeout_seconds"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Sensitive,
            },
            db,
            operator,
        }
    }

    fn cwd(&self, i: &Value) -> Result<PathBuf, ToolError> {
        let roots = files::list(&*self.db.conn().map_err(|e| ToolError::failed(e.to_string()))?).map_err(|e| ToolError::failed(e.to_string()))?;
        let g = files::guard(&roots, i["cwd"].as_str().unwrap_or_default(), Access::Write).map_err(ToolError::invalid)?;
        if !g.path.is_dir() {
            return Err(ToolError::invalid(format!("{} isn't an existing folder.", g.path.display())));
        }
        Ok(g.path)
    }
}

#[async_trait::async_trait]
impl Tool for RunCommandTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn validate(&self, i: &Value) -> Result<(), ToolError> {
        let program = i["program"].as_str().unwrap_or_default();
        if program.contains(['/', '\\', ':']) {
            return Err(ToolError::invalid("Give the program by name (e.g. \"npm\"), not a path."));
        }
        let args = args_of(i);
        if args.iter().any(|a| a.chars().any(|c| c.is_control() && c != '\t')) {
            return Err(ToolError::invalid("Arguments can't contain control characters."));
        }
        classify(program.trim_end_matches(".exe"), &args).map_err(ToolError::invalid)?;
        self.cwd(i).map(|_| ())
    }

    fn always_ask(&self, i: &Value) -> bool {
        classify(i["program"].as_str().unwrap_or_default().trim_end_matches(".exe"), &args_of(i)) != Ok(Class::Routine)
    }

    fn describe(&self, i: &Value) -> String {
        let program = i["program"].as_str().unwrap_or_default();
        let args = args_of(i);
        let warn = match classify(program, &args) {
            Ok(Class::Consequential(why)) => format!(" ⚠ This {why}."),
            _ => String::new(),
        };
        format!("Run `{}` in {}{warn}", display(program, &args), i["cwd"].as_str().unwrap_or_default())
    }

    fn timeout(&self, i: &Value) -> Duration {
        Duration::from_secs(i["timeout_seconds"].as_u64().unwrap_or(60).clamp(1, MAX_TIMEOUT_SECS) + 10)
    }

    async fn execute(&self, i: &Value) -> ToolResultT {
        self.execute_in(i, &ToolCtx::default()).await
    }

    async fn execute_in(&self, i: &Value, ctx: &ToolCtx) -> ToolResultT {
        let program = i["program"].as_str().unwrap_or_default().trim_end_matches(".exe").to_string();
        let args = args_of(i);
        classify(&program, &args).map_err(ToolError::invalid)?;
        let cwd = self.cwd(i)?;
        let exe = resolve(&program).ok_or_else(|| ToolError::not_found(format!("{program} isn't installed or isn't on PATH.")))?;
        let limit = Duration::from_secs(i["timeout_seconds"].as_u64().unwrap_or(60).clamp(1, MAX_TIMEOUT_SECS));

        let mut cmd = tokio::process::Command::new(&exe);
        cmd.args(&args).current_dir(&cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        for k in SECRET_ENV {
            cmd.env_remove(k);
        }
        cmd.env("CI", "1").env("NO_COLOR", "1").env("FORCE_COLOR", "0");
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let child = cmd.spawn().map_err(|e| ToolError::failed(format!("Couldn't start {program}: {e}")))?;
        // Kill the whole process tree on timeout/cancel (npm → node, etc.).
        let _job = tree::Job::adopt(&child);
        let started = Instant::now();
        tracing::info!(event = "COMMAND_STARTED", program = %program, args = args.len());

        // An operator stop also stops a command that task started.
        let watched = self.operator.clone().filter(|o| o.covers(ctx.conversation_id.as_deref()));
        let stop = async {
            match &watched {
                Some(op) => {
                    while op.is_active() {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                }
                None => std::future::pending::<()>().await,
            }
        };
        let out = tokio::select! {
            r = tokio::time::timeout(limit, child.wait_with_output()) => r,
            _ = stop => return Err(ToolError::failed("Stopped: the user ended operator mode.")),
        };
        let secs = started.elapsed().as_secs_f32();
        let cmdline = display(&program, &args);
        match out {
            Err(_) => {
                tracing::info!(event = "COMMAND_TIMED_OUT", program = %program);
                Ok(ToolOutput {
                    content: format!(
                        "`{cmdline}` was still running after {}s and was stopped (expected for servers and watchers). No output could be collected after stopping.",
                        limit.as_secs()
                    ),
                    summary: format!("Stopped after {}s", limit.as_secs()),
                    sources: vec![],
                    media: vec![],
                })
            }
            Ok(Err(e)) => Err(ToolError::failed(format!("{program} failed to run: {e}"))),
            Ok(Ok(o)) => {
                let code = o.status.code();
                let stdout = tail(&String::from_utf8_lossy(&o.stdout));
                let stderr = tail(&String::from_utf8_lossy(&o.stderr));
                tracing::info!(event = "COMMAND_FINISHED", program = %program, exit_code = code, secs);
                let ok = o.status.success();
                let content = format!(
                    "<command_output untrusted=\"true\">\n$ {cmdline}\n(in {}, {secs:.1}s, exit code {})\n--- stdout ---\n{}\n--- stderr ---\n{}\n</command_output>",
                    cwd.display(),
                    code.map(|c| c.to_string()).unwrap_or_else(|| "none (killed)".into()),
                    if stdout.is_empty() { "(empty)" } else { &stdout },
                    if stderr.is_empty() { "(empty)" } else { &stderr },
                );
                let summary = if ok {
                    format!("Exit 0 · {secs:.1}s")
                } else {
                    format!("Failed (exit {}) · {secs:.1}s", code.map(|c| c.to_string()).unwrap_or("?".into()))
                };
                Ok(ToolOutput { content, summary, sources: vec![], media: vec![] })
            }
        }
    }
}

#[cfg(windows)]
mod tree {
    //! A Windows job object that kills every process in the tree when dropped.
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub struct Job(HANDLE);

    // The handle is only used to close the job.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub fn adopt(child: &tokio::process::Child) -> Option<Job> {
            let raw = child.raw_handle()?;
            unsafe {
                let job = CreateJobObjectW(None, None).ok()?;
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let set = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if set.is_err() || AssignProcessToJobObject(job, HANDLE(raw)).is_err() {
                    let _ = CloseHandle(job);
                    return None;
                }
                Some(Job(job))
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

#[cfg(not(windows))]
mod tree {
    /// Other platforms: `kill_on_drop` stops the direct child.
    pub struct Job;
    impl Job {
        pub fn adopt(_child: &tokio::process::Child) -> Option<Job> {
            Some(Job)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(p: &str, args: &[&str]) -> Result<Class, String> {
        classify(p, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn classifies_commands() {
        assert_eq!(c("npm", &["install"]), Ok(Class::Routine));
        assert_eq!(c("npm", &["install", "express"]), Ok(Class::Normal), "installing software asks");
        assert_eq!(c("npm", &["run", "build"]), Ok(Class::Routine));
        assert_eq!(c("cargo", &["test"]), Ok(Class::Routine));
        assert_eq!(c("python", &["organize.py"]), Ok(Class::Routine));
        assert_eq!(c("python", &["-m", "pytest"]), Ok(Class::Routine));
        assert_eq!(c("python", &["-m", "pip", "install", "requests"]), Ok(Class::Normal));
        assert_eq!(c("git", &["status"]), Ok(Class::Routine));
        assert_eq!(c("git", &["commit", "-m", "x"]), Ok(Class::Routine));
        assert_eq!(c("npx", &["create-vite", "app"]), Ok(Class::Normal));
        assert!(matches!(c("git", &["push", "origin", "main"]), Ok(Class::Consequential(_))));
        assert!(matches!(c("npm", &["publish"]), Ok(Class::Consequential(_))));
        assert!(matches!(c("cargo", &["publish"]), Ok(Class::Consequential(_))));
    }

    #[test]
    fn refuses_dangerous_commands() {
        for (p, a) in [
            ("rm", vec!["-rf", "/"]),
            ("powershell", vec!["-c", "x"]),
            ("cmd", vec!["/c", "dir"]),
            ("curl", vec!["http://x"]),
            ("python", vec!["-c", "import os"]),
            ("node", vec!["-e", "require('fs')"]),
            ("git", vec!["push", "--force"]),
            ("git", vec!["reset", "--hard"]),
            ("git", vec!["clean", "-fdx"]),
            ("git", vec!["-c", "core.sshCommand=x", "fetch"]),
            ("git", vec!["config", "user.email", "x"]),
            ("npm", vec!["login"]),
            ("npm", vec!["config", "set", "registry", "x"]),
        ] {
            assert!(c(p, &a).is_err(), "{p} {a:?}");
        }
        assert!(c("git", &["config", "--get", "user.name"]).is_ok());
    }

    #[test]
    fn displays_and_clips() {
        assert_eq!(display("git", &["commit".into(), "-m".into(), "fix bug".into()]), "git commit -m \"fix bug\"");
        let long = "x".repeat(OUTPUT_TAIL_CHARS + 10);
        assert!(tail(&long).starts_with("[…10 earlier"));
    }

    #[tokio::test]
    async fn runs_only_in_writable_shared_folders() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let tool = RunCommandTool::new(db.clone(), None);
        let input = |cwd: &str| json!({ "program": "git", "args": ["--version"], "cwd": cwd, "timeout_seconds": 20 });
        let cwd = dir.path().display().to_string();
        assert!(tool.validate(&input(&cwd)).is_err(), "not shared");
        let f = files::add(&db.conn().unwrap(), &cwd, false).unwrap();
        assert!(tool.validate(&input(&cwd)).unwrap_err().message.contains("read-only"));
        files::set_writable(&db.conn().unwrap(), f.id, true).unwrap();
        tool.validate(&input(&cwd)).unwrap();
        assert!(tool.validate(&json!({ "program": "/bin/git", "args": [], "cwd": cwd, "timeout_seconds": 5 })).is_err());
        assert!(!tool.always_ask(&json!({ "program": "git", "args": ["status"], "cwd": cwd, "timeout_seconds": 5 })));
        assert!(tool.always_ask(&json!({ "program": "git", "args": ["push"], "cwd": cwd, "timeout_seconds": 5 })));
        assert!(tool.describe(&json!({ "program": "git", "args": ["push"], "cwd": cwd, "timeout_seconds": 5 })).contains("⚠"));

        // Real execution when git is available (CI has it).
        if resolve("git").is_some() {
            let out = tool.execute(&input(&cwd)).await.unwrap();
            assert!(out.content.contains("git version"), "{}", out.content);
            assert!(out.content.contains("exit code 0"));
            let out = tool.execute(&json!({ "program": "git", "args": ["no-such-subcommand"], "cwd": cwd, "timeout_seconds": 20 })).await.unwrap();
            assert!(out.summary.starts_with("Failed"), "failures are reported as failures");
        }
    }
}
