# IGRIS security model

IGRIS gives an AI model the ability to act on a personal computer. The design goal is that the model can only do
what the user has explicitly allowed, that every action is visible and recorded, and that content from outside
(web pages, files, images, tool output) can never take control of the assistant.

## Trust boundaries

| Component | Trust | Notes |
| --- | --- | --- |
| Rust backend | Trusted | Holds secrets, the database and every OS capability. |
| Web UI (WebView) | Semi-trusted | Can only call IGRIS's own typed IPC commands (`src-tauri/src/lib.rs`); no generic shell, fs or http plugins are exposed. Strict CSP. |
| AI model output | **Untrusted** | Can only request tools; every call is validated and permission-checked. Rendered as Markdown without raw HTML. |
| Web pages, search results, files, PDFs, images, screenshots | **Untrusted** | Fenced as data (`<untrusted_web_content>`, `<attached_document>`, file wrappers) and the system prompt forbids following instructions inside them. |

## Controls

**Tool layer (`tools/`)**
- The model cannot run commands, scripts or arbitrary programs. There is no shell tool.
- Every call goes through: JSON-shape check → offered-tool check (hallucinated tools get an error, never a fake
  success) → strict JSON-schema validation → tool-specific validation → permission policy → approval → timeout →
  append-only audit log.
- Permission levels: SAFE (read-only, never asks), LOW (contained local changes; asks if *Ask first* is on),
  SENSITIVE and CRITICAL (always ask, every time). Approvals expire after 5 minutes and can't be auto-approved.
- *Allow for this chat*: the user may trust one conversation for overwrite/move file and close app only. The trust
  is in memory (cleared on restart), recorded as `trusted` in the audit log, and never covers trash or screenshots.
- The tool set is frozen per conversation, so a conversation can't gain capabilities mid-way.
- SENSITIVE tools: overwrite/move/trash files, close applications, take a screenshot, start operator mode, run a
  command. CRITICAL: confirmed consequential operator actions (send, publish, buy, delete…).
- Levels in the operator brief map as SAFE = Safe, LOCAL = Low, SENSITIVE = Sensitive, CONSEQUENTIAL = Critical.

**Operator mode (`operator/`, `computer/`, `tools/computer.rs`)**
- IGRIS can only operate the computer inside a task the user approved (`operator_start`, SENSITIVE). The approval
  states that the mouse, keyboard and screen will be used and that screenshots go to the AI provider.
- Control is always visible (screen border, orb, main-window banner) and always interruptible: Stop/Pause buttons,
  a global stop shortcut (Esc by default) and "IGRIS, stop" by voice. Stop ends the task immediately; typing stops
  between chunks. When the user uses the mouse or keyboard, IGRIS must look again before acting; switching windows
  pauses it. A task never outlives the chat turn that started it.
- Actions refuse to run when the screen changed under them (different window, element moved). Consequential
  controls (send, post, publish, buy, pay, delete, submit…) and send shortcuts are refused unless the user confirms
  them through `computer_confirmed_action` (CRITICAL, always asks, described from what is actually on screen).
  These are guard rails based on control names and known shortcuts, not a guarantee — an app can label a sending
  control unusually. Read confirmation prompts.
- IGRIS never types into terminals or the Run dialog and can't press Win+R/Win+X; commands only run through
  `run_command`. IGRIS's own windows are excluded from window lists and captures.
- Screenshots and on-screen text are untrusted content (`<untrusted_screen_content>`). Operator screenshots are
  sent to the model only — never written to disk or the database — and only the two most recent stay in a request.
  Control names read through accessibility are kept in the conversation's tool results like other tool output;
  password field contents are never read.
- Limits: 150 actions and 8 failures per task, 5 minutes paused, 80 model round trips.
- Input to windows of elevated (administrator) apps is blocked by Windows and reported as a failure.

**Commands (`tools/terminal.rs`)**
- No shell: a program from a fixed list of developer tools plus an argument array; no pipes, redirects, `&&` or
  globbing. Runs only in a writable shared folder, with IGRIS's API keys removed from the environment, a time limit
  (max 10 minutes), cancellation, and the whole process tree killed on timeout (Windows job object).
- Every command is shown and needs approval. Routine build/test commands can be allowed for a chat; installs and
  other commands always ask; publishing (`git push`, `npm publish`, `cargo publish`…) always asks with a warning.
  Refused: force-push, hard reset, `git clean`, credential/login and config changes, inline code (`python -c`,
  `node -e`). Commands can still run project code (that is what build and test scripts do) — approve commands for
  projects you trust.

**Files (`files/`)**
- Only folders the user shares are reachable; read-only unless *Allow changes* is on.
- Paths are canonicalised; `..` escapes and symlinks out of a shared folder are refused.
- Credential files (`.env*`, SSH/GPG/cloud credential folders, `*.pem`, `*.key`, keystores, password databases)
  are never read or written. Drive roots and system folders can't be shared. The home folder can, but secret
  folders inside it (`.ssh`, `.gnupg`, `.aws`, `.azure`, `.kube`, `.docker`, `AppData`, `.config`, `.local`, browser
  and mail profiles, keychains) are always refused.
- Deletion only moves to the Recycle Bin / Trash. Executables and scripts can't be opened.

**Apps and processes**: only allowlisted applications can be launched or closed (by name, never by path or
arguments); closing sends a normal close request, never a force kill.

**Web (`tools/web.rs`)**: `fetch_url` blocks private, loopback, link-local and metadata addresses (checked after DNS
resolution, with the connection pinned to the checked address and every redirect re-checked), accepts text content
only, and caps size and time.

**Secrets**
- API keys live only in the backend (`.env` / environment); the UI only learns whether a key is configured.
- Logs record events, ids and counts — never message content, file content, prompts or keys.
- The tool audit log clips every field and replaces any field that looks like a password, API key, private key,
  payment card or government ID with `[redacted …]`.
- Memory refuses to store passwords, keys, card and ID numbers.
- `.env` is git-ignored; only `.env.example` is committed.

**Attachments (`attachments/`)**: file type is sniffed from the bytes (PNG, JPEG, GIF, WebP, PDF only), with size
and dimension limits. Files are stored under random UUID names in the app data folder, so names from the user or
model never become paths. PDF parsing runs with panics contained (release builds use `panic = "unwind"` so a
malformed file can't crash the app).

**WebView**: CSP `default-src 'self'; script-src 'self'`; images only from the app, `data:` and `blob:`, so
remote images in model output can't load (no tracking pixels or data exfiltration through image URLs). Links open
only in the system browser and only for `http(s)`. Tauri capabilities are limited to window controls, opening
http(s) URLs and the native open dialog.

**Database**: SQLite with foreign keys; every query uses bound parameters (formatted SQL only interpolates constant
column lists).

## Review (Phase 10)

Done for the 0.1 hardening pass:
- `npm audit`: 0 vulnerabilities.
- `cargo audit`: 0 vulnerabilities. Three advisory warnings remain, all in transitive dependencies:
  - `glib` 0.18 *unsound* (RUSTSEC-2024-0429) and `proc-macro-error` *unmaintained* (RUSTSEC-2024-0370) come from
    Tauri's Linux GTK stack (not used on Windows). The affected `glib` iterator isn't used by IGRIS. Fixed upstream
    when Tauri moves to newer gtk-rs.
  - `ttf-parser` *unmaintained* (RUSTSEC-2026-0192) via `pdf-extract`/`lopdf`, only used to extract PDF text;
    parsing is panic-contained.
- Fixed in this pass: tool audit redaction of secrets; release `panic = "abort"` → `"unwind"`.
- CI runs typecheck, lint, tests, `cargo fmt`, `clippy -D warnings`, `cargo test` (Linux + Windows), `npm audit` and
  `cargo audit` on every push.

## Known limitations

- A compromised WebView (e.g. via a future XSS bug) could call any IGRIS IPC command the UI can call. The CSP and
  the absence of raw-HTML rendering are the mitigations; there is no per-command origin check beyond Tauri's.
- Approval prompts show the action description; a model could still phrase a harmful but *allowed* action
  convincingly. Read approval cards before allowing them.
- Data at rest (SQLite database, attachments) is protected only by the OS user account; it is not encrypted.
- Screenshots are sent to the configured AI provider after approval; their content may include anything on screen.
- With the wake word on, every phrase the microphone picks up is sent to the configured speech service to check for
  "IGRIS". It is off by default. Spoken "yes" approvals are only as reliable as transcription; the approval card is
  still shown on screen.

## Reporting

Please report security issues privately to the repository owner rather than opening a public issue.
